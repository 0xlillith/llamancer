//! A single bounded background worker with cooperative cancellation.
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
};

pub struct Job<T> {
    cancel: Arc<AtomicBool>,
    result: Receiver<io::Result<T>>,
    worker: Option<JoinHandle<()>>,
}
impl<T: Send + 'static> Job<T> {
    pub fn start(
        task: impl FnOnce(Arc<AtomicBool>) -> io::Result<T> + Send + 'static,
    ) -> io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancel);
        let (tx, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("llamancer-worker".into())
            .spawn(move || {
                let outcome = task(token);
                let _ = tx.send(outcome);
            })?;
        Ok(Self {
            cancel,
            result,
            worker: Some(worker),
        })
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn poll(&mut self) -> Option<io::Result<T>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("Background worker disconnected")))
            }
        }
    }
    pub fn finish(mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completes_and_cancels() {
        let mut job = Job::start(|cancel| {
            while !cancel.load(Ordering::Acquire) {
                thread::yield_now();
            }
            Ok(42)
        })
        .unwrap();
        job.cancel();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(result) = job.poll() {
                assert_eq!(result.unwrap(), 42);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(1));
        }
        job.finish();
    }
}
