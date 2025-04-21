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
    timestamp: u64,
}

impl Order {
    fn new(side: Side, quantity: i64, price: i64, decimals: i32) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
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
            .then_with(|| self.timestamp.cmp(&other.timestamp))
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
        self.asks
            .lock()
            .map(|heap| heap.iter().map(|ask| ask.0.clone()).collect())
            .unwrap_or_default()
    }

    fn get_bids(&self) -> Vec<Order> {
        self.bids
            .lock()
            .map(|heap| heap.iter().cloned().collect())
            .unwrap_or_default()
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
