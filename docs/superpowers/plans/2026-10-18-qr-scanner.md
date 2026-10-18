# QR Scanner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a Linux-first Rust/Iced tray utility for local screenshot and camera QR decoding.

**Architecture:** One Cargo application, with independent decoding, screenshot, camera, and tray modules. An Iced daemon owns application state and windows; bounded workers deliver tagged results so obsolete operations cannot overwrite current sessions.

**Tech Stack:** Stable Rust, stable Iced (0.14 API family checked during research), ashpd, ksni, nokhwa, rqrr, image, url, and a runtime compatible with the selected async libraries. Pin resolved versions in Cargo.lock; verify current APIs before implementation.

**Spec:** `docs/superpowers/specs/2026-10-18-qr-scanner-design.md`

## Global Constraints

- KDE Plasma on Wayland is the first target, with GNOME, Hyprland, and Sway compatibility.
- Application source should not require handwritten C++.
- No scan history, telemetry, network decoding, autostart, or automatic link opening.
- Global shortcuts are deferred; desktop shortcuts may invoke command-line scan actions.
- No camera activity or screen polling while idle.
- Only one decode operation runs at a time.
- Limit decode to approximately five frames per second initially and preview to at most fifteen.
- Drop obsolete frames rather than building a queue.
- Windows and macOS are future targets.
- There are no promised memory or latency numbers before benchmarking.

## Review Focus

- A second launcher or scan invocation must reach the existing background process instead of silently creating competing tray instances (Task 1).
- Portal URIs may contain escaped characters; cancellation and invalid local image paths must not crash or leave hidden windows inaccessible (Task 3).
- Camera unplug/reopen and delayed frames after close must not leak resources or resurrect closed sessions (Tasks 2 and 4).
- Scaled previews, reverse drags, and zero-area selections must map to valid source pixels or a clear rejection (Task 3).
- Payloads may contain invalid UTF-8 or deceptive URL schemes; preserve bytes and offer Open only for parsed HTTP/HTTPS URLs (Task 2).

## File Structure

- `Cargo.toml`, `Cargo.lock`, `.gitignore`: build and dependency configuration.
- `src/main.rs`, `src/lib.rs`, `src/cli.rs`, `src/instance.rs`: entry point, CLI, and per-user instance forwarding.
- `src/app/mod.rs`, `src/app/state.rs`, `src/app/view.rs`: Iced daemon, pure state transitions, windows, and UI.
- `src/decode.rs`, `src/results.rs`, `src/jobs.rs`: decoding, payload handling, and serialized bounded work.
- `src/capture/mod.rs`, `src/capture/portal.rs`, `src/capture/crop.rs`: source types, Linux portal, and crop geometry.
- `src/camera/mod.rs`, `src/camera/native.rs`: camera messages and native capture worker.
- `src/tray.rs`: Linux StatusNotifierItem service and tray commands.
- `tests/decode.rs`, `tests/fixtures/`: real QR fixture integration coverage.
- `assets/qr-scanner.svg`, `packaging/qr-scanner.desktop`, `README.md`, `docs/verification.md`: install assets, operation, and measured/manual validation.

## Task 1: Runnable tray application and instance lifecycle

**Files:** Cargo/build files; entry, CLI, instance, app, tray modules; icon; README.

**Interfaces:**
- `cli::Command { Show, ScanScreen, ScanCamera, Background, Quit }`.
- `instance::forward_or_listen(command: Command) -> anyhow::Result<InstanceRole>`; `InstanceRole` distinguishes a forwarded command from the primary listener. Use a per-user Unix socket in a validated private runtime directory; remove stale sockets only after confirming no live listener. Other targets return an explicit unsupported-platform error initially.
- `tray::start(commands: tokio::sync::mpsc::Sender<Command>) -> anyhow::Result<TrayService>`; expose tray-host availability changes to application state.
- `app::run(command: Command, primary: InstanceRole) -> iced::Result` owns the daemon and event subscriptions.
- `app::state::State` owns window IDs, host availability, current session, busy/error states, and results. Define separate `Message` variants for user commands and asynchronous outcomes.

- [ ] Write state and instance tests: closing the last window with a tray host keeps the process alive; without a host it exits; background mode without a host opens the main window; a second invocation forwards its command; stale socket recovery works and concurrent live primary processes do not unlink one another's sockets.
- [ ] Run the targeted tests and confirm failure before implementation. Bootstrap only the Cargo test harness needed to execute them.
- [ ] Implement CLI parsing, private socket command forwarding, Iced daemon wiring, a small main window, tray host handling, and Show/Quit actions. Tray events wake the application rather than being periodically polled. Scan actions can report that their source is not yet implemented until their owning task lands.
- [ ] Add build/run instructions and ignore `target/`. Use conservative Cargo features; do not install system packages without checking prerequisites.
- [ ] Run `cargo test`, `cargo check`, and manually launch on KDE. Verify tray visibility, close/reopen, second invocation forwarding, and Quit.
- [ ] Commit the runnable shell with message `feat: add Iced tray application lifecycle`.

## Task 2: Decoder, result handling, and serialized jobs

**Files:** `src/decode.rs`, `src/results.rs`, `src/jobs.rs`, app state/view; decoder fixtures/tests.

**Interfaces:**
- `results::QrResult { payload: Vec<u8>, display_text: String, open_url: Option<url::Url> }`; construct with `QrResult::from_payload(payload: Vec<u8>) -> Self`. Invalid UTF-8 uses a clearly labeled hex representation; validate URL scheme, host, and parse success before enabling Open.
- `decode::decode(image: &image::GrayImage) -> anyhow::Result<Vec<QrResult>>` returns unique payloads in stable discovery order.
- `jobs::JobId(u64)` and `jobs::DecodeOutcome { id: JobId, result: anyhow::Result<Vec<QrResult>> }`.
- `jobs::Decoder::submit(id: JobId, image: image::GrayImage)` schedules a bounded latest job; `cancel()` invalidates pending work. One worker executes decode; consumers reject stale IDs even when active computation cannot be interrupted.

- [ ] Add QR fixtures and tests named `single_qr`, `multiple_qrs`, `rotated_qr`, `blank_image`, and `invalid_image`. Generate deterministic fixtures locally with a dev-only QR encoder or commit small fixtures with documented provenance. Assert exact payloads, count, and failure behavior.
- [ ] Add payload tests asserting byte preservation, safe invalid-UTF-8 display, rejection of `javascript:` and `file:` links, and successful HTTP/HTTPS parsing. Add job tests for serialized execution, bounded replacement, and ignoring a completed old session.
- [ ] Run the new tests and confirm they fail before implementing the decoder and workers.
- [ ] Implement rqrr decoding, payload conversion, worker lifecycle, and results UI with explicit Copy/Open. Deduplicate by payload bytes within each session. Display distinct empty-result and failure states.
- [ ] Run `cargo test` and manually exercise Copy and explicit Open using a fixture scan path or developer harness without adding a permanent debug UI.
- [ ] Commit with message `feat: decode QR images locally and display results`.

## Task 3: Wayland screen capture and region selection

**Files:** capture modules; app state/view; screenshot-related tests in module test sections.

**Interfaces:**
- `capture::CaptureOutcome { Captured(image::DynamicImage), Cancelled }` as enum variants.
- `capture::screenshot() -> impl Future<Output = anyhow::Result<CaptureOutcome>>` requests a portal screenshot and normalizes the local image.
- `capture::crop::PixelRect { x: u32, y: u32, width: u32, height: u32 }`.
- `capture::crop::map_selection(image_size: (u32, u32), displayed_image: (f32, f32, f32, f32), drag: ((f32, f32), (f32, f32))) -> Option<PixelRect>` accounts for the image's actual displayed rectangle, including letterboxing.

- [ ] Write crop tests for scaled images, letterboxing, reverse drags, bounds clamping, zero-area selection, and nonfinite coordinates. Assert exact pixel rectangles; reject empty/invalid selections.
- [ ] Write portal-adapter tests for cancellation, permission failure, percent-encoded file URIs, non-file URIs, missing files, and corrupt image content. Abstract the request result and image loader so these tests do not require a live portal.
- [ ] Run the new tests and confirm failure.
- [ ] Implement ashpd capture with portal-version/capability detection. Do not send unsupported area-target options. Hide app windows before capture; restore a usable window after cancellation or failure. Decode the returned image, retain it only for the active results/crop flow, and expose Scan full image and Select region controls.
- [ ] Implement an ordinary-window crop preview with drag rectangle and Escape cancellation. Feed the cropped image to Task 2's worker. Release the old image and texture on session replacement or close.
- [ ] Run `cargo test` and manually verify KDE area capture, full-image scan, cancellation, two QRs, and a scaled crop. Record observed portal behavior and installed interface version.
- [ ] Commit with message `feat: scan Wayland screenshots and selected regions`.

## Task 4: Camera preview and QR scanning

**Files:** camera modules; app state/view; camera worker tests.

**Interfaces:**
- `camera::Device { id: String, label: String }`; `camera::devices() -> anyhow::Result<Vec<Device>>`.
- `camera::Frame { width: u32, height: u32, rgb: Vec<u8> }` provides validated tightly packed preview pixels and grayscale conversion for decoding.
- `camera::start(device_id: &str, session: JobId) -> anyhow::Result<CameraSession>` owns stop control and latest-frame delivery; `CameraSession::stop(&mut self)` signals cleanup without blocking the UI.
- Camera outcomes include the session ID and distinguish frames, errors, and stopped acknowledgments.

- [ ] Test with a fake frame source: bounded latest-frame delivery, maximum preview/decode scheduling rates (15/5 per second using a controllable clock), malformed frame length rejection, unplug errors, stop/reopen, and ignoring frames after close. Check the fake device's release counter after shutdown.
- [ ] Run those tests and confirm failure.
- [ ] Inspect the installed camera and native build prerequisites. Validate nokhwa capture with a minimal local probe before integrating preview; document unavailable hardware rather than pretending the camera path was verified.
- [ ] Implement device selection, preview conversion, throttled submission to the single decoder, and per-session deduplication. Do not queue every frame. Selecting a new source stops the previous camera session. Window close, Stop, and Quit release capture resources and preview buffers.
- [ ] Run `cargo test` and manually verify a real camera QR scan, stop/reopen, device error handling, and no camera indicator after closing. Record any hardware or permission blocker explicitly.
- [ ] Commit with message `feat: add on-demand camera QR scanning`.

## Task 5: Desktop delivery, verification, and measurements

**Files:** desktop entry, README, verification report; targeted fixes in modules owning discovered failures.

**Interfaces:** Preserve CLI commands and module boundaries from Tasks 1–4. The desktop entry invokes the normal launcher behavior; README documents background and scan CLI invocation and native Linux prerequisites.

- [ ] Add launcher assets and native build/install instructions, including portal configuration and optional GNOME tray extension. Explain that native camera access is implemented and Flatpak Camera portal/PipeWire support is deferred.
- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo build --release`; require successful exit status and resolve failures before reporting success.
- [ ] Follow the spec's KDE manual acceptance cases, including no-tray behavior, delayed results after close, repeated CLI commands, capture cancellation, multiple results, clipboard, camera cleanup, and Quit. Report each as passed, failed, or unverified.
- [ ] Measure release startup time, idle CPU and RSS/PSS, full-image decoding latency, and camera CPU/memory. Record hardware, renderer, image sizes, and commands in `docs/verification.md`. Compare software/GPU rendering on the same workload and select the default from observed behavior; do not invent figures if graphical tools or hardware are unavailable.
- [ ] Add explicit unverified checklists for GNOME, Hyprland, and Sway. No claim of full cross-desktop validation without running those sessions.
- [ ] Review the final diff against the approved spec, resolve material issues, and commit with message `docs: document desktop setup and verification results`.

## Execution and Review

All five tasks share application interfaces, so inline execution is the recommended method. Complete each task's checks before continuing. A final independent review can check lifecycle, bounded work, and Wayland failure behavior. Do not mark hardware acceptance or performance measurements complete when the environment prevents them; deliver functioning code and clearly identify the remaining checks.
