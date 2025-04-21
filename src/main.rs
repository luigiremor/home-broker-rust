mod tui;

use crossbeam::channel::{bounded, Receiver, Sender};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::time;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Order {
    id: String,
    side: Side,
    quantity: i64,
    price: i64,
    decimals: i32,
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
    asks: Arc<Mutex<Vec<Order>>>,
    bids: Arc<Mutex<Vec<Order>>>,
    matched: Arc<Mutex<Vec<(Order, Order)>>>,
    order_sender: Sender<Order>,
    order_receiver: Receiver<Order>,
}

impl OrderBook {
    fn new() -> Self {
        let (sender, receiver) = bounded(100);
        OrderBook {
            asks: Arc::new(Mutex::new(Vec::new())),
            bids: Arc::new(Mutex::new(Vec::new())),
            matched: Arc::new(Mutex::new(Vec::new())),
            order_sender: sender,
            order_receiver: receiver,
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
        self.asks.lock().expect("Failed to lock asks").clone()
    }

    fn get_bids(&self) -> Vec<Order> {
        self.bids.lock().expect("Failed to lock bids").clone()
    }

    fn get_matched_orders(&self) -> Vec<(Order, Order)> {
        self.matched.lock().expect("Failed to lock matched").clone()
    }

    fn start_matching_engine(&self) {
        let asks = Arc::clone(&self.asks);
        let bids = Arc::clone(&self.bids);
        let matched = Arc::clone(&self.matched);
        let receiver = self.order_receiver.clone();

        std::thread::spawn(move || {
            while let Ok(order) = receiver.recv() {
                match order.side {
                    Side::Buy => {
                        let mut asks_guard = asks.lock().expect("Failed to lock asks");
                        let mut bids_guard = bids.lock().expect("Failed to lock bids");
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        if let Some(ask_idx) =
                            asks_guard.iter().position(|ask| ask.price <= order.price)
                        {
                            let matched_ask = asks_guard.remove(ask_idx);
                            matched_guard.push((order.clone(), matched_ask));
                        } else {
                            bids_guard.push(order);
                        }
                    }
                    Side::Sell => {
                        let mut asks_guard = asks.lock().expect("Failed to lock asks");
                        let mut bids_guard = bids.lock().expect("Failed to lock bids");
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        if let Some(bid_idx) =
                            bids_guard.iter().position(|bid| bid.price >= order.price)
                        {
                            let matched_bid = bids_guard.remove(bid_idx);
                            matched_guard.push((matched_bid, order.clone()));
                        } else {
                            asks_guard.push(order);
                        }
                    }
                }
            }
        });
    }
}

fn generate_random_order() -> Order {
    let mut rng = rand::thread_rng();

    // Generate valid price between $90.00 and $110.00 with 2 decimal places
    let price = rng.gen_range(9000..=11000);

    // Generate valid quantity between 1 and 100
    let quantity = rng.gen_range(1..=100);

    Order {
        id: Uuid::new_v4().to_string(),
        side: if rng.gen_bool(0.5) {
            Side::Buy
        } else {
            Side::Sell
        },
        quantity,
        price,
        decimals: 2,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let orderbook = Arc::new(OrderBook::new());
    orderbook.start_matching_engine();

    // Create TUI
    let mut tui = tui::Tui::new()?;

    // Spawn order generator thread
    let orderbook_clone = Arc::clone(&orderbook);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let order = generate_random_order();

            if let Err(e) = orderbook_clone.submit_order(order) {
                eprintln!("Failed to submit order: {}", e);
            }
        }
    });

    // Main loop for TUI updates
    let mut interval = time::interval(Duration::from_millis(100));
    loop {
        interval.tick().await;
        tui.draw(&orderbook)?;
    }
}
