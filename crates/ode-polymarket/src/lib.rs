use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, interval_at},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub const GAMMA_BASE: &str = "https://gamma-api.polymarket.com";
pub const CLOB_BASE: &str = "https://clob.polymarket.com";
pub const DATA_BASE: &str = "https://data-api.polymarket.com";
pub const MARKET_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const USER_AGENT: &str = "olho-de-deus-polymarket-readonly/0.3";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Yes,
    No,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeeSchedule {
    pub enabled: Option<bool>,
    pub rate: Option<Decimal>,
    pub exponent: Option<u32>,
    pub taker_only: Option<bool>,
    pub rebate_rate: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Market {
    pub id: String,
    pub slug: String,
    pub question: String,
    pub condition_id: String,
    pub version: String,
    pub active: Option<bool>,
    pub closed: Option<bool>,
    pub enable_order_book: Option<bool>,
    pub accepting_orders: Option<bool>,
    pub min_tick_size: Option<Decimal>,
    pub min_order_size: Option<Decimal>,
    pub fees: FeeSchedule,
    yes_asset_id: String,
    no_asset_id: String,
}

impl Market {
    pub fn asset_id(&self, outcome: Outcome) -> &str {
        match outcome {
            Outcome::Yes => &self.yes_asset_id,
            Outcome::No => &self.no_asset_id,
        }
    }

    pub fn can_read_book(&self) -> bool {
        self.active != Some(false)
            && self.closed != Some(true)
            && self.enable_order_book != Some(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookLevel {
    pub price: Decimal,
    pub size: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderBook {
    pub asset_id: String,
    pub timestamp_ms: i64,
    pub hash: Option<String>,
    /// Ascending by price; best bid is the last level.
    pub bids: Vec<BookLevel>,
    /// Ascending by price; best ask is the first level.
    pub asks: Vec<BookLevel>,
    pub min_order_size: Option<Decimal>,
    pub tick_size: Option<Decimal>,
    pub last_trade_price: Option<Decimal>,
}

impl OrderBook {
    pub fn best_bid(&self) -> Option<&BookLevel> {
        self.bids.last()
    }

    pub fn best_ask(&self) -> Option<&BookLevel> {
        self.asks.first()
    }

    pub fn is_valid(&self) -> bool {
        self.best_bid()
            .is_some_and(|level| level.price > Decimal::ZERO)
            && self
                .best_ask()
                .is_some_and(|level| level.price > Decimal::ZERO)
            && self
                .best_bid()
                .zip(self.best_ask())
                .is_some_and(|(bid, ask)| bid.price < ask.price)
            && self.bids.iter().chain(&self.asks).all(|level| {
                level.price > Decimal::ZERO
                    && level.price < Decimal::ONE
                    && level.size >= Decimal::ZERO
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceHistoryPoint {
    pub timestamp_ms: i64,
    pub price: Decimal,
    pub resolution_seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeTick {
    pub asset_id: String,
    pub timestamp_ms: i64,
    pub price: Decimal,
    pub size: Decimal,
    pub side: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketFeedEvent {
    Connected,
    Disconnected {
        reason: String,
    },
    Book {
        book: OrderBook,
        received_at_ms: i64,
        full_snapshot: bool,
    },
    Trade {
        tick: TradeTick,
        received_at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct RawBookState {
    asset_id: String,
    market: Option<String>,
    timestamp_ms: i64,
    hash: Option<String>,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
}

impl RawBookState {
    fn new(asset_id: &str) -> Self {
        Self {
            asset_id: asset_id.to_owned(),
            market: None,
            timestamp_ms: 0,
            hash: None,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    fn snapshot(
        &self,
        min_order_size: Option<Decimal>,
        tick_size: Option<Decimal>,
        last_trade_price: Option<Decimal>,
    ) -> OrderBook {
        OrderBook {
            asset_id: self.asset_id.clone(),
            timestamp_ms: self.timestamp_ms,
            hash: self.hash.clone(),
            bids: self
                .bids
                .iter()
                .map(|(price, size)| BookLevel {
                    price: *price,
                    size: *size,
                })
                .collect(),
            asks: self
                .asks
                .iter()
                .map(|(price, size)| BookLevel {
                    price: *price,
                    size: *size,
                })
                .collect(),
            min_order_size,
            tick_size,
            last_trade_price,
        }
    }

    fn replace_side(levels: &mut BTreeMap<Decimal, Decimal>, value: Option<&Value>) -> Result<()> {
        levels.clear();
        for level in value
            .and_then(Value::as_array)
            .context("book snapshot sem níveis")?
        {
            let price = decimal_field(level, "price")?;
            let size = decimal_field(level, "size")?;
            if price <= Decimal::ZERO || price >= Decimal::ONE || size < Decimal::ZERO {
                bail!("nível de book fora do domínio válido");
            }
            if size > Decimal::ZERO {
                levels.insert(price, size);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PolymarketClient {
    http: Client,
}

impl Default for PolymarketClient {
    fn default() -> Self {
        Self::new().expect("fixed HTTP client configuration is valid")
    }
}

impl PolymarketClient {
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("não foi possível criar cliente HTTP somente leitura")?;
        Ok(Self { http })
    }

    pub async fn market_by_slug(&self, slug: &str) -> Result<Market> {
        if slug.is_empty()
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            bail!("slug deve conter somente letras ASCII, números e hífens");
        }
        let url = format!("{GAMMA_BASE}/markets/slug/{slug}");
        let response = self
            .http
            .get(url)
            .send()
            .await
            .context("falha ao consultar Gamma")?;
        let status = response.status();
        let body: Value = response
            .error_for_status()
            .with_context(|| format!("Gamma retornou HTTP {status}"))?
            .json()
            .await
            .context("resposta Gamma não é JSON válido")?;
        parse_market(&body).context("metadados Gamma incompletos ou incompatíveis")
    }

    pub async fn order_book(&self, asset_id: &str) -> Result<OrderBook> {
        let response = self
            .http
            .get(format!("{CLOB_BASE}/book"))
            .query(&[("token_id", asset_id)])
            .send()
            .await
            .context("falha ao consultar book público CLOB")?;
        let status = response.status();
        let body: Value = response
            .error_for_status()
            .with_context(|| format!("CLOB book retornou HTTP {status}"))?
            .json()
            .await
            .context("resposta CLOB book não é JSON válido")?;
        parse_rest_book(&body).context("book CLOB inválido")
    }

    pub async fn price_history(
        &self,
        asset_id: &str,
        bucket_seconds: u64,
    ) -> Result<Vec<PriceHistoryPoint>> {
        if !(60..=86_400).contains(&bucket_seconds) {
            bail!("bucket_seconds precisa estar entre 60 e 86400");
        }
        let response = self
            .http
            .get(format!("{DATA_BASE}/v2/prices-history"))
            .query(&[
                ("token_id", asset_id),
                ("interval", "1d"),
                ("bucket_seconds", &bucket_seconds.to_string()),
                ("limit", "10000"),
            ])
            .send()
            .await
            .context("falha ao consultar histórico público Data API")?;
        let status = response.status();
        let body: Value = response
            .error_for_status()
            .with_context(|| format!("Data API prices-history retornou HTTP {status}"))?
            .json()
            .await
            .context("resposta Data API history não é JSON válido")?;
        let points = body
            .get("data")
            .and_then(Value::as_array)
            .context("histórico sem array data")?;
        let mut output = Vec::with_capacity(points.len());
        for point in points {
            let timestamp_seconds = integer_field(point, "timestamp")?;
            let timestamp_ms = timestamp_seconds
                .checked_mul(1000)
                .context("timestamp de histórico excede limite")?;
            let price = decimal_field(point, "price")?;
            let resolution_seconds = integer_field(point, "resolution_seconds")? as u64;
            if timestamp_ms > 0 && price > Decimal::ZERO && price < Decimal::ONE {
                output.push(PriceHistoryPoint {
                    timestamp_ms,
                    price,
                    resolution_seconds,
                });
            }
        }
        output.sort_by_key(|point| point.timestamp_ms);
        output.dedup_by_key(|point| point.timestamp_ms);
        Ok(output)
    }
}

fn parse_market(body: &Value) -> Result<Market> {
    let id = string_field(body, "id")?;
    let slug = string_field(body, "slug")?;
    let question = string_field(body, "question")?;
    let condition_id = string_field(body, "conditionId")?;
    let version = string_field(body, "version")?.to_ascii_lowercase();
    if version != "v1" && version != "v2" {
        bail!("versão de mercado Polymarket não suportada: {version}");
    }
    let outcomes = string_array_field(body, "outcomes")?;
    let identifier_field = if version == "v2" {
        "positionIds"
    } else {
        "clobTokenIds"
    };
    let asset_ids = string_array_field(body, identifier_field)?;
    if outcomes.len() != 2 || asset_ids.len() != 2 {
        bail!("esperados exatamente dois outcomes binários e dois IDs compatíveis com a versão");
    }
    let mut yes_asset_id = None;
    let mut no_asset_id = None;
    for (label, asset_id) in outcomes.iter().zip(asset_ids) {
        match label.trim().to_ascii_lowercase().as_str() {
            "yes" => yes_asset_id = Some(asset_id),
            "no" => no_asset_id = Some(asset_id),
            _ => bail!("outcome não binário ou rótulo desconhecido: {label}"),
        }
    }
    let fee_schedule = body.get("feeSchedule").unwrap_or(&Value::Null);
    let fees = FeeSchedule {
        enabled: body.get("feesEnabled").and_then(Value::as_bool),
        rate: optional_decimal_field(fee_schedule, "rate"),
        exponent: optional_u32_field(fee_schedule, "exponent"),
        taker_only: fee_schedule.get("takerOnly").and_then(Value::as_bool),
        rebate_rate: optional_decimal_field(fee_schedule, "rebateRate"),
    };
    Ok(Market {
        id,
        slug,
        question,
        condition_id,
        version,
        active: body.get("active").and_then(Value::as_bool),
        closed: body.get("closed").and_then(Value::as_bool),
        enable_order_book: body.get("enableOrderBook").and_then(Value::as_bool),
        accepting_orders: body.get("acceptingOrders").and_then(Value::as_bool),
        min_tick_size: optional_decimal_field(body, "orderPriceMinTickSize"),
        min_order_size: optional_decimal_field(body, "orderMinSize"),
        fees,
        yes_asset_id: yes_asset_id.context("outcome Yes ausente")?,
        no_asset_id: no_asset_id.context("outcome No ausente")?,
    })
}

fn parse_rest_book(body: &Value) -> Result<OrderBook> {
    let asset_id = string_field(body, "asset_id")?;
    let timestamp_ms = integer_field(body, "timestamp")?;
    let mut state = RawBookState::new(&asset_id);
    state.market = body
        .get("market")
        .and_then(Value::as_str)
        .map(str::to_owned);
    state.timestamp_ms = timestamp_ms;
    state.hash = body.get("hash").and_then(Value::as_str).map(str::to_owned);
    RawBookState::replace_side(&mut state.bids, body.get("bids"))?;
    RawBookState::replace_side(&mut state.asks, body.get("asks"))?;
    let book = state.snapshot(
        optional_decimal_field(body, "min_order_size"),
        optional_decimal_field(body, "tick_size"),
        optional_decimal_field(body, "last_trade_price"),
    );
    if !book.is_valid() {
        bail!("book vazio, cruzado ou com preços fora de (0,1)");
    }
    Ok(book)
}

fn string_array_field(body: &Value, name: &str) -> Result<Vec<String>> {
    let value = body
        .get(name)
        .with_context(|| format!("campo {name} ausente"))?;
    let parsed = match value {
        Value::String(serialized) => serde_json::from_str::<Value>(serialized)
            .with_context(|| format!("campo {name} não contém JSON-array válido"))?,
        other => other.clone(),
    };
    parsed
        .as_array()
        .with_context(|| format!("campo {name} não é array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .context("item do array não é string")
        })
        .collect()
}

fn string_field(body: &Value, name: &str) -> Result<String> {
    body.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .with_context(|| format!("campo string {name} ausente/vazio"))
}

fn integer_field(body: &Value, name: &str) -> Result<i64> {
    let value = body
        .get(name)
        .with_context(|| format!("campo {name} ausente"))?;
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    value
        .as_str()
        .context("timestamp/inteiro não é número nem string")?
        .parse::<i64>()
        .with_context(|| format!("campo {name} inválido"))
}

fn decimal_field(body: &Value, name: &str) -> Result<Decimal> {
    body.get(name)
        .and_then(decimal_from_value)
        .with_context(|| format!("campo decimal {name} ausente/inválido"))
}

fn optional_decimal_field(body: &Value, name: &str) -> Option<Decimal> {
    body.get(name).and_then(decimal_from_value)
}

fn optional_u32_field(body: &Value, name: &str) -> Option<u32> {
    body.get(name).and_then(|value| {
        value
            .as_u64()
            .and_then(|number| u32::try_from(number).ok())
            .or_else(|| value.as_str().and_then(|number| number.parse::<u32>().ok()))
    })
}

fn decimal_from_value(value: &Value) -> Option<Decimal> {
    match value {
        Value::String(text) => Decimal::from_str(text).ok(),
        Value::Number(number) => Decimal::from_str(&number.to_string()).ok(),
        _ => None,
    }
}

fn update_level(levels: &mut BTreeMap<Decimal, Decimal>, price: Decimal, size: Decimal) {
    if size == Decimal::ZERO {
        levels.remove(&price);
    } else {
        levels.insert(price, size);
    }
}

fn apply_ws_value(
    value: &Value,
    target_asset_id: &str,
    book_state: &mut RawBookState,
    min_order_size: Option<Decimal>,
    tick_size: Option<Decimal>,
) -> Result<Vec<ParsedFeedMessage>> {
    let mut messages = Vec::new();
    if let Some(items) = value.as_array() {
        for item in items {
            messages.extend(apply_ws_value(
                item,
                target_asset_id,
                book_state,
                min_order_size,
                tick_size,
            )?);
        }
        return Ok(messages);
    }
    let event_type = value
        .get("event_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match event_type {
        "book" => {
            if value.get("asset_id").and_then(Value::as_str) != Some(target_asset_id) {
                return Ok(messages);
            }
            RawBookState::replace_side(&mut book_state.bids, value.get("bids"))?;
            RawBookState::replace_side(&mut book_state.asks, value.get("asks"))?;
            book_state.market = value
                .get("market")
                .and_then(Value::as_str)
                .map(str::to_owned);
            book_state.timestamp_ms = integer_field(value, "timestamp")?;
            book_state.hash = value.get("hash").and_then(Value::as_str).map(str::to_owned);
            let book = book_state.snapshot(min_order_size, tick_size, None);
            if book.is_valid() {
                messages.push(ParsedFeedMessage::Book {
                    book,
                    full_snapshot: true,
                });
            }
        }
        "price_change" => {
            let changes = value
                .get("price_changes")
                .and_then(Value::as_array)
                .context("price_change sem price_changes")?;
            let mut relevant = false;
            for change in changes {
                if change.get("asset_id").and_then(Value::as_str) != Some(target_asset_id) {
                    continue;
                }
                relevant = true;
                let price = decimal_field(change, "price")?;
                let size = decimal_field(change, "size")?;
                if price <= Decimal::ZERO || price >= Decimal::ONE || size < Decimal::ZERO {
                    bail!("delta de book fora do domínio válido");
                }
                match change.get("side").and_then(Value::as_str) {
                    Some("BUY") => update_level(&mut book_state.bids, price, size),
                    Some("SELL") => update_level(&mut book_state.asks, price, size),
                    _ => bail!("lado do delta de book desconhecido"),
                }
                if let Some(hash) = change.get("hash").and_then(Value::as_str) {
                    book_state.hash = Some(hash.to_owned());
                }
            }
            if relevant {
                book_state.timestamp_ms = integer_field(value, "timestamp")?;
                let book = book_state.snapshot(min_order_size, tick_size, None);
                if book.is_valid() {
                    messages.push(ParsedFeedMessage::Book {
                        book,
                        full_snapshot: false,
                    });
                }
            }
        }
        "last_trade_price" => {
            if value.get("asset_id").and_then(Value::as_str) != Some(target_asset_id) {
                return Ok(messages);
            }
            let tick = TradeTick {
                asset_id: target_asset_id.to_owned(),
                timestamp_ms: integer_field(value, "timestamp")?,
                price: decimal_field(value, "price")?,
                size: decimal_field(value, "size")?,
                side: value.get("side").and_then(Value::as_str).map(str::to_owned),
            };
            if tick.timestamp_ms > 0
                && tick.price > Decimal::ZERO
                && tick.price < Decimal::ONE
                && tick.size >= Decimal::ZERO
            {
                messages.push(ParsedFeedMessage::Trade(tick));
            }
        }
        // Lifecycle, tick-size and best_bid_ask events do not alter the depth used for paper fills.
        _ => {}
    }
    Ok(messages)
}

#[derive(Debug, Clone, PartialEq)]
enum ParsedFeedMessage {
    Book {
        book: OrderBook,
        full_snapshot: bool,
    },
    Trade(TradeTick),
}

/// Public, unauthenticated market channel. There is deliberately no user channel,
/// signer, private key, order endpoint, or order submission in this crate.
pub async fn run_market_stream(
    asset_id: String,
    min_order_size: Option<Decimal>,
    tick_size: Option<Decimal>,
    sender: mpsc::Sender<MarketFeedEvent>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut delay_seconds = 1_u64;
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let mut socket = match connect_async(MARKET_WS_URL).await {
            Ok((socket, _)) => socket,
            Err(error) => {
                send_event(
                    &sender,
                    MarketFeedEvent::Disconnected {
                        reason: error.to_string(),
                    },
                )
                .await?;
                wait_or_shutdown(&mut shutdown, delay_seconds).await?;
                delay_seconds = (delay_seconds * 2).min(30);
                continue;
            }
        };
        let subscription = serde_json::json!({
            "assets_ids": [asset_id],
            "type": "market",
            "initial_dump": true,
            "custom_feature_enabled": false
        });
        if let Err(error) = socket
            .send(Message::Text(subscription.to_string().into()))
            .await
        {
            send_event(
                &sender,
                MarketFeedEvent::Disconnected {
                    reason: error.to_string(),
                },
            )
            .await?;
            wait_or_shutdown(&mut shutdown, delay_seconds).await?;
            delay_seconds = (delay_seconds * 2).min(30);
            continue;
        }
        send_event(&sender, MarketFeedEvent::Connected).await?;
        delay_seconds = 1;
        let mut book_state = RawBookState::new(&asset_id);
        let mut heartbeat = interval_at(
            Instant::now() + Duration::from_secs(10),
            Duration::from_secs(10),
        );
        let mut disconnect_reason = "WebSocket encerrado".to_owned();
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                }
                _ = heartbeat.tick() => {
                    if let Err(error) = socket.send(Message::Text("PING".into())).await {
                        disconnect_reason = error.to_string();
                        break;
                    }
                }
                incoming = socket.next() => {
                    match incoming {
                        Some(Ok(Message::Text(text))) => {
                            if text.as_str() == "PONG" { continue; }
                            let value: Value = match serde_json::from_str(text.as_str()) {
                                Ok(value) => value,
                                Err(_) => continue,
                            };
                            let parsed = match apply_ws_value(&value, &asset_id, &mut book_state, min_order_size, tick_size) {
                                Ok(parsed) => parsed,
                                Err(error) => {
                                    disconnect_reason = format!("evento WebSocket inválido: {error:#}");
                                    break;
                                }
                            };
                            let received_at_ms = chrono_millis();
                            for message in parsed {
                                let event = match message {
                                    ParsedFeedMessage::Book { book, full_snapshot } => MarketFeedEvent::Book { book, received_at_ms, full_snapshot },
                                    ParsedFeedMessage::Trade(tick) => MarketFeedEvent::Trade { tick, received_at_ms },
                                };
                                send_event(&sender, event).await?;
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            if let Err(error) = socket.send(Message::Pong(payload)).await {
                                disconnect_reason = error.to_string();
                                break;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            disconnect_reason = frame.map(|frame| frame.reason.to_string()).unwrap_or_else(|| "close frame".to_owned());
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => { disconnect_reason = error.to_string(); break; }
                        None => break,
                    }
                }
            }
        }
        send_event(
            &sender,
            MarketFeedEvent::Disconnected {
                reason: disconnect_reason,
            },
        )
        .await?;
        wait_or_shutdown(&mut shutdown, delay_seconds).await?;
        delay_seconds = (delay_seconds * 2).min(30);
    }
}

async fn send_event(sender: &mpsc::Sender<MarketFeedEvent>, event: MarketFeedEvent) -> Result<()> {
    sender
        .send(event)
        .await
        .context("consumidor do monitor Polymarket foi encerrado")
}

async fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>, seconds: u64) -> Result<()> {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => Ok(()),
        _ = shutdown.changed() => Ok(()),
    }
}

fn chrono_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const V1_MARKET: &str = r#"{
      "id":"1","slug":"btc-above","question":"Will BTC rise?","conditionId":"0xabc",
      "version":"v1","outcomes":"[\"Yes\",\"No\"]",
      "clobTokenIds":"[\"yes-token\",\"no-token\"]","active":true,"closed":false,
      "enableOrderBook":true,"acceptingOrders":true,"orderPriceMinTickSize":0.001,"orderMinSize":5,
      "feesEnabled":true,"feeSchedule":{"rate":0.04,"exponent":1,"takerOnly":true,"rebateRate":0.25}
    }"#;

    #[test]
    fn gamma_market_parser_uses_outcome_labels_and_version_specific_ids() {
        let market = parse_market(&serde_json::from_str(V1_MARKET).unwrap()).unwrap();
        assert_eq!(market.asset_id(Outcome::Yes), "yes-token");
        assert_eq!(market.asset_id(Outcome::No), "no-token");
        assert_eq!(market.fees.rate, Some(Decimal::new(4, 2)));
        assert_eq!(market.fees.exponent, Some(1));

        let v2 = json!({
            "id":"2","slug":"btc-v2","question":"BTC?","conditionId":"0xdef",
            "version":"v2","outcomes":["No","Yes"],
            "positionIds":["no-position","yes-position"],"clobTokenIds":["legacy-no","legacy-yes"]
        });
        let market = parse_market(&v2).unwrap();
        assert_eq!(market.asset_id(Outcome::Yes), "yes-position");
        assert_eq!(market.asset_id(Outcome::No), "no-position");
    }

    #[test]
    fn unsupported_or_misaligned_market_ids_fail_closed() {
        let unsupported =
            json!({"id":"1","slug":"x","question":"x","conditionId":"c","version":"v3"});
        assert!(parse_market(&unsupported).is_err());
        let mut malformed: Value = serde_json::from_str(V1_MARKET).unwrap();
        malformed["positionIds"] = json!(["v2-only"]);
        assert!(parse_market(&malformed).is_ok());
        malformed["version"] = json!("v2");
        assert!(parse_market(&malformed).is_err());
    }

    #[test]
    fn book_snapshot_and_level_delta_preserve_aggregate_size_semantics() {
        let mut state = RawBookState::new("yes-token");
        let snapshot = json!({
            "event_type":"book","asset_id":"yes-token","market":"0xabc","timestamp":"1000","hash":"h0",
            "bids":[{"price":"0.40","size":"10"},{"price":"0.45","size":"5"}],
            "asks":[{"price":"0.55","size":"6"},{"price":"0.60","size":"8"}]
        });
        let messages = apply_ws_value(&snapshot, "yes-token", &mut state, None, None).unwrap();
        let ParsedFeedMessage::Book {
            book,
            full_snapshot,
        } = &messages[0]
        else {
            panic!("book snapshot expected")
        };
        assert!(*full_snapshot);
        assert_eq!(book.best_bid().unwrap().price, Decimal::new(45, 2));
        assert_eq!(book.best_ask().unwrap().price, Decimal::new(55, 2));

        let delta = json!({"event_type":"price_change","timestamp":"1100","price_changes":[
            {"asset_id":"yes-token","price":"0.45","size":"0","side":"BUY","hash":"h1"},
            {"asset_id":"yes-token","price":"0.50","size":"9","side":"BUY","hash":"h2"}
        ]});
        let messages = apply_ws_value(&delta, "yes-token", &mut state, None, None).unwrap();
        let ParsedFeedMessage::Book {
            book,
            full_snapshot,
        } = &messages[0]
        else {
            panic!("book delta expected")
        };
        assert!(!full_snapshot);
        assert_eq!(book.best_bid().unwrap().price, Decimal::new(50, 2));
        assert_eq!(book.bids.len(), 2);
    }

    #[test]
    fn trade_parser_filters_other_assets_and_invalid_prices() {
        let mut state = RawBookState::new("yes-token");
        let trade = json!({"event_type":"last_trade_price","asset_id":"no-token","timestamp":"1234","price":"0.5","size":"3","side":"BUY"});
        assert!(
            apply_ws_value(&trade, "yes-token", &mut state, None, None)
                .unwrap()
                .is_empty()
        );
        let trade = json!({"event_type":"last_trade_price","asset_id":"yes-token","timestamp":"1234","price":"0.5","size":"3","side":"BUY"});
        let parsed = apply_ws_value(&trade, "yes-token", &mut state, None, None).unwrap();
        assert!(
            matches!(&parsed[0], ParsedFeedMessage::Trade(tick) if tick.price == Decimal::new(5,1))
        );
    }

    #[test]
    fn rest_book_sorts_levels_and_rejects_crossed_or_empty_books() {
        let body = json!({
            "asset_id":"yes-token","timestamp":"1000","hash":"h",
            "bids":[{"price":"0.40","size":"10"},{"price":"0.45","size":"5"}],
            "asks":[{"price":"0.60","size":"8"},{"price":"0.55","size":"6"}],
            "min_order_size":"5","tick_size":"0.01","last_trade_price":"0.5"
        });
        let book = parse_rest_book(&body).unwrap();
        assert_eq!(book.best_bid().unwrap().price, Decimal::new(45, 2));
        assert_eq!(book.best_ask().unwrap().price, Decimal::new(55, 2));
        let crossed = json!({
            "asset_id":"yes-token","timestamp":"1000","bids":[{"price":"0.6","size":"1"}],
            "asks":[{"price":"0.5","size":"1"}]
        });
        assert!(parse_rest_book(&crossed).is_err());
    }
}
