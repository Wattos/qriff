# QR Scanner

A Linux-first local QR scanner built with Rust and Iced. The current shell has a
main window, a StatusNotifierItem tray menu, and single-instance command routing.
Screen and camera actions currently display an implementation-pending message.

## Build and run

Use stable Rust and Cargo. Development was checked on KDE neon 24.04 / Plasma
Wayland. Build prerequisites include a C linker, pkg-config, Wayland/X11 client
libraries and xkbcommon development files (Debian/Ubuntu: build-essential,
pkg-config, libwayland-dev, libxkbcommon-dev, libx11-dev, libxcursor-dev,
libxi-dev, libxrandr-dev). Check installed prerequisites before installing packages.
The current renderer is Iced's software tiny-skia backend; no GPU backend is
compiled by default. Capture and camera dependencies will add prerequisites later.

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

The files in `assets/` provide a desktop entry and SVG icon for eventual native
packaging. They are not installed automatically. For a local desktop entry,
replace its Exec with the absolute path to the built binary and its Icon with
the absolute path to `assets/qr-scanner.svg` before installing it.

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
and later screenshot permission/cancellation and camera release. These desktops
have not been manually verified. Capture, crop, decoding, camera, and benchmarks
remain subsequent implementation work.
