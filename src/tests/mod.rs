#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::core::errors::OrderError;
    use crate::core::orderbook::OrderBook;
    use crate::models::{Order, Side};

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

        assert!(order1 > order2);

        let order3 = create_order(Side::Buy, 10100, 50);
        assert!(order3 > order1);
    }

    #[test]
    fn test_invalid_orders() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let invalid_qty = create_order(Side::Buy, 10000, 0);
        assert!(matches!(
            orderbook.submit_order(invalid_qty),
            Err(OrderError::InvalidQuantity)
        ));

        let invalid_price = create_order(Side::Buy, 0, 50);
        assert!(matches!(
            orderbook.submit_order(invalid_price),
            Err(OrderError::InvalidPrice)
        ));
    }

    #[test]
    fn test_order_matching_buy() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let sell_order = create_order(Side::Sell, 10000, 50);
        orderbook.submit_order(sell_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let buy_order = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(buy_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].buy_order.price, buy_order.price);
        assert_eq!(matched[0].sell_order.price, sell_order.price);
        assert_eq!(matched[0].quantity, 50);
    }

    #[test]
    fn test_order_matching_sell() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let buy_order = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(buy_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let sell_order = create_order(Side::Sell, 10000, 50);
        orderbook.submit_order(sell_order.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].buy_order.price, buy_order.price);
        assert_eq!(matched[0].sell_order.price, sell_order.price);
        assert_eq!(matched[0].quantity, 50);
    }

    #[test]
    fn test_order_book_sorting() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let buy_orders: Vec<Order> = vec![
            create_order(Side::Buy, 10000, 50),
            create_order(Side::Buy, 10200, 50),
            create_order(Side::Buy, 10100, 50),
        ];

        let sell_orders: Vec<Order> = vec![
            create_order(Side::Sell, 10300, 50),
            create_order(Side::Sell, 10500, 50),
            create_order(Side::Sell, 10400, 50),
        ];

        for order in buy_orders.iter().chain(sell_orders.iter()) {
            orderbook.submit_order(order.clone()).unwrap();
            thread::sleep(Duration::from_millis(50));
        }
        thread::sleep(Duration::from_millis(100));

        let bids = orderbook.get_bids();
        assert!(!bids.is_empty(), "Bids should not be empty");
        for i in 0..bids.len() - 1 {
            assert!(
                bids[i].price >= bids[i + 1].price,
                "Bids not sorted by price desc"
            );
            if bids[i].price == bids[i + 1].price {
                assert!(
                    bids[i].timestamp < bids[i + 1].timestamp,
                    "Bids with same price not sorted by time asc"
                );
            }
        }

        let asks = orderbook.get_asks();
        assert!(!asks.is_empty(), "Asks should not be empty");
        for i in 0..asks.len() - 1 {
            assert!(
                asks[i].price <= asks[i + 1].price,
                "Asks not sorted by price asc"
            );
            if asks[i].price == asks[i + 1].price {
                assert!(
                    asks[i].timestamp < asks[i + 1].timestamp,
                    "Asks with same price not sorted by time asc"
                );
            }
        }
    }

    #[test]
    fn test_price_time_priority() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let order1_ts_earlier = create_order(Side::Buy, 10000, 50);
        thread::sleep(Duration::from_millis(10));
        let order2_ts_later = create_order(Side::Buy, 10000, 50);

        orderbook.submit_order(order2_ts_later.clone()).unwrap();
        thread::sleep(Duration::from_millis(50));
        orderbook.submit_order(order1_ts_earlier.clone()).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 2, "Should be two bids in the book");
        assert_eq!(
            bids[0].timestamp, order1_ts_earlier.timestamp,
            "First bid should be the one with earlier timestamp"
        );
        assert_eq!(
            bids[1].timestamp, order2_ts_later.timestamp,
            "Second bid should be the one with later timestamp"
        );
        assert!(
            bids[0].timestamp < bids[1].timestamp,
            "Bid with earlier timestamp should be prioritized (come first). First: {}, Second: {}",
            bids[0].timestamp,
            bids[1].timestamp
        );
    }

    #[test]
    fn test_no_match_conditions() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let buy_order = create_order(Side::Buy, 10000, 50);
        let sell_order = create_order(Side::Sell, 10100, 50);

        orderbook.submit_order(buy_order).unwrap();
        thread::sleep(Duration::from_millis(50));
        orderbook.submit_order(sell_order).unwrap();
        thread::sleep(Duration::from_millis(100));

        let matched = orderbook.get_matched_orders();
        assert_eq!(matched.len(), 0, "No orders should be matched");

        let bids = orderbook.get_bids();
        let asks = orderbook.get_asks();
        assert_eq!(bids.len(), 1, "Should be one buy order in bids");
        assert_eq!(asks.len(), 1, "Should be one sell order in asks");
    }

    #[test]
    fn test_shutdown_and_processing() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let order1 = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(order1).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids_after_1 = orderbook.get_bids();
        assert_eq!(bids_after_1.len(), 1);
        assert_eq!(bids_after_1[0].price, 10000);

        let order2 = create_order(Side::Buy, 10100, 50);
        orderbook.submit_order(order2).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids_after_2 = orderbook.get_bids();
        assert_eq!(bids_after_2.len(), 2);
        assert!(bids_after_2.iter().any(|order| order.price == 10000));
        assert!(bids_after_2.iter().any(|order| order.price == 10100));
    }

    #[test]
    fn test_partial_fill() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let sell_order = create_order(Side::Sell, 10000, 10);
        orderbook
            .submit_order(sell_order.clone())
            .expect("Failed to submit sell order");
        thread::sleep(Duration::from_millis(100));

        let buy_order = create_order(Side::Buy, 10000, 15);
        orderbook
            .submit_order(buy_order.clone())
            .expect("Failed to submit buy order");
        thread::sleep(Duration::from_millis(100));

        let trades = orderbook.get_matched_orders();
        assert_eq!(trades.len(), 1, "Should be one trade");
        assert_eq!(trades[0].quantity, 10, "Trade quantity should be 10");
        assert_eq!(trades[0].price, 10000, "Trade price should be 100.00");

        let asks = orderbook.get_asks();
        assert!(
            asks.is_empty(),
            "Asks should be empty after full fill of sell order"
        );

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 1, "Should be one remaining bid");
        assert_eq!(bids[0].remaining, 5, "Remaining bid quantity should be 5");
        assert_eq!(bids[0].price, 10000);
    }

    #[test]
    fn test_multiple_partial_fills() {
        let orderbook_instance = OrderBook::new();
        let orderbook = Arc::new(orderbook_instance);

        let sell_orders = vec![
            create_order(Side::Sell, 10000, 5),
            create_order(Side::Sell, 10100, 3),
            create_order(Side::Sell, 10200, 7),
        ];

        for order in sell_orders.iter() {
            orderbook
                .submit_order(order.clone())
                .expect("Failed to submit sell order");
            thread::sleep(Duration::from_millis(50));
        }

        let buy_order = create_order(Side::Buy, 10200, 8);
        orderbook
            .submit_order(buy_order)
            .expect("Failed to submit buy order");
        thread::sleep(Duration::from_millis(150));

        let trades = orderbook.get_matched_orders();
        assert_eq!(trades.len(), 2, "Should be two trades");

        assert_eq!(trades[0].quantity, 3);
        assert_eq!(trades[0].price, 10100);
        assert_eq!(trades[0].sell_order.price, 10100);

        assert_eq!(trades[1].quantity, 5);
        assert_eq!(trades[1].price, 10000);
        assert_eq!(trades[1].sell_order.price, 10000);

        let asks = orderbook.get_asks();
        assert_eq!(asks.len(), 1, "Should be one remaining ask order (s3)");
        assert_eq!(asks[0].price, 10200);
        assert_eq!(asks[0].remaining, 7);

        let bids = orderbook.get_bids();
        assert!(
            bids.is_empty(),
            "Bids should be empty as buy order was filled"
        );
    }
}
