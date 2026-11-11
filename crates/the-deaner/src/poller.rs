//! Background polling that never blocks the caller.
//!
//! Each key (a repo or workspace path) gets at most one job in flight, at most
//! `max_parallel` jobs run at once, and the caller only ever reads the latest
//! result. A slow or hung repo delays its own badge and nothing else.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Entry<T> {
    value: Option<T>,
    checked: Option<Instant>,
    running: bool,
}

struct Inner<T> {
    entries: Mutex<HashMap<PathBuf, Entry<T>>>,
    max_parallel: usize,
}

pub struct Poller<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for Poller<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone + Send + 'static> Poller<T> {
    #[must_use]
    pub fn new(max_parallel: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                entries: Mutex::new(HashMap::new()),
                max_parallel: max_parallel.max(1),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Entry<T>>> {
        // A panicking job must not take polling down with it.
        self.inner
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The latest successful result for `key`.
    #[must_use]
    pub fn get(&self, key: &Path) -> Option<T> {
        self.lock().get(key).and_then(|e| e.value.clone())
    }

    /// Whether a job for `key` is running.
    #[must_use]
    pub fn is_running(&self, key: &Path) -> bool {
        self.lock().get(key).is_some_and(|e| e.running)
    }

    /// Start `job` for `key` on its own thread if the last check is older than
    /// `every`, none is running for this key, and a slot is free. Returns
    /// immediately either way; `done` runs after the result is stored.
    ///
    /// A job that returns `Err` keeps the previous value (a timeout should not
    /// blank a badge) and is retried after `every`.
    pub fn request<E>(
        &self,
        key: &Path,
        every: Duration,
        job: impl FnOnce() -> Result<T, E> + Send + 'static,
        done: impl FnOnce() + Send + 'static,
    ) -> bool {
        {
            let mut entries = self.lock();
            let running = entries.values().filter(|e| e.running).count();
            let entry = entries.entry(key.to_path_buf()).or_insert(Entry {
                value: None,
                checked: None,
                running: false,
            });
            let fresh = entry.checked.is_some_and(|at| at.elapsed() < every);
            if entry.running || fresh || running >= self.inner.max_parallel {
                return false;
            }
            entry.running = true;
        }
        let me = self.clone();
        let key = key.to_path_buf();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            {
                let mut entries = me.lock();
                if let Some(entry) = entries.get_mut(&key) {
                    entry.running = false;
                    entry.checked = Some(Instant::now());
                    if let Ok(Ok(value)) = result {
                        entry.value = Some(value);
                    }
                }
            }
            done();
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_until(mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn slow_key_does_not_block_others() {
        let p: Poller<u32> = Poller::new(4);
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let slow = Path::new("/slow");
        let fast = Path::new("/fast");
        assert!(p.request(
            slow,
            Duration::ZERO,
            move || {
                let _ = release_rx.recv();
                Ok::<_, ()>(1)
            },
            || {}
        ));
        assert!(p.request(fast, Duration::ZERO, || Ok::<_, ()>(2), || {}));
        assert!(
            wait_until(|| p.get(fast) == Some(2)),
            "fast result arrives while slow is stuck"
        );
        assert!(p.is_running(slow) && p.get(slow).is_none());
        release_tx.send(()).unwrap();
        assert!(wait_until(|| p.get(slow) == Some(1)));
    }

    #[test]
    fn one_job_per_key_and_bounded_parallelism() {
        let p: Poller<u32> = Poller::new(2);
        let (tx, rx) = mpsc::channel::<()>();
        let rx = Arc::new(Mutex::new(rx));
        let blocker = |rx: Arc<Mutex<mpsc::Receiver<()>>>| {
            move || {
                let _ = rx.lock().unwrap().recv();
                Ok::<_, ()>(0)
            }
        };
        assert!(p.request(Path::new("/a"), Duration::ZERO, blocker(rx.clone()), || {}));
        assert!(
            !p.request(Path::new("/a"), Duration::ZERO, || Ok::<_, ()>(9), || {}),
            "already running"
        );
        assert!(p.request(Path::new("/b"), Duration::ZERO, blocker(rx.clone()), || {}));
        assert!(
            !p.request(Path::new("/c"), Duration::ZERO, || Ok::<_, ()>(9), || {}),
            "no free slot"
        );
        tx.send(()).unwrap();
        tx.send(()).unwrap();
        assert!(wait_until(
            || !p.is_running(Path::new("/a")) && !p.is_running(Path::new("/b"))
        ));
        assert!(p.request(Path::new("/c"), Duration::ZERO, || Ok::<_, ()>(3), || {}));
    }

    #[test]
    fn errors_keep_last_value_and_respect_interval() {
        let p: Poller<u32> = Poller::new(2);
        let k = Path::new("/k");
        let (done_tx, done_rx) = mpsc::channel();
        let d = done_tx.clone();
        p.request(
            k,
            Duration::ZERO,
            || Ok::<_, ()>(7),
            move || d.send(()).unwrap(),
        );
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let d = done_tx.clone();
        p.request(
            k,
            Duration::ZERO,
            || Err::<u32, _>("timed out"),
            move || d.send(()).unwrap(),
        );
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            p.get(k),
            Some(7),
            "a failed refresh keeps the last good value"
        );
        assert!(
            !p.request(k, Duration::from_secs(60), || Ok::<_, ()>(8), || {}),
            "checked recently"
        );
    }

    #[test]
    fn panicking_job_frees_its_slot() {
        let p: Poller<u32> = Poller::new(1);
        let (tx, rx) = mpsc::channel();
        p.request(
            Path::new("/p"),
            Duration::ZERO,
            || -> Result<u32, ()> { panic!("boom") },
            move || tx.send(()).unwrap(),
        );
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(p.request(Path::new("/q"), Duration::ZERO, || Ok::<_, ()>(1), || {}));
    }
}
