//! A small fixed thread pool for work that must not run on the caller's
//! thread: lookups, downloads, imports. `run` turns a blocking closure into a
//! future, which is what UniFFI needs to expose an `async fn` to Swift.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Why a job did not produce a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolError {
    /// The pool shut down before the job ran.
    Closed,
    /// The job panicked. The worker thread survives.
    Panicked(String),
}

pub struct ThreadPool {
    sender: Option<Sender<Job>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..size {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("paper-engine-{index}"))
                .spawn(move || worker(receiver))
                .expect("spawning an engine thread");
        }
        ThreadPool {
            sender: Some(sender),
        }
    }

    /// Runs `job` on a pool thread and resolves with its result.
    pub fn run<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
        &self,
        job: F,
    ) -> impl Future<Output = Result<T, PoolError>> + Send + use<T, F> {
        let (sender, receiver) = futures_channel::oneshot::channel();
        let queued = self
            .sender
            .as_ref()
            .and_then(|pool| {
                pool.send(Box::new(move || {
                    let result = catch_unwind(AssertUnwindSafe(job)).map_err(|payload| {
                        let message = payload
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or_else(|| "unknown panic".to_string());
                        tracing::error!(message, "engine job panicked");
                        PoolError::Panicked(message)
                    });
                    let _ = sender.send(result);
                }))
                .ok()
            })
            .is_some();
        async move {
            if !queued {
                return Err(PoolError::Closed);
            }
            receiver.await.map_err(|_| PoolError::Closed)?
        }
    }
}

impl Drop for ThreadPool {
    /// Closes the queue. Queued jobs are dropped; running jobs finish on their
    /// own threads, so dropping the pool never blocks the caller on a slow
    /// network request.
    fn drop(&mut self) {
        self.sender.take();
    }
}

fn worker(receiver: Arc<Mutex<Receiver<Job>>>) {
    loop {
        let job = receiver.lock().unwrap().recv();
        match job {
            Ok(job) => job(),
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn jobs_run_on_pool_threads_and_results_come_back() {
        let pool = ThreadPool::new(2);
        let caller = thread::current().id();
        let futures: Vec<_> = (0..8)
            .map(|i| pool.run(move || (i * 2, thread::current().id())))
            .collect();
        for (i, future) in futures.into_iter().enumerate() {
            let (value, thread_id) = pollster::block_on(future).unwrap();
            assert_eq!(value, i * 2);
            assert_ne!(thread_id, caller);
        }
    }

    #[test]
    fn dropping_the_pool_does_not_wait_for_running_jobs() {
        let pool = ThreadPool::new(1);
        let (block_sender, block_receiver) = channel::<()>();
        let blocker = pool.run(move || {
            let _ = block_receiver.recv();
            7
        });
        let queued = pool.run(|| 1);
        let started = Instant::now();
        drop(pool);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "drop returned only after {:?}",
            started.elapsed()
        );
        drop(block_sender);
        assert_eq!(pollster::block_on(blocker), Ok(7), "running jobs finish");
        // The queued job may have run or been dropped depending on timing; both are fine.
        assert!(matches!(
            pollster::block_on(queued),
            Ok(1) | Err(PoolError::Closed)
        ));
    }

    #[test]
    fn a_panicking_job_is_reported_and_the_worker_survives() {
        let pool = ThreadPool::new(1);
        let result = pollster::block_on(pool.run(|| panic!("boom {}", 1)));
        assert_eq!(result, Err(PoolError::Panicked("boom 1".to_string())));
        assert_eq!(
            pollster::block_on(pool.run(|| 2)),
            Ok(2),
            "the only worker still runs jobs"
        );
    }
}
