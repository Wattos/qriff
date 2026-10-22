use super::*;
use crate::{app::style, results::ResultAction};
use iced::{
    Color, Element, Length, Point, Rectangle, Theme, keyboard, mouse,
    widget::{
        button,
        canvas::{self, Frame, Geometry, Path, Stroke},
        column, container, opaque, pin, row, scrollable, stack, text, text_editor,
    },
};

struct Screen<'a> {
    model: &'a Model,
    transform: Option<geometry::Transform>,
}
struct Frozen<'a> {
    model: &'a Model,
    transform: Option<geometry::Transform>,
}
impl canvas::Program<Event> for Frozen<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::BLACK);
        if let Some(transform) = self.transform {
            let origin = transform.to_surface([0., 0.]);
            let end = transform.to_surface([
                self.model.frozen.image.width() as f32,
                self.model.frozen.image.height() as f32,
            ]);
            frame.draw_image(
                Rectangle::new(origin, Size::new(end.x - origin.x, end.y - origin.y)),
                &self.model.frozen.handle,
            );
        }
        vec![frame.into_geometry()]
    }
}
impl canvas::Program<Event> for Screen<'_> {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Event>> {
        let publish = |e| Some(iced::widget::Action::publish(e).and_capture());
        match event {
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hit = cursor.position_in(bounds).and_then(|p| {
                    self.transform
                        .and_then(|t| geometry::hit_test(&self.model.scan.detections, &t, p))
                });
                if hit != self.model.hovered {
                    publish(Event::Hover(hit))
                } else {
                    None
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => publish(Event::Hover(None)),
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                let hit = cursor.position_in(bounds).and_then(|p| {
                    self.transform
                        .and_then(|t| geometry::hit_test(&self.model.scan.detections, &t, p))
                });
                publish(hit.map_or(Event::Background, Event::Select))
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                match key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::Escape) => publish(Event::Escape),
                    keyboard::Key::Named(keyboard::key::Named::Tab) => {
                        publish(Event::Next(modifiers.shift()))
                    }
                    keyboard::Key::Named(keyboard::key::Named::Enter) => {
                        publish(Event::ActivateFocused)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
    fn mouse_interaction(
        &self,
        _: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor
            .position_in(bounds)
            .and_then(|p| {
                self.transform
                    .and_then(|t| geometry::hit_test(&self.model.scan.detections, &t, p))
            })
            .is_some()
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let Some(transform) = self.transform else {
            return vec![frame.into_geometry()];
        };
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Color::from_rgba(0., 0., 0., 0.48),
        );
        for (index, detection) in self.model.scan.detections.iter().enumerate() {
            if detection.result_index >= self.model.scan.results.len() {
                continue;
            }
            let corners = detection.corners.map(|p| transform.to_surface(p));
            let path = Path::new(|b| {
                b.move_to(corners[0]);
                for p in &corners[1..] {
                    b.line_to(*p);
                }
                b.close();
            });
            let selected = self.model.selected == Some(index);
            let hovered = self.model.hovered == Some(index);
            let focused = self.model.focus == Some(Focus::Detection(index));
            let color = if focused {
                Color::from_rgb(1., 0.81, 0.25)
            } else if selected {
                theme.palette().primary
            } else if hovered {
                Color::WHITE
            } else {
                Color::from_rgb(0.45, 0.85, 1.)
            };
            frame.fill(
                &path,
                Color {
                    a: if selected {
                        0.2
                    } else if hovered {
                        0.12
                    } else {
                        0.04
                    },
                    ..color
                },
            );
            // Dark backing keeps the crisp colored outline visible on light screenshots.
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color::from_rgba(0., 0., 0., 0.8))
                    .with_width(if focused || selected { 6. } else { 4. }),
            );
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(color)
                    .with_width(if focused || selected { 3. } else { 2. }),
            );
            let badge = geometry::badge_bounds(
                geometry::detection_bounds(detection, &transform),
                bounds.size(),
            );
            let (x, y) = (badge.x, badge.y);
            frame.fill_rectangle(badge.position(), badge.size(), color);
            frame.fill_text(canvas::Text {
                content: (index + 1).to_string(),
                position: Point::new(x + 8., y + 3.),
                color: Color::BLACK,
                size: 16.into(),
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}
fn control<'a>(
    m: &Model,
    label: &'a str,
    target: Focus,
    primary: bool,
) -> iced::widget::Button<'a, Event> {
    let focused = m.focus == Some(target);
    button(text(label).size(13))
        .padding([8, 12])
        .style(move |theme, status| style::focused(theme, status, primary, focused))
}
fn panel(m: &Model, index: usize, bounds: Rectangle) -> Element<'_, Event> {
    let result = &m.scan.results[index];
    let title = match result.content {
        QrContent::Url(_) => "Website",
        QrContent::Phone { .. } => "Phone number",
        QrContent::Wifi(_) => "Wi-Fi network",
        QrContent::Text => "Text",
        QrContent::Binary => "Binary",
        QrContent::Invalid { .. } => "Invalid content",
    };
    let mut contents = column![].spacing(10);
    if let Some(content) = &m.text {
        let focused = m.focus == Some(Focus::Text);
        contents = contents.push(
            container(
                text_editor(content)
                    .id(m.editor_id())
                    .key_binding(editor_binding)
                    .on_action(Event::Text)
                    .height(130)
                    .padding(8)
                    .size(15)
                    .style(|theme, status| {
                        let mut s = text_editor::default(theme, status);
                        s.background = Color::TRANSPARENT.into();
                        s.border.width = 0.;
                        s
                    }),
            )
            .style(move |theme| style::focus_ring(theme, focused)),
        );
    }
    let mut footer = column![].spacing(8);
    if m.can_reveal() {
        footer = footer.push(
            row![
                text(if m.revealed {
                    "Details visible"
                } else {
                    "Password / encoded details hidden"
                })
                .size(12)
                .width(Length::Fill),
                control(
                    m,
                    if m.revealed { "Hide" } else { "Reveal" },
                    Focus::Reveal,
                    false
                )
                .on_press(Event::Reveal)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }
    if let QrContent::Invalid { reason, .. } = &result.content {
        contents = contents.push(text(reason).size(12));
    }
    let mut controls =
        row![control(m, "Copy", Focus::Copy, false).on_press(Event::Copy)].spacing(8);
    if let Some(action) = result.action() {
        let label = if m.pending(index) {
            "Working…"
        } else {
            match action {
                ResultAction::OpenUrl(_) => "Open URL",
                ResultAction::Call(_) => "Call number",
                ResultAction::ConnectWifi(_) => "Connect to Wi-Fi",
            }
        };
        let b = control(m, label, Focus::Activate, true);
        controls = controls.push(if m.pending(index) {
            b
        } else {
            b.on_press(Event::Activate)
        });
    }
    footer = footer.push(controls);
    if let Some(feedback) = m.copies.get(&index) {
        contents = contents.push(
            text(match feedback {
                CopyFeedback::Copied => "Copied",
                CopyFeedback::Failed(_) => "Windowless clipboard unavailable.",
            })
            .size(12),
        );
    }
    if let Some(state) = m.actions.get(&index) {
        match &state.status {
            ActionStatus::Failed(error) => {
                contents = contents.push(text(error.to_string()).size(12))
            }
            ActionStatus::Succeeded => contents = contents.push(text("Action completed.").size(12)),
            _ => {}
        }
    }
    let body = column![
        text(title).size(16),
        scrollable(contents)
            .width(Length::Fill)
            .height(Length::Fill),
        footer
    ]
    .spacing(10);
    opaque(
        container(body)
            .padding(16)
            .width(bounds.width)
            .height(bounds.height)
            .style(style::card),
    )
}
pub(super) fn view(m: &Model) -> Element<'_, Event> {
    iced::widget::responsive(move |viewport| surface(m, viewport)).into()
}
fn surface(m: &Model, viewport: Size) -> Element<'_, Event> {
    let transform =
        geometry::Transform::new([m.frozen.image.width(), m.frozen.image.height()], viewport);
    // Separate stack layers preserve image-before-vectors order in tiny-skia.
    let mut layers = stack![
        iced::widget::canvas(Frozen {
            model: m,
            transform
        })
        .width(Length::Fill)
        .height(Length::Fill),
        iced::widget::canvas(Screen {
            model: m,
            transform
        })
        .width(Length::Fill)
        .height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    if let (Some(result), Some(bounds)) = (m.selected_result(), m.panel_bounds(viewport)) {
        layers = layers.push(pin(panel(m, result, bounds)).position(bounds.position()));
    }
    // Render exactly the bounds used by the model's visibility rules.
    if let Some(bounds) = m.close_bounds(viewport) {
        layers = layers.push(
            pin(opaque(
                control(m, "Close · Esc", Focus::Close, false)
                    .width(bounds.width)
                    .height(bounds.height)
                    .on_press(Event::Close),
            ))
            .position(bounds.position()),
        );
    }
    layers.into()
}

pub(super) fn editor_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Event>> {
    match press.key.as_ref() {
        keyboard::Key::Named(keyboard::key::Named::Tab) => Some(text_editor::Binding::Custom(
            Event::Next(press.modifiers.shift()),
        )),
        keyboard::Key::Named(keyboard::key::Named::Escape) => {
            Some(text_editor::Binding::Custom(Event::Escape))
        }
        keyboard::Key::Named(keyboard::key::Named::Enter) => {
            Some(text_editor::Binding::Custom(Event::ActivateFocused))
        }
        _ => match text_editor::Binding::from_key_press(press) {
            Some(text_editor::Binding::Copy | text_editor::Binding::Cut) => {
                Some(text_editor::Binding::Custom(Event::CopySelection))
            }
            binding => binding,
        },
    }
}
