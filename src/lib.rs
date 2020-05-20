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
//! Since all stops must happen gracefully, i.e. by requesting the child thread
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

use std::ops::Drop;
use std::mem;
use std::thread::{self, Thread, JoinHandle, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

/// A simplified std::sync::atomic::AtomicBool
pub struct SimpleAtomicBool(AtomicBool);

impl SimpleAtomicBool {
    /// Create a new instance
    pub fn new(v: bool) -> SimpleAtomicBool {
        SimpleAtomicBool(AtomicBool::new(v))
    }

    /// Return the current value
    pub fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Set a new value
    pub fn set(&self, v: bool) {
        self.0.store(v, Ordering::SeqCst)
    }
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

pub fn spawn_with_builder<F, T>(thread_builder: thread::Builder, f: F) -> std::io::Result<StoppableHandle<T>> where
  F: FnOnce(&SimpleAtomicBool) -> T,
  F: Send + 'static, T: Send + 'static {
    let stopped = Arc::new(SimpleAtomicBool::new(false));
    let stopped_w = Arc::downgrade(&stopped);

    let handle = thread_builder.spawn(move || f(&*stopped))?;

    return Ok(StoppableHandle{
        join_handle: handle,
        stopped: stopped_w,
    });
}

/// Guard a stoppable thread
///
/// When `Stopping` is dropped (usually by going out of scope), the contained
/// thread will be stopped.
///
/// Note: This does not guarantee that `stop()` will be called (the original
/// scoped thread was removed from stdlib for this reason).
pub struct Stopping<T> {
    handle: Option<StoppableHandle<T>>
}

impl<T> Stopping<T> {
    pub fn new(handle: StoppableHandle<T>) -> Stopping<T> {
        Stopping{
            handle: Some(handle)
        }
    }
}

impl<T> Drop for Stopping<T> {
    fn drop(&mut self) {
        let handle = mem::replace(&mut self.handle, None);

        if let Some(h) = handle {
            h.stop();
        };
    }
}

/// Guard and join stoppable thread
///
/// Like `Stopping`, but waits for the thread to finish. See notes about
/// guarantees on `Stopping`.
pub struct Joining<T> {
    handle: Option<StoppableHandle<T>>
}

impl<T> Joining<T> {
    pub fn new(handle: StoppableHandle<T>) -> Joining<T> {
        Joining{
            handle: Some(handle)
        }
    }
}

impl<T> Drop for Joining<T> {
    fn drop(&mut self) {
        let handle = mem::replace(&mut self.handle, None);

        if let Some(h) = handle {
            h.stop().join().ok();
        };
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

    // wait a few cycles
    sleep(Duration::from_millis(100));

    let join_handle = work_work.stop();
    let result = join_handle.join().unwrap();

    // assume some work has been done
    assert!(result > 1);
}

#[cfg(test)]
#[test]
fn test_guard() {
    use std::thread::sleep;
    use std::time::Duration;
    use std::sync;

    let stopping_count = sync::Arc::new(sync::Mutex::new(0));
    let joining_count = sync::Arc::new(sync::Mutex::new(0));

    fn count_upwards(stopped: &SimpleAtomicBool,
                     var: sync::Arc<sync::Mutex<u64>>) {
        // increases a mutex-protected counter every 10 ms, exits once the
        // value is > 500
        while !stopped.get() {
            let mut guard = var.lock().unwrap();

            *guard += 1;

            if *guard > 500 {
                break
            }

            sleep(Duration::from_millis(10))
        }
    }

    {
        // seperate scope to cause early Drops
        let scount = stopping_count.clone();
        let stopping = Stopping::new(spawn(move |stopped|
                                     count_upwards(stopped, scount)));

        let jcount = joining_count.clone();
        let joining = Joining::new(spawn(move |stopped|
                                    count_upwards(stopped, jcount)));
        sleep(Duration::from_millis(1))
    }

    // threads should not have counted far
    sleep(Duration::from_millis(100));

    let sc = stopping_count.lock().unwrap();
    assert!(*sc > 1 && *sc < 5);
    let jc = joining_count.lock().unwrap();
    assert!(*sc > 1 && *jc < 5);
}


#[cfg(test)]
#[test]
fn test_stoppable_thead_builder_with_name() {
    use std::thread::sleep;
    use std::time::Duration;

    let thread_name = "test_builder";
    let thread_builder = thread::Builder::new().name(String::from(thread_name));

    let spawn_result = spawn_with_builder(thread_builder, |stopped| {
        let mut count: u64 = 0;
        while !stopped.get() {
            count += 1;
            sleep(Duration::from_millis(10));
        }
        count
    });

    // wait a few cycles
    sleep(Duration::from_millis(100));

    let stoppable_handle = spawn_result.unwrap();
    assert!(stoppable_handle.thread().name().unwrap() == thread_name);

    let join_handle = stoppable_handle.stop();
    let result = join_handle.join().unwrap();

    // assume some work has been done
    assert!(result > 1);
}