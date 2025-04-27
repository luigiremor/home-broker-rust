use super::Side;
use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Order {
    pub side: Side,
    pub price: i64,
    pub decimals: i32,
    pub original_qty: i64,
    pub remaining: i64,
    pub timestamp: u128,
}

impl Order {
    pub fn new(side: Side, quantity: i64, price: i64, decimals: i32) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .expect("Failed to get timestamp");

        Self {
            side,
            price,
            decimals,
            original_qty: quantity,
            remaining: quantity,
            timestamp,
        }
    }
}

impl PartialEq for Order {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price && self.timestamp == other.timestamp
    }
}

impl Eq for Order {}

impl PartialOrd for Order {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Order {
    fn cmp(&self, other: &Self) -> Ordering {
        self.price
            .cmp(&other.price)
            .then_with(|| other.timestamp.cmp(&self.timestamp))
    }
}
