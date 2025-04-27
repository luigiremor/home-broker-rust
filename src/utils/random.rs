use crate::models::{Order, Side};
use rand::Rng;

pub fn generate_random_order() -> Order {
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
