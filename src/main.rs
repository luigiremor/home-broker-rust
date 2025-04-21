use crossbeam::channel::{bounded, Receiver, Sender};
use std::sync::{Arc, Mutex};
use thiserror::Error;
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
    order_sender: Sender<Order>,
    order_receiver: Receiver<Order>,
}

impl OrderBook {
    fn new() -> Self {
        let (sender, receiver) = bounded(100);
        OrderBook {
            asks: Arc::new(Mutex::new(Vec::new())),
            bids: Arc::new(Mutex::new(Vec::new())),
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

    fn start_matching_engine(&self) {
        let asks = Arc::clone(&self.asks);
        let bids = Arc::clone(&self.bids);
        let receiver = self.order_receiver.clone();

        std::thread::spawn(move || {
            while let Ok(order) = receiver.recv() {
                match order.side {
                    Side::Buy => {
                        let mut asks_guard = asks.lock().expect("Failed to lock asks");
                        let mut bids_guard = bids.lock().expect("Failed to lock bids");

                        // Try to match with existing sell orders
                        if let Some(ask_idx) =
                            asks_guard.iter().position(|ask| ask.price <= order.price)
                        {
                            asks_guard.remove(ask_idx);
                        } else {
                            bids_guard.push(order);
                        }
                    }
                    Side::Sell => {
                        let mut asks_guard = asks.lock().expect("Failed to lock asks");
                        let mut bids_guard = bids.lock().expect("Failed to lock bids");

                        // Try to match with existing buy orders
                        if let Some(bid_idx) =
                            bids_guard.iter().position(|bid| bid.price >= order.price)
                        {
                            bids_guard.remove(bid_idx);
                        } else {
                            asks_guard.push(order);
                        }
                    }
                }
            }
        });
    }
}

#[tokio::main]
async fn main() {
    let orderbook = OrderBook::new();
    orderbook.start_matching_engine();

    // Example usage
    let buy_order = Order {
        id: Uuid::new_v4().to_string(),
        side: Side::Buy,
        quantity: 100,
        price: 1000,
        decimals: 2,
    };

    let sell_order = Order {
        id: Uuid::new_v4().to_string(),
        side: Side::Sell,
        quantity: 50,
        price: 990,
        decimals: 2,
    };

    if let Err(e) = orderbook.submit_order(buy_order) {
        eprintln!("Failed to submit buy order: {}", e);
    }

    if let Err(e) = orderbook.submit_order(sell_order) {
        eprintln!("Failed to submit sell order: {}", e);
    }

    // Keep the main thread running for a while to process orders
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
}
