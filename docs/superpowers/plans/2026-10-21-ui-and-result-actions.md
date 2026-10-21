# Desktop UI and QR Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the approved KDE-style layout, remembered single-result copying, and explicit URL, telephone, and Wi-Fi actions in the running Rust/Iced app.

**Architecture:** Retain the capture/camera/decoder lifecycle and software renderer. Add pure payload classification and clipboard policy, private preference storage, and asynchronous platform actions; extract view/style code from the large app controller. Each result action carries source-session and result identity so stale completions cannot affect a newer scan.

**Tech Stack:** Existing Rust/Iced 0.14, image/rqrr, serde/serde_json, url, Tokio and zbus. Use NetworkManager D-Bus for Wi-Fi; no shell or password command arguments. Add only dependencies required by an implemented boundary.

**Spec:** `docs/superpowers/specs/2026-10-19-ui-and-result-actions-design.md`

## Global Constraints

- Iced and its lightweight software renderer remain the defaults.
- Preserve quick screenshot capture without the recurring desktop capture dialog, explicit interactive fallback, tray behavior, camera resource limits, generation filtering, local decoding, and no scan history.
- Automatic copying defaults off on a fresh installation.
- Turning the setting on does not retroactively copy an already displayed result.
- A batch containing multiple distinct codes does not auto-copy any of them.
- Camera duplicate frames never repeatedly overwrite the clipboard.
- Pass secrets through the NetworkManager D-Bus API rather than command-line arguments, shell strings, logs, or persisted scanner settings.
- Repeated clicks while a request is pending do not create competing connection attempts.
- Never execute network changes, launch calling software, or navigate web pages in automated tests or merely to verify the implementation.
- Report physical actions and desktop sessions not actually exercised as unverified.

## Review Focus

- A malformed/enterprise Wi-Fi QR must never be treated as an open or personal network (Task 1).
- Slow preference writes must preserve the latest checkbox choice and remain outside the UI thread (Task 2).
- A stale action completion after a source switch must not overwrite the current result/error state (Task 3).
- Continuous camera duplicates and multi-code batches must not repeatedly replace the clipboard (Task 2).
- Narrow windows and long QR payloads must retain usable actions and scrollable results (Task 4).

## File Structure

- `src/results.rs`: raw payload preservation and typed content classification.
- `src/results/wifi.rs`: escaped Wi-Fi QR parser and redacted credentials.
- `tests/result_actions.rs`: content fixtures and action eligibility.
- `src/settings.rs`: private JSON preference load/save and configuration errors.
- `src/app/clipboard.rs`: pure single-result copy policy.
- `src/actions/mod.rs`: action requests, identities, errors, and URI handoff.
- `src/actions/network_manager.rs`: Linux Wi-Fi device selection and D-Bus connection requests.
- `src/app/mod.rs`, `src/app/state.rs`: lifecycle, settings/action/clipboard messages and state.
- `src/app/view.rs`, `src/app/style.rs`: responsive native-style layout and theme.
- `src/app/tests.rs`: existing controller tests moved without unrelated changes, plus integration regressions.
- `src/lib.rs`, `Cargo.toml`, `Cargo.lock`: exports and necessary dependencies/features.
- `README.md`, `docs/verification.md`: settings/actions usage and observed verification.

## Task 1: Classify actionable QR payloads

**Files:** results modules, `tests/result_actions.rs`, existing decoder/tests and URL-action call sites.

**Interfaces:**
- Keep `QrResult::from_payload(Vec<u8>) -> QrResult` and raw `payload` / copy `display_text`. Add `content: QrContent` and `action(&self) -> Option<ResultAction>`.
- `QrContent` variants: `Url(url::Url)`, `Phone { uri: url::Url, number: String }`, `Wifi(WifiNetwork)`, `Text`, `Binary`, `Invalid { kind: ContentKind, reason: String }`. `ContentKind` identifies Website, Phone, or Wifi for invalid-format presentation.
- `ResultAction` variants: `OpenUrl(url::Url)`, `Call(url::Url)`, `ConnectWifi(WifiNetwork)`; all cloneable for asynchronous delivery.
- `WifiNetwork { ssid: String, security: WifiSecurity, password: Option<String>, hidden: bool }`; `WifiSecurity` distinguishes Open, WpaPersonal, Sae, and Unsupported(String). Implement redacted Debug for credentials.
- `results::wifi::parse(input: &str) -> Result<WifiNetwork, String>` validates a recognized `WIFI:` payload. Unsupported security remains displayable but `action()` returns None with a useful explanation in the view.

- [ ] Add tests before implementation: original safe URL rules, `tel:+41446681800`, a number with visual separators/extension, plain numeric text without Call, empty telephone payload, injected controls/query/invalid phone syntax, and binary payload preservation.
- [ ] Add Wi-Fi fixtures with literal expected fields: open `WIFI:T:nopass;S:Guest;;`, WPA password, SAE, hidden network, escaped `;`, `:`, `,`, quote and backslash, missing/empty SSID, conflicting duplicates, incomplete escape/terminator, missing password, unknown auth, WEP and EAP. Assert unsupported/enterprise data never produces ConnectWifi. Verify Debug omits the password.
- [ ] Run `cargo test --test result_actions` and observe missing-behavior failures. Implement the parser/classifier, preserving existing byte decoding and URL restrictions. Reject malformed/conflicting fields, preserve supported escaping, and treat enterprise fields/authentication conservatively.
- [ ] Migrate existing `open_url` field call sites to the typed URL action without adding phone/Wi-Fi side effects yet. Run `cargo test` and Clippy; existing Copy/Open behavior remains functional.
- [ ] Commit `feat: classify QR links phone numbers and Wi-Fi networks`.

## Task 2: Preferences and automatic clipboard policy

**Files:** `src/settings.rs`, `src/app/clipboard.rs`, app state/controller, exports/dependencies.

**Interfaces:**
- `settings::Preferences { auto_copy_single: bool }` derives serde with a default false value and tolerates unknown JSON fields.
- `settings::load() -> LoadedPreferences` returns `{ preferences: Preferences, path: Option<PathBuf>, warning: Option<String> }`; path resolves from XDG_CONFIG_HOME or HOME. Missing file uses defaults without warning; corrupt/unreadable file uses defaults with a visible warning.
- `settings::save(path: &Path, preferences: &Preferences) -> anyhow::Result<()>` writes private 0700 directory / 0600 file with atomic same-directory replacement; reject unexpected symlinks/nonregular destination files. Do not log payloads or credentials.
- `app::clipboard::AutoCopyPolicy::on_results(&mut self, enabled: bool, session: JobId, results: &[QrResult], newly_added: usize) -> Option<String>` copies only an introduced single distinct result when the session total is one, at most once per session/payload. Manual Copy always uses display_text.
- App adds preference-load/save messages and a shared toggle message. Save work is serialized/coalesced so a late earlier write cannot win. Copy feedback contains session/result identity and stays separate from scan errors.

- [ ] Write failing settings tests for default/load, unknown fields, corrupt file not overwritten by load, unavailable path, private atomic writes, save failure, and ordered/coalesced saves retaining the newest setting.
- [ ] Write failing clipboard tests: disabled, exactly one new result, two distinct codes in one batch, duplicate batch entries, repeated camera frames, second distinct code later, stale outcome, a new session, toggle-on without a new result, and binary/raw Wi-Fi copy content. Assert actual selected copy text, not only bookkeeping fields.
- [ ] Run focused tests to observe RED, then implement configuration storage and clipboard policy. Load/save asynchronously; defaults keep scanning usable while load is pending or fails. An explicit setting change permits replacing a corrupt file.
- [ ] Integrate accepted-result bookkeeping after existing session filtering/deduplication. Preserve all distinct results; do not alter worker capacity, camera timing, Stop preservation, or decoder shutdown.
- [ ] Add controller tests for stale decode outcomes never triggering clipboard work, toggles producing no retroactive copy, and preference errors not clearing scan results. Run `cargo test` and Clippy.
- [ ] Commit `feat: remember single-result automatic copying`.

## Task 3: Explicit desktop and Wi-Fi actions

**Files:** actions modules, app controller/state, action request tests.

**Interfaces:**
- `actions::ActionKey { session: JobId, result_index: usize }` identifies UI delivery; a monotonically increasing request token additionally distinguishes repeated requests for the same result.
- `actions::execute(action: ResultAction) -> impl Future<Output = Result<(), ActionError>>`; `ActionError` exposes useful redacted user feedback. Desktop URI handoff uses a validated one-argument `xdg-open` call off the UI thread.
- NetworkManager adapter constructs a typed `a{sa{sv}}` connection settings dictionary and uses zbus on the system bus. Open networks omit wireless-security; WpaPersonal uses wpa-psk; Sae uses sae. Include SSID bytes, hidden flag, and `connection.autoconnect=false`. Credentials exist only in the D-Bus request/in-memory result.
- Wi-Fi chooses an available managed wireless device, preferring an enabled compatible device already serving the desktop. Include network availability/device discovery and active-connection completion monitoring with a finite timeout; do not report success merely because D-Bus accepted the request. Do not disconnect a network during tests/probes.
- App owns per-result pending/success/error state and rejects duplicate pending actions. New source clears presentation state, while late completion is discarded by session/request identity.

- [ ] Write failing action eligibility/controller tests: decode and Copy never dispatch an action; explicit URL/Call/Connect click does; repeated pending click dispatches once; an old completion cannot change a new session or newer request; failures retain readable results and Copy.
- [ ] Write request-construction tests for open/WPA/SAE, SSID byte preservation and hidden flag, autoconnect false, no credentials in Debug/error output, and unsupported/malformed requests rejected before D-Bus. Test device/service absence, authorization failure, rejected password and activation timeout via the adapter boundary without real network mutations.
- [ ] Run focused tests to observe RED. Implement asynchronous URI and NetworkManager adapters using current official API / installed introspection as needed. Ensure actions originate only from a user click and never from auto-copy. Show telephone-handler and Wi-Fi capability errors instead of pretending the platform supports them.
- [ ] Run `cargo test` and Clippy. Confirm no tests invoke real xdg-open or NetworkManager mutation methods; read-only capability inspection is permitted.
- [ ] Commit `feat: add explicit URL phone and Wi-Fi actions`.

## Task 4: Implement the approved desktop layout

**Files:** `src/app/view.rs`, `src/app/style.rs`, controller/state, moved app tests, assets only if needed.

**Interfaces:**
- `app::view::view(app: &App, window_id: iced::window::Id) -> iced::Element<'_, Message>` renders existing controller state; view helpers do not execute actions or perform I/O.
- `app::style` centralizes Breeze-like spacing, color palettes, typography, card/button borders and icon styling. Use system light/dark hints; fall back to BreezeLight/BreezeDark consistently. Do not introduce a browser or always-running animation timer.
- Viewport resize messages choose two-pane layout at widths >=760 logical pixels and stacked layout below 760. Preserve source image coordinate fitting and generation/epoch identity for crop selection.
- Messages: More/Settings visibility, synchronized auto-copy toggle, per-result action, per-result password reveal, and existing capture/camera commands. Secrets remain masked by default, including after source change.

- [ ] Extract existing view and controller tests into their focused modules without behavior changes. Keep the app entry/IPC/tray/capture subscriptions stable.
- [ ] Implement toolbar Scan screen / Camera / More / Settings; move Desktop capture and Quit into More. Implement left capture/source pane and right scrollable result cards matching `docs/design/ui-layout-preview.html`. Provide a useful empty state, distinct scan/error/cancelled states, dimensions, Rescan, crop instructions, and camera selector/Stop.
- [ ] Implement cards for URL/Phone/Wi-Fi/Text/Binary/Invalid: readable content, Copy and relevant primary action, password Show/Hide, unsupported explanations, and per-result pending/error feedback. Auto-copy checkbox and copied feedback appear in the status bar and Settings.
- [ ] Add focused behavior tests for responsive breakpoint selection, reveal reset, More command routing, synchronized preference controls, and scan-error preservation during copy/action feedback. Do not add tests mirroring styling constants or static labels.
- [ ] Run tests/Clippy and release build. Launch the redesigned app on KDE. Capture actual light/dark and representative state screenshots using deterministic in-memory preview data or a temporary developer harness; do not pretend a browser mockup is the app. Check narrow width, long payloads, multiple results, keyboard focus and basic controls; report unavailable physical checks explicitly.
- [ ] Commit `feat: apply the approved KDE-style scanner layout`.

## Task 5: Final integration and running-build handoff

**Files:** README, verification report, narrowly scoped fixes identified by review.

- [ ] Review all spec requirements against the implemented behavior, including quick/interactive capture, Stop retaining results, clipboard policy, private preference-only persistence, explicit action dispatch and no action-on-decode. Run final review after task reviews.
- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_*.py'`, and `cargo build --release`; require successful exit status and fix material failures.
- [ ] Smoke-test native launch, second invocation, tray Quit, window close/reopen and real camera start/stop where available. Observe idle CPU/memory briefly and record any regression without presenting historical numbers as fresh measurements. Do not place a call, navigate a URL, or connect Wi-Fi as an automatic verification step.
- [ ] Update README with auto-copy rules, preference path/default, supported QR actions/security modes, telephone handler/NetworkManager requirements, first-use permissions, and KDE-style Iced limitations. Update verification with actual screenshots, commands/results and unverified physical actions/other desktop sessions.
- [ ] Commit `docs: document redesigned scanner and result actions`.
- [ ] Integrate locally following the user's established local-merge preference after green review/checks. Rebuild in the main checkout and clean up only the owned worktree. Reopen the new release build so the app that starts matches the approved design.

## Execution handoff

Use the user's established subagent-driven implementation/review method. Tasks are sequential because each exposes interfaces used by the following task. Task reviews are scoped to the task; a final whole-branch review checks settings/clipboard/action/lifecycle interactions. The controller resolves routine API details against the spec, records decisions and keeps working without extra intermediate permission questions.
