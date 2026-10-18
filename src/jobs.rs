use crate::results::QrResult;
use std::sync::{Arc, Condvar, Mutex};
use tokio::sync::watch;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(pub u64);
#[derive(Debug)]
pub struct DecodeOutcome {
    pub id: JobId,
    pub result: anyhow::Result<Vec<QrResult>>,
}
struct Queue {
    pending: Option<(JobId, image::GrayImage)>,
    generation: u64,
    stopped: bool,
}
/// A single sleeping worker, with at most one replaceable pending frame and
/// one replaceable completed outcome. No timer or idle polling is used.
pub struct Decoder {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    outcomes: watch::Receiver<Option<Arc<DecodeOutcome>>>,
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
        let queue = Arc::new((
            Mutex::new(Queue {
                pending: None,
                generation: 0,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let (sender, outcomes) = watch::channel(None);
        let shared = queue.clone();
        let worker = std::thread::Builder::new()
            .name("qr-decoder".into())
            .spawn(move || {
                let (lock, ready) = &*shared;
                loop {
                    let (id, image, generation) = {
                        let mut state = lock.lock().expect("decoder queue");
                        while state.pending.is_none() && !state.stopped {
                            state = ready.wait(state).expect("decoder queue");
                        }
                        if state.stopped {
                            break;
                        }
                        let (id, image) = state.pending.take().expect("pending decode");
                        (id, image, state.generation)
                    };
                    let result = decode(&image);
                    let state = lock.lock().expect("decoder queue");
                    if !state.stopped && state.generation == generation {
                        sender.send_replace(Some(Arc::new(DecodeOutcome { id, result })));
                    }
                }
            })
            .expect("start decoder thread");
        Self {
            queue,
            outcomes,
            worker: Some(worker),
        }
    }
    pub fn submit(&self, id: JobId, image: image::GrayImage) {
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().expect("decoder queue");
        state.pending = Some((id, image));
        ready.notify_one();
    }
    pub fn cancel(&self) {
        let mut state = self.queue.0.lock().expect("decoder queue");
        state.generation = state.generation.wrapping_add(1);
        state.pending = None;
    }
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<DecodeOutcome>>> {
        self.outcomes.clone()
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        {
            let mut state = self.queue.0.lock().expect("decoder queue");
            state.stopped = true;
            state.pending = None;
            self.queue.1.notify_one();
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
                outcomes.changed().await.unwrap();
                if outcomes.borrow().as_ref().is_some_and(|v| v.id == JobId(3)) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(entered.try_recv().is_err());
        assert_eq!(
            outcomes.borrow().as_ref().unwrap().result.as_ref().unwrap()[0].payload,
            [3]
        );
    }
    #[test]
    fn cancel_discards_pending_and_active_outcome() {
        let (started, entered) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let decoder = Decoder::with_decode(move |_| {
            started.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(vec![])
        });
        let outcomes = decoder.subscribe();
        decoder.submit(JobId(1), image::GrayImage::new(1, 1));
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        decoder.submit(JobId(2), image::GrayImage::new(1, 1));
        decoder.cancel();
        release.send(()).unwrap();
        drop(decoder); // joins worker, so there is no timing assumption in assertions.
        assert!(outcomes.borrow().is_none());
        assert!(entered.try_recv().is_err());
    }
}
