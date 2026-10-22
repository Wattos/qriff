# Install QRiff on Linux

Download a published version from [GitHub Releases](https://github.com/Wattos/qriff/releases).
You do not need Rust or a compiler to run a release bundle. Draft releases are
visible only to repository collaborators until published.

The bundles support **Linux x86_64**, targeting Ubuntu 22.04 / glibc 2.35 and newer.
KDE Plasma on Wayland is the primary tested desktop. Other compatible Linux
distributions may work; GNOME, Hyprland and Sway have not been manually validated.

## AppImage — recommended

Download `QRiff-1.0.0-linux-x86_64.AppImage` and `SHA256SUMS` from the same release.
Replace `1.0.0` below if you choose a different version.

```sh
sha256sum --check --ignore-missing SHA256SUMS
chmod +x QRiff-1.0.0-linux-x86_64.AppImage
./QRiff-1.0.0-linux-x86_64.AppImage
```

A normal launch opens the window. To start in the tray instead:

```sh
./QRiff-1.0.0-linux-x86_64.AppImage --background
```

If your system cannot mount AppImages with FUSE, use the extraction fallback:

```sh
./QRiff-1.0.0-linux-x86_64.AppImage --appimage-extract-and-run
```

## Archive — no FUSE needed

Download `QRiff-1.0.0-linux-x86_64.tar.gz` and `SHA256SUMS` from the same release.

```sh
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf QRiff-1.0.0-linux-x86_64.tar.gz
./QRiff-1.0.0-linux-x86_64/AppRun
```

Keep the extracted folder together. Use `AppRun` so QRiff finds its bundled
libraries and clipboard helper. Add `--background` to start in the tray.
The optional `dependency-sources.tar.gz` download contains dependency source
packages and is not needed to run QRiff.

## Desktop requirements

The bundles include the clipboard helper (`wl-copy`) and application client
libraries. Your host still provides the desktop services:

- A Wayland desktop session and user D-Bus session.
- `xdg-desktop-portal` and a backend that implements Screenshot.
- **Spectacle** for KDE's direct region selector.
- `xdg-utils` and configured handlers to open URLs and telephone links.
- **NetworkManager** and appropriate permissions to connect to Wi-Fi.
- A StatusNotifierItem/AppIndicator tray host for the tray experience. GNOME
  normally needs a tray extension; without a host QRiff uses its window.
- An accessible camera for camera scanning.

The bundles do not automatically install an application-menu launcher. See the
[source installation guide](https://github.com/Wattos/qriff/blob/master/docs/usage.md#build-and-run)
for launcher and icon installation instructions.

## First scan

Use **Scan screen** in the window to see the captured image and its decoded
results together. Use **Scan screen** from the tray for the windowless flow:
one code is copied with a notification, while multiple codes become selectable
highlights. **Scan region** lets you mark an area; camera scanning opens a window.
Opening links, making calls, and connecting to Wi-Fi require an explicit action.

## Build from source

See the [build and run guide](https://github.com/Wattos/qriff/blob/master/docs/usage.md#build-and-run)
for Rust, native build dependencies, and installation commands.
