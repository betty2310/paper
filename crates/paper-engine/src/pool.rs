//! A small fixed thread pool for work that must not run on the caller's
//! thread: lookups, downloads, imports. `run` turns a blocking closure into a
//! future, which is what UniFFI needs to expose an `async fn` to Swift.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use paper_core::PaperError;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct ThreadPool {
    sender: Option<Sender<Job>>,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = (0..size)
            .map(|index| {
                let receiver = Arc::clone(&receiver);
                thread::Builder::new()
                    .name(format!("paper-engine-{index}"))
                    .spawn(move || worker(receiver))
                    .expect("spawning an engine thread")
            })
            .collect();
        ThreadPool {
            sender: Some(sender),
            workers,
        }
    }

    /// Runs `job` on a pool thread and resolves with its result. Resolves with
    /// `PaperError::Closed` if the pool shut down before the job ran.
    pub fn run<T: Send + 'static, F: FnOnce() -> T + Send + 'static>(
        &self,
        job: F,
    ) -> impl Future<Output = Result<T, PaperError>> + Send + use<T, F> {
        let (sender, receiver) = futures_channel::oneshot::channel();
        let queued = self
            .sender
            .as_ref()
            .and_then(|pool| {
                pool.send(Box::new(move || {
                    let _ = sender.send(job());
                }))
                .ok()
            })
            .is_some();
        async move {
            if !queued {
                return Err(PaperError::Closed);
            }
            receiver.await.map_err(|_| PaperError::Closed)
        }
    }
}

impl Drop for ThreadPool {
    /// Closes the queue and waits for running jobs. Queued jobs are dropped.
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
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
    fn dropped_pool_resolves_pending_futures_with_closed() {
        let pool = ThreadPool::new(1);
        let (block_sender, block_receiver) = channel::<()>();
        let blocker = pool.run(move || {
            let _ = block_receiver.recv();
        });
        let queued = pool.run(|| 1);
        drop(block_sender);
        drop(pool);
        assert_eq!(pollster::block_on(blocker), Ok(()));
        // The queued job may have run or been dropped depending on timing; both are fine.
        assert!(matches!(
            pollster::block_on(queued),
            Ok(1) | Err(PaperError::Closed)
        ));
    }
}
