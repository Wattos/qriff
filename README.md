# QR Scanner

A Linux-first local QR scanner built with Rust and Iced. The current shell has a
main window, a StatusNotifierItem tray menu, and single-instance command routing.
Scan a portal screenshot, crop a region, or scan continuously from a local camera.
Results stay in memory and offer Copy plus explicit actions for validated web URLs,
telephone numbers, and Wi-Fi networks.

## Build and run

Use stable Rust and Cargo. Development was checked on KDE neon 24.04 / Plasma
Wayland. Build prerequisites include a C linker, pkg-config, Wayland/X11 client
libraries and xkbcommon development files (Debian/Ubuntu: build-essential,
pkg-config, libwayland-dev, libxkbcommon-dev, libx11-dev, libxcursor-dev,
libxi-dev, libxrandr-dev). Check installed prerequisites before installing packages.
The current renderer is Iced's software tiny-skia backend; no GPU backend is
compiled by default. Native camera builds also need libclang and Linux V4L2
headers (Debian/Ubuntu: libclang-dev, linux-libc-dev). The direct V4L2 backend
does not need libudev-dev or libv4l-dev. Screenshot capture uses the desktop
portal and its installed screenshot backend.

```sh
cargo build
cargo run
cargo run -- --background
cargo run -- --scan-screen
cargo run -- --scan-camera
cargo run -- --quit
cargo test
cargo check
cargo clippy --all-targets -- -D warnings
```

A normal launch opens the main window. `--background` starts in the tray when a
tray host exists, otherwise it opens the window. Closing the last window keeps
the application alive with a tray host, and exits without one. Losing the tray
host while hidden opens a window. Open window and Quit are in the tray menu.
Further launches forward commands to the existing process.

The control socket lives in `$XDG_RUNTIME_DIR/qr-scanner/control.sock`; the runtime
directory must be owned by the current user and private (0700). A lifetime file
lock protects socket creation and stale recovery. The lock file deliberately
remains after exit; it must never be deleted while any instance is running.

Install for the current user after building a release (no root needed):

```sh
cargo build --release
install -Dm755 target/release/qr-scanner "$HOME/.local/bin/qr-scanner"
install -Dm644 assets/qr-scanner.svg "$HOME/.local/share/icons/hicolor/scalable/apps/qr-scanner.svg"
mkdir -p "$HOME/.local/share/applications"
sed "s|^Exec=.*|Exec=\"$HOME/.local/bin/qr-scanner\"|" assets/qr-scanner.desktop > "$HOME/.local/share/applications/qr-scanner.desktop"
```

The generated desktop entry uses the installed binary's absolute path; terminal
commands require `$HOME/.local/bin` in PATH. Remove those three installed files
to uninstall. The launcher opens the normal main window.

Install and enable `xdg-desktop-portal` plus the backend matching your session:
KDE uses `xdg-desktop-portal-kde`; GNOME uses `xdg-desktop-portal-gnome`.
Hyprland normally uses its portal backend; Sway commonly uses
`xdg-desktop-portal-wlr`. Confirm that your backend actually implements the
Screenshot portal; screencast support alone is insufficient. Desktop selection
and backend routing use the session's `XDG_CURRENT_DESKTOP` and portal
`portals.conf`; consult the distribution's session configuration if capture fails.
GNOME tray support needs an optional AppIndicator/StatusNotifierItem extension
(such as Ubuntu AppIndicators); without one the application uses its window.

No scan history, telemetry, autostart, network decoding, or automatic link opening
is included. The idle shell does no camera capture or screen polling.
Windows and macOS are future work; this build reports an explicit unsupported
platform error there.

## Interface and result actions

The KDE/Breeze-style Iced window places capture controls and preview beside
scrollable result cards. Narrow windows stack the source above the results.
Scan screen takes a fresh quick screenshot; Select region crops the retained
image, and Rescan decodes that same image again. Camera starts live capture;
Stop releases it and retains the decoded results. A new source starts a fresh
session. More contains Desktop capture… and Quit; Quit also remains in the tray.
Settings and the status bar expose the same automatic-copy checkbox.

This is an Iced approximation of Breeze, not native Qt widgets. It uses desktop
appearance hints with light/dark fallbacks, native window decorations, and the
software renderer. Actual native-window fixture screenshots and remaining
physical checks are in [verification](docs/verification.md#tray-overlay-verification-2026-10-22).

Scan screen automatically copies a lone detected QR code and shows a notification,
without opening a selection overlay. This shortcut applies even when the optional
auto-copy preference is off. Multiple QR locations open the chooser, including
codes with identical contents. Copy failures keep the result in Last scan.

Automatically copy single results defaults **off** for camera/selection behavior.
Region detection does not automatically copy. In the multi-code screen overlay,
selecting a highlight may copy that payload once when the preference is enabled. Selecting the same
payload again (including another QR with identical bytes) does not recopy it;
selecting another payload may copy that one once. Changing the preference does
not copy an already selected result.

Camera decoding retains its single-result policy: a current-session decode
copies only when it introduces one new distinct payload and the
session still contains exactly one distinct result. Repeated frames and
multiple-code batches do not recopy. A new scan resets copy bookkeeping.
Manual Copy always remains available.

Both copy modes preserve the original UTF-8 QR payload (including a complete
encoded Wi-Fi payload), or the labeled hexadecimal representation for binary
data. Automatic copying a Wi-Fi QR therefore puts its credentials on the
clipboard; password masking in the card does not change what Copy copies.

Only preferences are persisted, in
`$XDG_CONFIG_HOME/qr-scanner/settings.json`, or
`$HOME/.config/qr-scanner/settings.json` when XDG_CONFIG_HOME is unset/empty.
The preference is `auto_copy_single`, default `false`. Writes use a private
0700 app directory, 0600 temporary file, and atomic replacement. Missing or
corrupt settings use defaults; corrupt files are left alone until a setting is
changed. A settings warning does not prevent scanning or discard results. No
payloads, camera images, or scan history are saved by the scanner.

Result actions run only after an explicit button click or keyboard activation;
decoding, revealing a password, and copying never open an external application
or request a network connection.

- **Web:** Open URL accepts HTTP/HTTPS with a parsed host, without credentials
  or control characters, and uses `xdg-open` to hand it to the desktop URI
  handler without a shell.
- **Telephone:** Only explicit validated `tel:` payloads offer Call number.
  A configured desktop telephone handler and `xdg-mime` are required; that
  application handles review and dialing. Ordinary numeric text has no calling action.
- **Wi-Fi:** Common `WIFI:` payloads support escaped fields, hidden SSIDs, open
  networks, WPA/WPA2 Personal, and WPA3/SAE where NetworkManager and hardware
  support it. Passwords stay masked until Show. WEP, enterprise/EAP and unknown
  security types remain readable/copyable with an unsupported explanation.
  Malformed/conflicting fields do not create a connection action; malformed
  Wi-Fi raw text is also hidden until explicitly revealed.

Connect to Wi-Fi requires Linux NetworkManager with the
`AddAndActivateConnection2` D-Bus API, an enabled managed compatible wireless
device, and permission under the desktop's NetworkManager authorization policy.
It may change the current network connection. Credentials go through D-Bus,
never a command line or scanner settings. The requested profile is **volatile**,
with `autoconnect=false`: it is not saved to disk and is removed after
disconnection. NetworkManager manages the active connection and credentials;
the scanner keeps no history. A pending request disables repeat activation;
completion from an older scan cannot replace the current scan's feedback.

Errors stay on the relevant card while Copy remains available: missing URI or
telephone handler, unavailable NetworkManager, disabled Wi-Fi, no compatible
device, unavailable network, denied authorization, rejected authentication,
activation failure, or timeout. Provider errors are reduced to fixed messages
so they cannot echo credentials. Actual handler and Wi-Fi availability depends
on the desktop and system setup.

## Desktop verification checklist

KDE Wayland is the first target. Check tray visibility and each menu action,
window close/reopen, a second invocation, and Quit. Check background startup
with and without a host, and host removal/reappearance. D-Bus registration and
process smoke checks are separate from visual acceptance.

GNOME needs a StatusNotifierItem extension for a tray. Hyprland and Sway need a
compatible panel and portal backend. On each desktop check normal launcher
startup and closing without a tray, tray menu and host changes, command forwarding,
screenshot permission/cancellation and camera release. These desktops have not
been manually verified. Physical portal/crop interaction, clipboard, telephone/URL
handoff, Wi-Fi connection and camera QR acceptance still need manual verification;
native camera open/release was probed on the development machine. Release
measurements and the explicit acceptance matrix are in
[docs/verification.md](docs/verification.md).

## Camera scanning

Scan camera opens a device selector and live preview. Stop camera, switching to
screen scanning, closing the window, and Quit release capture resources. A new
camera selection waits for the previous worker to release its device. Repeated
payloads appear once per scanning session; no frames or results are saved.

Native Linux capture is enabled in the normal build. It uses the safe Rust v4l
API with bounded waits and nokhwa-core pixel conversion. Nokhwa native capture
was evaluated successfully on the Integrated RGB Camera, but its Linux stream
has no exposed read timeout, preventing reliable stop if a camera stalls.

The app requests 640×480, accepts the driver's nearest mode up to 1080p, and
prefers MJPEG, then YUYV, RGB, or grayscale. It requests 15 camera frames per
second; drivers may clamp that request. Obsolete raw frames are discarded before
conversion, previews are limited to 15/s, and QR submissions to 5/s. Delivery
retains only the newest frame while completed QR results remain lossless.

Capture startup can wait up to one second; subsequent frame waits are bounded
at 500 ms so low-light delays do not immediately fail. A stalled stream reports
an error and releases the device; use the selector or Scan camera to reopen it.
Permission denial, busy/unplugged devices, and unsupported formats surface as
camera errors. Direct device access requires permission to open `/dev/video*`;
Flatpak Camera portal/PipeWire integration is future work.

## Rendering and capture follow-up

The default build uses tiny-skia. For an optional GPU comparison build, use
`cargo build --release --features gpu`; select `ICED_BACKEND=wgpu` or
`ICED_BACKEND=tiny-skia`. See the measurements before choosing a backend.

Scan screen uses noninteractive portal capture and immediately decodes the full
screenshot. The portal may ask for screenshot permission on first use; after
approval, KDE can remember consent without showing its capture/options dialog
on each scan. Permissions remain controlled by the desktop, so other portal
backends may behave differently. Select region narrows an already captured image.

Desktop capture… is an explicit fallback that opens the desktop's interactive
capture UI (including native area selection when advertised). Denial or cancellation
never silently triggers a second capture request. The app does not alter portal
permission settings.

For app-specific permission identification, use the installed desktop launcher.
A development binary launched directly from a terminal can appear as a generic
application on older portal versions; do not assume its consent is app-specific.
The first permission dialog remains controlled by the desktop.

### Tray scans and desktop QR selection

Primary activation of the tray icon opens its menu. Choose Scan screen, Scan
region, Scan camera, Last scan, Open window or Quit. Scan camera opens its preview;
Open window explicitly opens the ordinary scanner. Screen and region requests
hide the normal window before capture and do not open it to show results while
a tray host is available. If the host disappears, the accessible-window fallback
keeps the application reachable.

Scan screen requests one quick portal screenshot and decodes locally. A lone QR
code is copied immediately, with notification feedback and actions in Last scan.
Only multiple QR locations show an undecorated fullscreen Iced overlay of the
frozen image with dimming and numbered QR outlines. This spatial flow requires one unchanged supported display and
capture dimensions matching its reported physical or logical dimensions within
one pixel. Multi-output, unavailable/mismatched geometry and changed layouts use
notifications and Last scan instead of guessing desktop positions. An output
change during selection closes the overlay safely. KDE Wayland with one
2560×1600 display at 140% is the first target; other desktops remain unverified.

Click a highlight to open its selectable text and Copy/action panel. Duplicate
payloads have separate highlights but one retained Last scan entry. Hover
emphasizes a region. Tab/Shift+Tab cycle detections and panel controls; Enter
selects the focused detection or activates its focused control. A panel that
covers a keyboard-focused detection is temporarily hidden to expose that target.
Close moves to a corner clear of the visible action panel. In a crowded layout
it can hide to uncover a focused detection or panel controls, then returns when
Tab reaches Close.
Clicking background clears selection and consumes the click. Escape clears the
selected panel first, then closes the overlay. The visible Close control is also
keyboard reachable. No codes or decode failure produces a concise notification
without an empty overlay; cancellation returns to the desktop. Repeated scan
requests are suppressed while capture/decode is pending. While a native capture
selector or permission request is outstanding, camera starts are also suppressed;
a notification asks you to finish or cancel the current capture before retrying.

Copy keeps the overlay open. Explicit Open URL, Call or Connect Wi-Fi closes it
before dispatch, while retaining pending-action suppression and feedback in
notifications/Last scan. Dismissing the overlay or notification retains results
and pending actions in memory. A new capture or source change rejects stale
callbacks. Scanning, hovering, selecting, revealing and copying never launch a
browser, dial or connect to a network.

On KDE, Scan region requires installed **Spectacle** and uses its background
native selector: drag on the desktop and release to capture, or Escape to cancel.
Each scan starts with no selected rectangle. Spectacle runs as an independent
instance with temporary selection settings, leaving your normal Spectacle
preferences and remembered region untouched.
The scanner suppresses Spectacle's result window and screenshot notification,
loads the image from a private temporary folder, and removes the folder after
loading, failure or cancellation. Selection times out after three minutes.
Spectacle does not supply a reliable desktop origin here, so region results use
a notification and Last scan rather than incorrectly positioned highlights.
A single region result can offer Copy and its validated action when the
notification server supports actions; multiple results use a summary.
On other desktops direct region selection requires Screenshot portal v3 with
Area support. If unavailable, the scanner reports that limitation in the app and
notification and points to Scan screen or explicit Desktop capture…. Missing
Spectacle gets separate installation and alternative-capture guidance. There is
no automatic second capture dialog.

Overlay and tray/notification copying needs `wl-copy` from **wl-clipboard** and
a compositor with Wayland data-control support. Full Copy and Ctrl+C/Cut of a
text selection both use stdin and a bounded wait, so clipboard ownership can
survive closing the temporary overlay. Missing/unsupported clipboard access
produces fixed failure feedback. Actual ownership after dismissal still needs
physical verification on the target compositor. The normal app window keeps
its existing Iced clipboard path.

Decoded text in the overlay and main window is read-only and supports mouse
selection and Ctrl+C for a subset. The Copy button always copies the complete
original payload. Wi-Fi passwords and malformed Wi-Fi raw data stay masked until
explicit reveal; hiding them destroys the displayed selectable buffer and its
selection. Notifications and tray labels never include those secret details.
Other QR text may appear in notifications, so consider desktop privacy when
scanning sensitive content. No screenshot history, QR payloads or camera images
are saved; Last scan is in-memory only.
