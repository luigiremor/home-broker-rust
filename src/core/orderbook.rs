use crate::core::rwlock::RWLock;
use crossbeam::channel::{bounded, Receiver, Sender};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use super::errors::OrderError;
use crate::models::{Order, Side, Trade};

pub struct OrderBook {
    asks: Arc<RWLock<BinaryHeap<Reverse<Order>>>>,
    bids: Arc<RWLock<BinaryHeap<Order>>>,
    matched: Arc<Mutex<VecDeque<Trade>>>,
    order_sender: Sender<Order>,
    order_receiver: Receiver<Order>,
    pub shutdown: broadcast::Sender<()>,
}

impl OrderBook {
    pub fn new() -> Self {
        let (sender, receiver) = bounded(100);
        let (shutdown_sender, _) = broadcast::channel(1);
        OrderBook {
            asks: Arc::new(RWLock::new(BinaryHeap::new())),
            bids: Arc::new(RWLock::new(BinaryHeap::new())),
            matched: Arc::new(Mutex::new(VecDeque::new())),
            order_sender: sender,
            order_receiver: receiver,
            shutdown: shutdown_sender,
        }
    }

    pub fn submit_order(&self, order: Order) -> Result<(), OrderError> {
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

    pub fn get_asks(&self) -> Vec<Order> {
        let mut asks = self
            .asks
            .read_lock()
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

    pub fn get_bids(&self) -> Vec<Order> {
        let mut bids = self.bids.read_lock().iter().cloned().collect::<Vec<_>>();
        bids.sort_by(|a, b| {
            b.price
                .cmp(&a.price)
                .then_with(|| a.timestamp.cmp(&b.timestamp))
        });
        bids
    }

    pub fn get_matched_orders(&self) -> Vec<Trade> {
        self.matched
            .lock()
            .expect("Failed to lock matched")
            .clone()
            .into_iter()
            .collect()
    }

    pub fn start_matching_engine(&self) {
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
                        let mut asks_guard = asks.write_lock();
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        while incoming.remaining > 0 {
                            match asks_guard.peek() {
                                Some(Reverse(ask)) if ask.price <= incoming.price => {
                                    let Reverse(mut best_ask) =
                                        asks_guard.pop().expect("Ask was just peeked");

                                    let trade_qty = incoming.remaining.min(best_ask.remaining);

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

                                    incoming.remaining -= trade_qty;
                                    best_ask.remaining -= trade_qty;

                                    matched_guard.push_front(trade);

                                    if best_ask.remaining > 0 {
                                        asks_guard.push(Reverse(best_ask));
                                    }
                                }
                                _ => break,
                            }
                        }

                        if incoming.remaining > 0 {
                            let mut bids_guard = bids.write_lock();
                            bids_guard.push(incoming);
                        }
                    }
                    Side::Sell => {
                        let mut bids_guard = bids.write_lock();
                        let mut matched_guard = matched.lock().expect("Failed to lock matched");

                        while incoming.remaining > 0 {
                            match bids_guard.peek() {
                                Some(bid) if bid.price >= incoming.price => {
                                    let mut best_bid =
                                        bids_guard.pop().expect("Bid was just peeked");

                                    let trade_qty = incoming.remaining.min(best_bid.remaining);

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

                                    incoming.remaining -= trade_qty;
                                    best_bid.remaining -= trade_qty;

                                    matched_guard.push_front(trade);

                                    if best_bid.remaining > 0 {
                                        bids_guard.push(best_bid);
                                    }
                                }
                                _ => break,
                            }
                        }

                        if incoming.remaining > 0 {
                            let mut asks_guard = asks.write_lock();
                            asks_guard.push(Reverse(incoming));
                        }
                    }
                }
            }
        });
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}
