use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

// Shared core of the channel
pub(crate) struct ChannelCore<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    senders_count: AtomicUsize,
    is_empty_cond: Condvar, // Signals that an item has been added (queue is not empty)
    is_not_full_cond: Condvar, // Signals that an item has been removed (queue is not full)
}

// Error returned by send if the channel is disconnected or operation fails
#[derive(Debug)]
pub struct SendError<T>(pub T); // Item is returned if send fails

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sending on a disconnected channel")
    }
}

impl<T: fmt::Debug> std::error::Error for SendError<T> {}

// Error returned by try_recv
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TryRecvError {
    Empty,
    Disconnected,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TryRecvError::Empty => write!(f, "receiving on an empty channel"),
            TryRecvError::Disconnected => write!(f, "receiving on a disconnected channel"),
        }
    }
}

impl std::error::Error for TryRecvError {}

// Error returned by recv
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RecvError {
    Disconnected,
}

impl fmt::Display for RecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "receiving on a disconnected channel")
    }
}

impl std::error::Error for RecvError {}

// Sender part of the channel
pub struct Sender<T> {
    core: Arc<ChannelCore<T>>,
}

impl<T> Sender<T> {
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        let mut queue_guard = match self.core.queue.lock() {
            Ok(guard) => guard,
            Err(poisoned_err) => {
                eprintln!("CustomMPSC Send: Queue lock poisoned: {:?}", poisoned_err);
                return Err(SendError(item)); // item is returned here
            }
        };

        while queue_guard.len() >= self.core.capacity {
            if self.core.senders_count.load(Ordering::Relaxed) == 0 {
                return Err(SendError(item)); // item is returned here
            }

            // If wait fails, the item is NOT consumed yet by push_back.
            // So, we can still return it in SendError.
            match self.core.is_not_full_cond.wait(queue_guard) {
                Ok(guard) => queue_guard = guard,
                Err(poisoned_err) => {
                    eprintln!("CustomMPSC Send: Condvar wait poisoned: {:?}", poisoned_err);
                    return Err(SendError(item)); // item is returned here
                }
            }
        }

        // Item is moved here.
        queue_guard.push_back(item);
        drop(queue_guard);
        self.core.is_empty_cond.notify_one();
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.core.senders_count.fetch_add(1, Ordering::Relaxed);
        Sender {
            core: Arc::clone(&self.core),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.core.senders_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            // This was the last sender, notify the receiver in case it's waiting
            self.core.is_empty_cond.notify_one();
            // Also notify any senders that might be blocked on a full queue,
            // so they can see it's now disconnected.
            self.core.is_not_full_cond.notify_all();
        }
    }
}

// Receiver part of the channel
pub struct Receiver<T> {
    core: Arc<ChannelCore<T>>,
    // We can add a flag here to indicate if this receiver is the "active" one,
    // though for single consumer it's implicit.
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, RecvError> {
        let mut queue_guard = self
            .core
            .queue
            .lock()
            .map_err(|_| RecvError::Disconnected)?; // Handle poisoned mutex
        loop {
            if let Some(item) = queue_guard.pop_front() {
                drop(queue_guard);
                self.core.is_not_full_cond.notify_one();
                return Ok(item);
            }
            // If queue is empty, check if senders are still active
            if self.core.senders_count.load(Ordering::Relaxed) == 0 {
                return Err(RecvError::Disconnected);
            }
            // Wait for an item to be pushed or for senders to disconnect
            queue_guard = self
                .core
                .is_empty_cond
                .wait(queue_guard)
                .map_err(|_| RecvError::Disconnected)?;
        }
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut queue_guard = self
            .core
            .queue
            .lock()
            .map_err(|_| TryRecvError::Disconnected)?; // Handle poisoned mutex
        if let Some(item) = queue_guard.pop_front() {
            drop(queue_guard);
            self.core.is_not_full_cond.notify_one();
            Ok(item)
        } else if self.core.senders_count.load(Ordering::Relaxed) == 0 {
            Err(TryRecvError::Disconnected)
        } else {
            Err(TryRecvError::Empty)
        }
    }
}

// Function to create a new channel
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "Channel capacity must be greater than 0");
    let core = Arc::new(ChannelCore {
        queue: Mutex::new(VecDeque::with_capacity(capacity)),
        capacity,
        senders_count: AtomicUsize::new(1), // Start with one sender
        is_empty_cond: Condvar::new(),
        is_not_full_cond: Condvar::new(),
    });
    (
        Sender {
            core: Arc::clone(&core),
        },
        Receiver { core },
    )
}
