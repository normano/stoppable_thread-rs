use std::thread;
use std::thread::{Thread, JoinHandle, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};


pub struct SimpleAtomicBool(AtomicBool);

impl SimpleAtomicBool {
    pub fn new(v: bool) -> SimpleAtomicBool {
        SimpleAtomicBool(AtomicBool::new(v))
    }

    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, v: bool) {
        self.0.store(v, Ordering::Relaxed)
    }
}

pub struct StoppableHandle<T> {
    join_handle: JoinHandle<T>,
    stopped: Weak<SimpleAtomicBool>,
}

impl<T> StoppableHandle<T> {
    pub fn thread(&self) -> &Thread {
        self.join_handle.thread()
    }

    pub fn join(self) -> Result<T> {
        self.join_handle.join()
    }

    pub fn stop(self) -> JoinHandle<T> {
        if let Some(v) = self.stopped.upgrade() {
            v.set(true)
        }

        self.join_handle
    }
}

pub fn spawn<F, T>(f: F) -> StoppableHandle<T> where
    F: FnOnce(&SimpleAtomicBool) -> T,
    F: Send + 'static, T: Send + 'static {
    let stopped = Arc::new(SimpleAtomicBool::new(false));
    let stopped_w = Arc::downgrade(&stopped);

    StoppableHandle{
        join_handle: thread::spawn(move || f(&*stopped)),
        stopped: stopped_w,
    }
}

#[cfg(test)]
#[test]
fn test_stoppable_thead() {
    use std::thread::sleep;
    use std::time::Duration;

    let work_work = spawn(|stopped| {
        let mut count: u64 = 0;
        while !stopped.get() {
            count += 1;
        }
        count
    });

    // wait a few cycles
    sleep(Duration::from_millis(100));

    let result = work_work.stop().join().unwrap();

    // assume some work has been done
    assert!(result > 1);
}
