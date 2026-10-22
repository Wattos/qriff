//! Keyboard navigation for controls Iced 0.14 does not yet make focusable.
use super::{App, Message};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Scan,
    Camera,
    More,
    Settings,
    Desktop,
    Quit,
    AutoCopy,
    SettingsAutoCopy,
    Copy(usize),
    Activate(usize),
    Reveal(usize),
    Region,
    Rescan,
    Stop,
}
impl Target {
    pub fn message(self, auto_copy: bool) -> Message {
        match self {
            Self::Scan => Message::UserCommand(crate::cli::Command::ScanScreen),
            Self::Camera => Message::UserCommand(crate::cli::Command::ScanCamera),
            Self::More => Message::ToggleMore,
            Self::Settings => Message::ToggleSettings,
            Self::Desktop => Message::DesktopCapture,
            Self::Quit => Message::UserCommand(crate::cli::Command::Quit),
            Self::AutoCopy | Self::SettingsAutoCopy => Message::AutoCopyChanged(!auto_copy),
            Self::Copy(i) => Message::CopyResult(i),
            Self::Activate(i) => Message::ActivateResult(i),
            Self::Reveal(i) => Message::RevealPassword(i),
            Self::Region => Message::SelectRegion,
            Self::Rescan => Message::ScanFull,
            Self::Stop => Message::StopCamera,
        }
    }
}
pub(super) fn targets(app: &App) -> Vec<Target> {
    let mut targets = vec![Target::Scan, Target::Camera, Target::More, Target::Settings];
    if app.more_open {
        targets.extend([Target::Desktop, Target::Quit]);
    }
    if app.settings_open {
        targets.push(Target::SettingsAutoCopy);
    }
    if app.camera_mode {
        targets.push(Target::Stop);
    } else if app.screenshot.is_some() {
        targets.extend([Target::Region, Target::Rescan]);
    }
    for (i, result) in app.state.results.iter().enumerate() {
        if matches!(&result.content, crate::results::QrContent::Wifi(n) if n.password.is_some())
            || matches!(
                result.content,
                crate::results::QrContent::Invalid {
                    kind: crate::results::ContentKind::Wifi,
                    ..
                }
            )
        {
            targets.push(Target::Reveal(i));
        }
        if result.action().is_some()
            && !app.result_actions.get(&i).is_some_and(|s| {
                s.key.session.0 == app.state.session
                    && matches!(s.status, crate::actions::ActionStatus::Pending)
            })
        {
            targets.push(Target::Activate(i));
        }
        targets.push(Target::Copy(i));
    }
    targets.push(Target::AutoCopy);
    targets
}
pub(super) fn next(app: &App, reverse: bool) -> Target {
    let targets = targets(app);
    let current = app.focus.and_then(|t| targets.iter().position(|x| *x == t));
    let index = match current {
        Some(i) if reverse => (i + targets.len() - 1) % targets.len(),
        Some(i) => (i + 1) % targets.len(),
        None if reverse => targets.len() - 1,
        None => 0,
    };
    targets[index]
}

pub(super) fn key_message(
    key: &iced::keyboard::Key,
    shift: bool,
    status: iced::event::Status,
) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};
    if matches!(key, Key::Named(Named::Escape)) {
        return Some(Message::CancelCrop);
    }
    if status == iced::event::Status::Captured {
        return None;
    }
    match key {
        Key::Named(Named::Tab) => Some(Message::FocusNext(shift)),
        Key::Named(Named::Enter | Named::Space) => Some(Message::KeyboardActivate),
        Key::Character(s) if s == " " => Some(Message::KeyboardActivate),
        _ => None,
    }
}
