//! Cooperative async task executor.
//!
//! Tasks share one kernel address space and run on the current stack as Rust
//! futures. They must yield or return voluntarily; a task that blocks or loops
//! without yielding stops every other task from making progress.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::task::Wake;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};
use spin::Mutex;

struct TaskState {
    ready: AtomicBool,
}

impl Wake for TaskState {
    fn wake(self: Arc<Self>) {
        self.ready.store(true, Ordering::Release);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.ready.store(true, Ordering::Release);
    }
}

/// A clonable signal that makes a pending task runnable.
///
/// `wake` only updates an atomic flag, so it can be used by interrupt handlers.
#[derive(Clone)]
pub struct TaskHandle {
    state: Arc<TaskState>,
}

impl TaskHandle {
    pub fn wake(&self) {
        self.state.ready.store(true, Ordering::Release);
    }
}

struct Task {
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    state: Arc<TaskState>,
    waker: Waker,
}

/// A cooperative executor. Each call to [`run_ready`](Self::run_ready) polls
/// each runnable task at most once, allowing other tasks to run after a yield.
pub struct Executor {
    tasks: Vec<Task>,
}

impl Executor {
    pub const fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    pub fn spawn<F>(&mut self, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let state = Arc::new(TaskState {
            ready: AtomicBool::new(true),
        });
        let waker = Waker::from(Arc::clone(&state));
        self.tasks.push(Task {
            future: Box::pin(future),
            state: Arc::clone(&state),
            waker,
        });
        TaskHandle { state }
    }

    /// Poll all currently runnable tasks once.
    ///
    /// Returns `true` if at least one task remains runnable and should be
    /// polled again before the CPU idles.
    pub fn run_ready(&mut self) -> bool {
        poll_tasks(&mut self.tasks);
        self.has_ready_tasks()
    }

    pub fn has_ready_tasks(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.state.ready.load(Ordering::Acquire))
    }
}

fn poll_tasks(tasks: &mut Vec<Task>) {
    let mut index = 0;
    while index < tasks.len() {
        let task = &mut tasks[index];
        if !task.state.ready.swap(false, Ordering::AcqRel) {
            index += 1;
            continue;
        }

        let mut context = Context::from_waker(&task.waker);
        match task.future.as_mut().poll(&mut context) {
            Poll::Ready(()) => {
                tasks.swap_remove(index);
            }
            Poll::Pending => index += 1,
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

static EXECUTOR: Mutex<Executor> = Mutex::new(Executor::new());

/// Spawn a task on the global kernel executor.
pub fn spawn<F>(future: F) -> TaskHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    EXECUTOR.lock().spawn(future)
}

/// Poll runnable global tasks. Returns whether runnable work remains.
pub fn run_ready() -> bool {
    let mut tasks = {
        let mut executor = EXECUTOR.lock();
        core::mem::take(&mut executor.tasks)
    };
    poll_tasks(&mut tasks);

    let mut executor = EXECUTOR.lock();
    executor.tasks.extend(tasks);
    executor.has_ready_tasks()
}

/// Check whether a task was woken after the last scheduler pass.
pub fn has_ready_tasks() -> bool {
    EXECUTOR.lock().has_ready_tasks()
}

/// Yield the current async task once so other runnable tasks can be polled.
pub fn yield_now() -> YieldNow {
    YieldNow { yielded: false }
}

pub struct YieldNow {
    yielded: bool,
}

impl Future for YieldNow {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.yielded {
            Poll::Ready(())
        } else {
            this.yielded = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Executor, yield_now};
    use alloc::sync::Arc;
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicBool, Ordering};
    use spin::Mutex;

    #[test]
    fn yielding_task_gives_other_tasks_a_turn() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let first_order = Arc::clone(&order);
        let mut executor = Executor::new();
        executor.spawn(async move {
            first_order.lock().push(1);
            yield_now().await;
            first_order.lock().push(3);
        });

        let second_order = Arc::clone(&order);
        executor.spawn(async move {
            second_order.lock().push(2);
        });

        assert!(executor.run_ready());
        assert_eq!(*order.lock(), [1, 2]);
        assert!(!executor.run_ready());
        assert_eq!(*order.lock(), [1, 2, 3]);
    }

    #[test]
    fn task_handle_wakes_a_pending_task() {
        let completed = Arc::new(AtomicBool::new(false));
        let task_completed = Arc::clone(&completed);
        let mut executor = Executor::new();
        let handle = executor.spawn(core::future::poll_fn(move |_| {
            if task_completed.load(Ordering::Acquire) {
                core::task::Poll::Ready(())
            } else {
                core::task::Poll::Pending
            }
        }));

        assert!(!executor.run_ready());
        completed.store(true, Ordering::Release);
        handle.wake();
        assert!(!executor.run_ready());
    }
}
