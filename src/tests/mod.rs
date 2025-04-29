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

        assert!(order1 > order2); // Earlier timestamp should be higher priority

        let order3 = create_order(Side::Buy, 10100, 50);
        assert!(order3 > order1); // Higher price should be higher priority
    }

    #[test]
    fn test_invalid_orders() {
        let orderbook = OrderBook::new();

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
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

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
    }

    #[test]
    fn test_order_matching_sell() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

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
    }

    #[test]
    fn test_order_book_sorting() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

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

        let bids = orderbook.get_bids();
        assert!(!bids.is_empty());
        for i in 0..bids.len() - 1 {
            assert!(bids[i].price >= bids[i + 1].price);
        }

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

        let order1 = create_order(Side::Buy, 10000, 50);
        orderbook.submit_order(order1).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].price, 10000);

        // Submit another order and verify it's processed
        let order2 = create_order(Side::Buy, 10100, 50);
        orderbook.submit_order(order2).unwrap();
        thread::sleep(Duration::from_millis(100));

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 2);
        assert!(bids.iter().any(|order| order.price == 10100));
    }

    #[test]
    fn test_partial_fill() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        let sell_order = create_order(Side::Sell, 100, 10);
        orderbook
            .submit_order(sell_order.clone())
            .expect("Failed to submit sell order");
        thread::sleep(Duration::from_millis(100));

        let buy_order = create_order(Side::Buy, 100, 15);
        orderbook
            .submit_order(buy_order.clone())
            .expect("Failed to submit buy order");
        thread::sleep(Duration::from_millis(100));

        let trades = orderbook.get_matched_orders();
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, 10);

        let bids = orderbook.get_bids();
        assert_eq!(bids.len(), 1);
        assert_eq!(bids[0].remaining, 5);
    }

    #[test]
    fn test_multiple_partial_fills() {
        let orderbook = Arc::new(OrderBook::new());
        orderbook.start_matching_engine();

        let sell_orders = vec![
            create_order(Side::Sell, 100, 5),
            create_order(Side::Sell, 101, 3),
            create_order(Side::Sell, 102, 7),
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

        assert_eq!(trades[1].quantity, 5);
        assert_eq!(trades[1].price, 100);

        assert_eq!(trades[0].quantity, 3);
        assert_eq!(trades[0].price, 101);
    }
}
