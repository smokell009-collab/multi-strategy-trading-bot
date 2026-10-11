use hmac::{Hmac, Mac};
use ode_domain::{OrderSide, OrderStatus};
use reqwest::{Client, Method, header::HeaderValue, redirect::Policy};
use rust_decimal::Decimal;
use serde::{Deserialize, de::DeserializeOwned};
use sha2::Sha256;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use url::form_urlencoded;

type HmacSha256 = Hmac<Sha256>;

/// Host constante de teste; não existe configuração de host/ambiente em runtime.
pub const TESTNET_BASE_URL: &str = "https://testnet.binance.vision/api";
const RECV_WINDOW_MS: &str = "5000";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
pub enum BinanceTestnetError {
    #[error("credenciais Testnet ausentes ou em formato inválido")]
    InvalidCredentials,
    #[error("parâmetro inválido: {0}")]
    InvalidInput(String),
    #[error("falha de rede na Binance Spot Testnet; nenhum retry automático foi feito")]
    Transport,
    #[error("resposta HTTP {status} da Binance Testnet (código {code:?}): {message}")]
    Api {
        status: u16,
        code: Option<i64>,
        message: String,
    },
    #[error("resposta JSON inválida da Binance Spot Testnet")]
    Json(#[from] serde_json::Error),
    #[error("erro ao construir cliente HTTP seguro")]
    HttpClient,
    #[error("relógio local inválido")]
    InvalidClock,
    #[error("falha ao assinar requisição HMAC")]
    Signing,
    #[error("símbolo não encontrado na Spot Testnet: {0}")]
    SymbolNotFound(String),
    #[error("símbolo não está habilitado para Spot na Testnet: {0}")]
    SymbolNotTrading(String),
    #[error("filtro de ordem Testnet inválido: {0}")]
    InvalidFilter(String),
    #[error("quantidade viola filtros do símbolo: {0}")]
    FilterViolation(String),
}

pub struct BinanceSpotTestnetClient {
    http: Client,
    api_key: HeaderValue,
    api_secret: Vec<u8>,
    server_time_offset_ms: i64,
    base_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetBalance {
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountInfo {
    pub can_trade: bool,
    pub balances: Vec<AssetBalance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuantityFilter {
    pub min_quantity: Decimal,
    pub max_quantity: Decimal,
    pub step_size: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolRules {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub trading_allowed: bool,
    pub market_quantity: QuantityFilter,
    pub lot_quantity: QuantityFilter,
    pub min_notional: Option<Decimal>,
    pub min_notional_applies_to_market: bool,
    pub max_notional: Option<Decimal>,
    pub max_notional_applies_to_market: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinanceOrder {
    pub symbol: String,
    pub exchange_order_id: String,
    pub client_order_id: String,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub original_quantity: Decimal,
    pub executed_quantity: Decimal,
    pub executed_quote: Decimal,
}

#[derive(Debug, Deserialize)]
struct ServerTimeResponse {
    #[serde(rename = "serverTime")]
    server_time: i64,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    code: Option<i64>,
    #[serde(default)]
    msg: String,
}

#[derive(Debug, Deserialize)]
struct RawAccount {
    #[serde(rename = "canTrade")]
    can_trade: bool,
    balances: Vec<RawBalance>,
}

#[derive(Debug, Deserialize)]
struct RawBalance {
    asset: String,
    free: String,
    locked: String,
}

#[derive(Debug, Deserialize)]
struct RawExchangeInfo {
    symbols: Vec<RawSymbol>,
}

#[derive(Debug, Deserialize)]
struct RawSymbol {
    symbol: String,
    status: String,
    #[serde(rename = "baseAsset")]
    base_asset: String,
    #[serde(rename = "quoteAsset")]
    quote_asset: String,
    #[serde(rename = "isSpotTradingAllowed")]
    is_spot_trading_allowed: Option<bool>,
    filters: Vec<RawFilter>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFilter {
    filter_type: String,
    min_qty: Option<String>,
    max_qty: Option<String>,
    step_size: Option<String>,
    min_notional: Option<String>,
    max_notional: Option<String>,
    apply_to_market: Option<bool>,
    apply_min_to_market: Option<bool>,
    apply_max_to_market: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawAveragePrice {
    price: String,
}

#[derive(Debug, Deserialize)]
struct RawOrder {
    symbol: String,
    #[serde(rename = "orderId")]
    order_id: serde_json::Value,
    #[serde(rename = "clientOrderId")]
    client_order_id: String,
    side: String,
    status: String,
    #[serde(rename = "origQty")]
    original_quantity: String,
    #[serde(rename = "executedQty")]
    executed_quantity: String,
    #[serde(rename = "cummulativeQuoteQty")]
    executed_quote: String,
}

impl BinanceSpotTestnetClient {
    /// Cria cliente somente após sincronizar o relógio com o endpoint público da Testnet.
    pub async fn connect(
        api_key: impl AsRef<str>,
        api_secret: impl AsRef<str>,
    ) -> Result<Self, BinanceTestnetError> {
        Self::connect_at(api_key.as_ref(), api_secret.as_ref(), TESTNET_BASE_URL).await
    }

    async fn connect_at(
        api_key: &str,
        api_secret: &str,
        base_url: &str,
    ) -> Result<Self, BinanceTestnetError> {
        let mut client = Self::without_time_sync(api_key, api_secret, base_url)?;
        let before = now_ms()?;
        let response: ServerTimeResponse = client
            .request_json(Method::GET, "/v3/time", &[], false)
            .await?;
        let after = now_ms()?;
        let midpoint = before.saturating_add(after) / 2;
        client.server_time_offset_ms = response.server_time.saturating_sub(midpoint);
        Ok(client)
    }

    fn without_time_sync(
        api_key: &str,
        api_secret: &str,
        base_url: &str,
    ) -> Result<Self, BinanceTestnetError> {
        if api_key.trim().is_empty()
            || api_secret.len() < 16
            || api_key.chars().any(char::is_control)
            || api_secret.chars().any(char::is_control)
            || (!base_url.starts_with("https://") && !cfg!(test))
        {
            return Err(BinanceTestnetError::InvalidCredentials);
        }
        let api_key =
            HeaderValue::from_str(api_key).map_err(|_| BinanceTestnetError::InvalidCredentials)?;
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .https_only(!cfg!(test))
            .build()
            .map_err(|_| BinanceTestnetError::HttpClient)?;
        Ok(Self {
            http,
            api_key,
            api_secret: api_secret.as_bytes().to_vec(),
            server_time_offset_ms: 0,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    pub async fn account(&self) -> Result<AccountInfo, BinanceTestnetError> {
        let account: RawAccount = self
            .request_json(Method::GET, "/v3/account", &[], true)
            .await?;
        let balances = account
            .balances
            .into_iter()
            .map(|balance| {
                Ok(AssetBalance {
                    asset: balance.asset,
                    free: decimal(&balance.free, "balance.free")?,
                    locked: decimal(&balance.locked, "balance.locked")?,
                })
            })
            .collect::<Result<Vec<_>, BinanceTestnetError>>()?;
        Ok(AccountInfo {
            can_trade: account.can_trade,
            balances,
        })
    }

    pub async fn symbol_rules(&self, symbol: &str) -> Result<SymbolRules, BinanceTestnetError> {
        validate_symbol(symbol)?;
        let info: RawExchangeInfo = self
            .request_json(
                Method::GET,
                "/v3/exchangeInfo",
                &[("symbol".to_owned(), symbol.to_owned())],
                false,
            )
            .await?;
        let raw = info
            .symbols
            .into_iter()
            .find(|item| item.symbol == symbol)
            .ok_or_else(|| BinanceTestnetError::SymbolNotFound(symbol.to_owned()))?;
        SymbolRules::from_raw(raw)
    }

    pub async fn average_price(&self, symbol: &str) -> Result<Decimal, BinanceTestnetError> {
        validate_symbol(symbol)?;
        let response: RawAveragePrice = self
            .request_json(
                Method::GET,
                "/v3/avgPrice",
                &[("symbol".to_owned(), symbol.to_owned())],
                false,
            )
            .await?;
        decimal(&response.price, "avgPrice.price")
    }

    pub async fn place_market_order(
        &self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
        client_order_id: &str,
    ) -> Result<BinanceOrder, BinanceTestnetError> {
        validate_symbol(symbol)?;
        validate_client_order_id(client_order_id)?;
        if quantity <= Decimal::ZERO {
            return Err(BinanceTestnetError::InvalidInput(
                "quantity deve ser maior que zero".into(),
            ));
        }
        let params = vec![
            ("symbol".to_owned(), symbol.to_owned()),
            ("side".to_owned(), side_name(side).to_owned()),
            ("type".to_owned(), "MARKET".to_owned()),
            ("quantity".to_owned(), quantity.normalize().to_string()),
            ("newClientOrderId".to_owned(), client_order_id.to_owned()),
            ("newOrderRespType".to_owned(), "FULL".to_owned()),
        ];
        let raw: RawOrder = self
            .request_json(Method::POST, "/v3/order", &params, true)
            .await?;
        parse_order(raw)
    }

    pub async fn query_order(
        &self,
        symbol: &str,
        client_order_id: &str,
    ) -> Result<BinanceOrder, BinanceTestnetError> {
        validate_symbol(symbol)?;
        validate_client_order_id(client_order_id)?;
        let raw: RawOrder = self
            .request_json(
                Method::GET,
                "/v3/order",
                &[
                    ("symbol".to_owned(), symbol.to_owned()),
                    ("origClientOrderId".to_owned(), client_order_id.to_owned()),
                ],
                true,
            )
            .await?;
        parse_order(raw)
    }

    pub async fn cancel_order(
        &self,
        symbol: &str,
        client_order_id: &str,
    ) -> Result<BinanceOrder, BinanceTestnetError> {
        validate_symbol(symbol)?;
        validate_client_order_id(client_order_id)?;
        let raw: RawOrder = self
            .request_json(
                Method::DELETE,
                "/v3/order",
                &[
                    ("symbol".to_owned(), symbol.to_owned()),
                    ("origClientOrderId".to_owned(), client_order_id.to_owned()),
                ],
                true,
            )
            .await?;
        parse_order(raw)
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        input_params: &[(String, String)],
        signed: bool,
    ) -> Result<T, BinanceTestnetError> {
        let mut params: Vec<(String, String)> = input_params
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if signed {
            params.push(("recvWindow".to_owned(), RECV_WINDOW_MS.to_owned()));
            params.push((
                "timestamp".to_owned(),
                now_ms()?
                    .saturating_add(self.server_time_offset_ms)
                    .to_string(),
            ));
        }
        let query = encode_params(&params);
        let url = if signed {
            let signature = sign_payload(&self.api_secret, &query)?;
            format!("{}{path}?{query}&signature={signature}", self.base_url)
        } else if query.is_empty() {
            format!("{}{path}", self.base_url)
        } else {
            format!("{}{path}?{query}", self.base_url)
        };
        let mut request = self.http.request(method, url);
        if signed {
            request = request.header("X-MBX-APIKEY", self.api_key.clone());
        }
        let response = request
            .send()
            .await
            .map_err(|_| BinanceTestnetError::Transport)?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|_| BinanceTestnetError::Transport)?;
        if !status.is_success() {
            let parsed = serde_json::from_str::<ApiErrorResponse>(&body).ok();
            return Err(BinanceTestnetError::Api {
                status: status.as_u16(),
                code: parsed.as_ref().and_then(|error| error.code),
                message: parsed
                    .map(|error| error.msg)
                    .filter(|message| !message.is_empty())
                    .unwrap_or_else(|| "resposta de erro sem mensagem".to_owned()),
            });
        }
        serde_json::from_str(&body).map_err(BinanceTestnetError::Json)
    }
}

impl SymbolRules {
    fn from_raw(raw: RawSymbol) -> Result<Self, BinanceTestnetError> {
        let trading_allowed =
            raw.status == "TRADING" && raw.is_spot_trading_allowed.unwrap_or(true);
        let mut lot: Option<QuantityFilter> = None;
        let mut market_lot: Option<QuantityFilter> = None;
        let mut min_notional = None;
        let mut min_notional_applies_to_market = false;
        let mut max_notional = None;
        let mut max_notional_applies_to_market = false;
        for filter in raw.filters {
            match filter.filter_type.as_str() {
                "LOT_SIZE" => lot = Some(parse_quantity_filter(&filter)?),
                "MARKET_LOT_SIZE" => market_lot = Some(parse_quantity_filter(&filter)?),
                "MIN_NOTIONAL" => {
                    min_notional = filter
                        .min_notional
                        .as_deref()
                        .map(|value| decimal(value, "MIN_NOTIONAL.minNotional"))
                        .transpose()?;
                    min_notional_applies_to_market = filter.apply_to_market.unwrap_or(false);
                }
                "NOTIONAL" => {
                    min_notional = filter
                        .min_notional
                        .as_deref()
                        .map(|value| decimal(value, "NOTIONAL.minNotional"))
                        .transpose()?;
                    min_notional_applies_to_market = filter.apply_min_to_market.unwrap_or(false);
                    max_notional = filter
                        .max_notional
                        .as_deref()
                        .map(|value| decimal(value, "NOTIONAL.maxNotional"))
                        .transpose()?;
                    max_notional_applies_to_market = filter.apply_max_to_market.unwrap_or(false);
                }
                _ => {}
            }
        }
        let lot_quantity =
            lot.ok_or_else(|| BinanceTestnetError::InvalidFilter("LOT_SIZE ausente".to_owned()))?;
        let market_quantity = market_lot
            .filter(|filter| filter.step_size > Decimal::ZERO)
            .unwrap_or_else(|| lot_quantity.clone());
        let rules = Self {
            symbol: raw.symbol,
            base_asset: raw.base_asset,
            quote_asset: raw.quote_asset,
            trading_allowed,
            market_quantity,
            lot_quantity,
            min_notional,
            min_notional_applies_to_market,
            max_notional,
            max_notional_applies_to_market,
        };
        if !rules.trading_allowed {
            return Err(BinanceTestnetError::SymbolNotTrading(rules.symbol));
        }
        Ok(rules)
    }

    /// A validação é conservadora; a exchange continua sendo autoridade final para os filtros.
    pub fn validate_market_order(
        &self,
        quantity: Decimal,
        average_price: Decimal,
    ) -> Result<(), BinanceTestnetError> {
        if quantity <= Decimal::ZERO || average_price <= Decimal::ZERO {
            return Err(BinanceTestnetError::InvalidInput(
                "quantity e preço de referência devem ser positivos".to_owned(),
            ));
        }
        validate_quantity(quantity, &self.market_quantity, "MARKET_LOT_SIZE")?;
        validate_quantity(quantity, &self.lot_quantity, "LOT_SIZE")?;
        let notional = quantity * average_price;
        if self.min_notional_applies_to_market
            && self.min_notional.is_some_and(|minimum| notional < minimum)
        {
            return Err(BinanceTestnetError::FilterViolation(
                "notional abaixo do mínimo para market".to_owned(),
            ));
        }
        if self.max_notional_applies_to_market
            && self.max_notional.is_some_and(|maximum| notional > maximum)
        {
            return Err(BinanceTestnetError::FilterViolation(
                "notional acima do máximo para market".to_owned(),
            ));
        }
        Ok(())
    }
}

fn parse_quantity_filter(filter: &RawFilter) -> Result<QuantityFilter, BinanceTestnetError> {
    Ok(QuantityFilter {
        min_quantity: decimal(filter.min_qty.as_deref().unwrap_or("0"), "quantity.minQty")?,
        max_quantity: decimal(filter.max_qty.as_deref().unwrap_or("0"), "quantity.maxQty")?,
        step_size: decimal(
            filter.step_size.as_deref().unwrap_or("0"),
            "quantity.stepSize",
        )?,
    })
}

fn validate_quantity(
    quantity: Decimal,
    filter: &QuantityFilter,
    name: &str,
) -> Result<(), BinanceTestnetError> {
    if quantity < filter.min_quantity {
        return Err(BinanceTestnetError::FilterViolation(format!(
            "quantidade abaixo do mínimo {name}"
        )));
    }
    if filter.max_quantity > Decimal::ZERO && quantity > filter.max_quantity {
        return Err(BinanceTestnetError::FilterViolation(format!(
            "quantidade acima do máximo {name}"
        )));
    }
    if filter.step_size > Decimal::ZERO && quantity % filter.step_size != Decimal::ZERO {
        return Err(BinanceTestnetError::FilterViolation(format!(
            "quantidade não respeita stepSize de {name}"
        )));
    }
    Ok(())
}

fn parse_order(raw: RawOrder) -> Result<BinanceOrder, BinanceTestnetError> {
    let side = match raw.side.as_str() {
        "BUY" => OrderSide::Buy,
        "SELL" => OrderSide::Sell,
        _ => {
            return Err(BinanceTestnetError::InvalidInput(
                "side da ordem desconhecido".into(),
            ));
        }
    };
    let status = match raw.status.as_str() {
        "NEW" => OrderStatus::Open,
        "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
        "FILLED" => OrderStatus::Filled,
        "CANCELED" | "EXPIRED" | "EXPIRED_IN_MATCH" => OrderStatus::Cancelled,
        "REJECTED" => OrderStatus::Rejected,
        _ => OrderStatus::Unknown,
    };
    let exchange_order_id = match raw.order_id {
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        _ => return Err(BinanceTestnetError::InvalidInput("orderId ausente".into())),
    };
    Ok(BinanceOrder {
        symbol: raw.symbol,
        exchange_order_id,
        client_order_id: raw.client_order_id,
        side,
        status,
        original_quantity: decimal(&raw.original_quantity, "origQty")?,
        executed_quantity: decimal(&raw.executed_quantity, "executedQty")?,
        executed_quote: decimal(&raw.executed_quote, "cummulativeQuoteQty")?,
    })
}

fn decimal(value: &str, field: &str) -> Result<Decimal, BinanceTestnetError> {
    Decimal::from_str(value).map_err(|_| {
        BinanceTestnetError::InvalidInput(format!("valor decimal inválido em {field}"))
    })
}

fn encode_params(params: &[(String, String)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn sign_payload(secret: &[u8], payload: &str) -> Result<String, BinanceTestnetError> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| BinanceTestnetError::Signing)?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn side_name(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    }
}

fn validate_symbol(symbol: &str) -> Result<(), BinanceTestnetError> {
    if symbol.is_empty() || !symbol.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(BinanceTestnetError::InvalidInput(
            "symbol deve conter somente letras e números".to_owned(),
        ));
    }
    Ok(())
}

fn validate_client_order_id(id: &str) -> Result<(), BinanceTestnetError> {
    if id.is_empty()
        || id.len() > 36
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(BinanceTestnetError::InvalidInput(
            "clientOrderId fora do formato permitido".to_owned(),
        ));
    }
    Ok(())
}

fn now_ms() -> Result<i64, BinanceTestnetError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BinanceTestnetError::InvalidClock)?
        .as_millis();
    i64::try_from(millis).map_err(|_| BinanceTestnetError::InvalidClock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn hmac_sha256_matches_binance_documentation_vector() {
        let secret = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
        let payload = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let signature = sign_payload(secret.as_bytes(), payload).unwrap();
        assert_eq!(signature.len(), 64);
        assert_eq!(
            signature,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }

    #[test]
    fn symbol_filters_reject_bad_step_size_and_notional() {
        let rules = SymbolRules {
            symbol: "BTCUSDT".into(),
            base_asset: "BTC".into(),
            quote_asset: "USDT".into(),
            trading_allowed: true,
            market_quantity: QuantityFilter {
                min_quantity: Decimal::new(1, 5),
                max_quantity: Decimal::ONE,
                step_size: Decimal::new(1, 5),
            },
            lot_quantity: QuantityFilter {
                min_quantity: Decimal::new(1, 5),
                max_quantity: Decimal::ONE,
                step_size: Decimal::new(1, 5),
            },
            min_notional: Some(Decimal::from(10)),
            min_notional_applies_to_market: true,
            max_notional: None,
            max_notional_applies_to_market: false,
        };
        assert!(
            rules
                .validate_market_order(Decimal::new(1, 4), Decimal::from(100_000))
                .is_ok()
        );
        assert!(
            rules
                .validate_market_order(Decimal::new(15, 6), Decimal::from(100_000))
                .is_err()
        );
        assert!(
            rules
                .validate_market_order(Decimal::new(1, 5), Decimal::from(10_000))
                .is_err()
        );
    }

    #[test]
    fn order_statuses_map_without_treating_unknown_as_pending() {
        let filled = raw_order("FILLED");
        let unknown = raw_order("SOMETHING_NEW");
        assert_eq!(parse_order(filled).unwrap().status, OrderStatus::Filled);
        assert_eq!(parse_order(unknown).unwrap().status, OrderStatus::Unknown);
    }

    #[tokio::test]
    async fn signed_market_order_sends_header_and_reconciliable_client_id_to_mock() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 16_384];
            let size = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..size]).to_string();
            let body = r#"{"symbol":"BTCUSDT","orderId":123,"clientOrderId":"ode-test-123","side":"BUY","status":"FILLED","origQty":"0.001","executedQty":"0.001","cummulativeQuoteQty":"60"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        let client = BinanceSpotTestnetClient::without_time_sync(
            "test-api-key",
            "test-secret-long-enough",
            &format!("http://{address}"),
        )
        .unwrap();
        let order = client
            .place_market_order(
                "BTCUSDT",
                OrderSide::Buy,
                Decimal::new(1, 3),
                "ode-test-123",
            )
            .await
            .unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(order.client_order_id, "ode-test-123");
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v3/order?"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-mbx-apikey: test-api-key")
        );
        assert!(request.contains("newClientOrderId=ode-test-123"));
        assert!(request.contains("signature="));
        assert!(request.contains("quantity=0.001"));
    }

    fn raw_order(status: &str) -> RawOrder {
        serde_json::from_value(serde_json::json!({
            "symbol":"BTCUSDT", "orderId":123, "clientOrderId":"ode-test-123",
            "side":"BUY", "status":status, "origQty":"0.001",
            "executedQty":"0.001", "cummulativeQuoteQty":"60"
        }))
        .unwrap()
    }
}
