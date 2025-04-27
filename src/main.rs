mod tui;

use crossbeam::channel::{bounded, Receiver, Sender};
use rand::Rng;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::time;

#[derive(Debug, Clone)]
struct Order {
    side: Side,
    quantity: i64,
    price: i64,
    decimals: i32,
    timestamp: u128,
}

impl Order {
    fn new(side: Side, quantity: i64, price: i64, decimals: i32) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        Self {
            side,
            quantity,
            price,
            decimals,
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

#[derive(Debug, Clone, PartialEq)]
enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Error)]
enum OrderError {
    #[error("Invalid order quantity")]
    InvalidQuantity,
    #[error("Invalid order price")]
    InvalidPrice,
    #[error("Channel send error")]
    ChannelError,
}

struct OrderBook {
    asks: Arc<Mutex<BinaryHeap<std::cmp::Reverse<Order>>>>,
    bids: Arc<Mutex<BinaryHeap<Order>>>,
    matched: Arc<Mutex<Vec<(Order, Order)>>>,
    order_sender: Sender<Order>,
    order_receiver: Receiver<Order>,
    shutdown: broadcast::Sender<()>,
}

impl OrderBook {
    fn new() -> Self {
        let (sender, receiver) = bounded(100);
        let (shutdown_sender, _) = broadcast::channel(1);
        OrderBook {
            asks: Arc::new(Mutex::new(BinaryHeap::new())),
            bids: Arc::new(Mutex::new(BinaryHeap::new())),
            matched: Arc::new(Mutex::new(Vec::new())),
            order_sender: sender,
            order_receiver: receiver,
            shutdown: shutdown_sender,
        }
    }

    fn submit_order(&self, order: Order) -> Result<(), OrderError> {
        if order.quantity <= 0 {
            return Err(OrderError::InvalidQuantity);
        }
        if order.price <= 0 {
            return Err(OrderError::InvalidPrice);
        }

        self.order_sender
            .send(order)
            .map_err(|_| OrderError::ChannelError)
    }

    fn get_asks(&self) -> Vec<Order> {
        let mut asks = self
            .asks
            .lock()
            .expect("Failed to lock asks")
            .iter()
            .map(|rev| rev.0.clone())
            .collect::<Vec<_>>();
        asks.sort_by(|a, b| {
            a.price
                .cmp(&b.price)
                .then_with(|| a.timestamp.cmp(&b.timestamp))
        });
        asks
    }

    fn get_bids(&self) -> Vec<Order> {
        let mut bids = self
            .bids
            .lock()
            .expect("Failed to lock bids")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        bids.sort_by(|a, b| {
            b.price
                .cmp(&a.price)
                .then_with(|| a.timestamp.cmp(&b.timestamp))
        });
        bids
    }

    fn get_matched_orders(&self) -> Vec<(Order, Order)> {
        self.matched.lock().expect("Failed to lock matched").clone()
    }

    fn start_matching_engine(&self) {
        let asks = Arc::clone(&self.asks);
        let bids = Arc::clone(&self.bids);
        let matched = Arc::clone(&self.matched);
        let receiver = self.order_receiver.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        std::thread::spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            if let Ok(order) = receiver.recv_timeout(Duration::from_millis(100)) {
                match order.side {
                    Side::Buy => {
                        let can_match = {
                            let asks_guard = asks.lock().expect("Failed to lock asks");
                            asks_guard
                                .peek()
                                .map(|Reverse(ask)| ask.price <= order.price)
                                .unwrap_or(false)
                        };

                        if can_match {
                            let mut asks_guard = asks.lock().expect("Failed to lock asks");
                            let mut matched_guard = matched.lock().expect("Failed to lock matched");
                            let Reverse(matched_ask) = asks_guard.pop().unwrap();
                            matched_guard.push((order.clone(), matched_ask));
                        } else {
                            let mut bids_guard = bids.lock().expect("Failed to lock bids");
                            bids_guard.push(order);
                        }
                    }
                    Side::Sell => {
                        let can_match = {
                            let bids_guard = bids.lock().expect("Failed to lock bids");
                            bids_guard
                                .peek()
                                .map(|bid| bid.price >= order.price)
                                .unwrap_or(false)
                        };

                        if can_match {
                            let mut bids_guard = bids.lock().expect("Failed to lock bids");
                            let mut matched_guard = matched.lock().expect("Failed to lock matched");
                            let matched_bid = bids_guard.pop().unwrap();
                            matched_guard.push((matched_bid, order.clone()));
                        } else {
                            let mut asks_guard = asks.lock().expect("Failed to lock asks");
                            asks_guard.push(Reverse(order));
                        }
                    }
                }
            }
        });
    }
}

fn generate_random_order() -> Order {
    let mut rng = rand::thread_rng();
    let price = rng.gen_range(9000..=11000);
    let quantity = rng.gen_range(1..=100);

    Order::new(
        if rng.gen_bool(0.5) {
            Side::Buy
        } else {
            Side::Sell
        },
        quantity,
        price,
        2,
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let orderbook = Arc::new(OrderBook::new());
    orderbook.start_matching_engine();

    let mut tui = tui::Tui::new()?;

    let orderbook_clone = Arc::clone(&orderbook);
    let mut shutdown_rx = orderbook.shutdown.subscribe();

    let order_generator = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let order = generate_random_order();
                    if let Err(e) = orderbook_clone.submit_order(order) {
                        eprintln!("Failed to submit order: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });

    let mut interval = time::interval(Duration::from_millis(100));

    loop {
        if tui::Tui::should_quit()? {
            println!("\nShutting down...");
            break;
        }

        interval.tick().await;
        if let Err(e) = tui.draw(&orderbook) {
            eprintln!("Failed to draw TUI: {}", e);
            break;
        }
    }

    // Cleanup in specific order
    if let Err(e) = tui.shutdown() {
        eprintln!("Error during TUI shutdown: {}", e);
    }

    let _ = orderbook.shutdown.send(());
    let _ = order_generator.await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn create_order(side: Side, price: i64, quantity: i64) -> Order {
        Order::new(side, quantity, price, 2)
    }

    #[test]
    fn test_order_creation() {
        let order = create_order(Side::Buy, 10000, 50);
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.price, 10000);
        assert_eq!(order.quantity, 50);
        assert_eq!(order.decimals, 2);
        assert!(order.timestamp > 0);
    }

    #[test]
    fn test_order_comparison() {
        let order1 = create_order(Side::Buy, 10000, 50);
        thread::sleep(Duration::from_millis(1));
        let order2 = create_order(Side::Buy, 10000, 50);

        // Same price, different timestamp
        assert!(order1 > order2); // Earlier timestamp should be higher priority

        let order3 = create_order(Side::Buy, 10100, 50);
        assert!(order3 > order1); // Higher price should be higher priority
    }

    #[test]
    fn test_invalid_orders() {
        let orderbook = OrderBook::new();

        // Test invalid quantity
        let invalid_qty = create_order(Side::Buy, 10000, 0);
        assert!(matches!(
            orderbook.submit_order(invalid_qty),
            Err(OrderError::InvalidQuantity)
        ));

        // Test invalid price
        let invalid_price = create_order(Side::Buy, 0, 50);
        assert!(matches!(
            orderbook.submit_order(invalid_price),
            Err(OrderError::InvalidPrice)
        ));
    }

    #[test]
    fn test_order_matching_buy() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Add sell order first
        let sell_order = create_order(Side::Sell, 10000, 50);
        orderbook.submit_order(sell_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        // Add matching buy order
        let buy_order = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(buy_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0.price, buy_order.price);
        assert_eq!(matched[0].1.price, sell_order.price);
    }

    #[test]
    fn test_order_matching_sell() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Add buy order first
        let buy_order = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(buy_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        // Add matching sell order
        let sell_order = create_order(Side::Sell, 10000, 50);
        orderbook.submit_order(sell_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0.price, buy_order.price);
        assert_eq!(matched[0].1.price, sell_order.price);
    }

    #[test]
    fn test_order_book_sorting() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Add multiple buy orders
        let buy_orders = vec![
            create_order(Side::Buy, 10000, 50),
            create_order(Side::Buy, 10200, 50),
            create_order(Side::Buy, 10100, 50),
        ];

        // Add multiple sell orders
        let sell_orders = vec![
            create_order(Side::Sell, 10300, 50),
            create_order(Side::Sell, 10500, 50),
            create_order(Side::Sell, 10400, 50),
        ];

        // Submit all orders
        for order in buy_orders.iter().chain(sell_orders.iter()) {
            orderbook.submit_order(order.clone()).unwrap();
            thread::sleep(Duration::from_millis(50));
        }

        // Check bid sorting (highest to lowest)
        let bids = orderbook.get_bids();
        assert!(!bids.is_empty());
        for i in 0..bids.len() - 1 {
            assert!(bids[i].price >= bids[i + 1].price);
        }

        // Check ask sorting (lowest to highest)
        let asks = orderbook.get_asks();
        assert!(!asks.is_empty());
        for i in 0..asks.len() - 1 {
            assert!(asks[i].price <= asks[i + 1].price);
        }
    }

    #[test]
    fn test_price_time_priority() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Create orders with same price but different timestamps
        let order1 = create_order(Side::Buy, 10000, 50);
        thread::sleep(Duration::from_millis(10));
        let order2 = create_order(Side::Buy, 10000, 50);

        orderbook.submit_order(order2.clone()).unwrap();
        orderbook.submit_order(order1.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 2);
        assert!(
            bids[0].timestamp < bids[1].timestamp,
            "First order should have earlier timestamp. First: {}, Second: {}",
            bids[0].timestamp,
            bids[1].timestamp
        );
    }

    #[test]
    fn test_no_match_conditions() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Add buy order lower than sell price
        let buy_order = create_order(Side::Buy, 10000, 50);
        let sell_order = create_order(Side::Sell, 10100, 50);

        orderbook.submit_order(buy_order).unwrap();
        orderbook.submit_order(sell_order).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 0);

        let bids = orderbook.get_bids();
        let asks = orderbook.get_asks();
        assert_eq!(bids.len(), 1);
        assert_eq!(asks.len(), 1);
    }

    #[test]
    fn test_shutdown() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Submit an order
        let order = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(order).unwrap();
        thread::sleep(Duration::from_millis(100)); // Wait for order processing

        // Trigger shutdown
        let _ = orderbook.shutdown.send(());
        thread::sleep(Duration::from_millis(200));

        // Try to submit another order after shutdown
        let order2 = create_order(Side::Buy, 10100, 50);
        orderbook.submit_order(order2).unwrap();
        thread::sleep(Duration::from_millis(100));

        // Verify only first order is there
        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].price, 10000);
    }
}
