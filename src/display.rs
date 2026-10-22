//! Read-only Wayland layout snapshots used to reject unsafe overlay placement.

use std::{
    collections::{BTreeMap, VecDeque},
    os::fd::{AsFd, AsRawFd, RawFd},
};

use iced::futures::SinkExt;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::WaylandError,
    protocol::{wl_callback, wl_output, wl_registry},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputGeometry {
    pub name: String,
    pub logical_position: [i32; 2],
    pub logical_size: [u32; 2],
    pub pixel_size: [u32; 2],
    pub transform: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub revision: u64,
    pub outputs: Vec<OutputGeometry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutState {
    Ready(Layout),
    Unavailable,
}

impl Layout {
    /// A combined or merely proportional screenshot cannot establish output placement.
    pub fn eligible_capture(&self, size: [u32; 2]) -> bool {
        let [output] = self.outputs.as_slice() else {
            return false;
        };
        if size.contains(&0)
            || output.logical_size.contains(&0)
            || output.pixel_size.contains(&0)
            || output.transform > 7
        {
            return false;
        }
        let mut physical = output.pixel_size;
        if output.transform % 2 == 1 {
            physical.swap(0, 1);
        }
        [physical, output.logical_size].into_iter().any(|reported| {
            reported
                .into_iter()
                .zip(size)
                .all(|(a, b)| a.abs_diff(b) <= 1)
        })
    }
}

/// Observe complete output batches without polling, surfaces, or screenshot requests.
/// The stream owns initialization and fd readiness; dropping it cancels both immediately.
/// Older xdg-output versions cannot synchronize core and logical properties atomically
/// and are deliberately unavailable for overlay placement.
pub fn events() -> impl iced::futures::Stream<Item = LayoutState> {
    observe(connect())
}

async fn connect() -> anyhow::Result<Connection> {
    // A separate protocol connection must use a fresh display socket. Duplicating
    // WAYLAND_SOCKET would share the application's protocol stream and object IDs.
    let display =
        std::env::var_os("WAYLAND_DISPLAY").ok_or_else(|| anyhow::anyhow!("no Wayland display"))?;
    let mut path = std::path::PathBuf::from(display);
    if !path.is_absolute() {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| anyhow::anyhow!("no runtime directory"))?;
        let runtime = std::path::PathBuf::from(runtime);
        anyhow::ensure!(runtime.is_absolute(), "invalid runtime directory");
        path = runtime.join(path);
    }
    let socket = tokio::net::UnixStream::connect(path).await?.into_std()?;
    Ok(Connection::from_socket(socket)?)
}

fn observe(
    connect: impl std::future::Future<Output = anyhow::Result<Connection>>,
) -> impl iced::futures::Stream<Item = LayoutState> {
    iced::stream::channel(8, async move |mut sender| {
        if sender.send(LayoutState::Unavailable).await.is_err() {
            return;
        }
        let Ok(connection) = connect.await else {
            return;
        };
        let result = match Observer::new(connection) {
            Ok(mut observer) => observer.run(&mut sender).await,
            Err(error) => Err(error),
        };
        if result.is_err() {
            let _ = sender.send(LayoutState::Unavailable).await;
        }
    })
}

struct Socket(Connection);
impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_fd().as_raw_fd()
    }
}

struct Observer {
    fd: tokio::io::unix::AsyncFd<Socket>,
    queue: EventQueue<ProtocolState>,
    state: ProtocolState,
}

impl Observer {
    fn new(connection: Connection) -> anyhow::Result<Self> {
        let queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        connection.display().sync(&qh, ());
        Ok(Self {
            // SAFETY: Socket owns a connection reference; its backend keeps the same
            // live fd until Socket is dropped after AsyncFd deregisters it.
            fd: unsafe { tokio::io::unix::AsyncFd::register(Socket(connection)) }
                .map_err(|error| error.into_parts().1)?,
            queue,
            state: ProtocolState::default(),
        })
    }

    async fn run(
        &mut self,
        sender: &mut iced::futures::channel::mpsc::Sender<LayoutState>,
    ) -> anyhow::Result<()> {
        use std::io::ErrorKind::WouldBlock;
        use tokio::io::Interest;
        loop {
            self.queue.dispatch_pending(&mut self.state)?;
            if self.state.registry_ready {
                anyhow::ensure!(self.state.manager.is_some(), "xdg-output v3 unavailable");
            }
            anyhow::ensure!(!self.state.manager_removed, "xdg-output removed");
            while let Some(event) = self.state.events.pop_front() {
                if sender.send(event).await.is_err() {
                    return Ok(());
                }
            }
            let blocked = match self.queue.flush() {
                Ok(()) => false,
                Err(WaylandError::Io(error)) if error.kind() == WouldBlock => true,
                Err(error) => return Err(error.into()),
            };
            let Some(read) = self.queue.prepare_read() else {
                continue;
            };
            let interest = if blocked {
                Interest::READABLE | Interest::WRITABLE
            } else {
                Interest::READABLE
            };
            let mut ready = self.fd.ready(interest).await?;
            if ready.ready().is_readable()
                || ready.ready().is_read_closed()
                || ready.ready().is_error()
            {
                match read.read() {
                    // libwayland can report a drained socket as a successful read
                    // with zero dispatched events. Treat it like WouldBlock so a
                    // cached readable flag cannot turn the observer into a busy loop.
                    Ok(0) => ready.clear_ready_matching(tokio::io::Ready::READABLE),
                    Ok(_) => {}
                    Err(WaylandError::Io(error)) if error.kind() == WouldBlock => {
                        ready.clear_ready_matching(tokio::io::Ready::READABLE);
                    }
                    Err(error) => return Err(error.into()),
                }
            } else {
                drop(read);
            }
            if blocked && ready.ready().is_writable() {
                match self.queue.flush() {
                    Ok(()) => {}
                    Err(WaylandError::Io(error)) if error.kind() == WouldBlock => {
                        ready.clear_ready_matching(tokio::io::Ready::WRITABLE);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
}

struct BoundOutput {
    output: wl_output::WlOutput,
    xdg: Option<zxdg_output_v1::ZxdgOutputV1>,
}

#[derive(Default)]
struct ProtocolState {
    reducer: LayoutReducer,
    outputs: BTreeMap<u32, BoundOutput>,
    manager: Option<(u32, zxdg_output_manager_v1::ZxdgOutputManagerV1)>,
    registry_ready: bool,
    manager_removed: bool,
    events: VecDeque<LayoutState>,
}

impl ProtocolState {
    fn update(&mut self, update: Update) {
        if let Some(event) = self.reducer.apply(update) {
            self.events.push_back(event);
        }
    }

    fn attach_logical_outputs(&mut self, qh: &QueueHandle<Self>) {
        if let Some((_, manager)) = &self.manager {
            for (&id, output) in &mut self.outputs {
                if output.xdg.is_none() {
                    output.xdg = Some(manager.get_xdg_output(&output.output, qh, id));
                }
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for ProtocolState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == "wl_output" => {
                state.update(Update::Added(name, version >= 2));
                if version >= 2 {
                    let output = registry.bind(name, version.min(4), qh, name);
                    state
                        .outputs
                        .insert(name, BoundOutput { output, xdg: None });
                    state.attach_logical_outputs(qh);
                }
            }
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == "zxdg_output_manager_v1"
                && version >= 3
                && state.manager.is_none() =>
            {
                state.manager = Some((name, registry.bind(name, 3, qh, ())));
                state.attach_logical_outputs(qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if state.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    state.manager_removed = true;
                }
                if state.reducer.outputs.contains_key(&name) {
                    if let Some(output) = state.outputs.remove(&name) {
                        if let Some(xdg) = output.xdg {
                            xdg.destroy();
                        }
                        if output.output.version() >= 3 {
                            output.output.release();
                        }
                    }
                    state.update(Update::Removed(name));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for ProtocolState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.registry_ready = true;
    }
}

impl Dispatch<wl_output::WlOutput, u32> for ProtocolState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let update = match event {
            wl_output::Event::Geometry { transform, .. } => {
                let transform = match transform {
                    WEnum::Value(value) => value as u32,
                    WEnum::Unknown(value) => value,
                };
                Update::Transform(*id, transform)
            }
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => {
                Update::PixelSize(*id, positive_size(width, height))
            }
            wl_output::Event::Scale { factor } => Update::Scale(*id, factor),
            wl_output::Event::Name { name } => Update::Name(*id, name),
            wl_output::Event::Done => Update::Done(*id),
            _ => return,
        };
        state.update(update);
    }
}

fn positive_size(width: i32, height: i32) -> [u32; 2] {
    [
        u32::try_from(width).unwrap_or(0),
        u32::try_from(height).unwrap_or(0),
    ]
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for ProtocolState {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let update = match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => Update::LogicalPosition(*id, [x, y]),
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                Update::LogicalSize(*id, positive_size(width, height))
            }
            zxdg_output_v1::Event::Name { name } => Update::Name(*id, name),
            // Deprecated in v3: only wl_output.done commits the combined geometry batch.
            _ => return,
        };
        state.update(update);
    }
}

wayland_client::delegate_noop!(ProtocolState: ignore zxdg_output_manager_v1::ZxdgOutputManagerV1);

#[derive(Default)]
struct LayoutReducer {
    outputs: BTreeMap<u32, PendingOutput>,
    revision: u64,
    geometry: Vec<GeometryKey>,
    published: Option<LayoutState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeometryKey {
    id: u32,
    position: [i32; 2],
    logical: [u32; 2],
    pixels: [u32; 2],
    transform: u32,
    // wl_output.scale is an integer buffer scale, not the fractional logical scale.
    scale: i32,
}

#[derive(Default)]
struct PendingOutput {
    supported: bool,
    name: String,
    position: Option<[i32; 2]>,
    logical: Option<[u32; 2]>,
    pixels: Option<[u32; 2]>,
    transform: Option<u32>,
    scale: i32,
    committed: Option<(GeometryKey, OutputGeometry)>,
}

impl PendingOutput {
    fn complete(&self, id: u32) -> Option<(GeometryKey, OutputGeometry)> {
        let key = GeometryKey {
            id,
            position: self.position?,
            logical: self.logical?,
            pixels: self.pixels?,
            transform: self.transform?,
            scale: self.scale,
        };
        if !self.supported
            || key.logical.contains(&0)
            || key.pixels.contains(&0)
            || key.transform > 7
            || key.scale <= 0
        {
            return None;
        }
        Some((
            key.clone(),
            OutputGeometry {
                name: if self.name.is_empty() {
                    format!("output-{id}")
                } else {
                    self.name.clone()
                },
                logical_position: key.position,
                logical_size: key.logical,
                pixel_size: key.pixels,
                transform: key.transform,
            },
        ))
    }
}

enum Update {
    Added(u32, bool),
    Removed(u32),
    Name(u32, String),
    LogicalPosition(u32, [i32; 2]),
    LogicalSize(u32, [u32; 2]),
    PixelSize(u32, [u32; 2]),
    Transform(u32, u32),
    Scale(u32, i32),
    Done(u32),
}

impl LayoutReducer {
    fn apply(&mut self, update: Update) -> Option<LayoutState> {
        let id = match update {
            Update::Added(id, supported) => {
                self.outputs.entry(id).or_insert_with(|| PendingOutput {
                    supported,
                    scale: 1,
                    ..PendingOutput::default()
                });
                return self.publish();
            }
            Update::Removed(id) => {
                self.outputs.remove(&id);
                return self.publish();
            }
            Update::Name(id, _)
            | Update::LogicalPosition(id, _)
            | Update::LogicalSize(id, _)
            | Update::PixelSize(id, _)
            | Update::Transform(id, _)
            | Update::Scale(id, _)
            | Update::Done(id) => id,
        };
        let output = self.outputs.get_mut(&id)?;
        match update {
            Update::Name(_, name) => output.name = name,
            Update::LogicalPosition(_, position) => output.position = Some(position),
            Update::LogicalSize(_, size) => output.logical = Some(size),
            Update::PixelSize(_, size) => output.pixels = Some(size),
            Update::Transform(_, transform) => output.transform = Some(transform),
            Update::Scale(_, scale) => output.scale = scale,
            Update::Done(_) => {
                output.committed = output.complete(id);
                return self.publish();
            }
            Update::Added(..) | Update::Removed(_) => unreachable!(),
        }
        None
    }

    fn publish(&mut self) -> Option<LayoutState> {
        let completed: Option<Vec<_>> = self
            .outputs
            .values()
            .map(|o| o.committed.as_ref())
            .collect();
        let state = if let Some(completed) = completed {
            let geometry: Vec<_> = completed.iter().map(|(key, _)| key.clone()).collect();
            if geometry != self.geometry {
                self.revision += 1;
                self.geometry = geometry;
            }
            if completed.is_empty() {
                LayoutState::Unavailable
            } else {
                LayoutState::Ready(Layout {
                    revision: self.revision,
                    outputs: completed
                        .into_iter()
                        .map(|(_, output)| output.clone())
                        .collect(),
                })
            }
        } else {
            LayoutState::Unavailable
        };
        let unchanged = match (&self.published, &state) {
            (Some(LayoutState::Ready(previous)), LayoutState::Ready(next)) => {
                previous.revision == next.revision
            }
            (Some(LayoutState::Unavailable), LayoutState::Unavailable) => true,
            _ => false,
        };
        if unchanged {
            return None;
        }
        self.published = Some(state.clone());
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kde() -> OutputGeometry {
        OutputGeometry {
            name: "eDP-1".into(),
            logical_position: [0, 0],
            logical_size: [1829, 1143],
            pixel_size: [2560, 1600],
            transform: 0,
        }
    }

    fn layout(outputs: Vec<OutputGeometry>) -> Layout {
        Layout {
            revision: 1,
            outputs,
        }
    }

    #[test]
    fn kde_fractional_geometry_accepts_only_reported_resolutions_and_one_pixel_rounding() {
        let layout = layout(vec![kde()]);
        for size in [[2560, 1600], [1829, 1143], [1828, 1144], [2561, 1599]] {
            assert!(layout.eligible_capture(size), "{size:?}");
        }
        for size in [
            [3000, 1600],
            [1280, 800],
            [3658, 2286],
            [1827, 1143],
            [0, 0],
        ] {
            assert!(!layout.eligible_capture(size), "{size:?}");
        }
    }

    #[test]
    fn empty_incomplete_and_multiple_outputs_are_ineligible() {
        assert!(!layout(vec![]).eligible_capture([2560, 1600]));
        assert!(!layout(vec![kde(), kde()]).eligible_capture([2560, 1600]));
        for output in [
            OutputGeometry {
                logical_size: [0, 1143],
                ..kde()
            },
            OutputGeometry {
                pixel_size: [2560, 0],
                ..kde()
            },
            OutputGeometry {
                transform: 8,
                ..kde()
            },
        ] {
            assert!(!layout(vec![output]).eligible_capture([2560, 1600]));
        }
    }

    #[test]
    fn rotation_orients_physical_capture_dimensions() {
        for transform in [1, 3, 5, 7] {
            let output = OutputGeometry {
                transform,
                logical_size: [1143, 1829],
                ..kde()
            };
            let layout = layout(vec![output]);
            assert!(layout.eligible_capture([1600, 2560]));
            assert!(layout.eligible_capture([1143, 1829]));
            assert!(!layout.eligible_capture([2560, 1600]));
        }
        for transform in [0, 2, 4, 6] {
            assert!(
                layout(vec![OutputGeometry { transform, ..kde() }]).eligible_capture([2560, 1600])
            );
        }
    }
    fn initial(reducer: &mut LayoutReducer, id: u32) -> Option<LayoutState> {
        reducer.apply(Update::Added(id, true));
        reducer.apply(Update::Name(id, "eDP-1".into()));
        reducer.apply(Update::PixelSize(id, [2560, 1600]));
        reducer.apply(Update::Transform(id, 0));
        reducer.apply(Update::Scale(id, 2));
        reducer.apply(Update::LogicalPosition(id, [0, 0]));
        reducer.apply(Update::LogicalSize(id, [1829, 1143]));
        reducer.apply(Update::Done(id))
    }

    fn ready(state: Option<LayoutState>) -> Layout {
        match state {
            Some(LayoutState::Ready(layout)) => layout,
            other => panic!("expected a complete layout, received {other:?}"),
        }
    }

    #[test]
    fn batches_wait_for_core_done_and_ignore_unchanged_geometry_and_names() {
        let mut reducer = LayoutReducer::default();
        assert_eq!(
            reducer.apply(Update::Added(1, true)),
            Some(LayoutState::Unavailable)
        );
        assert_eq!(reducer.apply(Update::Done(1)), None);
        assert_eq!(reducer.apply(Update::PixelSize(1, [2560, 1600])), None);
        assert_eq!(reducer.apply(Update::Transform(1, 0)), None);
        assert_eq!(reducer.apply(Update::LogicalPosition(1, [0, 0])), None);
        assert_eq!(reducer.apply(Update::LogicalSize(1, [1829, 1143])), None);
        assert_eq!(reducer.apply(Update::Name(1, "eDP-1".into())), None);
        let first = ready(reducer.apply(Update::Done(1)));
        assert_eq!(first, layout(vec![kde()]));
        assert_eq!(reducer.apply(Update::Done(1)), None);
        reducer.apply(Update::Name(1, "unrelated label".into()));
        assert_eq!(reducer.apply(Update::Done(1)), None);
    }

    #[test]
    fn geometry_changes_advance_once_for_each_complete_batch() {
        let mut reducer = LayoutReducer::default();
        assert_eq!(ready(initial(&mut reducer, 1)).revision, 1);
        for (update, revision) in [
            (Update::LogicalPosition(1, [-1829, 0]), 2),
            (Update::Scale(1, 3), 3),
            (Update::LogicalSize(1, [1707, 1067]), 4),
            (Update::PixelSize(1, [1920, 1200]), 5),
            (Update::Transform(1, 2), 6),
        ] {
            assert_eq!(reducer.apply(update), None);
            assert_eq!(ready(reducer.apply(Update::Done(1))).revision, revision);
            assert_eq!(reducer.apply(Update::Done(1)), None);
        }
        reducer.apply(Update::Transform(1, 1));
        reducer.apply(Update::LogicalSize(1, [1067, 1707]));
        let rotated = ready(reducer.apply(Update::Done(1)));
        assert_eq!(rotated.revision, 7);
        assert_eq!(rotated.outputs[0].logical_size, [1067, 1707]);
        assert_eq!(rotated.outputs[0].transform, 1);
    }

    #[test]
    fn hotplug_is_unavailable_until_complete_and_removal_changes_identity() {
        let mut reducer = LayoutReducer::default();
        initial(&mut reducer, 1);
        assert_eq!(
            reducer.apply(Update::Added(2, true)),
            Some(LayoutState::Unavailable)
        );
        let two = ready(initial(&mut reducer, 2));
        assert_eq!(two.revision, 2);
        assert!(!two.eligible_capture([2560, 1600]));
        let remaining = ready(reducer.apply(Update::Removed(1)));
        assert_eq!(remaining.revision, 3);
        assert_eq!(remaining.outputs.len(), 1);
        assert_eq!(
            reducer.apply(Update::Removed(2)),
            Some(LayoutState::Unavailable)
        );
        assert_eq!(ready(initial(&mut reducer, 3)).revision, 5);
    }

    #[test]
    fn incomplete_invalid_or_unsupported_outputs_never_publish_ready() {
        let mut reducer = LayoutReducer::default();
        reducer.apply(Update::Added(1, true));
        reducer.apply(Update::PixelSize(1, [2560, 1600]));
        reducer.apply(Update::LogicalSize(1, [1829, 1143]));
        assert_eq!(reducer.apply(Update::Done(1)), None);
        reducer.apply(Update::LogicalPosition(1, [0, 0]));
        reducer.apply(Update::Transform(1, 8));
        assert_eq!(reducer.apply(Update::Done(1)), None);
        reducer.apply(Update::Transform(1, 0));
        reducer.apply(Update::Scale(1, 0));
        assert_eq!(reducer.apply(Update::Done(1)), None);
        reducer.apply(Update::Scale(1, 2));
        reducer.apply(Update::LogicalSize(1, [0, 1143]));
        assert_eq!(reducer.apply(Update::Done(1)), None);
        reducer.apply(Update::LogicalSize(1, [1829, 1143]));
        assert_eq!(ready(reducer.apply(Update::Done(1))).revision, 1);
        assert_eq!(
            reducer.apply(Update::Added(2, false)),
            Some(LayoutState::Unavailable)
        );
        assert_eq!(reducer.apply(Update::Done(2)), None);
        assert_eq!(ready(reducer.apply(Update::Removed(2))).revision, 1);
    }

    #[tokio::test]
    async fn dropping_subscription_closes_a_silent_initializing_connection() {
        use iced::futures::{FutureExt, StreamExt};
        use tokio::io::AsyncReadExt;
        let (client, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let mut peer = tokio::net::UnixStream::from_std(peer).unwrap();
        let connection = wayland_client::Connection::from_socket(client).unwrap();
        let mut stream = Box::pin(observe(async { Ok(connection) }));
        assert_eq!(stream.next().await, Some(LayoutState::Unavailable));
        assert!(
            stream.next().now_or_never().is_none(),
            "initialization must wait asynchronously"
        );
        drop(stream);
        let mut requests = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            peer.read_to_end(&mut requests),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            !requests.is_empty(),
            "observer must request registry initialization"
        );
    }

    #[tokio::test]
    async fn connection_failure_and_peer_disconnect_emit_unavailable_without_panicking() {
        use iced::futures::{FutureExt, StreamExt};
        let mut failed = Box::pin(observe(async { anyhow::bail!("unavailable test socket") }));
        assert_eq!(failed.next().await, Some(LayoutState::Unavailable));
        assert_eq!(failed.next().await, None);
        let (client, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let connection = wayland_client::Connection::from_socket(client).unwrap();
        let mut stream = Box::pin(observe(async { Ok(connection) }));
        assert_eq!(stream.next().await, Some(LayoutState::Unavailable));
        assert!(stream.next().now_or_never().is_none());
        drop(peer);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
                .await
                .unwrap(),
            Some(LayoutState::Unavailable)
        );
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn initialization_returns_to_readiness_after_draining_registry_events() {
        use iced::futures::StreamExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (client, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let mut peer = tokio::net::UnixStream::from_std(peer).unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let mut stream = Box::pin(observe(async { Ok(connection) }));
        assert_eq!(stream.next().await, Some(LayoutState::Unavailable));
        // wl_display.get_registry followed by sync. Reply with a harmless registry
        // global but leave the initial sync unanswered, as a stalled server might.
        let mut requests = [0; 24];
        peer.read_exact(&mut requests).await.unwrap();
        let registry = u32::from_ne_bytes(requests[8..12].try_into().unwrap());
        let mut body = Vec::new();
        body.extend(99_u32.to_ne_bytes());
        body.extend(7_u32.to_ne_bytes());
        body.extend(b"unused\0\0");
        body.extend(1_u32.to_ne_bytes());
        let mut message = Vec::new();
        message.extend(registry.to_ne_bytes());
        message.extend((((body.len() + 8) as u32) << 16).to_ne_bytes());
        message.extend(body);
        peer.write_all(&message).await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), stream.next())
                .await
                .is_err(),
            "an idle initializing observer must yield instead of spinning"
        );
        drop(stream);
        let mut remaining_requests = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            peer.read_to_end(&mut remaining_requests),
        )
        .await
        .unwrap()
        .unwrap();
    }
}
