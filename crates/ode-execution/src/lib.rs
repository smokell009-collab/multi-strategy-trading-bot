use ode_domain::{MarketId, OrderSide, OrderStatus};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct OrderIntent {
    pub client_order_id: String,
    pub market_id: MarketId,
    pub side: OrderSide,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalOrder {
    pub intent: OrderIntent,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VenueOrder {
    pub client_order_id: String,
    pub market_id: MarketId,
    pub side: OrderSide,
    pub quantity: Decimal,
    pub status: OrderStatus,
    pub filled_quantity: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterOutcome {
    Inserted,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    EmptyClientOrderId,
    InvalidQuantity,
    IdempotencyConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationIssue {
    LocalOrderMissingAtVenue(String),
    VenueOrderUnknownLocally(String),
    DuplicateVenueOrderId(String),
    OrderFieldsMismatch(String),
    StatusMismatch(String),
    FilledQuantityMismatch(String),
}

#[derive(Debug, Default)]
pub struct OrderLedger {
    orders: BTreeMap<String, LocalOrder>,
}

impl OrderLedger {
    /// Repetir exatamente a mesma intenção é idempotente; reutilizar a chave com outro payload é erro.
    pub fn register_once(&mut self, intent: OrderIntent) -> Result<RegisterOutcome, RegisterError> {
        if intent.client_order_id.trim().is_empty() {
            return Err(RegisterError::EmptyClientOrderId);
        }
        if intent.quantity <= Decimal::ZERO {
            return Err(RegisterError::InvalidQuantity);
        }
        if let Some(existing) = self.orders.get(&intent.client_order_id) {
            return if existing.intent == intent {
                Ok(RegisterOutcome::Duplicate)
            } else {
                Err(RegisterError::IdempotencyConflict)
            };
        }
        self.orders.insert(
            intent.client_order_id.clone(),
            LocalOrder {
                intent,
                status: OrderStatus::Pending,
                filled_quantity: Decimal::ZERO,
            },
        );
        Ok(RegisterOutcome::Inserted)
    }

    pub fn update_local(&mut self, id: &str, status: OrderStatus, filled: Decimal) -> bool {
        let Some(order) = self.orders.get_mut(id) else {
            return false;
        };
        if filled < Decimal::ZERO || filled > order.intent.quantity {
            return false;
        }
        order.status = status;
        order.filled_quantity = filled;
        true
    }

    pub fn reconcile(&self, venue_orders: &[VenueOrder]) -> Vec<ReconciliationIssue> {
        let mut venue_by_id: BTreeMap<&str, &VenueOrder> = BTreeMap::new();
        let mut issues = Vec::new();
        for order in venue_orders {
            if venue_by_id
                .insert(order.client_order_id.as_str(), order)
                .is_some()
            {
                issues.push(ReconciliationIssue::DuplicateVenueOrderId(
                    order.client_order_id.clone(),
                ));
            }
        }
        let local_ids: BTreeSet<&str> = self.orders.keys().map(String::as_str).collect();

        for (id, local) in &self.orders {
            let Some(venue) = venue_by_id.get(id.as_str()) else {
                issues.push(ReconciliationIssue::LocalOrderMissingAtVenue(id.clone()));
                continue;
            };
            if venue.market_id != local.intent.market_id
                || venue.side != local.intent.side
                || venue.quantity != local.intent.quantity
            {
                issues.push(ReconciliationIssue::OrderFieldsMismatch(id.clone()));
            }
            if venue.status != local.status {
                issues.push(ReconciliationIssue::StatusMismatch(id.clone()));
            }
            if venue.filled_quantity != local.filled_quantity {
                issues.push(ReconciliationIssue::FilledQuantityMismatch(id.clone()));
            }
        }
        for venue in venue_orders {
            if !local_ids.contains(venue.client_order_id.as_str()) {
                issues.push(ReconciliationIssue::VenueOrderUnknownLocally(
                    venue.client_order_id.clone(),
                ));
            }
        }
        issues
    }

    pub fn len(&self) -> usize {
        self.orders.len()
    }
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(id: &str, quantity: i64) -> OrderIntent {
        OrderIntent {
            client_order_id: id.to_owned(),
            market_id: MarketId::from("BTCUSDT"),
            side: OrderSide::Buy,
            quantity: Decimal::from(quantity),
        }
    }

    #[test]
    fn duplicate_submission_is_idempotent_and_does_not_add_a_second_order() {
        let mut ledger = OrderLedger::default();
        assert_eq!(
            ledger.register_once(intent("ema-1", 2)),
            Ok(RegisterOutcome::Inserted)
        );
        assert_eq!(
            ledger.register_once(intent("ema-1", 2)),
            Ok(RegisterOutcome::Duplicate)
        );
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn reusing_idempotency_key_with_different_payload_is_rejected() {
        let mut ledger = OrderLedger::default();
        ledger.register_once(intent("ema-1", 2)).unwrap();
        assert_eq!(
            ledger.register_once(intent("ema-1", 3)),
            Err(RegisterError::IdempotencyConflict)
        );
    }

    #[test]
    fn reconciliation_detects_missing_unknown_and_divergent_orders() {
        let mut ledger = OrderLedger::default();
        ledger.register_once(intent("local-open", 2)).unwrap();
        ledger.register_once(intent("local-missing", 1)).unwrap();
        ledger.update_local("local-open", OrderStatus::Open, Decimal::ZERO);
        let venue = vec![
            VenueOrder {
                client_order_id: "local-open".into(),
                market_id: MarketId::from("ETHUSDT"),
                side: OrderSide::Sell,
                quantity: Decimal::ONE,
                status: OrderStatus::Filled,
                filled_quantity: Decimal::ONE,
            },
            VenueOrder {
                client_order_id: "external-order".into(),
                market_id: MarketId::from("ETHUSDT"),
                side: OrderSide::Sell,
                quantity: Decimal::ONE,
                status: OrderStatus::Filled,
                filled_quantity: Decimal::ONE,
            },
        ];
        let issues = ledger.reconcile(&venue);
        assert!(
            issues.contains(&ReconciliationIssue::LocalOrderMissingAtVenue(
                "local-missing".into()
            ))
        );
        assert!(
            issues.contains(&ReconciliationIssue::VenueOrderUnknownLocally(
                "external-order".into()
            ))
        );
        assert!(issues.contains(&ReconciliationIssue::OrderFieldsMismatch(
            "local-open".into()
        )));
        assert!(issues.contains(&ReconciliationIssue::StatusMismatch("local-open".into())));
        assert!(
            issues.contains(&ReconciliationIssue::FilledQuantityMismatch(
                "local-open".into()
            ))
        );
    }

    #[test]
    fn venue_status_and_fill_divergence_are_reported() {
        let mut ledger = OrderLedger::default();
        ledger.register_once(intent("order-1", 2)).unwrap();
        ledger.update_local("order-1", OrderStatus::Open, Decimal::ZERO);
        let venue = vec![VenueOrder {
            client_order_id: "order-1".into(),
            market_id: MarketId::from("BTCUSDT"),
            side: OrderSide::Buy,
            quantity: Decimal::from(2),
            status: OrderStatus::Filled,
            filled_quantity: Decimal::from(2),
        }];
        let issues = ledger.reconcile(&venue);
        assert!(issues.contains(&ReconciliationIssue::StatusMismatch("order-1".into())));
        assert!(
            issues.contains(&ReconciliationIssue::FilledQuantityMismatch(
                "order-1".into()
            ))
        );
    }

    #[test]
    fn duplicate_venue_ids_are_reported_as_anomalies() {
        let order = VenueOrder {
            client_order_id: "duplicate".into(),
            market_id: MarketId::from("BTCUSDT"),
            side: OrderSide::Buy,
            quantity: Decimal::ONE,
            status: OrderStatus::Open,
            filled_quantity: Decimal::ZERO,
        };
        let issues = OrderLedger::default().reconcile(&[order.clone(), order]);
        assert!(issues.contains(&ReconciliationIssue::DuplicateVenueOrderId(
            "duplicate".into()
        )));
        assert!(
            issues.contains(&ReconciliationIssue::VenueOrderUnknownLocally(
                "duplicate".into()
            ))
        );
    }
}
