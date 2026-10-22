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
                s.key.session == app.result_session()
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

pub(super) const RESULTS_SCROLL: &str = "focus-results-scroll";
pub(super) const SOURCE_SCROLL: &str = "focus-source-scroll";
pub(super) const OUTER_SCROLL: &str = "focus-outer-scroll";
impl Target {
    fn anchor(self) -> String {
        format!("keyboard-target-{self:?}")
    }
    fn scroller(self, wide: bool) -> Option<&'static str> {
        match self {
            Self::Copy(_) | Self::Activate(_) | Self::Reveal(_) => {
                Some(if wide { RESULTS_SCROLL } else { OUTER_SCROLL })
            }
            Self::Region | Self::Rescan | Self::Stop => {
                Some(if wide { SOURCE_SCROLL } else { OUTER_SCROLL })
            }
            _ => None,
        }
    }
}
pub(super) fn anchor<'a>(
    target: Target,
    element: impl Into<iced::Element<'a, Message>>,
) -> iced::Element<'a, Message> {
    iced::widget::container(element).id(target.anchor()).into()
}
pub(super) fn reveal(target: Target, wide: bool) -> iced::Task<bool> {
    match target.scroller(wide) {
        Some(scroller) => iced::advanced::widget::operate(Locate::new(target, scroller)),
        None => iced::Task::done(true),
    }
}

use iced::{
    Rectangle, Vector,
    advanced::widget::{
        Id, Operation,
        operation::{Outcome, Scrollable, scrollable::AbsoluteOffset},
    },
};
#[derive(Clone, Copy)]
struct Viewport {
    bounds: Rectangle,
    content: Rectangle,
    translation: Vector,
}
struct Locate {
    target: Id,
    scroller: Id,
    control: Option<Rectangle>,
    viewport: Option<Viewport>,
}
impl Locate {
    fn new(target: Target, scroller: &str) -> Self {
        Self {
            target: target.anchor().into(),
            scroller: scroller.to_owned().into(),
            control: None,
            viewport: None,
        }
    }
}
impl Operation<bool> for Locate {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<bool>)) {
        operate(self)
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.target) {
            self.control = Some(bounds)
        }
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        if id == Some(&self.scroller) {
            self.viewport = Some(Viewport {
                bounds,
                content,
                translation,
            })
        }
    }
    fn finish(&self) -> Outcome<bool> {
        let (Some(control), Some(viewport)) = (self.control, self.viewport) else {
            return Outcome::Some(false);
        };
        let visual_top = control.y - viewport.translation.y;
        let visual_bottom = visual_top + control.height;
        let top = viewport.bounds.y + 6.0;
        let bottom = viewport.bounds.y + viewport.bounds.height - 6.0;
        let offset = if visual_top < top {
            control.y - top
        } else if visual_bottom > bottom {
            control.y + control.height - bottom
        } else {
            viewport.translation.y
        };
        let offset = offset.clamp(
            0.0,
            (viewport.content.height - viewport.bounds.height).max(0.0),
        );
        let visible = control.y - offset >= viewport.bounds.y
            && control.y + control.height - offset <= viewport.bounds.y + viewport.bounds.height;
        Outcome::Chain(Box::new(Position {
            scroller: self.scroller.clone(),
            offset,
            visible,
            applied: false,
        }))
    }
}
struct Position {
    scroller: Id,
    offset: f32,
    visible: bool,
    applied: bool,
}
impl Operation<bool> for Position {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<bool>)) {
        operate(self)
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _: Rectangle,
        _: Rectangle,
        _: Vector,
        state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.scroller) {
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some(self.offset),
            });
            self.applied = true;
        }
    }
    fn finish(&self) -> Outcome<bool> {
        Outcome::Some(self.applied && self.visible)
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    #[derive(Default)]
    struct ScrollState {
        y: f32,
    }
    impl Scrollable for ScrollState {
        fn snap_to(
            &mut self,
            _: iced::advanced::widget::operation::scrollable::RelativeOffset<Option<f32>>,
        ) {
        }
        fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
            if let Some(y) = offset.y {
                self.y = y;
            }
        }
        fn scroll_by(&mut self, _: AbsoluteOffset, _: Rectangle, _: Rectangle) {}
    }
    #[test]
    fn exact_control_bounds_scroll_past_variable_height_cards_in_both_layouts() {
        // Card heights deliberately differ; the focus target is the final card's action,
        // below a 1600px text payload. No result-index-to-height approximation is possible.
        for wide in [true, false] {
            let target = Target::Activate(7);
            let scroller = target.scroller(wide).unwrap();
            assert_eq!(scroller, if wide { RESULTS_SCROLL } else { OUTER_SCROLL });
            let mut locate = Locate::new(target, scroller);
            let viewport = Rectangle {
                x: 20.,
                y: 110.,
                width: 500.,
                height: 320.,
            };
            let content = Rectangle {
                height: 3100.,
                ..viewport
            };
            let control = Rectangle {
                x: 40.,
                y: 2940.,
                width: 150.,
                height: 36.,
            };
            let mut state = ScrollState::default();
            locate.scrollable(
                Some(&Id::from(scroller.to_owned())),
                viewport,
                content,
                Vector::ZERO,
                &mut state,
            );
            locate.container(Some(&Id::from(target.anchor())), control);
            let Outcome::Chain(mut position) = locate.finish() else {
                panic!("must position exact target")
            };
            assert!(
                matches!(position.finish(), Outcome::Some(false)),
                "not ready before applying scroll"
            );
            position.scrollable(
                Some(&Id::from(scroller.to_owned())),
                viewport,
                content,
                Vector::ZERO,
                &mut state,
            );
            assert!(state.y > 2500.);
            assert!(control.y - state.y >= viewport.y);
            assert!(control.y + control.height - state.y <= viewport.y + viewport.height);
            assert!(matches!(position.finish(), Outcome::Some(true)));
            // Reverse traversal back to the first card must scroll upward as well.
            let first = Rectangle { y: 160., ..control };
            let mut locate = Locate::new(Target::Copy(0), scroller);
            locate.scrollable(
                Some(&Id::from(scroller.to_owned())),
                viewport,
                content,
                Vector::new(0., state.y),
                &mut state,
            );
            locate.container(Some(&Id::from(Target::Copy(0).anchor())), first);
            let Outcome::Chain(mut position) = locate.finish() else {
                panic!("must reposition")
            };
            position.scrollable(
                Some(&Id::from(scroller.to_owned())),
                viewport,
                content,
                Vector::ZERO,
                &mut state,
            );
            assert!(first.y - state.y >= viewport.y);
            assert!(first.y + first.height - state.y <= viewport.y + viewport.height);
        }
    }
}
