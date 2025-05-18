use std::sync::{Arc, Mutex};
use std::thread;

use crate::sync::mpsc;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Job>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel::<Job>(100);
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        if self.sender.send(job).is_err() {
            panic!("ThreadPool has shut down, failed to send job.");
        }
    }
}

struct Worker {
    id: usize,
    handle: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Self {
        let handle = thread::spawn(move || loop {
            let message = {
                let guard = match receiver.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        eprintln!(
                            "Worker {}: Mutex poisoned, shutting down. Error: {}",
                            id, poisoned
                        );
                        break;
                    }
                };
                guard.recv()
            };

            match message {
                Ok(job) => {
                    job();
                }
                Err(_) => {
                    break;
                }
            }
        });

        Worker {
            id,
            handle: Some(handle),
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(std::mem::replace(&mut self.sender, mpsc::channel(1).0));
        for worker in &mut self.workers {
            println!("Shutting down worker {}", worker.id);
            if let Some(handle) = worker.handle.take() {
                if let Err(e) = handle.join() {
                    eprintln!("Worker thread {} panicked: {:?}", worker.id, e);
                }
            }
        }
    }
}
