# Tray QR Selection Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open the tray menu on primary click, then let users capture, select highlighted desktop QR codes, and copy or act without opening the main app.

**Architecture:** Keep the existing bounded decoder for camera and normal-window scans. Add a spatial screenshot decoder, pure overlay geometry/model, an event-driven Wayland output observer, and a distinct temporary Iced fullscreen window role. Reuse the in-progress notification, native-region and selectable-text work after bringing it into compliance with the approved spec.

**Tech Stack:** Rust, Iced 0.14/tiny-skia, rqrr 0.11, ksni 0.3, zbus 5, existing screenshot portal, Spectacle on KDE, wl-copy for windowless Wayland copying; Wayland client/output protocol bindings already present transitively.

**Spec:** `docs/superpowers/specs/2026-10-22-tray-overlay-design.md` (approved).

## Global Constraints

- Keep Rust, Iced, local decoding, tiny-skia by default, existing explicit QR actions, and the camera preview window. No C++ or framework migration.
- Primary activation opens the tray menu. It neither scans nor opens the main window.
- Menu items: Scan screen, Scan region, Scan camera, Last scan, Open window, Quit.
- Two QR codes with equal bytes have two clickable regions and one deduplicated payload.
- Detection and highlighting never open a URL, dial or connect to a network.
- Screen scans do not automatically copy before selection, even with one code. Camera auto-copy rules remain unchanged.
- No secret raw text goes into notifications, tray labels, logs, command arguments or settings.
- A new capture or source change rejects old UI callbacks; closing the overlay alone retains results and pending actions.
- First acceptance target is the user's KDE Wayland session with one connected 2560×1600 display at 140% logical scaling.
- Never stretch a multi-output screenshot onto one display and claim the highlights are aligned with the desktop.
- Output changes during selection cancel the overlay safely.
- No continuous screen capture, additional camera use, network decoding, screenshot history or saved QR payloads.
- GNOME, Hyprland and Sway remain intended targets, not verified claims.

## Review Focus

1. Two equal-payload QR codes at different positions: each must be selectable but selecting both must not repeat automatic copying (Tasks 2/5).
2. Display geometry arriving late, fractional rounding or hotplug between capture and selection: no misplaced overlay or stale clickable targets (Tasks 3/5).
3. External action requested while the overlay closes: no duplicate request, lost completion, or normal-window-close reset (Task 5).
4. Wi-Fi text revealed then hidden/dismissed while selection events remain queued: secrets must leave selectable buffers and stale events must be rejected (Tasks 1/5).
5. Notification daemon restart/failure, or multiple pending actions: tray retains meaningful feedback without opening the main window (Tasks 1/6).

## Workspace and file responsibilities

Continue in `.worktrees/tray-scanning`, branch `feat/tray-scanning`. Spec commit
`9556a28` is the branch's current committed head. It contains inherited uncommitted
product changes; preserve them and review them, never discard/recreate the worktree.
The main branch is `d6cfa67`. Share builds using:
`CARGO_TARGET_DIR=/home/filipw/Personal/qr-scanner-linux/target`.

- `src/desktop.rs`: in-memory tray presentation, secret-safe labels and notification service.
- `src/tray.rs`: menu-first activation and dynamic Last scan entries.
- `src/capture/mod.rs`: quick/region providers and private Spectacle temporary files.
- `src/app/clipboard.rs`: automatic-copy policies and bounded stdin-only wl-copy adapter.
- `src/app/view.rs`: normal-window selectable result text; keep its existing overall layout.
- `src/decode.rs`: payload decoder and positional screenshot decoder.
- `src/overlay/{mod,geometry,view}.rs`: pure overlay state, coordinate transforms, screenshot/highlight canvas and action panel.
- `src/display.rs`: read-only Wayland output layout subscription and screenshot eligibility.
- `src/app/overlay.rs`: window-role lifecycle and integration helpers, keeping the large app controller focused.
- `src/app/mod.rs`, `src/app/focus.rs`: route existing messages and keys by window role.
- `tests/spatial_decoding.rs`, module/controller tests: behavior and native-boundary regressions.
- `README.md`, `docs/verification.md`, `docs/screenshots/`: accurate delivery and actual Iced evidence.

## Task 1: Complete and review the inherited desktop integration

**Files:** Modify `Cargo.toml`, `src/{lib,cli,tray,desktop}.rs`, `src/capture/mod.rs`, `src/app/{mod,clipboard,view,focus,tests}.rs`.

**Interfaces:**
- Retain `Command::{Show,ScanScreen,ScanRegion,ScanCamera,Background,Quit,CopyResult(u64,usize),ActivateResult(u64,usize)}`. Display Show as Open window.
- Retain `desktop::Snapshot { session: u64, results: Vec<ResultEntry>, notification: Option<Notice> }`; add `ResultEntry.feedback: Option<String>` for fixed success/failure feedback.
- Retain `desktop::notifications(watch::Receiver<Snapshot>, mpsc::Sender<Command>) -> impl Future<Output=anyhow::Result<()>>`.
- Retain `tray::start(mpsc::Sender<Command>, watch::Receiver<Snapshot>) -> anyhow::Result<TrayService>`.
- Retain `clipboard::write_windowless(String) -> impl Future<Output=Result<(),ClipboardUnavailable>>` and guarded `Message::SelectText(JobId,usize,text_editor::Action)`.

- [ ] **Step 1: Reconcile inherited tests against the spec before changing code.**
  Replace the obsolete test expecting a primary click to scan with a test asserting
  `<Item as ksni::Tray>::MENU_ON_ACTIVATE` is true and direct `activate` sends no
  ScanScreen/Show command. Exercise Last scan callbacks with literal session/index
  expectations; pending disables its action and failed feedback remains visible.
  Add controller cases for two distinct action failures with retained per-result
  feedback, and hidden/revealed malformed Wi-Fi text with queued stale selection.
  Existing private-D-Bus/fake-executable tests remain; add a fake notification server
  failure/restart case showing results remain in the tray without a normal window.
- [ ] **Step 2: Run the focused tests and record actual red evidence.**
  `cargo test --offline primary_click` and the new controller/privacy tests must
  fail on the obsolete activation/feedback behavior, rather than compilation errors.
- [ ] **Step 3: Implement menu-first behavior and complete desktop feedback.**
  Set `const MENU_ON_ACTIVATE: bool = true`; remove the scan-producing activation
  override. Label the Show menu item Open window. Keep raw QR payloads out of
  presentation/debug logs. Preserve event-driven notification delivery and validate
  session/index/action keys at both notification and controller boundaries.
  Ensure per-result feedback works for multiple results; notification failure
  leaves tray interaction functioning. Bound initial notification method waits
  as well as Notify/CloseNotification waits; a stuck notification server cannot
  consume an unbounded task. A finite failure with usable tray fallback is
  acceptable; automatic retry is not required.
  Audit existing Spectacle cancellation/failure/private file cleanup, timeout and
  exact stdin-only clipboard behavior. Do not execute real external actions.
  Keep main-window decoded text read-only/selectable; Tab must leave its text
  control, Ctrl+C uses selection, hiding Wi-Fi destroys its secret buffer.
- [ ] **Step 4: Verify `cargo test --offline`, formatting and Clippy.**
  Expect all tests passing and no warnings. The inherited work previously passed
  106 Rust tests; establish the current baseline freshly, do not assume the count.
- [ ] **Step 5: Commit the complete inherited desktop work and fixes.**
  Stage only the listed product files; commit `feat: add menu-first tray scans and desktop result actions`.

## Task 2: Preserve spatial detections and define overlay geometry

**Files:** Modify `src/decode.rs`, `src/lib.rs`; create `src/overlay/mod.rs`, `src/overlay/geometry.rs`, `tests/spatial_decoding.rs`.

**Interfaces:**
- `decode::Detection { result_index: usize, corners: [[f32;2];4] }` in top-left, top-right, bottom-right, bottom-left detector order.
- `decode::SpatialScan { results: Vec<QrResult>, detections: Vec<Detection> }`.
- `decode::decode_spatial(&image::GrayImage) -> anyhow::Result<SpatialScan>`.
- Preserve `decode::decode(&image::GrayImage) -> anyhow::Result<Vec<QrResult>>`, discovery order, size limits, raw bytes and error behavior; delegate to the same spatial decoding implementation and discard positions.
- `overlay::geometry::Transform::new([u32;2], iced::Size) -> Option<Transform>`; `to_surface([f32;2]) -> iced::Point`; `to_image(iced::Point) -> Option<[f32;2]>`.
- `geometry::hit_test(&[Detection], &Transform, iced::Point) -> Option<usize>` returns a detection index, using its quadrilateral, not the whole screenshot.
- `geometry::panel_bounds(iced::Rectangle, iced::Size, iced::Size) -> iced::Rectangle` takes selected highlight, requested panel size, viewport.

- [ ] **Step 1: Write spatial fixture and geometry tests.**
  Generate offline QR fixtures as in `tests/decoding.rs`, at literal offsets `(20,30)`
  and `(400,30)`. Equal payloads must yield `results.len()==1`,
  `detections.len()==2`, both linked to result index 0, with quadrilateral centers
  inside the independently known QR image placements. Distinct/rotated/binary
  fixtures preserve payloads and bounds; blank images remain successful/empty.
  Test a rotated quadrilateral with points outside it but inside its bounding box;
  those points must not select it. Test inverse mapping for physical `(140,280)`
  to logical `(100,200)` using image `(1400,1400)` and surface `(1000,1000)`.
  Zero/NaN geometry is rejected. Panel bounds at each viewport corner stay inside
  the viewport, including when the requested panel exceeds it.
- [ ] **Step 2: Run `cargo test --offline --test spatial_decoding` and module geometry tests.**
  Establish red tests for missing positional behavior, then implement the interfaces.
- [ ] **Step 3: Retain every successfully decoded grid's bounds.**
  Use rqrr `grid.bounds` and link duplicate payloads to the existing result index.
  Never attach a failed grid's partial bytes. Keep all size/allocation limits.
  Use one image-to-surface transform for drawing, hit testing and panel placement;
  panel rendering must scroll within the clamped bounds for large content.
- [ ] **Step 4: Run `cargo test --offline`.**
  Existing payload-only and decoder/backpressure tests must remain green; do not
  migrate the camera worker or change `jobs::DecodeOutcome` for positional scans.
- [ ] **Step 5: Commit `feat: retain QR detection positions and overlay geometry`.**

## Task 3: Observe display layout and reject unsafe overlay mappings

**Files:** Create `src/display.rs`; modify `Cargo.toml`, `Cargo.lock`, `src/lib.rs`; module tests.

**Interfaces:**
- `display::OutputGeometry { name: String, logical_position: [i32;2], logical_size: [u32;2], pixel_size: [u32;2], transform: u32 }`.
- `display::Layout { revision: u64, outputs: Vec<OutputGeometry> }` (complete snapshots only).
- `display::LayoutState::{Ready(Layout),Unavailable}`.
- `display::events() -> impl iced::futures::Stream<Item=LayoutState>`.
- `Layout::eligible_capture(&self, [u32;2]) -> bool`: exactly one complete output,
  screenshot dimensions matching either oriented physical or reported logical
  size (at most one pixel rounding difference per dimension); otherwise false.
  No arbitrary same-aspect-ratio inference. Ignore unrelated output metadata
  changes when deciding whether geometry revision must advance.

- [ ] **Step 1: Write eligibility/state tests with literal display fixtures.**
  KDE fixture: logical `(1829,1143)`, pixels `(2560,1600)`, one output at origin.
  Both reported resolutions are eligible. Reject empty/incomplete layouts, two
  outputs even if their total dimensions resemble one monitor, and mismatched
  screenshot `(3000,1600)`. Cover rotated physical dimensions, position/scale/size
  changes, repeated unchanged snapshots and output removal. Each geometric change
  advances revision exactly once after a complete protocol update.
- [ ] **Step 2: Run the new tests, then implement pure validation.**
  `cargo test --offline display::` must pass after implementation.
- [ ] **Step 3: Implement the read-only Wayland observer.**
  Add direct Linux dependencies `wayland-client = "0.31"` and
  `wayland-protocols = { version = "0.32", features = ["client", "unstable"] }`,
  reusing existing locked versions. Bind wl_output plus xdg-output logical
  geometry, observe initial completion and subsequent protocol changes, and
  publish complete layout snapshots. Use Tokio fd readiness/Wayland prepared
  reads and dispatch pending events; no timer or periodic subprocess polling.
  Dispatch initialization/roundtrips off the UI thread. Unsupported protocol,
  connection failure or incomplete topology emits Unavailable. Dropping the
  subscription must release its connection/read task; do not leave a blocked
  background thread. Do not parse kscreen-doctor output in production.
- [ ] **Step 4: Verify protocol event reducer tests and read-only KDE discovery.**
  Record observed geometry without taking a screenshot or altering display setup.
  Run formatting, Clippy and full tests; make no claim about untested compositors.
- [ ] **Step 5: Commit `feat: observe Wayland display geometry for safe QR overlays`.**

## Task 4: Build the overlay model and native Iced view

**Files:** Modify `src/overlay/mod.rs`; create `src/overlay/view.rs`; reuse `src/app/style.rs` and result-text helpers without duplicating action validation.

**Interfaces:**
- `overlay::Model` owns `session: JobId`, `layout_revision: u64`, frozen `Arc<capture::Prepared>`, `SpatialScan`, `hovered: Option<usize>`, `selected: Option<usize>`, viewport and overlay focus state.
- `overlay::Event::{Hover(Option<usize>),Select(usize),Background,Escape,Close,Next(bool),ActivateFocused,Copy,Activate,Reveal,Text(text_editor::Action)}`.
- `overlay::Effect::{None,Copy(usize),Activate(usize),Dismiss}`.
- `Model::new(JobId,u64,Arc<Prepared>,SpatialScan) -> Model`; `Model::update(Event,bool) -> Effect`; `Model::view() -> iced::Element<'_,Event>`; `Model::resize(iced::Size)`.
- `Model::update`'s bool is the current auto-copy preference. Keep a set of payload
  result indexes already auto-copied on selection, including aliases from duplicate
  detections. The model cannot execute OS actions. Its own read-only text buffers
  and reveal state are dropped/replaced on deselection, hide and dismissal.
- Controller supplies pending/action/copy presentation through narrow setters on
  the model, using typed existing action state; signatures must not carry raw
  provider error strings.

- [ ] **Step 1: Write model tests before view code.**
  Selection maps detection to payload. With auto-copy enabled, selecting either
  of two detections linked to result 0 returns Copy(0) once, then None; selecting
  result 1 returns Copy(1) once. Disabled preference never copies on selection;
  enabling it does not retroactively copy. Manual Copy always returns Copy of the
  selected result. No event can return Activate without an explicit action press.
  Escape with a panel clears selection; a second Escape returns Dismiss.
  Background clears selection; invalid indexes do nothing. Keyboard focus cycles
  numbered detections, visible panel controls and Close in both directions;
  selection changes cannot leave an invisible action armed. Wi-Fi secret buffers
  contain no password until reveal, and hiding clears both text and selection.
- [ ] **Step 2: Run `cargo test --offline overlay::` and implement the pure model.**
  Expect targeted cases to fail before implementation and pass afterward.
- [ ] **Step 3: Implement screenshot/highlight view using the same Transform.**
  Draw the frozen screenshot once, then flat translucent shade and crisp numbered
  detection outlines; selected/hovered/focused regions have distinct emphasis.
  Use actual quadrilateral hit regions. Consume background clicks without
  pass-through. Place the compact action panel inside geometry::panel_bounds;
  use read-only selectable text, Copy and validated action labels, password reveal,
  pending/error feedback and a visible Close control. Keep the visual style
  consistent with the existing native light/dark app. No blur buffer, animation
  timer, or main-window toolbar inside the overlay.
- [ ] **Step 4: Verify view layout through a native safe fixture harness.**
  Render one, several, rotated and edge-positioned codes, plus a long-text panel.
  Capture actual Iced own-window images; distinguish them from physical desktop
  acceptance. Remove temporary harness code before the commit.
- [ ] **Step 5: Commit `feat: add interactive fullscreen QR selection view`.**

## Task 5: Integrate capture, overlay lifecycle, copying and actions

**Files:** Create `src/app/overlay.rs`; modify `src/app/{mod,focus,tests,clipboard}.rs`; add `src/app/overlay_tests.rs` for focused integration tests.

**Interfaces:**
- App owns `layout: LayoutState`, optional overlay window/model, retired overlay
  IDs, and optional deferred external request (existing ActionKey/token/action).
- `Message::SpatialFinished(JobId,u64,Result<Arc<SpatialScan>,String>)` associates
  decode with its capture and layout revision. Fixed/redacted user errors only.
- `Message::OverlayEvent(JobId,window::Id,overlay::Event)`,
  `Message::LayoutChanged(LayoutState)`,
  `Message::WindowResized(window::Id,iced::Size)` route by role and identity.
- Spatial decoding runs in `spawn_blocking` inside an Iced Task after screen
  capture; preserve the existing camera worker and region fallback path.
- Overlay helpers on App: `open_overlay(...) -> Task<Message>`,
  `dismiss_overlay() -> Task<Message>`, `handle_overlay_event(...) -> Task<Message>`.
  Keep helper details in `src/app/overlay.rs`; avoid copying action execution logic.

- [ ] **Step 1: Write failing controller lifecycle tests.**
  Successful tray screen capture plus positional decode opens only the undecorated
  fullscreen overlay; no normal-window IDs are created. Region capture without
  an origin stays notification/Last scan only. A camera command opens a normal
  preview. No-code/error/cancelled captures do not create an empty overlay.
  Gate overlay creation on current known eligible layout and unchanged revision;
  late initial layout, hotplug while decoding, resizes inconsistent with the
  captured layout and stale callbacks cannot create misaligned clickable surfaces.
- [ ] **Step 2: Implement capture routing and role-specific windows.**
  Open overlay with `window::Settings { fullscreen:true, decorations:false,
  exit_on_close_request:true, ..Default::default() }`; route `view(id)` by role.
  Keep overlay IDs separate from `state.window_ids` (normal windows). Subscribe
  to display events; route window resize/key input with its ID. Query/check actual
  fullscreen surface dimensions before accepting hits. Capture contains neither
  old overlay nor normal app window. Suppress screen-scan pre-selection auto-copy;
  feed model selection effects into guarded copy/action handlers. Unavailable or
  multi-output layout retains notification/Last scan fallback with a clear reason.
- [ ] **Step 3: Add close/action race and privacy regressions, then implement them.**
  Overlay close releases image/model/revealed buffers but retains result identity,
  Last scan, pending tokens and outcomes. Register a pending explicit action before
  requesting overlay close, but dispatch it only after matching WindowClosed;
  duplicate clicks during close create no second task. A source reset before that
  close discards the deferred request. Never send an overlay Closed event through
  the normal camera/main-window source reset path. Source switches and Quit close
  overlays and cancel stale spatial work. Tray loss safely restores the main UI.
  Test retained old notifications/copy completions, camera Stop/failure pending
  actions, repeated scans, keyboard activation and malformed Wi-Fi stale buffers.
  Unpolled action tasks/private adapters must keep tests free of real side effects.
- [ ] **Step 4: Verify all integrated behavior.**
  Run focused overlay/model/controller tests, then `cargo test --offline`,
  `cargo fmt --check` and `cargo clippy --offline --all-targets -- -D warnings`.
  Existing camera, decoder backpressure, exact payload copying and settings tests
  must stay green. Fix any failure by name before proceeding.
- [ ] **Step 5: Commit `feat: integrate tray-first QR overlay capture and action lifecycle`.**

## Task 6: Native evidence, delivery documentation and final integration

**Files:** Modify `README.md`, `docs/verification.md`; add safe native captures under `docs/screenshots/`.

- [ ] **Step 1: Capture native overlay evidence on the current KDE display.**
  Use synthetic QR fixtures and actual Iced surfaces at 140% scale. Show initial
  multi-code highlights, selected action panel, long/selectable text and masked
  Wi-Fi. Verify coordinate alignment and edge-panel clamping. Check overlay close
  removes its process resources and returns to tray. Physical gestures that cannot
  be exercised stay explicitly unverified; never substitute a browser mockup.
- [ ] **Step 2: Update docs around the final implemented behavior.**
  Remove obsolete claims that clicking the tray icon scans, that all screen results
  are notification-only, or that screenshots are automatically copied before code
  selection. Document single-display eligibility, unsupported-layout fallback,
  native region provider requirements, read-only partial selection, selected-code
  auto-copy, privacy, pending actions and dismissal. Keep prior benchmark dates
  and numbers clearly historical; do not infer performance guarantees.
- [ ] **Step 3: Run release checks from the feature worktree.**
  `cargo fmt --check`; `cargo clippy --offline --all-targets -- -D warnings`;
  `cargo test --offline`; `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_*.py'`;
  `cargo build --offline --release`; `git diff --check`. All must exit 0.
- [ ] **Step 4: Commit `docs: document desktop QR overlays and verification`.**
- [ ] **Step 5: Conduct independent whole-branch review and finish locally.**
  Review every product change from `d6cfa67`, including inherited work, against
  the spec. Fix blocking findings and re-review fixes. Follow the user's existing
  local-merge preference: fast-forward main only after review and a clean working
  tree, rebuild/test main, remove only this owned worktree/feature branch after
  verification, then restart the latest release app. Preserve other worktrees,
  personal settings and unrelated processes. Do not push or install globally.

## Plan self-review

Spec coverage: tray/notifications/region/selectable text Task 1; positional bounds
Task 2; layout eligibility/change handling Task 3; overlay UX/selection policy
Task 4; lifecycle/security/camera preservation Task 5; native evidence and delivery
Task 6. All five Review Focus cases have named owning tasks and concrete tests.

Shared interfaces are defined once: SpatialScan/Detection in Task 2, LayoutState
in Task 3, Model/Event/Effect in Task 4, and guarded controller messages in Task 5.
A cropped region without origin never enters the full-desktop alignment path.
The existing camera decode signature stays unchanged. No placeholder code or
production harness is required to pass an intermediate gate.
