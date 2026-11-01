//! The one worker thread that drives a picker's asynchronous work: the live
//! feed, previews, and actions.
//!
//! Jobs are spawned with [`Background::spawn`]. Shutting down drops every job
//! still running, which cancels its futures; commands started through
//! `dots-process` are killed with them. Nothing here may block the runtime:
//! jobs use asynchronous I/O only.
//!
//! A job that panics is a defect, not a stale view. The picker polls
//! [`Background::failed`] to stop early, and [`Background::shutdown`] returns
//! the panic even when it happened after the last poll, so the picker can
//! restore the terminal and resume it on its own thread. Cancellation by
//! shutdown is expected and ignored.

use std::any::Any;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::thread;

use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::{JoinError, JoinHandle};

#[derive(Debug)]
pub(crate) struct Background {
    handle: Handle,
    jobs: Vec<JoinHandle<()>>,
    panic: Option<Box<dyn Any + Send>>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Background {
    pub(crate) fn start() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let handle = runtime.handle().clone();
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = thread::Builder::new()
            .name("picker-background".to_owned())
            .spawn(move || {
                // Spawned jobs run while this drives the runtime; dropping the
                // runtime afterwards drops every job that is left.
                runtime.block_on(async {
                    let _ = stopped.await;
                });
                drop(runtime);
            })?;
        Ok(Self {
            handle,
            jobs: Vec::new(),
            panic: None,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// Runs `job` until it finishes or the background shuts down.
    pub(crate) fn spawn(&mut self, job: impl Future<Output = ()> + Send + 'static) {
        self.jobs.push(self.handle.spawn(job));
    }

    /// Whether a job has panicked so far. The panic is kept for
    /// [`Self::shutdown`].
    ///
    /// # Panics
    ///
    /// When a job was cancelled while the background runs, which only
    /// shutting down does.
    pub(crate) fn failed(&mut self) -> bool {
        for result in self.finished() {
            if let Err(error) = result {
                match error.try_into_panic() {
                    Ok(panic) => {
                        self.panic.get_or_insert(panic);
                    }
                    Err(cancelled) => {
                        panic!("a picker job was cancelled while the picker ran: {cancelled}")
                    }
                }
            }
        }
        self.panic.is_some()
    }

    /// Stops every job and waits for the worker thread to exit. Returns the
    /// first job panic, whenever it happened, else a panic of the worker
    /// thread itself.
    pub(crate) fn shutdown(mut self) -> thread::Result<()> {
        let joined = self.stop_and_join();
        // With the runtime gone, every job has finished or been cancelled.
        for result in self.finished() {
            if let Err(error) = result
                && error.is_panic()
            {
                self.panic.get_or_insert(error.into_panic());
            }
        }
        match self.panic.take() {
            Some(panic) => Err(panic),
            None => joined,
        }
    }

    /// Removes the jobs that have finished and returns their results.
    fn finished(&mut self) -> Vec<Result<(), JoinError>> {
        let mut results = Vec::new();
        self.jobs.retain_mut(|job| {
            if !job.is_finished() {
                return true;
            }
            match Pin::new(job).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(result) => results.push(result),
                Poll::Pending => unreachable!("a finished job has its result"),
            }
            false
        });
        results
    }

    fn stop_and_join(&mut self) -> thread::Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        match self.thread.take() {
            Some(thread) => thread.join(),
            None => Ok(()),
        }
    }
}

impl Drop for Background {
    /// Reached without [`Background::shutdown`] only while unwinding, when
    /// the first panic is the one to report.
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::*;

    struct Flag(Arc<AtomicBool>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn shutdown_drops_jobs_that_never_finish() {
        let mut background = Background::start().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = Flag(Arc::clone(&dropped));
        background.spawn(async move {
            let _flag = flag;
            std::future::pending::<()>().await;
        });
        background.spawn(async {});
        std::thread::sleep(Duration::from_millis(20));
        assert!(!dropped.load(Ordering::SeqCst));
        assert!(!background.failed(), "finishing is not failing");
        assert!(background.shutdown().is_ok(), "cancelled by shutdown");
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn a_job_that_panics_is_reported_not_silently_lost() {
        let mut background = Background::start().unwrap();
        background.spawn(async { panic!("job invariant") });
        let start = Instant::now();
        while !background.failed() {
            assert!(start.elapsed() < Duration::from_secs(5), "never reported");
            std::thread::sleep(Duration::from_millis(5));
        }
        let panic = background.shutdown().unwrap_err();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"job invariant"));
    }

    /// Unwinding drops this, telling the test the panic is under way.
    struct Unwinding(std::sync::mpsc::Sender<()>);
    impl Drop for Unwinding {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    /// A job that panics once `go` fires, signalling `unwinding` as it does.
    pub(crate) fn panics_on(
        background: &mut Background,
    ) -> (oneshot::Sender<()>, std::sync::mpsc::Receiver<()>) {
        let (go, wait) = oneshot::channel();
        let (unwinding, observed) = std::sync::mpsc::channel();
        background.spawn(async move {
            let _ = wait.await;
            let _unwinding = Unwinding(unwinding);
            panic!("job invariant");
        });
        (go, observed)
    }

    #[test]
    fn shutdown_reports_a_panic_after_the_last_check() {
        let mut background = Background::start().unwrap();
        let (go, unwinding) = panics_on(&mut background);
        assert!(!background.failed());
        go.send(()).unwrap();
        // The job's poll is still unwinding on the worker thread, which cannot
        // see the stop signal until it has recorded the panic.
        unwinding.recv_timeout(Duration::from_secs(5)).unwrap();
        let panic = background.shutdown().unwrap_err();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"job invariant"));
    }
}
