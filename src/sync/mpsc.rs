use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub(crate) struct ChannelCore<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    senders_count: AtomicUsize,
    is_empty_cond: Condvar,
    is_not_full_cond: Condvar,
}

#[derive(Debug)]
pub struct SendError<T>(pub T);

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sending on a disconnected channel")
    }
}

impl<T: fmt::Debug> std::error::Error for SendError<T> {}

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

pub struct Sender<T> {
    core: Arc<ChannelCore<T>>,
}

impl<T> Sender<T> {
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        let mut queue_guard = match self.core.queue.lock() {
            Ok(guard) => guard,
            Err(poisoned_err) => {
                eprintln!("CustomMPSC Send: Queue lock poisoned: {:?}", poisoned_err);
                return Err(SendError(item));
            }
        };

        while queue_guard.len() >= self.core.capacity {
            if self.core.senders_count.load(Ordering::Relaxed) == 0 {
                return Err(SendError(item));
            }

            match self.core.is_not_full_cond.wait(queue_guard) {
                Ok(guard) => queue_guard = guard,
                Err(poisoned_err) => {
                    eprintln!("CustomMPSC Send: Condvar wait poisoned: {:?}", poisoned_err);
                    return Err(SendError(item));
                }
            }
        }

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
            self.core.is_empty_cond.notify_one();
            self.core.is_not_full_cond.notify_all();
        }
    }
}

pub struct Receiver<T> {
    core: Arc<ChannelCore<T>>,
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, RecvError> {
        let mut queue_guard = self
            .core
            .queue
            .lock()
            .map_err(|_| RecvError::Disconnected)?;
        loop {
            if let Some(item) = queue_guard.pop_front() {
                drop(queue_guard);
                self.core.is_not_full_cond.notify_one();
                return Ok(item);
            }
            if self.core.senders_count.load(Ordering::Relaxed) == 0 {
                return Err(RecvError::Disconnected);
            }
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
            .map_err(|_| TryRecvError::Disconnected)?;
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

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "Channel capacity must be greater than 0");
    let core = Arc::new(ChannelCore {
        queue: Mutex::new(VecDeque::with_capacity(capacity)),
        capacity,
        senders_count: AtomicUsize::new(1),
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
