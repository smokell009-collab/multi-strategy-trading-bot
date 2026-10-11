use super::{Args, TestnetCommand, TestnetSide, risk_engine};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use ode_binance_testnet::{AccountInfo, BinanceOrder, BinanceSpotTestnetClient};
use ode_domain::{
    DataQuality, Direction, MarketId, MarketSnapshot, OrderSide, OrderStatus, PortfolioSnapshot,
    StrategyId, TradeSignal,
};
use ode_execution::{
    ExecutionSafety, JournalError, OrderIntent, PersistedOrder, RegisterOutcome, SqliteOrderJournal,
};
use ode_risk::{RiskInput, RiskPolicy};
use rust_decimal::Decimal;
use serde_json::json;
use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::time::sleep;

pub(super) const TESTNET_CONFIRMATION: &str = "BINANCE-SPOT-TESTNET";
static ORDER_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(super) fn validate_testnet_command(args: &Args) -> Result<()> {
    let Some(command) = &args.testnet_command else {
        return Ok(());
    };
    if args.symbol != "BTCUSDT" {
        bail!("Fase 5 permite somente BTCUSDT; não há agregação de risco entre mercados");
    }
    if args.backtest_csv.is_some()
        || args.backtest_report.is_some()
        || args.dashboard
        || args.enable_ai_advisory
        || args.once
        || args.activate_kill_switch.is_some()
    {
        bail!("subcomandos Testnet não podem ser combinados com paper, backtest ou dashboard");
    }
    match command {
        TestnetCommand::Preflight => {}
        TestnetCommand::PlaceMarketOrder {
            quantity,
            confirm_testnet_order,
            ..
        } => {
            if *quantity <= Decimal::ZERO {
                bail!("quantity Testnet deve ser maior que zero");
            }
            if confirm_testnet_order != TESTNET_CONFIRMATION {
                bail!("ordem Testnet exige --confirm-testnet-order {TESTNET_CONFIRMATION}");
            }
        }
        TestnetCommand::ReconcileOrder { client_order_id } => {
            if !valid_testnet_client_order_id(client_order_id) {
                bail!("clientOrderId inválido para reconciliação");
            }
        }
    }
    Ok(())
}

pub(super) async fn execute(args: &Args) -> Result<()> {
    let command = args
        .testnet_command
        .as_ref()
        .context("subcomando Testnet ausente")?;
    match command {
        TestnetCommand::Preflight => run_preflight(args).await,
        TestnetCommand::PlaceMarketOrder { side, quantity, .. } => {
            let policy = RiskPolicy::from_path(&args.risk_overrides).with_context(|| {
                format!(
                    "não foi possível carregar overrides em {}",
                    args.risk_overrides.display()
                )
            })?;
            let risk_engine = risk_engine(args, policy)?;
            run_market_order(args, *side, *quantity, &risk_engine).await
        }
        TestnetCommand::ReconcileOrder { client_order_id } => {
            run_reconcile(args, client_order_id).await
        }
    }
}

async fn connect_client() -> Result<BinanceSpotTestnetClient> {
    let api_key = env::var("BINANCE_API_KEY")
        .context("BINANCE_API_KEY ausente; configure uma chave da Spot Testnet no ambiente")?;
    let api_secret = env::var("BINANCE_SECRET")
        .context("BINANCE_SECRET ausente; configure o segredo da Spot Testnet no ambiente")?;
    BinanceSpotTestnetClient::connect(&api_key, &api_secret)
        .await
        .context("não foi possível conectar à Binance Spot Testnet")
}

async fn run_preflight(args: &Args) -> Result<()> {
    let client = connect_client().await?;
    let rules = client.symbol_rules(&args.symbol).await?;
    let average_price = client.average_price(&args.symbol).await?;
    let account = client.account().await?;
    let (base_free, base_locked) = balance(&account, &rules.base_asset);
    let (quote_free, quote_locked) = balance(&account, &rules.quote_asset);
    let result = json!({
        "environment": "binance-spot-testnet",
        "symbol": rules.symbol,
        "trading_allowed": rules.trading_allowed,
        "can_trade": account.can_trade,
        "base_asset": rules.base_asset,
        "quote_asset": rules.quote_asset,
        "average_price": average_price.to_string(),
        "base_free": base_free.to_string(),
        "base_locked": base_locked.to_string(),
        "quote_free": quote_free.to_string(),
        "quote_locked": quote_locked.to_string(),
        "orders_sent": 0
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn run_market_order(
    args: &Args,
    testnet_side: TestnetSide,
    quantity: Decimal,
    risk_engine: &ode_risk::RiskEngine,
) -> Result<()> {
    let side = match testnet_side {
        TestnetSide::Buy => OrderSide::Buy,
        TestnetSide::Sell => OrderSide::Sell,
    };
    let client = connect_client().await?;
    let rules = client.symbol_rules(&args.symbol).await?;
    let average_price = client.average_price(&args.symbol).await?;
    let price_observed_at_ms = Utc::now().timestamp_millis();
    rules.validate_market_order(quantity, average_price)?;
    let account = client.account().await?;
    if !account.can_trade {
        bail!("a conta Testnet não permite trading; nenhuma ordem foi enviada");
    }

    let (base_free, base_locked) = balance(&account, &rules.base_asset);
    let (quote_free, quote_locked) = balance(&account, &rules.quote_asset);
    let base_total = base_free + base_locked;
    let quote_total = quote_free + quote_locked;
    let equity = quote_total + base_total * average_price;
    let minimum_base = rules
        .market_quantity
        .min_quantity
        .max(rules.lot_quantity.min_quantity);
    let has_base_position = if minimum_base > Decimal::ZERO {
        base_total >= minimum_base
    } else {
        base_total > Decimal::ZERO
    };
    if equity <= Decimal::ZERO {
        bail!("equity do par na conta Testnet é zero ou inválido");
    }
    let journal = SqliteOrderJournal::open(&args.testnet_orders_db).with_context(|| {
        format!(
            "não foi possível abrir journal {}",
            args.testnet_orders_db.display()
        )
    })?;
    ensure_no_unresolved_order(&journal, &args.symbol)?;
    let kill_switch_active = ode_risk::KillSwitch::new(&args.kill_switch_file).is_active()?;
    let safety = ExecutionSafety {
        kill_switch_active,
        market_state_known: true,
        account_state_known: true,
        reconciliation_required: false,
    };
    let session_date = Utc::now().date_naive().to_string();
    let session_start_equity = journal.risk_session_start(
        &format!("binance-spot-testnet:{}", args.symbol),
        &session_date,
        equity,
    )?;

    match side {
        OrderSide::Buy => {
            if has_base_position {
                bail!(
                    "já existe saldo base suficiente para uma posição neste par; venda/reconcilie antes de nova entrada"
                );
            }
            safety
                .authorize_new_order()
                .map_err(|reason| anyhow::anyhow!(reason.to_string()))?;
            let now_ms = Utc::now().timestamp_millis();
            let signal = TradeSignal {
                strategy_id: StrategyId::from("manual_testnet_order"),
                market_id: MarketId::from(args.symbol.as_str()),
                direction: Direction::Buy,
                estimated_probability: None,
                expected_net_edge_bps: 0,
                max_slippage_bps: args.slippage_bps,
                signal_timestamp_ms: price_observed_at_ms,
                expires_at_ms: price_observed_at_ms.saturating_add(60_000),
            };
            let market = MarketSnapshot {
                market_id: MarketId::from(args.symbol.as_str()),
                price: average_price,
                observed_at_ms: price_observed_at_ms,
                quality: DataQuality::Valid,
            };
            let portfolio = PortfolioSnapshot {
                equity,
                cash: quote_free,
                daily_start_equity: session_start_equity,
                open_exposure: base_total * average_price,
                has_open_position: has_base_position,
            };
            let ticket = risk_engine.authorize_entry(RiskInput {
                signal: &signal,
                market: &market,
                portfolio: &portfolio,
                now_ms,
                kill_switch_active: false,
                execution_slippage_bps: args.slippage_bps,
            })?;
            if quantity > ticket.quantity {
                bail!(
                    "quantidade solicitada excede o ticket do motor de risco; máximo calculado {}",
                    ticket.quantity
                );
            }
            let worst_case_cost = quantity
                * average_price
                * (Decimal::ONE
                    + Decimal::from(args.slippage_bps + args.fee_bps) / Decimal::from(10_000));
            if worst_case_cost > quote_free {
                bail!("saldo quote livre insuficiente para a compra, incluindo custos estimados");
            }
        }
        OrderSide::Sell => {
            if quantity > base_free {
                bail!(
                    "quantidade de venda excede o saldo base livre; saldo bloqueado não pode ser usado"
                );
            }
        }
    }

    let client_order_id = new_client_order_id();
    let intent = OrderIntent {
        client_order_id: client_order_id.clone(),
        market_id: MarketId::from(args.symbol.as_str()),
        side,
        quantity,
    };
    match journal.register_once_if_no_active(&intent)? {
        RegisterOutcome::Inserted => {}
        RegisterOutcome::Duplicate => {
            bail!("clientOrderId já existe no journal; o POST não será repetido")
        }
    }
    if side == OrderSide::Buy && ode_risk::KillSwitch::new(&args.kill_switch_file).is_active()? {
        journal.update_order(
            &client_order_id,
            OrderStatus::Rejected,
            Decimal::ZERO,
            Decimal::ZERO,
            None,
            Some("kill-switch ativado antes do POST; ordem não enviada"),
        )?;
        bail!("kill-switch foi ativado antes do envio; nenhuma ordem foi enviada");
    }

    let first_response = match client
        .place_market_order(&args.symbol, side, quantity, &client_order_id)
        .await
    {
        Ok(order) => order,
        Err(_) => {
            journal.update_order(
                &client_order_id,
                OrderStatus::Unknown,
                Decimal::ZERO,
                Decimal::ZERO,
                None,
                Some("resposta do POST incerta; consulte por clientOrderId; sem retry automático"),
            )?;
            match client.query_order(&args.symbol, &client_order_id).await {
                Ok(order) => order,
                Err(_) => {
                    bail!(
                        "resultado da submissão é incerto; não reenvie. Consulte `testnet-reconcile-order --client-order-id {client_order_id}`"
                    )
                }
            }
        }
    };

    let mut local = journal
        .get_order(&client_order_id)?
        .context("intent desapareceu do journal local")?;
    let mut venue = first_response;
    local = update_from_venue(&journal, &local, &venue)?;
    for _ in 0..5 {
        if is_terminal(local.status) {
            break;
        }
        sleep(Duration::from_millis(300)).await;
        match client.query_order(&args.symbol, &client_order_id).await {
            Ok(response) => {
                venue = response;
                match update_from_venue(&journal, &local, &venue) {
                    Ok(updated) => local = updated,
                    Err(error) => {
                        let _ = journal.update_order(
                            &client_order_id,
                            OrderStatus::Unknown,
                            local.filled_quantity,
                            local.executed_quote,
                            None,
                            Some("resposta da venue divergente; revisão necessária"),
                        );
                        return Err(error);
                    }
                }
            }
            Err(_) => {
                local = journal.update_order(
                    &client_order_id,
                    OrderStatus::Unknown,
                    local.filled_quantity,
                    local.executed_quote,
                    None,
                    Some("consulta de reconciliação falhou; não repetir POST"),
                )?;
                break;
            }
        }
    }
    print_order(&local);
    println!("ordem testnet enviada; nenhum caminho de produção existe neste binário");
    Ok(())
}

async fn run_reconcile(args: &Args, client_order_id: &str) -> Result<()> {
    let journal = SqliteOrderJournal::open(&args.testnet_orders_db)?;
    let local = journal.get_order(client_order_id)?.with_context(|| {
        format!(
            "clientOrderId {client_order_id} não consta no journal local; não será criada ordem"
        )
    })?;
    let client = connect_client().await?;
    let venue = client.query_order(&args.symbol, client_order_id).await?;
    let updated = update_from_venue(&journal, &local, &venue)?;
    print_order(&updated);
    Ok(())
}

fn update_from_venue(
    journal: &SqliteOrderJournal,
    local: &PersistedOrder,
    venue: &BinanceOrder,
) -> Result<PersistedOrder> {
    let fields_match = venue.client_order_id == local.intent.client_order_id
        && venue.symbol == local.intent.market_id.0
        && venue.side == local.intent.side
        && venue.original_quantity == local.intent.quantity;
    if !fields_match {
        if !is_terminal(local.status) {
            let _ = journal.update_order(
                &local.intent.client_order_id,
                OrderStatus::Unknown,
                local.filled_quantity,
                local.executed_quote,
                None,
                Some("campos locais e da venue divergentes; revisão necessária"),
            );
        }
        bail!(
            "campos da ordem da Testnet divergem do intent persistido; nenhuma nova ordem será enviada"
        );
    }
    journal
        .update_order(
            &local.intent.client_order_id,
            venue.status,
            venue.executed_quantity,
            venue.executed_quote,
            Some(&venue.exchange_order_id),
            None,
        )
        .map_err(|error| match error {
            JournalError::InvalidUpdate => anyhow::anyhow!(
                "fill/status da venue viola o journal local; ordem mantida para revisão"
            ),
            other => anyhow::Error::new(other),
        })
}

fn print_order(order: &PersistedOrder) {
    let output = json!({
        "environment": "binance-spot-testnet",
        "symbol": order.intent.market_id.0,
        "side": format!("{:?}", order.intent.side).to_ascii_lowercase(),
        "client_order_id": order.intent.client_order_id,
        "exchange_order_id": order.exchange_order_id,
        "requested_quantity": order.intent.quantity.to_string(),
        "filled_quantity": order.filled_quantity.to_string(),
        "executed_quote": order.executed_quote.to_string(),
        "status": format!("{:?}", order.status).to_ascii_lowercase(),
        "updated_at_ms": order.updated_at_ms,
        "last_error": order.last_error
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_owned())
    );
}

fn balance(account: &AccountInfo, asset: &str) -> (Decimal, Decimal) {
    account
        .balances
        .iter()
        .find(|balance| balance.asset == asset)
        .map(|balance| (balance.free, balance.locked))
        .unwrap_or((Decimal::ZERO, Decimal::ZERO))
}

fn new_client_order_id() -> String {
    let counter = ORDER_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "ode-{}-{}-{counter}",
        Utc::now().timestamp_millis(),
        std::process::id()
    )
}

fn valid_testnet_client_order_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 36
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
}

fn is_terminal(status: OrderStatus) -> bool {
    status.is_terminal()
}

fn ensure_no_unresolved_order(journal: &SqliteOrderJournal, symbol: &str) -> Result<()> {
    if let Some(unresolved) = journal
        .orders_for_market(symbol)?
        .into_iter()
        .find(|order| !is_terminal(order.status))
    {
        bail!(
            "há ordem {} em estado {:?}; reconcilie-a antes de criar qualquer ordem nova",
            unresolved.intent.client_order_id,
            unresolved.status
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Args, TestnetCommand, TestnetSide};

    #[test]
    fn generated_client_order_ids_fit_venue_constraints() {
        let id = new_client_order_id();
        assert!(valid_testnet_client_order_id(&id));
        assert!(id.len() <= 36);
    }

    #[test]
    fn command_validation_requires_exact_testnet_confirmation() {
        let mut args = test_args();
        args.testnet_command = Some(TestnetCommand::PlaceMarketOrder {
            side: TestnetSide::Buy,
            quantity: Decimal::new(1, 4),
            confirm_testnet_order: "yes".into(),
        });
        assert!(validate_testnet_command(&args).is_err());
        if let Some(TestnetCommand::PlaceMarketOrder {
            confirm_testnet_order,
            ..
        }) = args.testnet_command.as_mut()
        {
            *confirm_testnet_order = TESTNET_CONFIRMATION.to_owned();
        }
        assert!(validate_testnet_command(&args).is_ok());
    }

    #[test]
    fn testnet_commands_refuse_paper_and_backtest_modes() {
        let mut args = test_args();
        args.testnet_command = Some(TestnetCommand::Preflight);
        args.once = true;
        assert!(validate_testnet_command(&args).is_err());
        args.once = false;
        args.backtest_csv = Some("data/history.csv".into());
        assert!(validate_testnet_command(&args).is_err());
    }

    #[test]
    fn testnet_scope_rejects_symbols_without_portfolio_wide_risk() {
        let mut args = test_args();
        args.symbol = "ETHUSDT".into();
        args.testnet_command = Some(TestnetCommand::Preflight);
        assert!(validate_testnet_command(&args).is_err());
    }

    #[test]
    fn unresolved_order_blocks_new_submission_until_terminal_reconciliation() {
        let path = env::temp_dir().join(format!(
            "ode-unresolved-{}-{}.sqlite3",
            std::process::id(),
            ORDER_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let journal = SqliteOrderJournal::open(&path).unwrap();
        let intent = OrderIntent {
            client_order_id: "ode-test-unresolved-1".into(),
            market_id: MarketId::from("BTCUSDT"),
            side: OrderSide::Buy,
            quantity: Decimal::new(1, 3),
        };
        journal.register_once(&intent).unwrap();
        assert!(ensure_no_unresolved_order(&journal, "BTCUSDT").is_err());
        journal
            .update_order(
                &intent.client_order_id,
                OrderStatus::Filled,
                intent.quantity,
                Decimal::from(60),
                Some("test-exchange-id"),
                None,
            )
            .unwrap();
        assert!(ensure_no_unresolved_order(&journal, "BTCUSDT").is_ok());
        drop(journal);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    fn test_args() -> Args {
        Args {
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            poll_seconds: 15,
            starting_balance: Decimal::from(10_000),
            risk_per_trade: Decimal::new(1, 2),
            max_daily_loss: Decimal::new(2, 2),
            max_allocation: Decimal::new(25, 2),
            stop_loss: Decimal::new(2, 2),
            fee_bps: 10,
            slippage_bps: 5,
            strategy_config: "configs/strategies.example.toml".into(),
            backtest_csv: None,
            in_sample_ratio: Decimal::new(7, 1),
            backtest_report: None,
            audit_db: "data/test.sqlite3".into(),
            dashboard: false,
            dashboard_bind: "127.0.0.1:8787".parse().unwrap(),
            enable_ai_advisory: false,
            risk_overrides: "configs/risk-overrides.example.toml".into(),
            kill_switch_file: "data/KILL_SWITCH.json".into(),
            activate_kill_switch: None,
            once: false,
            testnet_orders_db: "data/testnet-orders.sqlite3".into(),
            testnet_command: None,
        }
    }
}
