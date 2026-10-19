# Desktop UI and QR result actions

## Approved intent

Improve the Rust/Iced scanner's appearance and organization to resemble a native KDE desktop utility. After scanning, offer relevant actions for web URLs, telephone numbers, and Wi-Fi networks. Provide a remembered option to copy single results automatically while leaving external actions explicit.

The user approved the interactive KDE/Breeze-style layout preview on 2026-10-19, and agreed that automatic copying applies when exactly one code is found; multiple-code scans require choosing what to copy.

This extends the existing Linux-first app. Preserve quick screenshot capture without the recurring desktop capture dialog, explicit interactive fallback, tray behavior, camera resource limits, generation filtering, local decoding, and no scan history. Iced and its lightweight software renderer remain the defaults. The UI approximates KDE styling; it does not become a Qt widget application.

## Layout and presentation

- Compact toolbar: primary Scan screen, Camera, a More menu containing Desktop capture, and Settings. Remove the prominent Quit button from the content area; keep Quit in the tray and More menu.
- Two-pane content at desktop sizes: capture preview and source controls on the left; result cards and their actions on the right. On narrow windows, stack source above results without clipping buttons or making result text unreadable.
- Before scanning, show an intentional empty state with a brief instruction, rather than an empty screenshot box. During capture/decoding, display clear progress. Distinguish cancellation, no QR found, capture failure, and action failure.
- Screenshot pane: preview, dimensions, Select region and Rescan. Retain Escape cancellation and correct scaling/letterboxing. Rescan decodes the retained image; Scan screen takes a new screenshot.
- Camera pane: live preview, device selector, and Stop. Stopping retains decoded results and releases the camera. A new source starts a fresh session.
- Result cards show a content-type label, readable payload details, Copy, and one relevant primary action. Long text wraps; all distinct results remain accessible in a scrollable region. Binary/unknown payloads retain readable raw/hex copy behavior without invented actions.
- Bottom status bar: current status and an Automatically copy single results checkbox, mirrored in Settings. Show concise copied/action feedback without replacing scan errors or discarding results.
- Use restrained Breeze-like borders, small corner radii, blue accent, clear typography, consistent spacing, icons with text, visible keyboard focus, and usable contrast. Follow system light/dark appearance through available desktop/toolkit hints; use consistent Breeze light/dark fallbacks. No webview or animated idle polling loop.

## Copy preferences

- Persist only preferences, not scanned payloads, in `$XDG_CONFIG_HOME/qr-scanner/settings.json`, falling back to `$HOME/.config/qr-scanner/settings.json`.
- Automatic copying defaults off on a fresh installation. Changing the checkbox persists the preference and keeps both controls synchronized. Settings failures are visible and do not prevent scanning. Missing/corrupt configuration uses defaults; malformed files are not silently overwritten until a user changes a setting. Use private directory/file permissions and atomic replacement.
- Copy the same QR content as manual Copy: original UTF-8 payload, or the existing labeled hex representation for binary payloads. For Wi-Fi this is the encoded QR content, not a reconstructed display label or only its password.
- Automatically copy only when accepting a successful current-session decode introduces exactly one result and the current session contains exactly one distinct payload. A batch containing multiple distinct codes does not auto-copy any of them.
- Camera duplicate frames never repeatedly overwrite the clipboard. Once another distinct result is present in a camera session, subsequent results require manual Copy. Starting another scan resets this session bookkeeping.
- Turning the setting on does not retroactively copy an already displayed result. Manual Copy remains available at all times. Copy feedback is session/result-aware so late messages cannot affect a newer scan.

## Result classification and actions

Preserve raw payload bytes separately from parsed action data. Classification runs locally. Malformed recognized formats remain copyable but show an explanation rather than an unsafe or misleading action.

### Web URLs

Preserve the existing validated HTTP/HTTPS policy: parsed host required, no credentials/control characters, no shell invocation. Display the destination and an Open URL button. Only a click invokes the desktop URI handler; failures appear on the relevant result.

### Phone numbers

Recognize explicit `tel:` QR payloads, not arbitrary numeric text. Display the number and a Call number button. Invoke the configured desktop `tel:` handler with a validated URI, allowing the user's calling application to review/place the call. The scanner does not dial automatically. If no handler is configured, show a useful error and retain Copy.

### Wi-Fi networks

Parse the common `WIFI:` QR format, including escaped delimiters and backslashes, SSID, authentication type, password, and hidden-network flag. Reject malformed or conflicting duplicate fields rather than guessing credentials. Display network name and security type; hide the password until Show is clicked. Copy still copies the complete raw payload.

Support connecting to open and personal WPA/WPA2 networks, and WPA3/SAE when supported by NetworkManager. WEP, enterprise/EAP, or other unsupported authentication remains readable/copyable with an explicit unsupported-connection explanation. Do not misclassify an unsupported network as open.

Connect to Wi-Fi explicitly requests a connection through Linux NetworkManager. Do not connect merely because a network code was scanned or auto-copied. NetworkManager manages its normal connection profile and credentials; the scanner does not save them as scan history. Do not enable automatic reconnection as a side effect of scanning. Preserve usable error feedback for missing NetworkManager, absent wireless device, authorization failure, unavailable network, and rejected credentials.

Pass secrets through the NetworkManager D-Bus API rather than command-line arguments, shell strings, logs, or persisted scanner settings. Connection work runs asynchronously and does not block the UI. Repeated clicks while a request is pending do not create competing connection attempts. A stale completion may finish the system operation already requested but must not overwrite a newer session's UI.

## Implementation boundaries

- Extract the growing view code into focused `app/view` and styling modules. Keep capture/camera worker behavior intact.
- Extend result classification in a pure module with typed URL, telephone, Wi-Fi, generic-text, and binary details. Keep platform execution separate from parsing and rendering.
- Add a settings module for loading/saving preferences and a clipboard policy boundary for deciding whether a newly accepted result should auto-copy.
- Add asynchronous platform action execution: existing desktop URI handoff for web/telephone, NetworkManager adapter for Wi-Fi. UI messages carry session/result identity and pending/completed/error state.
- Never execute network changes, launch calling software, or navigate web pages in automated tests or merely to verify the implementation. Tests exercise parsing, action requests, and simulated completion/error behavior; real actions require an intentional user click.

## Verification

- Parser fixtures: valid/invalid HTTP links, valid/invalid `tel:` URIs, open/WPA/SAE Wi-Fi, hidden SSID, escaped separators/backslashes, duplicate/missing fields, unsupported authentication, malformed UTF-8, and binary content.
- Clipboard behavior: preference off/on, one result, multiple distinct results, duplicates, later camera results, stale decode completions, setting toggle without retroactive copy, and explicit manual copy.
- Settings: defaults, valid load, corrupt file, unknown fields, private atomic writes, unavailable configuration path, and persistence error feedback without lost scan results.
- Action behavior: no action during decode/copy, explicit click only, pending-button state, duplicate-click suppression, stale completion filtering, URI-handler errors, and NetworkManager errors without leaked credentials.
- UI checks: light/dark screenshots, empty/single/multiple/phone/Wi-Fi/error states, narrow window, long payloads, keyboard focus, capture crop controls, camera Stop retaining results, and toolbar/tray actions.
- Run formatting, Clippy, Rust tests, applicable existing Python regression, and release build. Reuse resource measurements only as historical evidence; check for new idle activity or obvious memory regressions in the redesigned build.
- Report physical actions and desktop sessions not actually exercised as unverified. Do not claim exact native Qt rendering or universal Wi-Fi/telephone availability.

## References

- Approved visual mockup: `docs/design/ui-layout-preview.html` (design only; its buttons do not perform OS actions).
- QR content formats: https://github.com/zxing/zxing/wiki/Barcode-Contents
- NetworkManager API: https://networkmanager.dev/docs/api/latest/
- Existing behavior and validation: `docs/verification.md` and the original scanner design.
