# QR Scanner

A Linux-first local QR scanner built with Rust and Iced. The current shell has a
main window, a StatusNotifierItem tray menu, and single-instance command routing.
Scan a portal screenshot, crop a region, or scan continuously from a local camera.
Results stay in memory and offer explicit Copy and safe HTTP/HTTPS Open actions.

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
host while hidden opens a window. Show window and Quit are in the tray menu.
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

## Desktop verification checklist

KDE Wayland is the first target. Check tray visibility and each menu action,
window close/reopen, a second invocation, and Quit. Check background startup
with and without a host, and host removal/reappearance. D-Bus registration and
process smoke checks are separate from visual acceptance.

GNOME needs a StatusNotifierItem extension for a tray. Hyprland and Sway need a
compatible panel and portal backend. On each desktop check normal launcher
startup and closing without a tray, tray menu and host changes, command forwarding,
screenshot permission/cancellation and camera release. These desktops have not
been manually verified. Physical portal/crop interaction, clipboard and camera QR
acceptance still need manual verification; native camera read/stop/reopen was
probed on the development machine. Release measurements and the explicit acceptance matrix are in [docs/verification.md](docs/verification.md).

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

A future capture flow should try noninteractive portal capture and show region
selection only when needed. KDE's current screenshot backend has a
noninteractive branch for `interactive=false`, but portal frontend permissions
can still prompt. This does not establish universally dialog-free capture.
The current app retains the verified interactive portal flow; improving its
visual integration and main-window polish remains follow-up work.
