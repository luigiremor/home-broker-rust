#[derive(Debug)]
struct Order {
    id: String,
    side: Side,
    quantity: i64,
    price: i64,
    decimals: i32,
}

#[derive(Debug)]
enum Side {
    Buy,
    Sell,
}

#[derive(Debug)]
struct OrderBook {
    asks: Vec<Order>,
    bids: Vec<Order>,
}

impl OrderBook {
    fn new() -> Self {
        OrderBook {
            asks: Vec::new(),
            bids: Vec::new(),
        }
    }
}

fn main() {
    let orderbook = OrderBook::new();
    println!("{:?}", orderbook);
}
