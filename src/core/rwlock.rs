use std::sync::{Condvar, Mutex};

pub struct RWLock<T> {
    lock_params: Mutex<LockParams<T>>,
    readers_condvar: Condvar,
    writers_condvar: Condvar,
}

struct LockParams<T> {
    data: T,
    num_readers: usize,
    writers_waiting: usize,
    writer_working: bool,
}

pub struct ReadGuard<'a, T> {
    rw_lock: &'a RWLock<T>,
    data_ptr: *const T,
}

pub struct WriteGuard<'a, T> {
    rw_lock: &'a RWLock<T>,
    data_ptr: *mut T,
}

impl<T> RWLock<T> {
    pub fn new(data: T) -> Self {
        Self {
            lock_params: Mutex::new(LockParams {
                data,
                num_readers: 0,
                writers_waiting: 0,
                writer_working: false,
            }),
            readers_condvar: Condvar::new(),
            writers_condvar: Condvar::new(),
        }
    }

    pub fn read_lock(&self) -> ReadGuard<T> {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        while lock_params_guard.writer_working || lock_params_guard.writers_waiting > 0 {
            lock_params_guard = self.readers_condvar.wait(lock_params_guard).unwrap();
        }
        lock_params_guard.num_readers += 1;

        ReadGuard {
            rw_lock: self,
            data_ptr: &lock_params_guard.data as *const T,
        }
    }

    pub fn write_lock(&self) -> WriteGuard<T> {
        let mut lock_params_guard = self.lock_params.lock().unwrap();
        lock_params_guard.writers_waiting += 1;
        while lock_params_guard.writer_working || lock_params_guard.num_readers > 0 {
            lock_params_guard = self.writers_condvar.wait(lock_params_guard).unwrap();
        }
        lock_params_guard.writers_waiting -= 1;
        lock_params_guard.writer_working = true;

        WriteGuard {
            rw_lock: self,
            data_ptr: &mut lock_params_guard.data as *mut T,
        }
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

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        let mut lock_params_guard = self.rw_lock.lock_params.lock().unwrap();
        lock_params_guard.num_readers -= 1;
        if lock_params_guard.num_readers == 0 {
            self.rw_lock.writers_condvar.notify_one();
        }
    }
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        let mut lock_params_guard = self.rw_lock.lock_params.lock().unwrap();
        lock_params_guard.writer_working = false;

        if lock_params_guard.writers_waiting > 0 {
            self.rw_lock.writers_condvar.notify_one();
        } else {
            self.rw_lock.readers_condvar.notify_all();
        }
    }
}

impl<T> std::ops::Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.data_ptr }
    }
}

impl<T> std::ops::Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.data_ptr }
    }
}

impl<T> std::ops::DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data_ptr }
    }
}
