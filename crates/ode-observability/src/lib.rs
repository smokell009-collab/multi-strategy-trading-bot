use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use ode_domain::PaperPortfolioState;
use ode_events::AuditEvent;
use ode_model_adapter::{OpenAiCompatibleAdapter, PublicMarketContext};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

const PAPER_STATE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("falha de sistema de arquivos: {0}")]
    Io(#[from] std::io::Error),
    #[error("falha SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("falha JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("snapshot paper inválido: {0}")]
    InvalidPaperState(String),
    #[error("token do dashboard ausente ou inseguro")]
    InvalidToken,
    #[error("dashboard aceita somente bind em endereço loopback")]
    NonLoopbackBind,
}

#[derive(Debug, Clone)]
pub struct AuditStore {
    path: PathBuf,
}

impl AuditStore {
    pub fn open(path: impl AsRef<FsPath>) -> Result<Self, ObservabilityError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { path };
        let connection = store.connection()?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS audit_events (
                 event_id TEXT PRIMARY KEY NOT NULL,
                 occurred_at TEXT NOT NULL,
                 event_type TEXT NOT NULL,
                 market_id TEXT NOT NULL,
                 details_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_events_time ON audit_events(occurred_at DESC);
             CREATE INDEX IF NOT EXISTS idx_events_type_market ON audit_events(event_type, market_id);
             CREATE TABLE IF NOT EXISTS bot_status (
                 singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                 updated_at TEXT NOT NULL,
                 payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS paper_portfolio_state (
                 account_key TEXT PRIMARY KEY NOT NULL,
                 schema_version INTEGER NOT NULL,
                 updated_at_ms INTEGER NOT NULL,
                 payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS alerts (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 alert_key TEXT NOT NULL UNIQUE,
                 category TEXT NOT NULL,
                 severity TEXT NOT NULL,
                 title TEXT NOT NULL,
                 market_id TEXT NOT NULL,
                 details_json TEXT NOT NULL,
                 first_seen_at TEXT NOT NULL,
                 last_seen_at TEXT NOT NULL,
                 occurrence_count INTEGER NOT NULL DEFAULT 1,
                 status TEXT NOT NULL DEFAULT 'active',
                 acknowledged_at TEXT,
                 resolved_at TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_alerts_status_time ON alerts(status, last_seen_at DESC);",
        )?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, rusqlite::Error> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
        Ok(connection)
    }

    pub fn append_event(&self, event: &AuditEvent) -> Result<(), ObservabilityError> {
        let details_json = serde_json::to_string(&event.details)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO audit_events(event_id, occurred_at, event_type, market_id, details_json)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                event.event_id.to_string(),
                event.occurred_at,
                event.event_type,
                event.market_id.0,
                details_json
            ],
        )?;
        if let Some((category, severity, title)) = alert_rule(&event.event_type) {
            self.raise_alert(
                &format!("{category}:{}", event.market_id.0),
                category,
                severity,
                title,
                &event.market_id.0,
                event.details.clone(),
            )?;
        }
        Ok(())
    }

    pub fn raise_alert(
        &self,
        alert_key: &str,
        category: &str,
        severity: &str,
        title: &str,
        market_id: &str,
        details: Value,
    ) -> Result<(), ObservabilityError> {
        let now = Utc::now().to_rfc3339();
        let details_json = serde_json::to_string(&details)?;
        self.connection()?.execute(
            "INSERT INTO alerts(alert_key, category, severity, title, market_id, details_json,
                                first_seen_at, last_seen_at, occurrence_count, status)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, 'active')
             ON CONFLICT(alert_key) DO UPDATE SET
                severity=excluded.severity,
                title=excluded.title,
                details_json=excluded.details_json,
                last_seen_at=excluded.last_seen_at,
                occurrence_count=alerts.occurrence_count + 1,
                status='active', acknowledged_at=NULL, resolved_at=NULL",
            params![
                alert_key,
                category,
                severity,
                title,
                market_id,
                details_json,
                now
            ],
        )?;
        Ok(())
    }

    pub fn update_status(&self, status: &BotStatus) -> Result<(), ObservabilityError> {
        let payload = serde_json::to_string(status)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO bot_status(singleton, updated_at, payload_json) VALUES(1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET updated_at=excluded.updated_at,
                                                  payload_json=excluded.payload_json",
            params![status.updated_at, payload],
        )?;
        if status.feed_state == "healthy" {
            connection.execute(
                "UPDATE alerts SET status='resolved', resolved_at=?1
                 WHERE category='market_data' AND status='active'",
                params![status.updated_at],
            )?;
        }
        Ok(())
    }

    pub fn get_status(&self) -> Result<Option<BotStatus>, ObservabilityError> {
        let connection = self.connection()?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload_json FROM bot_status WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).map_err(ObservabilityError::from))
            .transpose()
    }

    pub fn load_paper_state(
        &self,
        account_key: &str,
    ) -> Result<Option<PaperPortfolioState>, ObservabilityError> {
        let connection = self.connection()?;
        let stored: Option<(i64, String)> = connection
            .query_row(
                "SELECT schema_version, payload_json FROM paper_portfolio_state WHERE account_key=?1",
                [account_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((stored_version, payload)) = stored else {
            return Ok(None);
        };
        let state: PaperPortfolioState = serde_json::from_str(&payload)?;
        validate_paper_state(&state)?;
        if stored_version != i64::from(state.schema_version) || state.account_key != account_key {
            return Err(ObservabilityError::InvalidPaperState(
                "account key/schema no banco não corresponde ao payload".into(),
            ));
        }
        Ok(Some(state))
    }

    pub fn save_paper_state(&self, state: &PaperPortfolioState) -> Result<(), ObservabilityError> {
        validate_paper_state(state)?;
        let payload = serde_json::to_string(state)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO paper_portfolio_state(account_key, schema_version, updated_at_ms, payload_json)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(account_key) DO UPDATE SET
                schema_version=excluded.schema_version,
                updated_at_ms=excluded.updated_at_ms,
                payload_json=excluded.payload_json",
            params![state.account_key, state.schema_version, state.updated_at_ms, payload],
        )?;
        Ok(())
    }

    /// Commit audit event, alert and paper state together so restart cannot split a fill from its snapshot.
    pub fn commit_paper_event(
        &self,
        event: &AuditEvent,
        state: &PaperPortfolioState,
    ) -> Result<(), ObservabilityError> {
        validate_paper_state(state)?;
        let event_details = serde_json::to_string(&event.details)?;
        let state_payload = serde_json::to_string(state)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO audit_events(event_id, occurred_at, event_type, market_id, details_json)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                event.event_id.to_string(),
                event.occurred_at,
                event.event_type,
                event.market_id.0,
                event_details
            ],
        )?;
        if let Some((category, severity, title)) = alert_rule(&event.event_type) {
            upsert_alert(
                &transaction,
                &format!("{category}:{}", event.market_id.0),
                category,
                severity,
                title,
                &event.market_id.0,
                event.details.clone(),
            )?;
        }
        transaction.execute(
            "INSERT INTO paper_portfolio_state(account_key, schema_version, updated_at_ms, payload_json)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(account_key) DO UPDATE SET
                schema_version=excluded.schema_version,
                updated_at_ms=excluded.updated_at_ms,
                payload_json=excluded.payload_json",
            params![state.account_key, state.schema_version, state.updated_at_ms, state_payload],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn query_events(
        &self,
        limit: usize,
        event_type: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<StoredAuditEvent>, ObservabilityError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT event_id, occurred_at, event_type, market_id, details_json
             FROM audit_events
             WHERE (?1 IS NULL OR event_type=?1) AND (?2 IS NULL OR occurred_at>=?2)
             ORDER BY occurred_at DESC, event_id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![event_type, since, limit.clamp(1, 500) as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (event_id, occurred_at, event_type, market_id, details) = row?;
            Ok(StoredAuditEvent {
                event_id,
                occurred_at,
                event_type,
                market_id,
                details: serde_json::from_str(&details)?,
            })
        })
        .collect()
    }

    pub fn query_alerts(
        &self,
        limit: usize,
        active_only: bool,
    ) -> Result<Vec<StoredAlert>, ObservabilityError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, alert_key, category, severity, title, market_id, details_json,
                    first_seen_at, last_seen_at, occurrence_count, status, acknowledged_at, resolved_at
             FROM alerts WHERE (?1=0 OR status='active') ORDER BY last_seen_at DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                if active_only { 1_i64 } else { 0_i64 },
                limit.clamp(1, 500) as i64
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (
                id,
                alert_key,
                category,
                severity,
                title,
                market_id,
                details,
                first_seen_at,
                last_seen_at,
                occurrence_count,
                status,
                acknowledged_at,
                resolved_at,
            ) = row?;
            Ok(StoredAlert {
                id,
                alert_key,
                category,
                severity,
                title,
                market_id,
                details: serde_json::from_str(&details)?,
                first_seen_at,
                last_seen_at,
                occurrence_count,
                status,
                acknowledged_at,
                resolved_at,
            })
        })
        .collect()
    }

    pub fn acknowledge_alert(&self, id: i64) -> Result<bool, ObservabilityError> {
        let now = Utc::now().to_rfc3339();
        let changed = self.connection()?.execute(
            "UPDATE alerts SET acknowledged_at=?1 WHERE id=?2 AND status='active' AND acknowledged_at IS NULL",
            params![now, id],
        )?;
        Ok(changed > 0)
    }
}

fn validate_paper_state(state: &PaperPortfolioState) -> Result<(), ObservabilityError> {
    let invalid = |reason: &str| ObservabilityError::InvalidPaperState(reason.to_owned());
    if state.schema_version != PAPER_STATE_SCHEMA_VERSION {
        return Err(invalid("versão do snapshot não suportada"));
    }
    if state.account_key.trim().is_empty()
        || state.symbol.trim().is_empty()
        || state.interval.trim().is_empty()
        || state.starting_balance <= rust_decimal::Decimal::ZERO
        || state.cash < rust_decimal::Decimal::ZERO
        || state.quantity < rust_decimal::Decimal::ZERO
        || state.entry_fee < rust_decimal::Decimal::ZERO
        || state.session_start_equity <= rust_decimal::Decimal::ZERO
        || state.entry_fee_bps > 1_000
        || state.position_slippage_bps > 1_000
        || state.updated_at_ms < 0
        || !matches!(state.last_signal.as_str(), "Buy" | "Sell" | "Hold")
    {
        return Err(invalid("campos fora dos limites esperados"));
    }
    if chrono::NaiveDate::parse_from_str(&state.session_date_utc, "%Y-%m-%d").is_err() {
        return Err(invalid("data de sessão UTC inválida"));
    }
    if state
        .last_mark_price
        .is_some_and(|price| price <= rust_decimal::Decimal::ZERO)
    {
        return Err(invalid("preço de marcação deve ser positivo"));
    }
    if state
        .last_processed_close_ms
        .is_some_and(|timestamp| timestamp < 0)
    {
        return Err(invalid("cursor de candle inválido"));
    }
    let has_position = state.quantity > rust_decimal::Decimal::ZERO;
    if has_position != (state.entry_price.is_some() && state.stop_price.is_some())
        || state
            .entry_price
            .is_some_and(|price| price <= rust_decimal::Decimal::ZERO)
        || state
            .stop_price
            .is_some_and(|price| price <= rust_decimal::Decimal::ZERO)
    {
        return Err(invalid("campos da posição aberta estão inconsistentes"));
    }
    if !has_position
        && (state.entry_fee != rust_decimal::Decimal::ZERO
            || state.entry_fee_bps != 0
            || state.position_slippage_bps != 0)
    {
        return Err(invalid("custos de posição existem sem posição aberta"));
    }
    Ok(())
}

fn upsert_alert(
    connection: &Connection,
    alert_key: &str,
    category: &str,
    severity: &str,
    title: &str,
    market_id: &str,
    details: Value,
) -> Result<(), ObservabilityError> {
    let now = Utc::now().to_rfc3339();
    let details_json = serde_json::to_string(&details)?;
    connection.execute(
        "INSERT INTO alerts(alert_key, category, severity, title, market_id, details_json,
                            first_seen_at, last_seen_at, occurrence_count, status)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1, 'active')
         ON CONFLICT(alert_key) DO UPDATE SET
            severity=excluded.severity,
            title=excluded.title,
            details_json=excluded.details_json,
            last_seen_at=excluded.last_seen_at,
            occurrence_count=alerts.occurrence_count + 1,
            status='active', acknowledged_at=NULL, resolved_at=NULL",
        params![
            alert_key,
            category,
            severity,
            title,
            market_id,
            details_json,
            now
        ],
    )?;
    Ok(())
}

fn alert_rule(event_type: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match event_type {
        "risk_rejected" => Some(("risk", "warning", "Entrada bloqueada pelo motor de risco")),
        "stale_market_data" => Some(("market_data", "warning", "Feed de mercado atrasado")),
        "market_data_error" => Some(("market_data", "warning", "Falha ao consultar o feed")),
        "kill_switch_active" => Some(("safety", "critical", "Kill-switch de emergência ativo")),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotStatus {
    pub mode: String,
    pub updated_at: String,
    pub symbol: String,
    pub interval: String,
    pub feed_state: String,
    pub last_price: Option<String>,
    pub last_candle_close_ms: Option<i64>,
    pub last_signal: String,
    pub equity: String,
    pub cash: String,
    pub quantity: String,
    pub has_position: bool,
    pub kill_switch_active: bool,
    pub latency_p50_ms: Option<i64>,
    pub latency_p95_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAuditEvent {
    pub event_id: String,
    pub occurred_at: String,
    pub event_type: String,
    pub market_id: String,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAlert {
    pub id: i64,
    pub alert_key: String,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub market_id: String,
    pub details: Value,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub occurrence_count: i64,
    pub status: String,
    pub acknowledged_at: Option<String>,
    pub resolved_at: Option<String>,
}

#[derive(Clone)]
struct AppState {
    store: AuditStore,
    token: Arc<str>,
    advisor: Option<Arc<OpenAiCompatibleAdapter>>,
    last_advisory: Arc<Mutex<Option<Instant>>>,
}

#[derive(Debug, Error)]
pub enum DashboardError {
    #[error(transparent)]
    Store(#[from] ObservabilityError),
    #[error("falha ao abrir listener local: {0}")]
    Bind(#[from] std::io::Error),
}

pub async fn spawn_dashboard(
    address: SocketAddr,
    store: AuditStore,
    token: String,
    advisor: Option<Arc<OpenAiCompatibleAdapter>>,
) -> Result<JoinHandle<()>, DashboardError> {
    if !address.ip().is_loopback() {
        return Err(ObservabilityError::NonLoopbackBind.into());
    }
    if token.len() < 32
        || token.len() > 4_096
        || !token
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || b"-._~+/=".contains(&character))
    {
        return Err(ObservabilityError::InvalidToken.into());
    }
    let listener = TcpListener::bind(address).await?;
    let state = AppState {
        store,
        token: Arc::from(token),
        advisor,
        last_advisory: Arc::new(Mutex::new(None)),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/api/status", get(api_status))
        .route("/api/events", get(api_events))
        .route("/api/alerts", get(api_alerts))
        .route("/api/alerts/{id}/ack", post(api_ack_alert))
        .route("/api/capabilities", get(api_capabilities))
        .route("/api/advisory", post(api_advisory))
        .layer(middleware::from_fn(security_headers))
        .with_state(state);
    let handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(%error, "servidor local do dashboard encerrou com erro");
        }
    });
    Ok(handle)
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    );
    response
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn app_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("static/app.js"),
    )
}

async fn style_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("static/style.css"),
    )
}

fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), StatusCode> {
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if supplied.len() == expected.len()
        && bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()))
    {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn store_status(error: ObservabilityError) -> StatusCode {
    tracing::error!(%error, "consulta persistente do dashboard falhou");
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn api_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<BotStatus>>, StatusCode> {
    authorize(&headers, &state.token)?;
    state.store.get_status().map(Json).map_err(store_status)
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
    event_type: Option<String>,
    since: Option<String>,
}

async fn api_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<StoredAuditEvent>>, StatusCode> {
    authorize(&headers, &state.token)?;
    if query
        .event_type
        .as_ref()
        .is_some_and(|value| value.len() > 80)
        || query.since.as_ref().is_some_and(|value| value.len() > 64)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .store
        .query_events(
            query.limit.unwrap_or(50),
            query.event_type.as_deref(),
            query.since.as_deref(),
        )
        .map(Json)
        .map_err(store_status)
}

#[derive(Debug, Deserialize)]
struct AlertsQuery {
    limit: Option<usize>,
    active_only: Option<bool>,
}

async fn api_alerts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AlertsQuery>,
) -> Result<Json<Vec<StoredAlert>>, StatusCode> {
    authorize(&headers, &state.token)?;
    state
        .store
        .query_alerts(
            query.limit.unwrap_or(50),
            query.active_only.unwrap_or(false),
        )
        .map(Json)
        .map_err(store_status)
}

async fn api_ack_alert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, StatusCode> {
    authorize(&headers, &state.token)?;
    let changed = state.store.acknowledge_alert(id).map_err(store_status)?;
    if changed {
        Ok(Json(serde_json::json!({"acknowledged": true})))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Serialize)]
struct Capabilities {
    ai_advisory_enabled: bool,
}

async fn api_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Capabilities>, StatusCode> {
    authorize(&headers, &state.token)?;
    Ok(Json(Capabilities {
        ai_advisory_enabled: state.advisor.is_some(),
    }))
}

#[derive(Serialize)]
struct AdvisoryResponse {
    model: String,
    text: String,
}

async fn api_advisory(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdvisoryResponse>, StatusCode> {
    authorize(&headers, &state.token)?;
    let advisor = state.advisor.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    {
        let mut last = state.last_advisory.lock().await;
        if last.is_some_and(|instant| instant.elapsed() < Duration::from_secs(30)) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        *last = Some(Instant::now());
    }
    let status = state
        .store
        .get_status()
        .map_err(store_status)?
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    if status.feed_state != "healthy" {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let updated_at = DateTime::parse_from_rfc3339(&status.updated_at)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .with_timezone(&Utc);
    let snapshot_age = Utc::now().signed_duration_since(updated_at);
    if snapshot_age < ChronoDuration::zero() || snapshot_age > ChronoDuration::seconds(60) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let context = PublicMarketContext {
        market: status.symbol.clone(),
        interval: status.interval.clone(),
        last_price: status.last_price.ok_or(StatusCode::SERVICE_UNAVAILABLE)?,
        last_signal: status.last_signal,
        feed_state: status.feed_state,
        candle_close_time_ms: status.last_candle_close_ms,
    };
    let comment = advisor.analyze(&context).await.map_err(|error| {
        tracing::warn!(%error, "provedor de comentário read-only falhou");
        StatusCode::BAD_GATEWAY
    })?;
    let audit = AuditEvent::new(
        "ai_advisory_generated",
        context.market.as_str(),
        serde_json::json!({"provider": comment.provider, "model": comment.model, "text_chars": comment.text.chars().count()}),
    );
    state.store.append_event(&audit).map_err(store_status)?;
    Ok(Json(AdvisoryResponse {
        model: comment.model,
        text: comment.text,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ode_domain::PaperPortfolioState;
    use serde_json::json;

    fn store() -> (AuditStore, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let store = AuditStore::open(directory.path().join("audit.sqlite3")).unwrap();
        (store, directory)
    }

    fn paper_state() -> PaperPortfolioState {
        PaperPortfolioState {
            schema_version: PAPER_STATE_SCHEMA_VERSION,
            account_key: "paper:ema_crossover:BTCUSDT:1m".into(),
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            starting_balance: rust_decimal::Decimal::from(10_000),
            cash: rust_decimal::Decimal::from(7_400),
            quantity: rust_decimal::Decimal::new(25, 2),
            entry_price: Some(rust_decimal::Decimal::from(100_000)),
            stop_price: Some(rust_decimal::Decimal::from(98_000)),
            entry_fee: rust_decimal::Decimal::new(25, 1),
            entry_fee_bps: 10,
            position_slippage_bps: 5,
            session_start_equity: rust_decimal::Decimal::from(10_000),
            session_date_utc: "2026-10-11".into(),
            last_processed_close_ms: Some(1_791_690_000_000),
            last_mark_price: Some(rust_decimal::Decimal::from(100_000)),
            last_signal: "Buy".into(),
            updated_at_ms: 1_791_690_000_001,
        }
    }

    #[test]
    fn paper_state_survives_reopening_and_unknown_versions_fail_closed() {
        let (store, directory) = store();
        let state = paper_state();
        store.save_paper_state(&state).unwrap();
        drop(store);

        let reopened = AuditStore::open(directory.path().join("audit.sqlite3")).unwrap();
        assert_eq!(
            reopened
                .load_paper_state("paper:ema_crossover:BTCUSDT:1m")
                .unwrap(),
            Some(state.clone())
        );
        let mut unsupported = state;
        unsupported.schema_version += 1;
        assert!(matches!(
            reopened.save_paper_state(&unsupported),
            Err(ObservabilityError::InvalidPaperState(_))
        ));
    }

    #[test]
    fn paper_event_and_snapshot_commit_together() {
        let (store, _directory) = store();
        let mut state = paper_state();
        state.last_signal = "Hold".into();
        let event = AuditEvent::new("signal_observed", "BTCUSDT", json!({"signal":"Hold"}));
        store.commit_paper_event(&event, &state).unwrap();
        assert_eq!(
            store.load_paper_state(&state.account_key).unwrap(),
            Some(state)
        );
        assert_eq!(
            store
                .query_events(10, Some("signal_observed"), None)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn sqlite_store_persists_events_alerts_status_and_acknowledgements() {
        let (store, _directory) = store();
        let event = AuditEvent::new("risk_rejected", "BTCUSDT", json!({"reason":"stale"}));
        store.append_event(&event).unwrap();
        let events = store.query_events(10, Some("risk_rejected"), None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event.event_id.to_string());
        let alerts = store.query_alerts(10, true).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].occurrence_count, 1);
        assert!(store.acknowledge_alert(alerts[0].id).unwrap());
        assert!(!store.acknowledge_alert(alerts[0].id).unwrap());

        let status = BotStatus {
            mode: "paper".into(),
            updated_at: Utc::now().to_rfc3339(),
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            feed_state: "healthy".into(),
            last_price: Some("100".into()),
            last_candle_close_ms: Some(1),
            last_signal: "HOLD".into(),
            equity: "10000".into(),
            cash: "10000".into(),
            quantity: "0".into(),
            has_position: false,
            kill_switch_active: false,
            latency_p50_ms: Some(10),
            latency_p95_ms: Some(10),
        };
        store.update_status(&status).unwrap();
        assert_eq!(store.get_status().unwrap().unwrap().symbol, "BTCUSDT");
        let active_alerts = store.query_alerts(10, true).unwrap();
        assert_eq!(active_alerts.len(), 1);
        assert!(active_alerts[0].acknowledged_at.is_some());
    }

    #[test]
    fn repeated_alerts_are_deduplicated_and_counted() {
        let (store, _directory) = store();
        for _ in 0..3 {
            let event = AuditEvent::new("market_data_error", "BTCUSDT", json!({"error":"offline"}));
            store.append_event(&event).unwrap();
        }
        let alerts = store.query_alerts(10, true).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].occurrence_count, 3);
        let mut status = BotStatus {
            mode: "paper".into(),
            updated_at: Utc::now().to_rfc3339(),
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            feed_state: "healthy".into(),
            last_price: None,
            last_candle_close_ms: None,
            last_signal: "HOLD".into(),
            equity: "10000".into(),
            cash: "10000".into(),
            quantity: "0".into(),
            has_position: false,
            kill_switch_active: false,
            latency_p50_ms: None,
            latency_p95_ms: None,
        };
        status.updated_at = Utc::now().to_rfc3339();
        store.update_status(&status).unwrap();
        assert!(store.query_alerts(10, true).unwrap().is_empty());
    }

    #[test]
    fn bearer_auth_is_exact_and_required() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            authorize(&headers, "a-token-that-is-long-enough-for-test"),
            Err(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert_eq!(
            authorize(&headers, "a-token-that-is-long-enough-for-test"),
            Err(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer a-token-that-is-long-enough-for-test"),
        );
        assert_eq!(
            authorize(&headers, "a-token-that-is-long-enough-for-test"),
            Ok(())
        );
    }

    #[tokio::test]
    async fn dashboard_refuses_non_loopback_bind_and_weak_token() {
        let (store, _directory) = store();
        let token = "x".repeat(32);
        let public_address: SocketAddr = "0.0.0.0:0".parse().unwrap();
        assert!(matches!(
            spawn_dashboard(public_address, store.clone(), token, None).await,
            Err(DashboardError::Store(ObservabilityError::NonLoopbackBind))
        ));
        let local_address: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert!(matches!(
            spawn_dashboard(local_address, store, "short".to_owned(), None).await,
            Err(DashboardError::Store(ObservabilityError::InvalidToken))
        ));
    }

    #[tokio::test]
    async fn status_handler_requires_bearer_and_returns_persisted_snapshot() {
        let (store, _directory) = store();
        let status = BotStatus {
            mode: "paper".into(),
            updated_at: Utc::now().to_rfc3339(),
            symbol: "BTCUSDT".into(),
            interval: "1m".into(),
            feed_state: "healthy".into(),
            last_price: Some("100".into()),
            last_candle_close_ms: Some(1),
            last_signal: "HOLD".into(),
            equity: "10000".into(),
            cash: "10000".into(),
            quantity: "0".into(),
            has_position: false,
            kill_switch_active: false,
            latency_p50_ms: Some(5),
            latency_p95_ms: Some(9),
        };
        store.update_status(&status).unwrap();
        let token = "test-token-that-is-longer-than-thirty-two-characters";
        let state = AppState {
            store,
            token: Arc::from(token),
            advisor: None,
            last_advisory: Arc::new(Mutex::new(None)),
        };
        assert!(matches!(
            api_status(State(state.clone()), HeaderMap::new()).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let Json(returned) = api_status(State(state), headers).await.unwrap();
        assert_eq!(returned.unwrap().symbol, "BTCUSDT");
    }
}
