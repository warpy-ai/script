//! Worker thread implementation for the work-stealing executor.
//!
//! Each worker maintains a local LIFO queue and can steal from other workers' queues
//! when its own queue is empty, providing good cache locality while ensuring load balancing.

use crossbeam_deque::{Injector, Stealer, Worker as CbWorker, Steal};
use parking::Parker;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::task::{Task, Timer};

/// A worker thread that processes tasks from its local queue and steals from others.
pub struct Worker {
    /// Worker index (for debugging/monitoring)
    pub index: usize,
    /// Local work queue (LIFO for cache locality)
    pub local: CbWorker<Arc<Task>>,
    /// Stealers for sibling workers' queues
    pub stealers: Vec<Stealer<Arc<Task>>>,
    /// Global injector queue for newly spawned tasks
    pub injector: Arc<Injector<Arc<Task>>>,
    /// Shared timer for scheduling delayed tasks
    pub timer: Arc<std::sync::Mutex<Timer>>,
    /// Shutdown signal
    pub shutdown: Arc<AtomicBool>,
    /// Parker for efficient thread parking when no work available
    pub parker: Parker,
}

impl Worker {
    /// Main worker loop - runs until shutdown signal received
    pub fn run(&self) {
        while !self.shutdown.load(Ordering::Relaxed) {
            match self.find_task() {
                Some(task) => self.execute_task(task),
                None => {
                    // No tasks available, process timers and park
                    self.process_timers();
                    // Park with timeout to periodically check for timers and shutdown
                    self.parker.park_timeout(Duration::from_millis(1));
                }
            }
        }

        // Drain remaining tasks on shutdown
        self.drain_tasks();
    }

    /// Find a task to execute using the work-stealing algorithm
    fn find_task(&self) -> Option<Arc<Task>> {
        // 1. Try local queue first (LIFO - better cache locality)
        if let Some(task) = self.local.pop() {
            return Some(task);
        }

        // 2. Try stealing from the global injector
        if let Some(task) = self.steal_from_injector() {
            return Some(task);
        }

        // 3. Try stealing from sibling workers
        self.steal_from_siblings()
    }

    /// Steal a batch of tasks from the global injector queue
    fn steal_from_injector(&self) -> Option<Arc<Task>> {
        loop {
            match self.injector.steal_batch_and_pop(&self.local) {
                Steal::Success(task) => return Some(task),
                Steal::Empty => return None,
                Steal::Retry => continue,
            }
        }
    }

    /// Steal a task from a random sibling worker
    fn steal_from_siblings(&self) -> Option<Arc<Task>> {
        if self.stealers.is_empty() {
            return None;
        }

        // Start from a random index to avoid contention and ensure fairness
        let start = fastrand::usize(..self.stealers.len());

        for i in 0..self.stealers.len() {
            let idx = (start + i) % self.stealers.len();
            loop {
                match self.stealers[idx].steal() {
                    Steal::Success(task) => return Some(task),
                    Steal::Empty => break, // Try next stealer
                    Steal::Retry => continue, // Retry same stealer
                }
            }
        }
        None
    }

    /// Execute a task using thread-safe polling
    fn execute_task(&self, task: Arc<Task>) {
        task.poll_threadsafe();
    }

    /// Process expired timers and add them to the local queue
    fn process_timers(&self) {
        if let Ok(mut timer) = self.timer.try_lock() {
            let expired = timer.poll();
            for task in expired {
                self.local.push(task);
            }
        }
    }

    /// Drain any remaining tasks when shutting down
    fn drain_tasks(&self) {
        // Execute remaining local tasks
        while let Some(task) = self.local.pop() {
            task.poll_threadsafe();
        }

        // Try to help drain the injector
        loop {
            match self.injector.steal() {
                Steal::Success(task) => {
                    task.poll_threadsafe();
                }
                Steal::Empty => break,
                Steal::Retry => continue,
            }
        }
    }

    /// Push a task to this worker's local queue
    pub fn push_local(&self, task: Arc<Task>) {
        self.local.push(task);
    }
}
