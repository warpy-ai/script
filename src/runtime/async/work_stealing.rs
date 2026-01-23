//! Work-stealing executor for multi-threaded async task execution.
//!
//! This module provides a high-performance executor that uses multiple worker threads
//! with work-stealing to efficiently process async tasks across all available CPU cores.

use crossbeam_deque::{Injector, Worker as CbWorker};
use parking::{Parker, Unparker};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::task::{Task, Timer, TASK_SCHEDULED};
use super::worker::Worker;
use super::reactor::ReactorHandle;

/// Handle to a worker thread
struct WorkerHandle {
    /// The worker thread handle
    thread: Option<JoinHandle<()>>,
    /// Unparker to wake up the worker
    unparker: Unparker,
}

/// A multi-threaded work-stealing executor.
///
/// The executor maintains:
/// - A global injector queue where new tasks are spawned
/// - Per-worker local queues with work-stealing between workers
/// - A shared timer for delayed task scheduling
/// - A reactor for I/O event notification
pub struct WorkStealingExecutor {
    /// Global task queue for newly spawned tasks
    injector: Arc<Injector<Arc<Task>>>,
    /// Worker thread handles
    workers: Vec<WorkerHandle>,
    /// All unparkers for waking workers
    unparkers: Arc<Vec<Unparker>>,
    /// Task ID generator
    id_gen: AtomicUsize,
    /// Shared timer
    timer: Arc<std::sync::Mutex<Timer>>,
    /// Reactor for I/O events
    reactor: Arc<std::sync::Mutex<ReactorHandle>>,
    /// Number of worker threads
    num_workers: usize,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
}

impl WorkStealingExecutor {
    /// Create a new work-stealing executor with the specified number of workers.
    ///
    /// If `num_workers` is 0, the number of workers will be automatically
    /// determined based on the number of available CPU cores.
    pub fn new(num_workers: usize) -> io::Result<Self> {
        let num_workers = if num_workers == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            num_workers
        };

        let injector = Arc::new(Injector::new());
        let timer = Arc::new(std::sync::Mutex::new(Timer::new()));
        let reactor = Arc::new(std::sync::Mutex::new(ReactorHandle::new()?));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Create worker local queues and collect stealers
        let locals: Vec<CbWorker<Arc<Task>>> = (0..num_workers)
            .map(|_| CbWorker::new_lifo())
            .collect();
        let stealers: Vec<_> = locals.iter().map(|w| w.stealer()).collect();

        // Create parkers/unparkers for all workers first
        let parker_pairs: Vec<_> = (0..num_workers)
            .map(|_| parking::pair())
            .collect();
        let unparkers: Arc<Vec<Unparker>> = Arc::new(
            parker_pairs.iter().map(|(_, u)| u.clone()).collect()
        );

        // Spawn worker threads
        let mut handles = Vec::with_capacity(num_workers);
        for (index, (local, (parker, unparker))) in locals
            .into_iter()
            .zip(parker_pairs.into_iter())
            .enumerate()
        {
            let worker = Worker {
                index,
                local,
                // Give each worker stealers for all OTHER workers
                stealers: stealers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, s)| s.clone())
                    .collect(),
                injector: injector.clone(),
                timer: timer.clone(),
                shutdown: shutdown.clone(),
                parker,
            };

            let handle = thread::Builder::new()
                .name(format!("tscl-worker-{}", index))
                .spawn(move || worker.run())?;

            handles.push(WorkerHandle {
                thread: Some(handle),
                unparker,
            });
        }

        Ok(Self {
            injector,
            workers: handles,
            unparkers,
            id_gen: AtomicUsize::new(0),
            timer,
            reactor,
            num_workers,
            shutdown,
        })
    }

    /// Spawn a new task on the executor.
    ///
    /// The task will be pushed to the global injector queue and a random
    /// worker will be woken to process it.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.id_gen.fetch_add(1, Ordering::Relaxed);
        let task = Task::new_with_executor(
            id,
            future,
            self.injector.clone(),
            self.unparkers.clone(),
        );
        self.injector.push(task);
        self.wake_one_worker();
    }

    /// Wake a random worker to check for new tasks.
    fn wake_one_worker(&self) {
        if !self.workers.is_empty() {
            let idx = fastrand::usize(..self.workers.len());
            self.workers[idx].unparker.unpark();
        }
    }

    /// Wake all workers.
    fn wake_all_workers(&self) {
        for worker in &self.workers {
            worker.unparker.unpark();
        }
    }

    /// Block on a future until it completes.
    ///
    /// This will poll the future on the current thread while also processing
    /// any I/O events and timers.
    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        let mut pinned = Box::pin(future);

        struct BlockOnWaker {
            woken: AtomicBool,
        }

        impl Wake for BlockOnWaker {
            fn wake(self: Arc<Self>) {
                self.woken.store(true, Ordering::Release);
            }

            fn wake_by_ref(self: &Arc<Self>) {
                self.woken.store(true, Ordering::Release);
            }
        }

        let waker_data = Arc::new(BlockOnWaker {
            woken: AtomicBool::new(false),
        });
        let waker = Waker::from(waker_data.clone());

        loop {
            let mut cx = Context::from_waker(&waker);

            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    // Poll I/O events
                    if let Ok(mut reactor) = self.reactor.try_lock() {
                        let _ = reactor.wait(0); // Non-blocking poll
                    }

                    // Process timers
                    if let Ok(mut timer) = self.timer.try_lock() {
                        let expired = timer.poll();
                        for task in expired {
                            self.injector.push(task);
                            self.wake_one_worker();
                        }
                    }

                    // If we weren't woken, sleep briefly
                    if !waker_data.woken.swap(false, Ordering::Acquire) {
                        std::thread::sleep(Duration::from_micros(100));
                    }
                }
            }
        }
    }

    /// Signal shutdown and wait for all workers to finish.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.wake_all_workers();
    }

    /// Get the number of worker threads.
    pub fn num_workers(&self) -> usize {
        self.num_workers
    }

    /// Get access to the reactor for I/O registration.
    pub fn reactor(&self) -> &Arc<std::sync::Mutex<ReactorHandle>> {
        &self.reactor
    }

    /// Get access to the timer for scheduling delayed tasks.
    pub fn timer(&self) -> &Arc<std::sync::Mutex<Timer>> {
        &self.timer
    }

    /// Schedule a task to run after a delay.
    pub fn schedule_at(&self, at: Instant, task: Arc<Task>) {
        self.timer.lock().unwrap().schedule(at, task);
    }
}

impl Drop for WorkStealingExecutor {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown();

        // Wait for all worker threads to complete
        for handle in &mut self.workers {
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn test_executor_creation() {
        let executor = WorkStealingExecutor::new(2).unwrap();
        assert_eq!(executor.num_workers(), 2);
    }

    #[test]
    fn test_spawn_and_run() {
        let executor = WorkStealingExecutor::new(2).unwrap();
        let counter = Arc::new(AtomicU32::new(0));

        for _ in 0..10 {
            let counter = counter.clone();
            executor.spawn(async move {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }

        // Give workers time to process
        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn test_block_on() {
        let executor = WorkStealingExecutor::new(2).unwrap();
        let result = executor.block_on(async { 42 });
        assert_eq!(result, 42);
    }
}
