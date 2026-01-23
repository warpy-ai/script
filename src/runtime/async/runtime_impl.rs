use std::collections::VecDeque;
use std::io;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use super::reactor::ReactorHandle;
use super::task::{Executor, Task, Timer};
use super::{Interest, Token};

#[cfg(feature = "work-stealing")]
use super::work_stealing::WorkStealingExecutor;

// ============================================================================
// Single-threaded Runtime (default)
// ============================================================================

#[cfg(not(feature = "work-stealing"))]
pub struct Runtime {
    executor: Executor,
    reactor: Mutex<ReactorHandle>,
    parked: Mutex<Vec<Arc<Task>>>,
    io_events: Mutex<Vec<(Token, Interest)>>,
    threads: Vec<thread::JoinHandle<()>>,
}

#[cfg(not(feature = "work-stealing"))]
impl Runtime {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            executor: Executor::new(),
            reactor: Mutex::new(ReactorHandle::new()?),
            parked: Mutex::new(Vec::new()),
            io_events: Mutex::new(Vec::new()),
            threads: Vec::new(),
        })
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.executor.spawn_task(future);
    }

    pub fn block_on<F>(&mut self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        let mut pinned = Box::pin(future);
        let waker = Arc::new(WakerData {
            woken: AtomicUsize::new(0),
        }) as Arc<dyn Wake>;

        loop {
            let mut cx = Context::from_waker(&Waker::from(waker.clone()));

            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    if waker.woken.load(Ordering::Relaxed) == 0 {
                        self.tick();
                    }
                }
            }
        }
    }

    pub fn tick(&mut self) {
        let timeout = self
            .executor
            .timer
            .lock()
            .unwrap()
            .next_deadline()
            .map(|d| {
                let now = Instant::now();
                if d > now {
                    d.duration_since(now).as_millis() as i32
                } else {
                    0
                }
            })
            .unwrap_or(-1);

        let events = self
            .reactor
            .lock()
            .unwrap()
            .wait(timeout)
            .unwrap_or_default();
        self.io_events.lock().unwrap().extend(events);

        // Process expired timers
        for task in self.executor.timer.lock().unwrap().poll() {
            self.executor.schedule(task);
        }

        // Process all tasks
        let mut pending = VecDeque::new();
        while let Some(task) = self.executor.tasks.pop_front() {
            if task.is_completed() {
                continue;
            }

            let waker_data = Arc::new(WakerData::new());
            let waker = Waker::from(waker_data as Arc<dyn Wake>);
            let mut cx = Context::from_waker(&waker);

            task.poll(&mut cx);

            if !task.is_completed() {
                pending.push_back(task);
            }
        }
        self.executor.tasks = pending;

        self.io_events.lock().unwrap().clear();
    }

    pub fn run<F>(&mut self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        self.block_on(future)
    }
}

// ============================================================================
// Multi-threaded Work-Stealing Runtime
// ============================================================================

#[cfg(feature = "work-stealing")]
pub struct Runtime {
    executor: WorkStealingExecutor,
    parked: Mutex<Vec<Arc<Task>>>,
    io_events: Mutex<Vec<(Token, Interest)>>,
}

#[cfg(feature = "work-stealing")]
impl Runtime {
    /// Create a new multi-threaded runtime with work-stealing.
    ///
    /// The number of worker threads is automatically determined based on
    /// the number of available CPU cores.
    pub fn new() -> io::Result<Self> {
        Self::with_workers(0)
    }

    /// Create a runtime with a specific number of worker threads.
    ///
    /// If `num_workers` is 0, uses the number of available CPU cores.
    pub fn with_workers(num_workers: usize) -> io::Result<Self> {
        Ok(Self {
            executor: WorkStealingExecutor::new(num_workers)?,
            parked: Mutex::new(Vec::new()),
            io_events: Mutex::new(Vec::new()),
        })
    }

    /// Spawn a task on the executor.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.executor.spawn(future);
    }

    /// Block the current thread until the future completes.
    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        self.executor.block_on(future)
    }

    /// Run a future to completion.
    pub fn run<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        self.block_on(future)
    }

    /// Get the number of worker threads.
    pub fn num_workers(&self) -> usize {
        self.executor.num_workers()
    }

    /// Shutdown the runtime gracefully.
    pub fn shutdown(&self) {
        self.executor.shutdown();
    }
}

// ============================================================================
// Shared utilities
// ============================================================================

struct WakerData {
    woken: AtomicUsize,
}

impl WakerData {
    fn new() -> Self {
        Self {
            woken: AtomicUsize::new(0),
        }
    }
}

impl Wake for WakerData {
    fn wake(self: Arc<Self>) {
        self.woken.store(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(1, Ordering::Relaxed);
    }
}

pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    #[cfg(not(feature = "work-stealing"))]
    {
        Runtime::new().unwrap().spawn(future);
    }
    #[cfg(feature = "work-stealing")]
    {
        Runtime::new().unwrap().spawn(future);
    }
}

pub fn sleep(duration: Duration) -> Sleep {
    Sleep {
        deadline: Instant::now() + duration,
    }
}

pub struct Sleep {
    deadline: Instant,
}

impl std::future::Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        if Instant::now() >= self.deadline {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
