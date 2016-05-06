//! A stoppable, thin wrapper around std::Thread.
//!
//! Uses `std::sync::atomic::AtomicBool` and `std::thread` to create stoppable
//! threads.
//!
//! The interface is very similar to that of `std::thread::Thread` (or rather
//! `std::thread::JoinHandle`) except that every closure passed in must accept
//! a `stopped` parameter, allowing to check whether or not a stop was
//! requested.
//!
//! Since all stops must have gracefully, i.e. by requesting the child thread
//! to stop, partial values can be returned if needed.
//!
//! Example:
//!
//! ```
//! use stoppable_thread;
//!
//! let handle = stoppable_thread::spawn(|stopped| {
//!     let mut count: u64 = 0;
//!
//!     while !stopped.get() {
//!         count += 1
//!     }
//!
//!     count
//! });
//!
//! // work in main thread
//!
//! // stop the thread. we also want to collect partial results
//! let child_count = handle.stop().join().unwrap();
//! ```

use std::thread;
use std::thread::{Thread, JoinHandle, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};


/// An simplified std::sync::atomic::AtomicBool
///
/// Use a more intuitive interface and does not allow the Ordering to be
/// specified (it's always `Ordering::Relaxed`)
pub struct SimpleAtomicBool(AtomicBool);

impl SimpleAtomicBool {

    /// Create a new instance
    pub fn new(v: bool) -> SimpleAtomicBool {
        SimpleAtomicBool(AtomicBool::new(v))
    }

    /// Return the current value
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Set a new value
    pub fn set(&self, v: bool) {
        self.0.store(v, Ordering::Relaxed)
    }
}

#[derive(Debug, PartialEq)]
pub enum ThreadState {
    Running, Stopping, Stopped
}

/// A handle for a stoppable thread
///
/// The interface is similar to `std::thread::JoinHandle<T>`, supporting
/// `thread` and `join` with the same signature.
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

    /// Stop the thread
    ///
    /// This will signal the thread to stop by setting the shared atomic
    /// `stopped` variable to `True`. The function will return immediately
    /// after, to wait for the thread to stop, use the returned `JoinHandle<T>`
    /// and `wait()`.
    pub fn stop(self) -> JoinHandle<T> {
        if let Some(v) = self.stopped.upgrade() {
            v.set(true)
        }

        self.join_handle
    }

    pub fn state(&self) -> ThreadState {
        match self.stopped.upgrade() {
            Some(v) => {
                if v.get() {
                    ThreadState::Stopping
                } else {
                    ThreadState::Running
                }
            }
            None => ThreadState::Stopped,
        }
    }
}

/// Spawn a stoppable thread
///
/// Works similar to like `std::thread::spawn`, except that a
/// `&SimpleAtomicBool` is passed into `f`.
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
            sleep(Duration::from_millis(10));
        }
        count
    });

    assert_eq!(work_work.state(), ThreadState::Running);

    // wait a few cycles
    sleep(Duration::from_millis(100));

    let join_handle = work_work.stop();
    assert_eq!(work_work.state(), ThreadState::Stopping);

    //let result = join_handle.join().unwrap();
    assert_eq!(work_work.state(), ThreadState::Stopped);

    // assume some work has been done
    //assert!(result > 1);
}
