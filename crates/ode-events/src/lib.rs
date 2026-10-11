use chrono::Utc;
use ode_domain::MarketId;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub event_id: Uuid,
    pub occurred_at: String,
    pub event_type: String,
    pub market_id: MarketId,
    pub details: Value,
}

impl AuditEvent {
    pub fn new(
        event_type: impl Into<String>,
        market_id: impl Into<MarketId>,
        details: Value,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            occurred_at: Utc::now().to_rfc3339(),
            event_type: event_type.into(),
            market_id: market_id.into(),
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audit_events_have_unique_ids_and_structured_details() {
        let a = AuditEvent::new("signal", "BTCUSDT", json!({"edge_bps": 5}));
        let b = AuditEvent::new("signal", "BTCUSDT", json!({"edge_bps": 5}));
        assert_ne!(a.event_id, b.event_id);
        assert_eq!(a.market_id.0, "BTCUSDT");
        assert_eq!(a.details["edge_bps"], 5);
    }
}
