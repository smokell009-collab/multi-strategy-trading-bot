use super::{OrderIntent, RegisterError, RegisterOutcome};
use ode_domain::{OrderSide, OrderStatus};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use rust_decimal::Decimal;
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("erro SQLite no journal de ordens: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("falha de serialização do journal: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Register(#[from] RegisterError),
    #[error("ordem local não encontrada: {0}")]
    NotFound(String),
    #[error("já existe uma ordem não terminal no mercado: {0}")]
    ActiveOrderExists(String),
    #[error("atualização da ordem violaria quantidade, fill ou estado terminal")]
    InvalidUpdate,
    #[error("registro persistido contém campos inválidos: {0}")]
    CorruptRecord(String),
    #[error("chave de sessão, data UTC ou patrimônio inválido")]
    InvalidRiskSession,
    #[error("relógio do sistema anterior ao Unix epoch")]
    InvalidClock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedOrder {
    pub intent: OrderIntent,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
    pub executed_quote: Decimal,
    pub exchange_order_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct SqliteOrderJournal {
    path: PathBuf,
}

type RawOrder = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

impl SqliteOrderJournal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                JournalError::CorruptRecord(format!(
                    "não foi possível criar pasta do journal: {error}"
                ))
            })?;
        }
        let journal = Self { path };
        let connection = journal.connection()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS testnet_orders (
                client_order_id TEXT PRIMARY KEY NOT NULL,
                market_id TEXT NOT NULL,
                side_json TEXT NOT NULL,
                quantity TEXT NOT NULL,
                status_json TEXT NOT NULL,
                filled_quantity TEXT NOT NULL,
                executed_quote TEXT NOT NULL,
                exchange_order_id TEXT,
                last_error TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS testnet_orders_market_created
                ON testnet_orders(market_id, created_at_ms);
            CREATE TABLE IF NOT EXISTS testnet_risk_sessions (
                account_key TEXT PRIMARY KEY NOT NULL,
                session_date_utc TEXT NOT NULL,
                starting_equity TEXT NOT NULL
            );",
        )?;
        Ok(journal)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<Connection, JournalError> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(connection)
    }

    /// Registra a intenção antes de qualquer chamada de criação de ordem.
    /// Uma chave repetida com o mesmo payload é identificada; payload divergente é bloqueado.
    pub fn register_once(&self, intent: &OrderIntent) -> Result<RegisterOutcome, JournalError> {
        validate_client_order_id(&intent.client_order_id)?;
        if intent.quantity <= Decimal::ZERO {
            return Err(RegisterError::InvalidQuantity.into());
        }
        let now = now_ms()?;
        let connection = self.connection()?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO testnet_orders(
                client_order_id, market_id, side_json, quantity, status_json,
                filled_quantity, executed_quote, exchange_order_id, last_error,
                created_at_ms, updated_at_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, '0', '0', NULL, NULL, ?6, ?6)",
            params![
                intent.client_order_id,
                intent.market_id.0,
                serde_json::to_string(&intent.side)?,
                intent.quantity.to_string(),
                serde_json::to_string(&OrderStatus::Pending)?,
                now,
            ],
        )?;
        if inserted == 1 {
            return Ok(RegisterOutcome::Inserted);
        }
        let existing = get_order_from(&connection, &intent.client_order_id)?
            .ok_or_else(|| JournalError::NotFound(intent.client_order_id.clone()))?;
        if existing.intent == *intent {
            Ok(RegisterOutcome::Duplicate)
        } else {
            Err(RegisterError::IdempotencyConflict.into())
        }
    }

    /// Registra uma intenção apenas se não houver outra ordem não terminal no mesmo mercado.
    /// A transação IMMEDIATE serializa processos concorrentes antes do POST à venue.
    pub fn register_once_if_no_active(
        &self,
        intent: &OrderIntent,
    ) -> Result<RegisterOutcome, JournalError> {
        validate_client_order_id(&intent.client_order_id)?;
        if intent.quantity <= Decimal::ZERO {
            return Err(RegisterError::InvalidQuantity.into());
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = get_order_from(&transaction, &intent.client_order_id)? {
            return if existing.intent == *intent {
                Ok(RegisterOutcome::Duplicate)
            } else {
                Err(RegisterError::IdempotencyConflict.into())
            };
        }
        let filled = serde_json::to_string(&OrderStatus::Filled)?;
        let cancelled = serde_json::to_string(&OrderStatus::Cancelled)?;
        let rejected = serde_json::to_string(&OrderStatus::Rejected)?;
        let active: Option<String> = transaction
            .query_row(
                "SELECT client_order_id FROM testnet_orders
                 WHERE market_id=?1 AND status_json NOT IN (?2, ?3, ?4)
                 ORDER BY created_at_ms, client_order_id LIMIT 1",
                params![intent.market_id.0, filled, cancelled, rejected],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(active_id) = active {
            return Err(JournalError::ActiveOrderExists(active_id));
        }
        let now = now_ms()?;
        transaction.execute(
            "INSERT INTO testnet_orders(
                client_order_id, market_id, side_json, quantity, status_json,
                filled_quantity, executed_quote, exchange_order_id, last_error,
                created_at_ms, updated_at_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, '0', '0', NULL, NULL, ?6, ?6)",
            params![
                intent.client_order_id,
                intent.market_id.0,
                serde_json::to_string(&intent.side)?,
                intent.quantity.to_string(),
                serde_json::to_string(&OrderStatus::Pending)?,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(RegisterOutcome::Inserted)
    }

    pub fn get_order(&self, client_order_id: &str) -> Result<Option<PersistedOrder>, JournalError> {
        get_order_from(&self.connection()?, client_order_id)
    }

    pub fn orders_for_market(&self, market_id: &str) -> Result<Vec<PersistedOrder>, JournalError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT client_order_id, market_id, side_json, quantity, status_json,
                    filled_quantity, executed_quote, exchange_order_id, last_error,
                    created_at_ms, updated_at_ms
             FROM testnet_orders WHERE market_id=?1 ORDER BY created_at_ms, client_order_id",
        )?;
        let rows = statement.query_map([market_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })?;
        rows.map(|row| decode_order(row?)).collect()
    }

    pub fn update_order(
        &self,
        client_order_id: &str,
        status: OrderStatus,
        filled_quantity: Decimal,
        executed_quote: Decimal,
        exchange_order_id: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<PersistedOrder, JournalError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing = get_order_from(&transaction, client_order_id)?
            .ok_or_else(|| JournalError::NotFound(client_order_id.to_owned()))?;
        if filled_quantity < existing.filled_quantity
            || filled_quantity < Decimal::ZERO
            || filled_quantity > existing.intent.quantity
            || executed_quote < existing.executed_quote
            || executed_quote < Decimal::ZERO
            || (status == OrderStatus::Filled && filled_quantity != existing.intent.quantity)
            || (status == OrderStatus::PartiallyFilled
                && (filled_quantity <= Decimal::ZERO
                    || filled_quantity >= existing.intent.quantity))
            || (is_terminal(existing.status) && status != existing.status)
        {
            return Err(JournalError::InvalidUpdate);
        }
        let updated_at_ms = now_ms()?;
        transaction.execute(
            "UPDATE testnet_orders SET status_json=?2, filled_quantity=?3,
                executed_quote=?4, exchange_order_id=COALESCE(?5, exchange_order_id),
                last_error=?6, updated_at_ms=?7 WHERE client_order_id=?1",
            params![
                client_order_id,
                serde_json::to_string(&status)?,
                filled_quantity.to_string(),
                executed_quote.to_string(),
                exchange_order_id,
                last_error,
                updated_at_ms,
            ],
        )?;
        let updated = get_order_from(&transaction, client_order_id)?
            .ok_or_else(|| JournalError::NotFound(client_order_id.to_owned()))?;
        transaction.commit()?;
        Ok(updated)
    }

    /// Mantém a âncora diária UTC por conta/mercado; altera-a só ao mudar a data.
    pub fn risk_session_start(
        &self,
        account_key: &str,
        utc_day: &str,
        current_equity: Decimal,
    ) -> Result<Decimal, JournalError> {
        if account_key.trim().is_empty()
            || !valid_utc_day(utc_day)
            || current_equity <= Decimal::ZERO
        {
            return Err(JournalError::InvalidRiskSession);
        }
        let connection = self.connection()?;
        let mut connection = connection;
        let transaction = connection.transaction()?;
        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT session_date_utc, starting_equity FROM testnet_risk_sessions WHERE account_key=?1",
                [account_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let start_equity = match stored {
            Some((day, value)) if day == utc_day => Decimal::from_str(&value)
                .map_err(|error| JournalError::CorruptRecord(format!("daily equity: {error}")))?,
            _ => {
                transaction.execute(
                    "INSERT INTO testnet_risk_sessions(account_key, session_date_utc, starting_equity)
                     VALUES(?1, ?2, ?3)
                     ON CONFLICT(account_key) DO UPDATE SET
                        session_date_utc=excluded.session_date_utc,
                        starting_equity=excluded.starting_equity",
                    params![account_key, utc_day, current_equity.to_string()],
                )?;
                current_equity
            }
        };
        transaction.commit()?;
        if start_equity <= Decimal::ZERO {
            return Err(JournalError::InvalidRiskSession);
        }
        Ok(start_equity)
    }
}

fn get_order_from(
    connection: &Connection,
    client_order_id: &str,
) -> Result<Option<PersistedOrder>, JournalError> {
    let raw: Option<RawOrder> = connection
        .query_row(
            "SELECT client_order_id, market_id, side_json, quantity, status_json,
                    filled_quantity, executed_quote, exchange_order_id, last_error,
                    created_at_ms, updated_at_ms
             FROM testnet_orders WHERE client_order_id=?1",
            [client_order_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?;
    raw.map(decode_order).transpose()
}

fn decode_order(raw: RawOrder) -> Result<PersistedOrder, JournalError> {
    let parse_decimal = |value: &str, field: &str| {
        Decimal::from_str(value)
            .map_err(|error| JournalError::CorruptRecord(format!("{field}: {error}")))
    };
    let (
        client_order_id,
        market_id,
        side,
        quantity,
        status,
        filled,
        executed,
        exchange_id,
        error,
        created,
        updated,
    ) = raw;
    Ok(PersistedOrder {
        intent: OrderIntent {
            client_order_id,
            market_id: market_id.into(),
            side: serde_json::from_str::<OrderSide>(&side)?,
            quantity: parse_decimal(&quantity, "quantity")?,
        },
        status: serde_json::from_str::<OrderStatus>(&status)?,
        filled_quantity: parse_decimal(&filled, "filled quantity")?,
        executed_quote: parse_decimal(&executed, "executed quote")?,
        exchange_order_id: exchange_id,
        last_error: error,
        created_at_ms: created,
        updated_at_ms: updated,
    })
}

fn validate_client_order_id(id: &str) -> Result<(), JournalError> {
    if id.is_empty()
        || id.len() > 36
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(RegisterError::InvalidClientOrderId.into());
    }
    Ok(())
}

fn is_terminal(status: OrderStatus) -> bool {
    status.is_terminal()
}

fn valid_utc_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn now_ms() -> Result<i64, JournalError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JournalError::InvalidClock)?
        .as_millis();
    i64::try_from(millis).map_err(|_| JournalError::InvalidClock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(id: &str, side: OrderSide, quantity: Decimal) -> OrderIntent {
        OrderIntent {
            client_order_id: id.into(),
            market_id: "BTCUSDT".into(),
            side,
            quantity,
        }
    }

    #[test]
    fn journal_persists_intent_and_rejects_duplicate_payload_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("orders.sqlite3");
        let journal = SqliteOrderJournal::open(&path).unwrap();
        let order = intent("ode-123-test", OrderSide::Buy, Decimal::new(1, 4));
        assert_eq!(
            journal.register_once(&order).unwrap(),
            RegisterOutcome::Inserted
        );
        assert_eq!(
            journal.register_once(&order).unwrap(),
            RegisterOutcome::Duplicate
        );
        assert!(matches!(
            journal.register_once(&intent("ode-123-test", OrderSide::Sell, Decimal::new(1, 4))),
            Err(JournalError::Register(RegisterError::IdempotencyConflict))
        ));
        drop(journal);
        let reopened = SqliteOrderJournal::open(path).unwrap();
        let stored = reopened.get_order("ode-123-test").unwrap().unwrap();
        assert_eq!(stored.intent, order);
        assert_eq!(stored.status, OrderStatus::Pending);
        assert_eq!(stored.filled_quantity, Decimal::ZERO);
    }

    #[test]
    fn market_guard_blocks_active_orders_and_releases_after_terminal_state() {
        let directory = tempfile::tempdir().unwrap();
        let journal = SqliteOrderJournal::open(directory.path().join("orders.sqlite3")).unwrap();
        let first = intent("ode-active-first", OrderSide::Buy, Decimal::new(1, 3));
        let second = intent("ode-active-second", OrderSide::Sell, Decimal::new(1, 3));
        assert_eq!(
            journal.register_once_if_no_active(&first).unwrap(),
            RegisterOutcome::Inserted
        );
        assert!(matches!(
            journal.register_once_if_no_active(&second),
            Err(JournalError::ActiveOrderExists(id)) if id == first.client_order_id
        ));
        journal
            .update_order(
                &first.client_order_id,
                OrderStatus::Filled,
                first.quantity,
                Decimal::from(60),
                Some("test-exchange-id"),
                None,
            )
            .unwrap();
        assert_eq!(
            journal.register_once_if_no_active(&second).unwrap(),
            RegisterOutcome::Inserted
        );
    }

    #[test]
    fn journal_updates_fills_monotonically_and_rejects_terminal_downgrades() {
        let directory = tempfile::tempdir().unwrap();
        let journal = SqliteOrderJournal::open(directory.path().join("orders.sqlite3")).unwrap();
        let order = intent("ode-456-test", OrderSide::Buy, Decimal::new(2, 4));
        journal.register_once(&order).unwrap();
        let partial = journal
            .update_order(
                &order.client_order_id,
                OrderStatus::PartiallyFilled,
                Decimal::new(1, 4),
                Decimal::from(5),
                Some("9821"),
                None,
            )
            .unwrap();
        assert_eq!(partial.status, OrderStatus::PartiallyFilled);
        let filled = journal
            .update_order(
                &order.client_order_id,
                OrderStatus::Filled,
                order.quantity,
                Decimal::from(10),
                None,
                None,
            )
            .unwrap();
        assert_eq!(filled.status, OrderStatus::Filled);
        assert!(matches!(
            journal.update_order(
                &order.client_order_id,
                OrderStatus::Unknown,
                order.quantity,
                Decimal::from(10),
                None,
                Some("late timeout"),
            ),
            Err(JournalError::InvalidUpdate)
        ));
    }

    #[test]
    fn daily_risk_session_is_stable_until_utc_date_changes() {
        let directory = tempfile::tempdir().unwrap();
        let journal = SqliteOrderJournal::open(directory.path().join("orders.sqlite3")).unwrap();
        assert_eq!(
            journal
                .risk_session_start("binance-testnet:BTCUSDT", "2026-10-11", Decimal::from(1000))
                .unwrap(),
            Decimal::from(1000)
        );
        assert_eq!(
            journal
                .risk_session_start("binance-testnet:BTCUSDT", "2026-10-11", Decimal::from(900))
                .unwrap(),
            Decimal::from(1000)
        );
        assert_eq!(
            journal
                .risk_session_start("binance-testnet:BTCUSDT", "2026-10-12", Decimal::from(900))
                .unwrap(),
            Decimal::from(900)
        );
    }
}
