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
    let orderbook_instance = OrderBook::new();
    let orderbook = Arc::new(orderbook_instance);

    let mut tui = Tui::new()?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let orderbook_clone = Arc::clone(&orderbook);
    let pool = Arc::new(ThreadPool::new(4));

    let generator_handle = thread::spawn(move || {
        while running_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));

            let orderbook_job = Arc::clone(&orderbook_clone);
            pool.execute(move || {
                let order = generate_random_order();
                if let Err(e) = orderbook_job.submit_order(order) {
                    eprintln!("Failed to submit order: {}", e);
                }
            });
        }
        println!("Order generator encerrado.");
    });

    loop {
        if Tui::should_quit()? {
            println!("\nShutting down main loop...");
            running.store(false, Ordering::SeqCst);
            break;
        }

        if let Err(e) = tui.draw(&orderbook) {
            eprintln!("Failed to draw TUI: {}", e);
            running.store(false, Ordering::SeqCst);
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    println!("Shutting down TUI...");
    if let Err(e) = tui.shutdown() {
        eprintln!("Error during TUI shutdown: {}", e);
    }

    println!("Waiting for order generator to complete...");
    if let Err(e) = generator_handle.join() {
        eprintln!("Order generator thread panicked: {:?}", e);
    } else {
        println!("Order generator completed.");
    }

    println!("Main function finished. OrderBook will be dropped if this is the last Arc.");
    Ok(())
}
