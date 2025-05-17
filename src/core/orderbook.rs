use crate::core::rwlock::RWLock;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::errors::OrderError;
use crate::models::{Order, Side, Trade};

pub struct OrderBook {
    asks: Arc<RWLock<BinaryHeap<Reverse<Order>>>>,
    bids: Arc<RWLock<BinaryHeap<Order>>>,
    matched: Arc<Mutex<VecDeque<Trade>>>,
    order_sender: SyncSender<Order>,
    shutdown_requested: Arc<AtomicBool>,
    matching_engine_handle: Option<JoinHandle<()>>,
}

impl OrderBook {
    pub fn new() -> Self {
        const CHANNEL_CAPACITY: usize = 1000;
        let (sender, receiver): (SyncSender<Order>, Receiver<Order>) =
            sync_channel::<Order>(CHANNEL_CAPACITY);

        let asks = Arc::new(RWLock::new(BinaryHeap::<Reverse<Order>>::new()));
        let bids = Arc::new(RWLock::new(BinaryHeap::new()));
        let matched = Arc::new(Mutex::new(VecDeque::new()));
        let shutdown_requested = Arc::new(AtomicBool::new(false));

        let asks_clone = Arc::clone(&asks);
        let bids_clone = Arc::clone(&bids);
        let matched_clone = Arc::clone(&matched);
        let shutdown_requested_clone = Arc::clone(&shutdown_requested);

        let matching_engine_handle = std::thread::spawn(move || {
            loop {
                if shutdown_requested_clone.load(Ordering::SeqCst) {
                    println!("Matching engine: Shutdown signal received, terminating.");
                    break;
                }

                match receiver.try_recv() {
                    Ok(mut incoming) => match incoming.side {
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
                                            let trade_qty =
                                                incoming.remaining.min(best_ask.remaining);

                                            let trade = Trade {
                                                price: best_ask.price,
                                                quantity: trade_qty,
                                                buy_order: incoming.clone(),
                                                sell_order: best_ask.clone(),
                                                timestamp: SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .map(|d| d.as_millis())
                                                    .unwrap_or_else(|err| {
                                                        eprintln!("SystemTime error: {:?}", err);
                                                        0
                                                    }),
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
                                            let trade_qty =
                                                incoming.remaining.min(best_bid.remaining);

                                            let trade = Trade {
                                                price: best_bid.price,
                                                quantity: trade_qty,
                                                buy_order: best_bid.clone(),
                                                sell_order: incoming.clone(),
                                                timestamp: SystemTime::now()
                                                    .duration_since(UNIX_EPOCH)
                                                    .map(|d| d.as_millis())
                                                    .unwrap_or_else(|err| {
                                                        eprintln!("SystemTime error: {:?}", err);
                                                        0
                                                    }),
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
                    },
                    Err(TryRecvError::Empty) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(TryRecvError::Disconnected) => {
                        println!("Matching engine: Channel disconnected, terminating.");
                        break;
                    }
                }
            }
            println!("Matching engine thread finished.");
        });

        OrderBook {
            asks,
            bids,
            matched,
            order_sender: sender,
            shutdown_requested,
            matching_engine_handle: Some(matching_engine_handle),
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
            .map_err(|e: SendError<Order>| {
                eprintln!("Failed to send order: {}", e);
                OrderError::ChannelError
            })
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
            Err(poisoned) => {
                eprintln!("Matched orders lock was poisoned. Recovering data.");
                poisoned.into_inner().clone().into_iter().collect()
            }
        }
    }
}

impl Drop for OrderBook {
    fn drop(&mut self) {
        println!("OrderBook: Drop called. Shutting down...");
        self.shutdown_requested.store(true, Ordering::SeqCst);

        if let Some(handle) = self.matching_engine_handle.take() {
            println!("OrderBook: Waiting for matching engine thread to join...");
            if handle.join().is_err() {
                eprintln!("Matching engine thread panicked during drop.");
            } else {
                println!("Matching engine thread joined successfully during drop.");
            }
        } else {
            println!(
                "OrderBook: No matching engine handle to join (already joined or never started)."
            );
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}
