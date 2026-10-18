use crate::results::QrResult;
use std::sync::{Arc, Condvar, Mutex};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(pub u64);
#[derive(Debug)]
pub struct DecodeOutcome {
    pub id: JobId,
    pub result: anyhow::Result<Vec<QrResult>>,
}
struct Queue {
    pending: Option<(JobId, Arc<image::GrayImage>)>,
    completed: Option<Arc<DecodeOutcome>>,
    generation: u64,
    stopped: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    completed: tokio::sync::Notify,
}
/// A bounded, lossless receiver for completed jobs. The app owns one consumer.
pub struct Outcomes(Arc<Shared>);
impl Outcomes {
    pub async fn recv(&mut self) -> Option<Arc<DecodeOutcome>> {
        loop {
            let notified = self.0.completed.notified();
            {
                let mut state = self.0.queue.lock().expect("decoder queue");
                if let Some(outcome) = state.completed.take() {
                    self.0.ready.notify_one();
                    return Some(outcome);
                }
                if state.stopped {
                    return None;
                }
            }
            // notify_one retains a permit if publication raced the await.
            notified.await;
        }
    }
}
/// A single sleeping worker with one replaceable pending input and one lossless
/// completed-output slot. A stalled consumer backpressures further decoding.
/// Cancellation and shutdown wake both input and output waits; no polling.
pub struct Decoder {
    shared: Arc<Shared>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
impl Decoder {
    pub fn new() -> Self {
        Self::with_decode(crate::decode::decode)
    }
    fn with_decode(
        decode: impl Fn(&image::GrayImage) -> anyhow::Result<Vec<QrResult>> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                pending: None,
                completed: None,
                generation: 0,
                stopped: false,
            }),
            ready: Condvar::new(),
            completed: tokio::sync::Notify::new(),
        });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("qr-decoder".into())
            .spawn(move || {
                let shared = worker_shared;
                loop {
                    let (id, image, generation) = {
                        let mut state = shared.queue.lock().expect("decoder queue");
                        while state.pending.is_none() && !state.stopped {
                            state = shared.ready.wait(state).expect("decoder queue");
                        }
                        if state.stopped {
                            break;
                        }
                        let (id, image) = state.pending.take().expect("pending decode");
                        (id, image, state.generation)
                    };
                    let result = decode(&image);
                    let mut state = shared.queue.lock().expect("decoder queue");
                    while state.completed.is_some()
                        && state.generation == generation
                        && !state.stopped
                    {
                        state = shared.ready.wait(state).expect("decoder queue");
                    }
                    if state.stopped {
                        break;
                    }
                    if state.generation == generation {
                        state.completed = Some(Arc::new(DecodeOutcome { id, result }));
                        shared.completed.notify_one();
                    }
                }
            })
            .expect("start decoder thread");
        Self {
            shared,
            worker: Some(worker),
        }
    }
    /// GrayImage transfers ownership; Arc<GrayImage> shares pixels without copying.
    pub fn submit(&self, id: JobId, image: impl Into<Arc<image::GrayImage>>) {
        let mut state = self.shared.queue.lock().expect("decoder queue");
        state.pending = Some((id, image.into()));
        self.shared.ready.notify_one();
    }
    pub fn cancel(&self) {
        let mut state = self.shared.queue.lock().expect("decoder queue");
        state.generation = state.generation.wrapping_add(1);
        state.pending = None;
        state.completed = None;
        self.shared.ready.notify_one();
    }
    pub fn subscribe(&self) -> Outcomes {
        Outcomes(self.shared.clone())
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        {
            let mut state = self.shared.queue.lock().expect("decoder queue");
            state.stopped = true;
            state.pending = None;
            state.completed = None;
            self.shared.ready.notify_one();
            self.shared.completed.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    #[tokio::test]
    async fn serialized_execution_and_bounded_replacement() {
        let (started, entered) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let decoder = Decoder::with_decode(move |image| {
            let marker = image.get_pixel(0, 0)[0];
            started.send(marker).unwrap();
            if marker == 1 {
                blocked.lock().unwrap().recv().unwrap();
            }
            Ok(vec![QrResult::from_payload(vec![marker])])
        });
        let mut outcomes = decoder.subscribe();
        decoder.submit(
            JobId(1),
            image::GrayImage::from_pixel(1, 1, image::Luma([1])),
        );
        assert_eq!(entered.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        decoder.submit(
            JobId(2),
            image::GrayImage::from_pixel(1, 1, image::Luma([2])),
        );
        decoder.submit(
            JobId(3),
            image::GrayImage::from_pixel(1, 1, image::Luma([3])),
        );
        assert!(entered.try_recv().is_err());
        release.send(()).unwrap();
        assert_eq!(entered.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let outcome = outcomes.recv().await.unwrap();
                if outcome.id == JobId(3) {
                    assert_eq!(outcome.result.as_ref().unwrap()[0].payload, [3]);
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(entered.try_recv().is_err());
    }
    #[tokio::test]
    async fn cancel_discards_pending_and_active_outcome() {
        let (started, entered) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let decoder = Decoder::with_decode(move |_| {
            started.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(vec![])
        });
        let mut outcomes = decoder.subscribe();
        decoder.submit(JobId(1), image::GrayImage::new(1, 1));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(JobId(2), image::GrayImage::new(1, 1));
        decoder.cancel();
        release.send(()).unwrap();
        drop(decoder); // joins worker, so there is no timing assumption in assertions.
        assert!(outcomes.recv().await.is_none());
        assert!(entered.try_recv().is_err());
    }
    #[tokio::test]
    async fn stalled_consumer_preserves_each_completed_payload() {
        let (started, entered) = mpsc::channel();
        let decoder = Decoder::with_decode(move |image| {
            let marker = image.get_pixel(0, 0)[0];
            started.send(marker).unwrap();
            Ok(vec![QrResult::from_payload(vec![marker])])
        });
        let mut outcomes = decoder.subscribe();
        decoder.submit(
            JobId(1),
            image::GrayImage::from_pixel(1, 1, image::Luma([1])),
        );
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(
            JobId(2),
            image::GrayImage::from_pixel(1, 1, image::Luma([2])),
        );
        // Second decode starts only after the first completed outcome was published.
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(
            JobId(3),
            image::GrayImage::from_pixel(1, 1, image::Luma([3])),
        );
        assert!(
            entered.recv_timeout(Duration::from_millis(200)).is_err(),
            "worker must stop at full completed-output queue"
        );
        let first = tokio::time::timeout(Duration::from_secs(3), async {
            outcomes.recv().await.unwrap()
        })
        .await
        .unwrap();
        assert_eq!(first.id, JobId(1));
        let second = tokio::time::timeout(Duration::from_secs(3), async {
            outcomes.recv().await.unwrap()
        })
        .await
        .unwrap();
        assert_eq!(second.id, JobId(2));
        assert_eq!(first.result.as_ref().unwrap()[0].payload, [1]);
        assert_eq!(second.result.as_ref().unwrap()[0].payload, [2]);
    }

    #[test]
    fn cancellation_and_shutdown_wake_output_backpressure() {
        let (started, entered) = mpsc::channel();
        let decoder = Decoder::with_decode(move |image| {
            let marker = image.get_pixel(0, 0)[0];
            started.send(marker).unwrap();
            Ok(vec![QrResult::from_payload(vec![marker])])
        });
        let _outcomes = decoder.subscribe();
        decoder.submit(
            JobId(1),
            image::GrayImage::from_pixel(1, 1, image::Luma([1])),
        );
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(
            JobId(2),
            image::GrayImage::from_pixel(1, 1, image::Luma([2])),
        );
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(
            JobId(3),
            image::GrayImage::from_pixel(1, 1, image::Luma([3])),
        );
        assert!(
            entered.recv_timeout(Duration::from_millis(200)).is_err(),
            "consumer should backpressure pending job 3"
        );
        decoder.cancel();
        decoder.submit(
            JobId(4),
            image::GrayImage::from_pixel(1, 1, image::Luma([4])),
        );
        assert_eq!(entered.recv_timeout(Duration::from_secs(3)).unwrap(), 4);
        decoder.submit(
            JobId(5),
            image::GrayImage::from_pixel(1, 1, image::Luma([5])),
        );
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(
            JobId(6),
            image::GrayImage::from_pixel(1, 1, image::Luma([6])),
        );
        assert!(
            entered.recv_timeout(Duration::from_millis(200)).is_err(),
            "consumer should backpressure pending job 6"
        );
        // Keep the consumer alive but unread: shutdown must wake a full output wait.
        let (finished, done) = mpsc::channel();
        std::thread::spawn(move || {
            drop(decoder);
            finished.send(()).unwrap();
        });
        done.recv_timeout(Duration::from_secs(3))
            .expect("shutdown deadlocked on unread output");
    }
}
