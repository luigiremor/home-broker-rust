use broker::core::orderbook::OrderBook;
use broker::core::threadpool::ThreadPool;
use broker::ui::Tui;
use broker::utils::generate_random_order;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let orderbook = Arc::new(OrderBook::new());
    orderbook.start_matching_engine();

    let mut tui = Tui::new()?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let orderbook_clone = Arc::clone(&orderbook);
    let pool = Arc::new(ThreadPool::new(4));
    let pool_clone = Arc::clone(&pool);

    let generator_handle = thread::spawn(move || {
        while running_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));

            let orderbook_job = Arc::clone(&orderbook_clone);
            pool_clone.execute(move || {
                let order = generate_random_order();
                if let Err(e) = orderbook_job.submit_order(order) {
                    eprintln!("Failed to submit order: {}", e);
                }
            });
        }
        println!("Order generator encerrado.");
    });

    while running.load(Ordering::SeqCst) {
        if Tui::should_quit()? {
            println!("\nShutting down...");
            break;
        }

        thread::sleep(Duration::from_millis(100));
        if let Err(e) = tui.draw(&orderbook) {
            eprintln!("Failed to draw TUI: {}", e);
            break;
        }
    }

    if let Err(e) = tui.shutdown() {
        eprintln!("Error during TUI shutdown: {}", e);
    }
    running.store(false, Ordering::SeqCst);
    let _ = generator_handle.join();
    let _ = orderbook.shutdown.send(());

    Ok(())
}
