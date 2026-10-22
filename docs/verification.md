# Verification and release measurements

Current redesigned-interface evidence is recorded in the dated section below.
All 2026-10-18/19 measurements and counts in the following original report are
**historical**, not measurements of the redesigned build.

2026-10-18, KDE neon 24.04, Plasma Wayland; Intel Core Ultra 7 265H
(16 logical CPUs), 62.2 GiB RAM; Intel Arrow Lake-P graphics plus NVIDIA
PCI device 2db8. Rust 1.99.0. No packages installed for verification.

## Reproduce

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
cargo run --release --example decode_benchmark
cargo build --release --features gpu
python3 tools/measure-runtime.py target/release/qr-scanner tiny-skia
python3 tools/measure-runtime.py target/release/qr-scanner wgpu
```

The runtime script runs only isolated instances with a private temporary
XDG_RUNTIME_DIR and the absolute real Wayland socket. It preserves the user's
existing app and session bus. It waits three seconds after the control socket
appears, then samples process ticks and `/proc/PID/smaps_rollup` for ten seconds.
CPU percent is relative to one core (100% = one core). RSS/PSS are a single end
sample, not peaks. Startup means process creation to control socket availability
(1 ms polling resolution), **not** first visible frame or user-perceived latency.
Each run repeats Show three times, then forwards Quit and requires process exit.
Idle is a visible unchanged normal window; camera uses `--scan-camera`, the
native 640×480 Integrated RGB Camera, conversion <=15/s and decoding <=5/s.
No physical QR target or camera image is saved. This is a short development
sample under an active desktop workload, not a controlled production guarantee.

The decode benchmark creates one deterministic QR on a white full-size grayscale
canvas, warms up twice, then times ten full-frame `decode` calls including rqrr's
grayscale copy/detection/payload decoding. It excludes portal capture, PNG load,
RGB-to-gray conversion and UI delivery. It asserts one result each iteration.
These simple synthetic images do not represent arbitrary photographic scenes.

## Historical observed results (2026-10-18)

Both rendering runs used the same release binary built with `--features gpu`,
with `ICED_BACKEND` forced to a single backend, the same window and camera,
and sequential ten-second samples after verification compilation had finished.
The physical wgpu adapter was not logged; both Intel and NVIDIA adapters exist.
No claim about a particular GPU vendor's performance follows from these numbers.

| Renderer / workload | Socket startup ms | CPU % (one core) | RSS KiB | PSS KiB |
| --- | ---: | ---: | ---: | ---: |
| tiny-skia / idle | 5.327 | 0.000 | 24,976 | 18,263 |
| tiny-skia / camera | 7.116 | 14.097 | 36,520 | 29,760 |
| wgpu / idle | 5.145 | 0.300 | 249,112 | 183,284 |
| wgpu / camera | 5.444 | 11.687 | 215,392 | 149,516 |

All four isolated runs exited 0 after repeated Show and Quit, with empty stderr;
both camera samples had `/dev/video0` open. Idle samples had no camera open.
Memory is process accounting; it does not include all compositor/GPU allocations.
The shorter camera RSS than idle in wgpu is a single-sample observation, not an
assertion that camera use reduces memory. Native hardware previously negotiated
MJPEG 640×480 at 30fps despite the requested 15fps; conversion/preview remain
limited to 15/s and decode submission to 5/s.

**Selected default: tiny-skia**, based on substantially lower memory and idle
CPU in this workload. Wgpu reduced camera CPU by approximately 2.4 percentage
points, but its process PSS was approximately 120 MiB higher during camera use.
Keep the optional backend for future measurements on other hardware. These are
one-run observations; no significance or worst-case guarantee is claimed.

| Full-frame grayscale decode | Samples | Min ms | Median ms | Max ms |
| --- | ---: | ---: | ---: | ---: |
| 1920×1080 | 10 | 15.613 | 16.136 | 26.779 |
| 3840×2160 | 10 | 64.350 | 65.118 | 66.095 |

Required checks all passed: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` (40 unit and 8
integration tests), `cargo build --release`. The earlier optional GPU release
build and `cargo clippy --all-targets --all-features -- -D warnings` also passed;
those optional checks were not repeated in the final fix wave.
Final review fixes were rechecked with the default fmt, clippy, full Rust test
suite and release build; the runtime probe regression also passes with
`python3 -m unittest discover -s tools -p 'test_measure_runtime.py'` (1 test).
Successful screen capture now schedules decoding immediately; Stop retains
results while rejecting stale camera/decode work; cancelled capture has distinct
feedback. This verification did not repeat benchmarks or physical acceptance;
the measurements above remain historical short observations.

`desktop-file-validate assets/qr-scanner.desktop` passed. The SVG icon and entry
are delivery assets; their installed launcher appearance remains unverified.

## KDE acceptance status

“Passed (automated/probe)” is mechanical evidence and does not imply a physical
click or visual acceptance. Prior task probes used this same desktop.

| Case | Status and evidence |
| --- | --- |
| Normal launch / tray registration / menu wiring | Passed (probe): native Wayland launch, StatusNotifierWatcher registration; D-Bus GetLayout lists Scan screen, Scan camera, Show window, Quit. Tray icon appearance unverified. |
| No tray host, background startup | Passed (probe): isolated `dbus-run-session`, Wayland trace shows surface/title creation; state test covers fallback. Physical close with no tray unverified. |
| Close to tray, reopen, tray host loss/reappearance | Passed (state tests); physical interactions and panel disappearance unverified. |
| Delayed screen/decode/camera result after close/source change | Passed (unit tests): generation filtering and retained image/preview cleanup. Physical timing interaction unverified. |
| Repeated CLI commands | Passed (probe): isolated Show repeated three times per measurement, followed by Quit. IPC election/forwarding integration tests also pass. |
| Portal support / permission / cancellation | KDE portal version 2 observed. Adapter tests pass cancellation, error and local-URI validation. Physical dialog acceptance/cancellation unverified. |
| Region selection / Escape / full-image scan | Crop coordinate and lifecycle tests pass. Physical drag, mixed-DPI appearance and Escape interaction unverified. |
| Multiple results / binary payload / duplicate suppression | Passed (offline decode/state tests). Visible result scrolling unverified. |
| Clipboard Copy / explicit safe Open | URL policy tests pass. Actual clipboard and physical Open click unverified (KDE rejects available virtual-keyboard automation). |
| Camera native capture / release / reopen | Passed (hardware probe): two 640×480 read/stop/reopen cycles. Stop+join 5.841 ms and 61.511 ms in Task 4; not guarantees. |
| Camera preview / physical QR / Stop / LED / unplug | Unverified physical acceptance. Fake-source tests pass scheduling, latest-frame delivery, stop/drop, unplug/error cleanup; real GUI probe opens `/dev/video0`. |
| Quit | Passed (probe): CLI Quit and D-Bus menu Quit exit 0. Physical tray click unverified. |

The available KDE session rejects `wtype` virtual keyboard input. No visual GUI
automation or physical click, camera LED, physical unplug, or real camera QR
acceptance is claimed. The functional mechanical checks do not close these gaps.

## Other sessions: all unverified

For **each** GNOME, Hyprland and Sway session, complete this checklist:

- [ ] Launcher and no-tray fallback; close/reopen and Quit.
- [ ] Tray menu with compatible extension/panel; host disappearance/reappearance.
- [ ] Repeated CLI forwarding and background startup.
- [ ] Screenshot permission, cancel, errors, full image, region and mixed-DPI scaling.
- [ ] Multiple results, Copy clipboard and explicit HTTP/HTTPS Open.
- [ ] Camera selector/preview, physical QR, Stop/close/Quit LED release, unplug/reopen.
- [ ] Delayed results after window close/source change; no stale window reopening.

Native device access is implemented. Flatpak Camera portal/PipeWire support is
deferred. The default Scan screen action now uses noninteractive portal capture, with
Desktop capture… as an explicit interactive fallback. A first-use permission
prompt remains desktop-controlled. No universally dialog-free promise is made.

## Quick capture update (2026-10-19)

Wire-format regression tests verify that quick capture sends `interactive=false`
and never requests the native area selector, including when the portal advertises
area support. The explicit fallback requests interactive capture and uses native
area selection only when supported. The app lifecycle test verifies retained
fallback mode across window closure, restored window state after a permission
error, and that the next Scan screen returns to quick mode. Source review confirms
that errors do not trigger an automatic second request; the lifecycle test does
not execute the returned portal request against a live service. Both default-mode and fallback tests
were observed failing before the changes.

The updated Rust suite has 43 unit tests and 8 integration tests. The measurements
above predate this capture-option change; no new capture latency or first-use
permission acceptance is inferred from them. Live first-use consent and repeated
quick capture on the installed KDE portal remain to be checked by the user.

## Redesigned interface verification (2026-10-22)

Verified the redesigned default tiny-skia build on KDE neon / Plasma Wayland.
The source baseline was `b93a072`; this delivery updates documentation only.
No packages were installed. Fresh required commands, all exit 0:

```sh
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_*.py'
cargo build --offline --release
```

Cargo used the shared `CARGO_TARGET_DIR` in the main checkout. Rust passed
78 library tests, 8 decoding integration tests, and 8 result/action integration
tests (94 total); Python passed 1 runtime-probe regression. The suite covers
quick versus interactive capture, crop scaling/cancellation, Stop retaining
results, stale generations, distinct-result copy policy, preference persistence
and warnings, parser/security fixtures, click-only actions, duplicate pending
suppression, sanitized errors, and stale action completion. Action tests use
simulated adapters or drop effect futures; they do not execute external actions.
Private atomic settings tests use temporary directories.

### Actual native interface captures

These are the real `app::view::view` rendered by Iced in native Wayland windows,
using deterministic in-memory fixture results and a synthetic preview. They are
not browser mockups or physical captures of QR targets. Iced's own-window
screenshots exclude compositor decorations; the application still uses native
window decorations. The temporary fixture harness was removed before the final
build; the production binary has no hidden fixture switch.

| State | Native capture |
| --- | --- |
| Empty, light / dark | [Light](screenshots/native-empty-light.png), [dark](screenshots/native-empty-dark.png) |
| Website and retained-image controls | [Website](screenshots/native-website-light.png) |
| Telephone | [Phone](screenshots/native-phone-light.png) |
| Wi-Fi, credentials masked, light / dark | [Light](screenshots/native-wifi-light.png), [dark](screenshots/native-wifi-dark.png) |
| Invalid Wi-Fi, raw payload hidden | [Invalid Wi-Fi](screenshots/native-invalid-wifi-light.png) |
| Multiple results and scrolling | [Multiple](screenshots/native-multiple-light.png) |
| Stacked layout at 560 logical pixels | [Narrow](screenshots/native-narrow-light.png) |
| Long wrapping payload | [Long](screenshots/native-long-light.png) |
| Scan error | [Error](screenshots/native-error-light.png) |
| Settings and mirrored preference | [Settings](screenshots/native-settings-light.png) |
| Keyboard focus | [Focus](screenshots/native-focus-light.png) |
| Focus scrolled past variable-height cards | [Desktop](screenshots/native-scroll-last-light.png), [narrow](screenshots/native-scroll-last-narrow-light.png) |

Exact widget-bound scrolling tests verify focused controls become visible before
keyboard activation. Delayed preference warnings invalidate old visibility
confirmations. Physical keyboard traversal of the camera device dropdown,
live system appearance changes, and dimensions smaller than the captured narrow
window remain unverified. The appearance is a Breeze-style Iced approximation,
not native Qt rendering.

### Current isolated native smoke and resources

The release probe used a private 0700 temporary `XDG_RUNTIME_DIR` **and** private
`XDG_CONFIG_HOME`, an absolute Wayland socket, and the existing session bus.
It neither read the user's auto-copy preference nor wrote the user's settings or
clipboard. The existing user-visible preview was left running. No URL, telephone
or Wi-Fi effect was executed. No desktop screenshot or camera image was saved.

After a two-second settle, one five-second idle sample observed **0 CPU ticks**
(**0.0% of one core** at 100 Hz), **25,980 KiB RSS**, and **19,233 KiB PSS**.
No camera device descriptor was open. This is a single brief development sample,
not a benchmark or peak-memory guarantee. The earlier renderer comparison and
camera CPU figures above remain historical and were not rerun.

Native launch and three second-invocation Show commands succeeded. The probe
identified its own StatusNotifierItem by process ID and read its menu (Scan
screen, Scan camera, Show window, Quit). `--scan-camera` opened `/dev/video0`.
A temporary KWin script closed only windows belonging to that probe PID: the
process stayed alive in the tray and all camera descriptors were released.
A subsequent Show command forwarded successfully and the process stayed alive;
this is IPC evidence, not visual confirmation of the reopened surface. Invoking
that probe's D-Bus menu Quit exited 0. Stderr was empty and the temporary config
directory remained empty. The KWin script was unloaded immediately afterward.
Physical Stop-button acceptance remains unverified; Stop retention/release is
covered by controller/worker tests. The first probe's menu query failed because
`gdbus` treated negative depth as an option; the isolated process was cleaned up,
and the lifecycle-only retry corrected the argument separator without repeating
the idle resource sample.

### Action and physical acceptance gaps

Automated tests verify that web URLs, explicit telephone numbers and supported
Wi-Fi networks are classified locally; unsupported/malformed content stays
copyable. Wi-Fi passwords and malformed Wi-Fi raw data are hidden until reveal.
Provider error strings and request Debug output cannot echo credentials. The
NetworkManager request tests verify `persist="volatile"` and
`connection.autoconnect=false`; no real connection was created to confirm its
lifetime on this machine. A successful request can change the active network;
that deliberate physical action remains for a user click.

Actual clipboard writes, launching browser/calling handlers, successful Wi-Fi
connection/authorization/authentication failures, profile removal after a real
disconnect, and physical button/tray clicks remain **unverified**. No external
action was performed as a verification shortcut. Portal first-use permission,
repeated quick capture without a recurring dialog, interactive fallback, crop
dragging/Escape/mixed DPI, physical QR acceptance, camera LED/unplug behavior,
and installed launcher appearance remain physical acceptance gaps. All GNOME,
Hyprland and Sway checks in the original checklist remain unverified. Automated
coverage and native fixture screenshots do not imply those checks passed.
