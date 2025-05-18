use super::errors::OrderError;
use crate::models::{Order, Side, Trade};
use crate::sync::mpsc::{self, SendError as CustomSendError, TryRecvError as CustomTryRecvError};
use crate::sync::rwlock::RWLock;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct OrderBook {
    asks: Arc<RWLock<BinaryHeap<Reverse<Order>>>>,
    bids: Arc<RWLock<BinaryHeap<Order>>>,
    matched: Arc<std::sync::Mutex<VecDeque<Trade>>>,
    order_sender: mpsc::Sender<Order>,
    shutdown_requested: Arc<AtomicBool>,
    matching_engine_handle: Option<JoinHandle<()>>,
}

impl OrderBook {
    #[allow(clippy::too_many_arguments)]
    fn process_order_matches<O: Ord + Clone>(
        incoming_order: &mut Order,
        opposing_book_heap: &mut BinaryHeap<O>,
        matched_orders_queue: &mut VecDeque<Trade>,
        price_condition_met: impl Fn(&O, &Order) -> bool,
        get_opposing_order_data_mut: impl Fn(&mut O) -> &mut Order,
        get_cloned_opposing_order_data: impl Fn(&O) -> Order,
    ) {
        while incoming_order.remaining > 0 {
            if let Some(opposing_order_in_heap_peek) = opposing_book_heap.peek() {
                if price_condition_met(opposing_order_in_heap_peek, incoming_order) {
                    if let Some(mut best_opposing_order_heap_item) = opposing_book_heap.pop() {
                        let resting_order_for_trade =
                            get_cloned_opposing_order_data(&best_opposing_order_heap_item);

                        let best_opposing_order_data_mut =
                            get_opposing_order_data_mut(&mut best_opposing_order_heap_item);

                        let trade_qty = incoming_order
                            .remaining
                            .min(best_opposing_order_data_mut.remaining);

                        let (buy_trade_order, sell_trade_order, trade_price) =
                            if incoming_order.side == Side::Buy {
                                (
                                    incoming_order.clone(),
                                    resting_order_for_trade.clone(),
                                    resting_order_for_trade.price,
                                )
                            } else {
                                (
                                    resting_order_for_trade.clone(),
                                    incoming_order.clone(),
                                    resting_order_for_trade.price,
                                )
                            };

                        let trade = Trade {
                            price: trade_price,
                            quantity: trade_qty,
                            buy_order: buy_trade_order,
                            sell_order: sell_trade_order,
                            timestamp: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis())
                                .unwrap_or_else(|err| {
                                    eprintln!(
                                        "SystemTime error: {:?}, defaulting timestamp to 0.",
                                        err
                                    );
                                    0
                                }),
                        };

                        incoming_order.remaining -= trade_qty;
                        best_opposing_order_data_mut.remaining -= trade_qty;

                        matched_orders_queue.push_front(trade);

                        if best_opposing_order_data_mut.remaining > 0 {
                            opposing_book_heap.push(best_opposing_order_heap_item);
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
    }

    pub fn new() -> Self {
        const CHANNEL_CAPACITY: usize = 1000;
        let (sender, receiver) = mpsc::channel::<Order>(CHANNEL_CAPACITY);

        let asks = Arc::new(RWLock::new(BinaryHeap::<Reverse<Order>>::new()));
        let bids = Arc::new(RWLock::new(BinaryHeap::new()));
        let matched = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let shutdown_requested = Arc::new(AtomicBool::new(false));

        let asks_clone = Arc::clone(&asks);
        let bids_clone = Arc::clone(&bids);
        let matched_clone = Arc::clone(&matched);
        let shutdown_requested_clone = Arc::clone(&shutdown_requested);

        let matching_engine_handle = std::thread::spawn(move || {
            loop {
                if shutdown_requested_clone.load(Ordering::SeqCst) {
                    println!(
                        "Matching engine: Shutdown signal received (OrderBook flag), terminating."
                    );
                    break;
                }

                match receiver.try_recv() {
                    Ok(mut incoming_order) => {
                        let mut matched_guard = match matched_clone.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };

                        match incoming_order.side {
                            Side::Buy => {
                                let mut asks_guard = asks_clone.write_lock();
                                Self::process_order_matches(
                                    &mut incoming_order,
                                    &mut asks_guard,
                                    &mut matched_guard,
                                    |ask_rev: &Reverse<Order>, buy_ord: &Order| {
                                        ask_rev.0.price <= buy_ord.price
                                    },
                                    |ask_rev: &mut Reverse<Order>| &mut ask_rev.0,
                                    |ask_rev: &Reverse<Order>| ask_rev.0.clone(),
                                );

                                if incoming_order.remaining > 0 {
                                    let mut bids_guard = bids_clone.write_lock();
                                    bids_guard.push(incoming_order);
                                }
                            }
                            Side::Sell => {
                                let mut bids_guard = bids_clone.write_lock();
                                Self::process_order_matches(
                                    &mut incoming_order,
                                    &mut bids_guard,
                                    &mut matched_guard,
                                    |bid: &Order, sell_ord: &Order| bid.price >= sell_ord.price,
                                    |bid: &mut Order| bid,
                                    |bid: &Order| bid.clone(),
                                );

                                if incoming_order.remaining > 0 {
                                    let mut asks_guard = asks_clone.write_lock();
                                    asks_guard.push(Reverse(incoming_order));
                                }
                            }
                        }
                    }
                    Err(CustomTryRecvError::Empty) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(CustomTryRecvError::Disconnected) => {
                        println!(
                            "Matching engine: Channel disconnected (custom mpsc), terminating."
                        );
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
            .map_err(|e: CustomSendError<Order>| {
                eprintln!("Failed to send order via custom channel: {}", e.0.price);
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
