use crate::jobs::JobId;
use anyhow::Context;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub label: String,
}
impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.label.fmt(f)
    }
}
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}
impl Frame {
    pub fn new(width: u32, height: u32, rgb: Vec<u8>) -> anyhow::Result<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(3));
        anyhow::ensure!(
            width > 0
                && height > 0
                && width <= 8192
                && height <= 8192
                && expected == Some(rgb.len()),
            "Malformed or oversized camera RGB frame"
        );
        Ok(Self { width, height, rgb })
    }
}
trait Source: Send + 'static {
    fn next(&mut self) -> anyhow::Result<Option<(Duration, Frame)>>;
}
#[derive(Default)]
struct Rates {
    preview: Option<Duration>,
    decode: Option<Duration>,
}
impl Rates {
    fn due(&mut self, now: Duration) -> (bool, bool) {
        fn check(last: &mut Option<Duration>, now: Duration, interval: Duration) -> bool {
            if last.is_none_or(|old| now.saturating_sub(old) >= interval) {
                *last = Some(now);
                true
            } else {
                false
            }
        }
        (
            check(&mut self.preview, now, Duration::from_nanos(66_666_667)),
            check(&mut self.decode, now, Duration::from_millis(200)),
        )
    }
}
#[derive(Debug)]
pub enum Event {
    Frame {
        preview: Option<iced::widget::image::Handle>,
        gray: Option<Arc<image::GrayImage>>,
    },
    Error(String),
    Stopped,
}
#[derive(Debug)]
pub struct Outcome {
    pub id: JobId,
    pub event: Event,
}
pub struct CameraSession {
    stop: Arc<AtomicBool>,
    events: tokio::sync::watch::Sender<Option<Arc<Outcome>>>,
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl CameraSession {
    fn spawn(
        id: JobId,
        factory: impl FnOnce(&AtomicBool) -> anyhow::Result<Box<dyn Source>> + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (events, _) = tokio::sync::watch::channel(None);
        let stopping = stop.clone();
        let output = events.clone();
        let worker = std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                let mut source = factory(&stopping)?;
                let mut rates = Rates::default();
                while !stopping.load(Ordering::Acquire) {
                    let Some((now, frame)) = source.next()? else {
                        continue;
                    };
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    let (preview, decode) = rates.due(now);
                    if !preview && !decode {
                        continue;
                    }
                    let rgb = image::RgbImage::from_raw(frame.width, frame.height, frame.rgb)
                        .ok_or_else(|| anyhow::anyhow!("Malformed camera frame"))?;
                    let gray = decode.then(|| Arc::new(image::imageops::grayscale(&rgb)));
                    let preview = preview.then(|| {
                        let rgba = image::DynamicImage::ImageRgb8(rgb).into_rgba8();
                        iced::widget::image::Handle::from_rgba(
                            frame.width,
                            frame.height,
                            rgba.into_raw(),
                        )
                    });
                    let event = Event::Frame { preview, gray };
                    if !stopping.load(Ordering::Acquire) {
                        output.send_replace(Some(Arc::new(Outcome { id, event })));
                    }
                }
                Ok(())
            })(); // source and native mappings are released before acknowledging termination.
            let event = match result {
                Err(e) if !stopping.load(Ordering::Acquire) => Event::Error(format!("{e:#}")),
                _ => Event::Stopped,
            };
            output.send_replace(Some(Arc::new(Outcome { id, event })));
        });
        Self {
            stop,
            events,
            worker: std::sync::Mutex::new(Some(worker)),
        }
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.events.send_replace(None);
    }
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<Option<Arc<Outcome>>> {
        self.events.subscribe()
    }
    pub fn join(&self) {
        self.stop();
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}
impl Drop for CameraSession {
    fn drop(&mut self) {
        self.stop();
    }
}
/// Call from a blocking task: enumeration opens device nodes to check capabilities.
pub fn devices() -> anyhow::Result<Vec<Device>> {
    use v4l::video::Capture;
    let mut found = Vec::new();
    let mut denied = None;
    for node in v4l::context::enum_devices() {
        let device = match v4l::Device::with_path(node.path()) {
            Ok(d) => d,
            Err(e) => {
                denied = Some(e);
                continue;
            }
        };
        if let Ok(formats) = device.enum_formats()
            && formats
                .iter()
                .any(|f| matches!(&f.fourcc.repr, b"MJPG" | b"YUYV" | b"RGB3" | b"GREY"))
        {
            let id = node.path().to_string_lossy().into_owned();
            found.push(Device {
                label: format!("{} ({id})", node.name().unwrap_or_else(|| "Camera".into())),
                id,
            });
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    if found.is_empty()
        && let Some(error) = denied
    {
        return Err(error.into());
    }
    Ok(found)
}
/// Starts an on-demand worker. Opening/reading/converting all happen on that worker.
pub fn start(device_id: &str, session: JobId) -> anyhow::Result<CameraSession> {
    start_after(device_id, session, Vec::new())
}
pub fn start_after(
    device_id: &str,
    session: JobId,
    previous: Vec<Arc<CameraSession>>,
) -> anyhow::Result<CameraSession> {
    let path = device_id.to_owned();
    Ok(CameraSession::spawn(session, move |stopping| {
        for old in previous {
            old.join();
        }
        anyhow::ensure!(
            !stopping.load(Ordering::Acquire),
            "Camera session cancelled before opening"
        );
        Ok(Box::new(Native::open(&path)?))
    }))
}
struct Native {
    stream: v4l::io::mmap::Stream<'static>,
    format: v4l::Format,
    pixel_format: nokhwa_core::types::FrameFormat,
    started: std::time::Instant,
    converted: Option<Duration>,
}
impl Native {
    fn open(path: &str) -> anyhow::Result<Self> {
        use v4l::video::Capture;
        let device = v4l::Device::with_path(path).context("Opening camera")?;
        let formats = device
            .enum_formats()
            .context("Enumerating camera formats")?;
        let fourcc = [*b"MJPG", *b"YUYV", *b"RGB3", *b"GREY"]
            .into_iter()
            .find(|code| formats.iter().any(|f| f.fourcc.repr == *code))
            .ok_or_else(|| anyhow::anyhow!("Camera has no supported pixel format"))?;
        let format = device
            .set_format(&v4l::Format::new(640, 480, v4l::FourCC::new(&fourcc)))
            .context("Setting camera format")?;
        anyhow::ensure!(
            format.width > 0
                && format.height > 0
                && u64::from(format.width) * u64::from(format.height) <= 1920 * 1080,
            "Camera requires a mode larger than 1080p"
        );
        let pixel_format = match &format.fourcc.repr {
            b"MJPG" => nokhwa_core::types::FrameFormat::MJPEG,
            b"YUYV" => nokhwa_core::types::FrameFormat::YUYV,
            b"RGB3" => nokhwa_core::types::FrameFormat::RAWRGB,
            b"GREY" => nokhwa_core::types::FrameFormat::GRAY,
            _ => anyhow::bail!("Camera changed to unsupported pixel format"),
        };
        // Drivers may reject requested frame intervals; scheduling remains independently bounded.
        if let Ok(mut params) = device.params() {
            params.interval = v4l::Fraction::new(1, 15);
            let _ = device.set_params(&params);
        }
        let mut stream =
            v4l::io::mmap::Stream::with_buffers(&device, v4l::buffer::Type::VideoCapture, 2)
                .context("Allocating camera buffers")?;
        stream.set_timeout(Duration::from_secs(1));
        Ok(Self {
            stream,
            format,
            pixel_format,
            started: std::time::Instant::now(),
            converted: None,
        })
    }
}
impl Source for Native {
    fn next(&mut self) -> anyhow::Result<Option<(Duration, Frame)>> {
        use v4l::io::traits::CaptureStream;
        let (data, metadata) = match self.stream.next() {
            Ok(frame) => frame,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                anyhow::bail!("Camera stopped delivering frames; stop and reopen the camera")
            }
            Err(e) => return Err(anyhow::Error::new(e).context("Reading camera frame")),
        };
        let now = self.started.elapsed();
        if self
            .converted
            .is_some_and(|old| now.saturating_sub(old) < Duration::from_nanos(66_666_667))
        {
            return Ok(None);
        }
        self.converted = Some(now);
        let bytes = data
            .get(..metadata.bytesused as usize)
            .context("Malformed camera buffer length")?;
        let normalized = if self.pixel_format == nokhwa_core::types::FrameFormat::MJPEG {
            std::borrow::Cow::Borrowed(bytes)
        } else {
            let bpp = match self.pixel_format {
                nokhwa_core::types::FrameFormat::YUYV => 2,
                nokhwa_core::types::FrameFormat::RAWRGB => 3,
                _ => 1,
            };
            let row = self.format.width as usize * bpp;
            packed_rows(
                bytes,
                row,
                (self.format.stride as usize).max(row),
                self.format.height as usize,
            )?
        };
        let buffer = nokhwa_core::buffer::Buffer::new(
            nokhwa_core::types::Resolution::new(self.format.width, self.format.height),
            &normalized,
            self.pixel_format,
        );
        let rgb = buffer.decode_image::<nokhwa_core::pixel_format::RgbFormat>()?;
        self.stream.set_timeout(Duration::from_millis(500));
        Ok(Some((
            now,
            Frame::new(rgb.width(), rgb.height(), rgb.into_raw())?,
        )))
    }
}

fn packed_rows(
    bytes: &[u8],
    row: usize,
    stride: usize,
    height: usize,
) -> anyhow::Result<std::borrow::Cow<'_, [u8]>> {
    anyhow::ensure!(
        row > 0 && stride >= row && height > 0,
        "Malformed camera row stride"
    );
    let required = stride
        .checked_mul(height - 1)
        .and_then(|n| n.checked_add(row))
        .context("Camera row size overflow")?;
    anyhow::ensure!(bytes.len() >= required, "Short camera frame");
    if stride == row {
        return Ok(std::borrow::Cow::Borrowed(&bytes[..required]));
    }
    let mut packed = Vec::with_capacity(row * height);
    for y in 0..height {
        packed.extend_from_slice(&bytes[y * stride..y * stride + row]);
    }
    Ok(std::borrow::Cow::Owned(packed))
}
#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicUsize, mpsc};
    enum Input {
        Frame(u64, u8),
        Fence(mpsc::Sender<()>),
        Error,
    }
    struct Fake {
        input: mpsc::Receiver<Input>,
        releases: Arc<AtomicUsize>,
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Source for Fake {
        fn next(&mut self) -> anyhow::Result<Option<(Duration, Frame)>> {
            match self.input.recv_timeout(Duration::from_millis(10)) {
                Ok(Input::Frame(ms, v)) => Ok(Some((
                    Duration::from_millis(ms),
                    Frame::new(1, 1, vec![v, v, v])?,
                ))),
                Ok(Input::Fence(done)) => {
                    let _ = done.send(());
                    Ok(None)
                }
                Ok(Input::Error) => anyhow::bail!("camera unplugged"),
                Err(_) => Ok(None),
            }
        }
    }
    fn fake() -> (CameraSession, mpsc::Sender<Input>, Arc<AtomicUsize>) {
        let (tx, rx) = mpsc::channel();
        let releases = Arc::new(AtomicUsize::new(0));
        let counter = releases.clone();
        (
            CameraSession::spawn(JobId(8), move |_| {
                Ok(Box::new(Fake {
                    input: rx,
                    releases: counter,
                }))
            }),
            tx,
            releases,
        )
    }
    #[test]
    fn worker_schedules_bounded_preview_and_decode_with_fake_clock() {
        let (session, tx, _) = fake();
        let mut rx = session.subscribe();
        let (mut previews, mut decodes) = (0, 0);
        for ms in 0..1000 {
            tx.send(Input::Frame(ms, 0)).unwrap();
            let (done, received) = mpsc::channel();
            tx.send(Input::Fence(done)).unwrap();
            received.recv_timeout(Duration::from_secs(3)).unwrap();
            if rx.has_changed().unwrap()
                && let Event::Frame { preview, gray } =
                    &rx.borrow_and_update().as_ref().unwrap().event
            {
                previews += usize::from(preview.is_some());
                decodes += usize::from(gray.is_some());
            }
        }
        assert_eq!(previews, 15);
        assert_eq!(decodes, 5);
        session.join();
    }
    #[test]
    fn dropping_session_signals_release_without_blocking() {
        let (session, tx, releases) = fake();
        let (done, received) = mpsc::channel();
        tx.send(Input::Fence(done)).unwrap();
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(session);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while releases.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn padded_native_rows_are_normalized_and_short_rows_rejected() {
        assert_eq!(
            packed_rows(&[1, 2, 3, 0, 4, 5, 6, 0], 3, 4, 2)
                .unwrap()
                .as_ref(),
            &[1, 2, 3, 4, 5, 6]
        );
        assert!(packed_rows(&[0; 5], 3, 4, 2).is_err());
        assert!(packed_rows(&[0; 8], 3, 2, 2).is_err());
    }
    #[test]
    fn bounded_latest_frame_replaces_obsolete_preview() {
        let (session, tx, _) = fake();
        let rx = session.subscribe();
        for i in 0..100 {
            tx.send(Input::Frame(i * 200, i as u8)).unwrap();
        }
        let (done, received) = mpsc::channel();
        tx.send(Input::Fence(done)).unwrap();
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        let current = rx.borrow().clone().expect("latest frame");
        assert_eq!(current.id, JobId(8));
        match &current.event {
            Event::Frame {
                gray: Some(gray), ..
            } => assert_eq!(gray.as_raw(), &[99]),
            _ => panic!("expected latest preview/decode frame"),
        }
        session.join();
    }
    #[test]
    fn stop_and_reopen_release_device_and_clear_preview() {
        for _ in 0..2 {
            let (session, _tx, releases) = fake();
            let rx = session.subscribe();
            session.join();
            assert_eq!(releases.load(Ordering::SeqCst), 1);
            assert!(matches!(
                &rx.borrow().as_ref().unwrap().event,
                Event::Stopped
            ));
        }
    }
    #[test]
    fn unplug_releases_source_and_reports_error() {
        let (session, tx, releases) = fake();
        let mut rx = session.subscribe();
        tx.send(Input::Error).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), rx.changed())
                .await
                .unwrap()
                .unwrap();
        });
        assert!(
            matches!(&rx.borrow().as_ref().unwrap().event,Event::Error(e) if e.contains("unplugged"))
        );
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        session.join();
    }

    use super::*;
    #[test]
    fn malformed_frames_are_rejected() {
        assert!(Frame::new(2, 2, vec![0; 11]).is_err());
        assert!(Frame::new(0, 1, vec![]).is_err());
        assert!(Frame::new(u32::MAX, u32::MAX, vec![]).is_err());
        assert!(Frame::new(1, 1, vec![255, 0, 0]).is_ok());
    }
    #[test]
    fn controllable_clock_limits_preview_and_decode() {
        let mut rates = Rates::default();
        let (mut p, mut d) = (0, 0);
        for ms in 0..1000 {
            let (a, b) = rates.due(Duration::from_millis(ms));
            p += usize::from(a);
            d += usize::from(b);
        }
        assert_eq!(p, 15);
        assert_eq!(d, 5);
        assert_eq!(rates.due(Duration::from_millis(1000)), (false, true));
    }
}
