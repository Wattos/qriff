# Task 1 implementation report

Status: DONE_WITH_CONCERNS (visual acceptance remains manual).

## Delivered

- Cargo binary/library using Iced 0.14.0 daemon, tiny-skia software rendering,
  Tokio subscriptions, ksni 0.3.6, zbus 5.19.0. Cargo.lock committed.
- CLI commands Show (default), ScanScreen, ScanCamera, Background, Quit; mutually
  exclusive flags. Other platforms explicitly fail in main; Linux modules gated.
- Private per-user Unix socket under validated XDG_RUNTIME_DIR/qr-scanner.
  A persistent lock file is opened without symlink following and exclusively held
  through primary lifetime and socket cleanup. Startup losers forward while the
  winner binds, with a bounded startup retry. Even after acquiring the lock, a
  successfully connected live socket is never removed. Only refused/absent stale
  sockets are recovered; non-socket files and non-private directories are rejected.
- Small main window, tray menu, Show/focus and Quit, source-pending scan status.
  State owns window IDs/count, tray host status, session, busy/error and results.
- Event-driven IPC acceptance and tray command channels. Host monitoring subscribes
  to watcher signals and owner changes before querying initial host state, with
  property caching disabled. Missing/failed tray falls back to a main window.
  Last-window close exits without a host and stays alive with a host; host loss
  while hidden opens a window. No idle recurring polling or capture workers.
- README build/run and desktop manual checklist, SVG icon and desktop entry.
  Assets are not installed automatically. Existing target ignore retained.

## RED/GREEN evidence

Bootstrapped Cargo harness, CLI type and todo! lifecycle/socket bodies first.
`cargo test --lib` logged to `/tmp/qr-task1-red.log`: 0 passed, 6 failed, all
expected missing behavior panics: last window with/without host, background
without host, hidden host loss, second forwarding, stale socket recovery.
After implementation a compiler ambiguity in Read::by_ref was corrected; socket
fixtures were explicitly made private (tempfile directories followed local umask).
All six then passed. Expanded verification added eight-way concurrent election,
live listener preservation without our lock, rejecting public directories, and
preserving a non-socket path. Tests use real filesystem sockets and file locks.
No mocking of lock/socket behavior.

Final commands (all exit 0):

- `cargo test`: 10 passed, 0 failed; binary/doc test targets also passed.
- `cargo check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean (three initial style lints
  corrected before final run).
- `cargo fmt`: applied.
- `cargo build`: clean.

## Native KDE smoke evidence

Environment KDE neon 24.04, KDE Wayland, Rust 1.99.0. No packages installed.

- Foreground-managed `target/debug/qr-scanner --background` stayed alive.
- StatusNotifierWatcher RegisteredStatusNotifierItems grew from six to seven,
  including `org.kde.StatusNotifierItem-2889563-1/StatusNotifierItem`.
- Separate Show and --scan-screen invocations exited 0 and forwarded to primary.
- D-Bus StatusNotifierItem Activate returned successfully.
- CLI --quit exited primary cleanly (0).
- Fresh final binary registered `org.kde.StatusNotifierItem-2894500-1`.
  GetLayout returned Scan screen (1), Scan camera (2), Show window (3), Quit (4).
  D-Bus menu Event for Quit (4) exited primary cleanly (0).
- An isolated `dbus-run-session` with no tray watcher ran --background; process
  remained alive, and WAYLAND_DEBUG client trace recorded get_xdg_surface and
  title assignment, proving fallback window creation. Forwarded --quit exited 0.

## Limits and handoff concerns

Visual tray appearance and physical close/reopen were not observed: no GUI
interaction tool is available. Host disappearance/reappearance was covered in
state tests and by event subscriptions but not induced on the user's real panel.
Do not describe these as manually accepted. Other desktops remain untested.

Software rendering is the conservative initial choice; performance comparison
and benchmarks remain later acceptance work. Scan handlers are explicitly
implementation-pending. Results are a temporary Vec<String> until the results
module lands. No decoder/capture/camera work was included.

The listener is owned by the external-events subscription alongside the lock
handle; maintain stable subscription identity when integrating future tasks.
The persistent instance.lock must never be unlinked while an instance is alive.

## Files

Cargo.toml, Cargo.lock, README.md, assets/qr-scanner.desktop,
assets/qr-scanner.svg, src/main.rs, src/lib.rs, src/cli.rs, src/instance.rs,
src/tray.rs, src/app/mod.rs, src/app/state.rs, and this report.
