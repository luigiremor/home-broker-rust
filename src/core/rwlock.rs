
pub struct RWLock {
    lock_params: Mutex<LockParams>
    readers_condvar: Condvar,
    writers_condvar: Condvar,
}

struct LockParams {
    num_readers: usize,
    writers_waiting: usize,
    writer_working: bool,
}

impl RWLock {
    pub fn new() -> Self {
        Self {
            lock_params: Mutex::new(LockParams {
                num_readers: 0,
                writers_waiting: 0,
                writer_working: false,
            }),
            readers_condvar: Condvar::new(),
            writers_condvar: Condvar::new(),
        }
    }

    pub fn read_lock(&self) {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        while lock_params_guard.writer_working || lock_params_guard.writers_waiting > 0 {
            lock_params_guard = self.readers_condvar.wait(lock_params_guard).unwrap();
        }
        lock_params_guard.num_readers += 1;
    } 

    pub fn read_unlock(&self) {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        lock_params_guard.num_readers -= 1;
        if lock_params_guard.num_readers == 0 {
            self.writers_condvar.notify_one();
        }
    }

    pub fn write_lock(&self) {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        lock_params_guard.writers_waiting += 1;
        while lock_params_guard.writer_working || lock_params_guard.num_readers > 0 {
            lock_params_guard = self.writers_condvar.wait(lock_params_guard).unwrap();
        }
        lock_params_guard.writers_waiting -= 1;
        lock_params_guard.writer_working = true;
    }

    pub fn write_unlock(&self) {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        lock_params_guard.writer_working = false;
        
        if lock_params_guard.writers_waiting > 0 {
            self.writers_condvar.notify_one();
        } else {
            self.readers_condvar.notify_all();
        }
    }
}