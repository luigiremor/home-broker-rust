use broker::core::orderbook::OrderBook;
use broker::ui::Tui;
use broker::utils::generate_random_order;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let orderbook = Arc::new(OrderBook::new());
    orderbook.start_matching_engine();

    let mut tui = Tui::new()?;

    let orderbook_clone: Arc<OrderBook> = Arc::clone(&orderbook);
    let mut shutdown_rx = orderbook.shutdown.subscribe();

    let order_generator = tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let order = generate_random_order();
                    if let Err(e) = orderbook_clone.submit_order(order) {
                        eprintln!("Failed to submit order: {}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
    });

    let mut interval = time::interval(Duration::from_millis(100));

    loop {
        if Tui::should_quit()? {
            println!("\nShutting down...");
            break;
        }

        interval.tick().await;
        if let Err(e) = tui.draw(&orderbook) {
            eprintln!("Failed to draw TUI: {}", e);
            break;
        }
    }

    if let Err(e) = tui.shutdown() {
        eprintln!("Error during TUI shutdown: {}", e);
    }

    let _ = orderbook.shutdown.send(());
    let _ = order_generator.await;

    Ok(())
}
