# Tray scanning and interactive desktop QR selection

## Intent and authority

The user wants to scan and choose QR codes where they appear on the desktop,
without opening the main QR Scanner window. The approved interaction is:
tray menu → capture → highlight detected QR codes over the captured desktop →
select a code → copy or perform a relevant action. The background is a frozen
screenshot with a dimming shade, like a screenshot selection interface.

This spec supersedes the earlier click-to-scan tray proposal and the proposal
to deliver multiple screen results only through a notification summary. Keep
Rust, Iced, local decoding, tiny-skia by default, existing explicit QR actions,
and the camera preview window. No C++ or framework migration.

The existing uncommitted work in `feat/tray-scanning` is a starting point,
not accepted overlay implementation. Review and adapt its notifications,
clipboard and selectable-text changes against this spec before delivery.

## Interaction

### Tray

Primary activation opens the tray menu. It neither scans nor opens the main
window. Use the tray library's menu-on-activate behavior rather than simulating
a pointer click or opening an application popup.

Menu items: Scan screen, Scan region, Scan camera, Last scan, Open window, Quit.
Last scan contains one entry per distinct payload with Copy and its validated
action when available. Retain results in memory after dismissing the overlay
or a notification. Show pending and failed action feedback in the tray so the
user does not have to open the main window to understand an action failure.

Open window explicitly opens the normal scanner UI. Scan camera explicitly
opens its camera preview. If the tray host disappears, retain the existing
accessible-window fallback; never leave an unreachable background process.

### Screen scan

1. Close any previous overlay and temporarily hide the main window before capture.
2. Request one quick desktop screenshot through the existing portal. Permissions
   remain controlled by the desktop; do not add a recurring capture/options dialog.
3. Detect QR codes locally, retaining their screenshot-space quadrilateral bounds.
4. Display a temporary undecorated fullscreen selection surface over the captured
   display. Draw the captured image in its original visual alignment, a dim shade
   over the background, and clear, outlined hit regions over decoded codes.
5. Hovering a code emphasizes it. Clicking it selects that detection and opens a
   compact action panel beside it. Clamp the panel inside the display; it must
   remain usable for codes at edges. Avoid covering the selected code when space
   permits. The panel shows selectable decoded text, Copy and the applicable
   Open URL, Call or Connect Wi-Fi action. Plain/binary/invalid data remains copyable.
6. Selecting another highlight replaces the panel. Clicking background clears the
   selection without activating the application underneath. Escape dismisses the
   selection panel first, then the overlay; a visible Close control also dismisses
   the overlay. Close must remain reachable by keyboard.

Use this overlay for one detected code as well as multiple codes, so selecting
and acting behaves consistently. Number highlights for keyboard selection;
Tab/Shift+Tab cycle detections and panel controls, Enter selects the focused
highlight, and visible focus must be on screen before activation is allowed.

No detected codes or a decode failure produces a concise notification and no
empty overlay. A cancelled capture returns to the previous desktop without
opening the main app. A new tray scan must not create competing capture or
selection surfaces while one request is pending.

### Region scan

On KDE use Spectacle's native background region selector with capture on release:
drag a rectangle over the desktop; release to capture, Escape to cancel.
Suppress its editor/result window and screenshot notification. Use a private
short-lived image directory, remove it after loading, and impose a finite timeout.

After region capture, decode and present the selected result/actions without
opening the normal app. If showing region highlights on the desktop, retain
reliable original region coordinates; a cropped image alone cannot establish
its original desktop position. When the provider does not return an origin,
use the result notification and Last scan actions rather than guessing overlay
coordinates. This preserves direct native region selection without inaccurate
highlights. The screen-scan overlay remains the primary spatial selection flow.

On other desktops use native portal Area selection when advertised. If direct
selection is unavailable, report that and leave Scan screen and explicit desktop
capture available; do not silently open an unexpected alternative capture dialog.

### Copy, actions and notifications

Detection and highlighting never open a URL, dial or connect to a network.
Selecting a code opens its action panel. The saved auto-copy preference may copy
the selected payload once on explicit selection; screen scans do not automatically
copy before selection, even with one code. Repeated selection of the same payload
in that scan must not overwrite the clipboard again. Selecting a different code
may copy that newly selected payload once. Camera auto-copy rules remain unchanged.
Manual Copy always copies the complete original payload. Drag selection and Ctrl+C
copy only the selected visible text. The text is read-only.

Wi-Fi passwords and malformed encoded Wi-Fi details stay masked until explicit
reveal. Hiding details destroys their selectable buffer and selection. No secret
raw text goes into notifications, tray labels, logs, command arguments or settings.

Copy gives feedback without forcing the main window open. Keep the overlay
available to select more codes. Explicit Open/Call/Connect closes the overlay
before dispatch, so it does not obstruct the destination app or a permission prompt.
Report action completion/failure through a notification and Last scan. Keep pending
suppression and request/session guards across that dismissal. A new capture or
source change rejects old UI callbacks; closing the overlay alone retains results
and pending actions. Notification dismissal must not discard results.

Use the existing freedesktop notification interface with action support detection.
A single region result can offer Copy and its validated action; multiple region
results use one summary and Last scan. Notification strings must be escaped when
the server supports markup. Old notification buttons must never act on a newer scan.

Windowless Wayland clipboard writes use `wl-copy` via stdin with a bounded wait
and fixed redacted errors. Report missing wl-clipboard or unsupported clipboard
access honestly. Do not open a hidden window to make Iced clipboard writes work.
The normal window retains the existing Iced clipboard path.

## Implementation boundaries

- **Detection:** preserve the existing payload-only decode interface for callers
  that do not need positions; add a spatial result containing decoded payloads
  and all successful detection quadrilaterals linked to those payloads. Two QR
  codes with equal bytes have two clickable regions and one deduplicated payload.
  Keep rotation and image/crop transforms explicit. Malformed grids do not produce
  actionable hit regions; valid grids in the same image remain usable.
- **Overlay model:** owns capture identity, frozen image, transform, detections,
  hover/selection and action-panel state. Geometry and hit testing are pure.
  Rendering consumes this model; it does not invoke desktop actions directly.
- **Overlay surface:** a temporary undecorated Iced fullscreen surface with a
  distinct role from main/camera windows. It consumes pointer input, handles
  cancellation and tears down when the scan ends. It must not trigger ordinary
  main-window-close behavior or reset retained result/action identity.
- **Controller:** coordinates capture, bounded background decoding, role-specific
  window lifecycle, selected-result clipboard policy and guarded action dispatch.
- **Desktop integration:** menu-first tray, in-memory Last scan model and
  notification service. Operates from events/signals, with no idle polling.

### Display correctness and portability

First acceptance target is the user's KDE Wayland session with one connected
2560×1600 display at 140% logical scaling. Use the existing Iced fullscreen path
for this target; do not introduce a layer-shell dependency merely to create a
single-display screenshot selection surface. A fullscreen surface is an overlay
in the interaction sense; the main app window remains closed.

Image-to-surface scaling, hit testing and action-panel placement share exactly
one transform. Never stretch a multi-output screenshot onto one display and
claim the highlights are aligned with the desktop. Detect unsupported or changed
output layouts and report the limitation with notifications/Last scan as a usable
fallback. Support for multi-output placement must require reliable per-output
capture bounds, logical geometry, scale and rotation; do not infer them from a
single combined screenshot's dimensions. Output changes during selection cancel
the overlay safely. Extend the surface abstraction for per-output surfaces when
that metadata is available, without changing payload/action interfaces.

GNOME, Hyprland and Sway remain intended targets, not verified claims. Their
capture permission, tray host and fullscreen behavior require acceptance checks.
No automatic GNOME Shell extension installation or compositor configuration.

## Performance and privacy

Decode off the UI thread. Reuse the captured image; draw a flat translucent shade
rather than running a blur shader or creating another blurred full-resolution
buffer. Cache geometry and text state; do not rebuild payload editors on pointer
motion or unchanged camera frames. Overlay resources and screenshot buffers must
be released on cancellation/dismissal, except payloads retained for Last scan.

No continuous screen capture, additional camera use, network decoding, screenshot
history or saved QR payloads. Private region files are removed after loading or
failure. Keep settings limited to the existing preference.

## Verification and acceptance

- Fixtures with one, several, rotated and duplicate-payload QR codes verify payload
  identity, retained bounds and independent clickable detections.
- Geometry tests cover fractional scale (including 1.4), reversed crop selection,
  image edges, panel clamping and unsupported/changed display layouts.
- Controller tests verify tray primary activation opens a menu; screen/region
  capture never opens the normal result window; camera does; overlay dismissal
  retains Last scan and pending actions; source changes reject stale callbacks.
- Test click/keyboard selection, no side effects on detection, explicit action
  dispatch, selection auto-copy deduplication and manual full/partial copying.
- Verify read-only selectable text and revealed/hidden valid and invalid Wi-Fi
  content; hidden buffers and stale selection messages cannot expose credentials.
- Private D-Bus and fake executable tests verify notification wiring, markup,
  pending/stale callbacks, command arguments, stdin payloads and file cleanup
  without altering the real clipboard, browser, telephone or Wi-Fi connection.
- Run formatting, Clippy, full Rust/Python suites and release build. Review the
  complete change against this spec, including the inherited uncommitted work.
- Capture actual native overlay screenshots with safe synthetic data. Confirm
  highlight placement, hover, panel layout and dismiss behavior on KDE at 140%.
  Distinguish automated/synthetic evidence from physical mouse/keyboard acceptance.
- Report real external actions, physical region drag, clipboard ownership and
  untested desktop/layout combinations as manual gaps unless actually exercised.

## Outside this change

Live desktop annotation or click-through, blur effects, a screenshot editor,
a Qt rewrite, automatic external QR actions, guaranteed multi-output alignment
without metadata, and installation of desktop extensions are outside this change.
