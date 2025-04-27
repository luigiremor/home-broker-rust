use super::Order;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Trade {
    pub price: i64,
    pub quantity: i64,
    pub buy_order: Order,
    pub sell_order: Order,
    pub timestamp: u128,
}
