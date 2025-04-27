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
    price: i64,
    decimals: i32,
    original_qty: i64,
    remaining: i64,
    timestamp: u128,
}

impl Order {
    fn new(side: Side, quantity: i64, price: i64, decimals: i32) -> Self {
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

#[derive(Debug, Clone)]
struct Trade {
    price: i64,
    quantity: i64,
    buy_order: Order,
    sell_order: Order,
    timestamp: u128,
}

struct OrderBook {
    asks: Arc<Mutex<BinaryHeap<std::cmp::Reverse<Order>>>>,
    bids: Arc<Mutex<BinaryHeap<Order>>>,
    matched: Arc<Mutex<Vec<Trade>>>,
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
        if order.remaining <= 0 {
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

    fn get_matched_orders(&self) -> Vec<Trade> {
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

            if let Ok(mut incoming) = receiver.recv_timeout(Duration::from_millis(100)) {
                match incoming.side {
                    Side::Buy => {
                        let mut asks_guard = asks.lock().expect("Failed to lock asks");
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        while incoming.remaining > 0 {
                            match asks_guard.peek() {
                                Some(Reverse(ask)) if ask.price <= incoming.price => {
                                    let Reverse(mut best_ask) =
                                        asks_guard.pop().expect("Ask was just peeked");

                                    let trade_qty = incoming.remaining.min(best_ask.remaining);

                                    // Create trade at ask price (price-time priority)
                                    let trade = Trade {
                                        price: best_ask.price,
                                        quantity: trade_qty,
                                        buy_order: incoming.clone(),
                                        sell_order: best_ask.clone(),
                                        timestamp: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map(|d| d.as_millis())
                                            .expect("Failed to get timestamp"),
                                    };

                                    // Update quantities
                                    incoming.remaining -= trade_qty;
                                    best_ask.remaining -= trade_qty;

                                    // Record the trade
                                    matched_guard.push(trade);

                                    // If ask still has quantity, push it back
                                    if best_ask.remaining > 0 {
                                        asks_guard.push(Reverse(best_ask));
                                    }
                                }
                                _ => break, // No matching asks available
                            }
                        }

                        // If incoming order still has remaining quantity, add to bids
                        if incoming.remaining > 0 {
                            let mut bids_guard = bids.lock().expect("Failed to lock bids");
                            bids_guard.push(incoming);
                        }
                    }
                    Side::Sell => {
                        let mut bids_guard = bids.lock().expect("Failed to lock bids");
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        while incoming.remaining > 0 {
                            match bids_guard.peek() {
                                Some(bid) if bid.price >= incoming.price => {
                                    let mut best_bid =
                                        bids_guard.pop().expect("Bid was just peeked");

                                    let trade_qty = incoming.remaining.min(best_bid.remaining);

                                    // Create trade at bid price (price-time priority)
                                    let trade = Trade {
                                        price: best_bid.price,
                                        quantity: trade_qty,
                                        buy_order: best_bid.clone(),
                                        sell_order: incoming.clone(),
                                        timestamp: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map(|d| d.as_millis())
                                            .expect("Failed to get timestamp"),
                                    };

                                    // Update quantities
                                    incoming.remaining -= trade_qty;
                                    best_bid.remaining -= trade_qty;

                                    // Record the trade
                                    matched_guard.push(trade);

                                    // If bid still has quantity, push it back
                                    if best_bid.remaining > 0 {
                                        bids_guard.push(best_bid);
                                    }
                                }
                                _ => break, // No matching bids available
                            }
                        }

                        // If incoming order still has remaining quantity, add to asks
                        if incoming.remaining > 0 {
                            let mut asks_guard = asks.lock().expect("Failed to lock asks");
                            asks_guard.push(Reverse(incoming));
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
        assert_eq!(order.original_qty, 50);
        assert_eq!(order.remaining, 50);
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
        assert_eq!(matched[0].buy_order.price, buy_order.price);
        assert_eq!(matched[0].sell_order.price, sell_order.price);
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
        assert_eq!(matched[0].buy_order.price, buy_order.price);
        assert_eq!(matched[0].sell_order.price, sell_order.price);
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

    #[test]
    fn test_partial_fill() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        // Add sell order for 10 units at 100
        let sell_order = create_order(Side::Sell, 100, 10);
        orderbook
            .submit_order(sell_order.clone())
            .expect("Failed to submit sell order");
        thread::sleep(Duration::from_millis(100));

        // Add buy order for 15 units at 100
        let buy_order = create_order(Side::Buy, 100, 15);
        orderbook
            .submit_order(buy_order.clone())
            .expect("Failed to submit buy order");
        thread::sleep(Duration::from_millis(100));

        let trades = orderbook.get_matched_orders();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, 10); // Should match 10 units

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].remaining, 5); // Should have 5 units remaining
    }

    #[test]
    fn test_multiple_partial_fills() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        let sell_orders = vec![
            create_order(Side::Sell, 100, 5), // 5 units at 100
            create_order(Side::Sell, 101, 3), // 3 units at 101
            create_order(Side::Sell, 102, 7), // 7 units at 102
        ];

        for order in sell_orders {
            orderbook
                .submit_order(order)
                .expect("Failed to submit sell order");
            thread::sleep(Duration::from_millis(50));
        }

        let buy_order = create_order(Side::Buy, 102, 8);
        orderbook
            .submit_order(buy_order)
            .expect("Failed to submit buy order");
        thread::sleep(Duration::from_millis(100));

        let trades = orderbook.get_matched_orders();
        assert_eq!(trades.len(), 2);

        assert_eq!(trades[0].quantity, 5);
        assert_eq!(trades[0].price, 100);

        assert_eq!(trades[1].quantity, 3);
        assert_eq!(trades[1].price, 101);
    }
}
