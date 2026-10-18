# Local QR scanner desktop app

## Intent and agreed constraints

Build a fast, lightweight desktop utility that reads QR codes from a selected screen area, a screenshot, or a device camera. The user selected Rust and Iced. KDE Plasma on Wayland is the first target, with GNOME, Hyprland, and Sway compatibility. Windows and macOS are future targets. Application source should not require handwritten C++.

The repository is initially empty. Rust tools are installed; the current session reports KDE and Wayland.

## Proposed first-version behavior

- A launcher entry opens a small main window with Scan screen and Scan camera actions. An explicit background-start option starts only the tray service.
- The tray menu offers Scan screen, Scan camera, Show window, and Quit. Closing windows leaves the app running when a tray host is available. Without a tray host, closing the last window exits; the app never becomes inaccessible by accident.
- Screen scanning hides app windows before requesting an interactive screenshot. Decode every QR code in the returned image locally. Offer region selection using the desktop capture UI where supported; otherwise allow drawing a rectangle over the captured image inside the application. An explicit full-image scan remains available.
- Camera scanning opens a preview and device selector, with stop and close controls. Scan continuously while that window is open, suppress repeated identical results within the session, and release the camera on stop or close.
- Results list decoded text with Copy actions. Open is offered only for validated HTTP/HTTPS URLs and requires a click. Empty results, cancellation, permission denial, unavailable portals, and camera failures have distinct UI states.
- No scan history, telemetry, network decoding, autostart, or automatic link opening. Global shortcuts are deferred; desktop shortcuts may invoke command-line scan actions.

These details are proposed defaults, not additional user requirements.

## Architecture

Use one Cargo application with focused modules rather than multiple services or crates initially:

- `app`: Iced daemon, window lifecycle, messages, and presentation state.
- `decode`: image-to-results logic independent of Iced and capture sources. Start with the Rust `rqrr` decoder; verify performance and representative images before retaining that choice.
- `capture`: a screenshot source interface with a Linux portal implementation using `ashpd`. Keep portal-specific types out of UI and decoder interfaces.
- `camera`: device enumeration, capture worker, frame normalization, and shutdown. Initially evaluate `nokhwa` native Linux capture. It must be verified on the actual camera before claiming support. Direct camera access is the initial native-package approach; sandboxed Camera portal/PipeWire support is separate future work.
- `tray`: Linux StatusNotifierItem integration using `ksni`, delivering commands into the application event stream.
- `results`: session results, deduplication, text representation, and URL validation.

Use the stable Iced release and its daemon lifecycle. UI and application logic are Rust. System libraries or dependency implementation languages are not a guarantee of an entirely Rust dependency tree.

## Data flow and resource use

Screen action -> portal request -> returned local image -> bounded decode worker -> result message -> results window.

Camera action -> capture worker -> latest frame -> bounded preview and decode paths -> result messages. Limit decode to approximately five frames per second initially and preview to at most fifteen, then tune using measurements. Drop obsolete frames rather than building a queue. Only one decode operation runs at a time; manual screen requests cancel or supersede stale work where practical. Tag jobs so late results cannot replace the current session.

Never perform image decoding, camera reads, or portal waits on the UI thread. Use event-driven delivery without a recurring tray polling timer. No camera activity or screen polling while idle. Destroy closed windows and release screenshots, textures, and camera buffers when no longer needed. Evaluate Iced's software renderer before choosing a default; retain a GPU option if preview performance requires it.

There are no promised memory or latency numbers before benchmarking. Record release-build startup time, idle CPU, proportional/resident memory, full-image decode latency, and camera CPU/memory on the development machine. Include child processes if any. Report hardware, renderer, image dimensions, and measurement method.

## Wayland and desktop behavior

The Screenshot portal is the primary capture route. Detect the installed interface version and supported targets rather than assuming newer area-target options exist. On older backends, request interactive capture without unsupported target options. Native selection and permission prompts belong to the portal backend and may differ by desktop.

The fallback crop UI operates on the screenshot inside an ordinary app window. It does not promise a desktop-spanning overlay, forced global positioning, or a seamless Spectacle replacement. Preserve source pixel coordinates when mapping scaled preview selections; normalize drag direction, clamp bounds, and account for display scaling.

KDE is the first manually tested environment. GNOME tray visibility may require an AppIndicator extension. Hyprland and Sway need a configured portal backend and a StatusNotifierItem-capable panel. Missing tray support must not prevent scanning from the launcher or command line. Do not mark a desktop supported solely because its portal follows the same interface.

## Packaging and portability

Deliver a native Linux development build, README with distro prerequisites, desktop entry, and icon first. Initialize Git and ignore build artifacts. Avoid installing system packages without checking what is present. Flatpak and other distribution formats follow after native capture/camera verification.

Keep capture and tray behind platform-specific modules so Windows/macOS implementations can be added. The first release must describe itself as Linux-first; cross-platform compilation and functionality are not yet acceptance criteria.

## Validation and acceptance

- Decoder fixtures: one QR, multiple QRs, rotated QR, blank image, and malformed input. Preserve arbitrary payload bytes internally and provide a safe textual representation when UTF-8 is invalid.
- Verify crop mapping on scaled images, reverse drags, and boundary selections.
- Verify cancellation, stale worker messages, and bounded frame delivery without real hardware where possible.
- Run formatting, compilation, linting, and relevant tests in the implementation phase.
- Manually verify KDE portal capture, region/full-image workflows, multiple results, clipboard, camera start/stop, tray actions, window reopening, and clean exit.
- Validate no-tray behavior, cancellation and missing-device errors. Provide a manual verification checklist for GNOME, Hyprland, and Sway; report untested environments explicitly.
- Benchmark a release build and report results without unsupported cross-framework performance claims.

## References

- Iced daemon: https://docs.rs/iced/latest/iced/fn.daemon.html
- Iced project and rendering backends: https://github.com/iced-rs/iced
- Screenshot portal: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html
- Rust portal API: https://docs.rs/ashpd/latest/ashpd/desktop/screenshot/index.html
- Linux tray: https://docs.rs/ksni/latest/ksni/
- Camera candidate: https://github.com/l1npengtul/nokhwa
