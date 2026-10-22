//! A toolbar button backed by Iced's native floating menu.
use super::{Message, focus::Target};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::widget::overlay::menu;
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

#[derive(Clone, Copy)]
enum Choice {
    Desktop,
    Quit,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Desktop => "Desktop capture…",
            Self::Quit => "Quit",
        })
    }
}
const CHOICES: [Choice; 2] = [Choice::Desktop, Choice::Quit];
#[derive(Default)]
struct State {
    menu: menu::State,
    hovered: Option<usize>,
}
pub(super) fn view<'a>(
    trigger: Element<'a, Message>,
    open: bool,
    focus: Option<Target>,
) -> Element<'a, Message> {
    Element::new(MoreMenu {
        trigger,
        open,
        focus,
        class: Box::new(|theme| {
            let mut style = menu::default(theme);
            style.border.radius = 5.0.into();
            style.border.color = super::style::border(theme);
            style.shadow = iced::Shadow {
                color: iced::Color::BLACK.scale_alpha(0.25),
                offset: Vector::new(0., 3.),
                blur_radius: 12.,
            };
            style
        }),
    })
}
struct MoreMenu<'a> {
    trigger: Element<'a, Message>,
    open: bool,
    focus: Option<Target>,
    class: <Theme as menu::Catalog>::Class<'a>,
}
impl Widget<Message, Theme, iced::Renderer> for MoreMenu<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.trigger.as_widget())]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.trigger));
    }
    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.trigger
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, iced::Renderer>> {
        if !self.open {
            return None;
        }
        let state = tree.state.downcast_mut::<State>();
        state.hovered = match self.focus {
            Some(Target::Desktop) => Some(0),
            Some(Target::Quit) => Some(1),
            _ => None,
        };
        fn hovered(choice: Choice) -> Message {
            Message::MoreHovered(match choice {
                Choice::Desktop => 0,
                Choice::Quit => 1,
            })
        }
        let width = 196.0_f32.min((viewport.width - 16.).max(1.));
        let position = iced::Point::new(
            (layout.bounds().x + translation.x)
                .min(viewport.x + viewport.width - width - 8.)
                .max(viewport.x + 8.),
            layout.bounds().y + translation.y + 4.,
        );
        let native = menu::Menu::new(
            &mut state.menu,
            &CHOICES,
            &mut state.hovered,
            |choice| match choice {
                Choice::Desktop => Message::DesktopCapture,
                Choice::Quit => Message::UserCommand(crate::cli::Command::Quit),
            },
            Some(&hovered),
            &self.class,
        )
        .width(width)
        .padding([9, 14])
        .text_size(13)
        .overlay(position, *viewport, layout.bounds().height, Length::Shrink);
        Some(overlay::Element::new(Box::new(Dismissible {
            inner: native,
        })))
    }
}
struct Dismissible<'a> {
    inner: overlay::Element<'a, Message, Theme, iced::Renderer>,
}
impl overlay::Overlay<Message, Theme, iced::Renderer> for Dismissible<'_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.inner
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }
    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if matches!(event, Event::Keyboard(_)) {
            // The app's window-key subscription handles menu navigation. Keep
            // the focused result editor underneath from seeing these keys.
            shell.capture_event();
            return;
        }
        let outside = match event {
            Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => !cursor.is_over(layout.bounds()),
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                !layout.bounds().contains(*position)
            }
            _ => false,
        };
        if outside {
            shell.publish(Message::DismissMore);
            shell.capture_event();
            return;
        }
        self.inner
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, shell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;

    #[tokio::test]
    async fn popup_preserves_button_layout_and_dispatches_only_menu_actions() {
        let renderer =
            <iced::Renderer as Headless>::new(iced::Font::DEFAULT, iced::Pixels(16.), None)
                .await
                .unwrap();
        let viewport = Rectangle::new(iced::Point::ORIGIN, Size::new(640., 360.));
        let limits = layout::Limits::new(Size::ZERO, viewport.size());
        let trigger = || {
            iced::widget::button("More")
                .width(80)
                .height(36)
                .on_press(Message::ToggleMore)
                .into()
        };
        let mut closed = view(trigger(), false, None);
        let mut closed_tree = Tree::new(closed.as_widget());
        let closed_node = closed
            .as_widget_mut()
            .layout(&mut closed_tree, &renderer, &limits);
        assert!(
            closed
                .as_widget_mut()
                .overlay(
                    &mut closed_tree,
                    Layout::new(&closed_node),
                    &renderer,
                    &viewport,
                    Vector::ZERO
                )
                .is_none()
        );
        let mut open = view(trigger(), true, None);
        let mut tree = Tree::new(open.as_widget());
        let button = open
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .move_to(iced::Point::new(580., 20.));
        assert_eq!(
            button.size(),
            closed_node.size(),
            "opening More must not shift the toolbar"
        );
        let mut popup = open
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&button),
                &renderer,
                &viewport,
                Vector::ZERO,
            )
            .unwrap();
        let node = popup.as_overlay_mut().layout(&renderer, viewport.size());
        let layout = Layout::new(&node);
        assert!(layout.bounds().x + layout.bounds().width <= viewport.width - 8.);
        assert_eq!(layout.bounds().y, 60.);
        let mut messages = Vec::new();
        let outside = mouse::Cursor::Available(iced::Point::new(10., 300.));
        {
            let mut shell = Shell::new(&mut messages);
            popup.as_overlay_mut().update(
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                layout,
                outside,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert_eq!(shell.event_status(), iced::event::Status::Captured);
        }
        assert!(matches!(messages.as_slice(), [Message::DismissMore]));
        messages.clear();
        let point = layout.bounds().position() + Vector::new(20., 15.);
        let cursor = mouse::Cursor::Available(point);
        let mut shell = Shell::new(&mut messages);
        popup.as_overlay_mut().update(
            &Event::Mouse(mouse::Event::CursorMoved { position: point }),
            layout,
            cursor,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
        );
        popup.as_overlay_mut().update(
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            layout,
            cursor,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
        );
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::DesktopCapture))
        );
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::UserCommand(crate::cli::Command::Quit)))
        );
    }
}
