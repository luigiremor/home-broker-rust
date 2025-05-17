use crate::core::rwlock::RWLock;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use super::errors::OrderError;
use crate::models::{Order, Side, Trade};

pub struct OrderBook {
    asks: Arc<RWLock<BinaryHeap<Reverse<Order>>>>,
    bids: Arc<RWLock<BinaryHeap<Order>>>,
    matched: Arc<Mutex<VecDeque<Trade>>>,
    order_sender: Option<Sender<Order>>,
    shutdown_requested: Arc<AtomicBool>,
    matching_engine_handle: Option<JoinHandle<()>>,
}

impl OrderBook {
    pub fn new() -> (Self, Receiver<Order>) {
        let (sender, receiver) = channel();
        let orderbook = OrderBook {
            asks: Arc::new(RWLock::new(BinaryHeap::new())),
            bids: Arc::new(RWLock::new(BinaryHeap::new())),
            matched: Arc::new(Mutex::new(VecDeque::new())),
            order_sender: Some(sender),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            matching_engine_handle: None,
        };
        (orderbook, receiver)
    }

    pub fn submit_order(&self, order: Order) -> Result<(), OrderError> {
        if order.remaining <= 0 {
            return Err(OrderError::InvalidQuantity);
        }
        if order.price <= 0 {
            return Err(OrderError::InvalidPrice);
        }

        if let Some(ref sender) = self.order_sender {
            sender.send(order).map_err(|_| OrderError::ChannelError)
        } else {
            Err(OrderError::ChannelError)
        }
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
        match self.matched.lock() {
            Ok(guard) => guard.clone().into_iter().collect(),
            Err(poisoned) => poisoned.into_inner().clone().into_iter().collect(),
        }
    }

    pub fn start_matching_engine(&mut self, receiver: Receiver<Order>) {
        if self.matching_engine_handle.is_some() {
            return;
        }

        let asks_clone = Arc::clone(&self.asks);
        let bids_clone = Arc::clone(&self.bids);
        let matched_clone = Arc::clone(&self.matched);

        let handle = std::thread::spawn(move || {
            while let Ok(mut incoming) = receiver.recv() {
                match incoming.side {
                    Side::Buy => {
                        let mut asks_guard = asks_clone.write_lock();
                        let mut matched_guard = match matched_clone.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        while incoming.remaining > 0 {
                            if let Some(Reverse(ask_peek)) = asks_guard.peek() {
                                if ask_peek.price <= incoming.price {
                                    if let Some(Reverse(mut best_ask)) = asks_guard.pop() {
                                        let trade_qty = incoming.remaining.min(best_ask.remaining);

                                        let trade = Trade {
                                            price: best_ask.price,
                                            quantity: trade_qty,
                                            buy_order: incoming.clone(),
                                            sell_order: best_ask.clone(),
                                            timestamp: SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .map(|d| d.as_millis())
                                                .unwrap_or_else(|_| 0),
                                        };

                                        incoming.remaining -= trade_qty;
                                        best_ask.remaining -= trade_qty;

                                        matched_guard.push_front(trade);

                                        if best_ask.remaining > 0 {
                                            asks_guard.push(Reverse(best_ask));
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        if incoming.remaining > 0 {
                            let mut bids_guard = bids_clone.write_lock();
                            bids_guard.push(incoming);
                        }
                    }
                    Side::Sell => {
                        let mut bids_guard = bids_clone.write_lock();
                        let mut matched_guard = match matched_clone.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        while incoming.remaining > 0 {
                            if let Some(bid_peek) = bids_guard.peek() {
                                if bid_peek.price >= incoming.price {
                                    if let Some(mut best_bid) = bids_guard.pop() {
                                        let trade_qty = incoming.remaining.min(best_bid.remaining);

                                        let trade = Trade {
                                            price: best_bid.price,
                                            quantity: trade_qty,
                                            buy_order: best_bid.clone(),
                                            sell_order: incoming.clone(),
                                            timestamp: SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .map(|d| d.as_millis())
                                                .unwrap_or_else(|_| 0),
                                        };

                                        incoming.remaining -= trade_qty;
                                        best_bid.remaining -= trade_qty;

                                        matched_guard.push_front(trade);

                                        if best_bid.remaining > 0 {
                                            bids_guard.push(best_bid);
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        if incoming.remaining > 0 {
                            let mut asks_guard = asks_clone.write_lock();
                            asks_guard.push(Reverse(incoming));
                        }
                    }
                }
            }
        });
        self.matching_engine_handle = Some(handle);
    }
}

impl Drop for OrderBook {
    fn drop(&mut self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        if self.order_sender.take().is_some() {}
        if let Some(handle) = self.matching_engine_handle.take() {
            if handle.join().is_err() {}
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        let (ob, _) = Self::new();
        ob
    }
}
