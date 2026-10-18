use crate::app::Message;
use iced::{
    Color, Point, Rectangle, mouse,
    widget::canvas::{self, Frame, Geometry, Path, Stroke},
};
pub struct Preview {
    pub handle: iced::widget::image::Handle,
    pub size: (u32, u32),
    pub session: crate::jobs::JobId,
    pub selecting: bool,
    pub epoch: u64,
}
#[derive(Default)]
pub struct Drag {
    key: Option<(crate::jobs::JobId, u64)>,
    start: Option<Point>,
    end: Option<Point>,
}
fn fitted(size: (u32, u32), bounds: Rectangle) -> Rectangle {
    let scale = (bounds.width / size.0 as f32).min(bounds.height / size.1 as f32);
    let w = size.0 as f32 * scale;
    let h = size.1 as f32 * scale;
    Rectangle {
        x: (bounds.width - w) / 2.,
        y: (bounds.height - h) / 2.,
        width: w,
        height: h,
    }
}
impl canvas::Program<Message> for Preview {
    type State = Drag;
    fn update(
        &self,
        state: &mut Drag,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        let key = Some((self.session, self.epoch));
        if state.key != key || !self.selecting {
            *state = Drag {
                key,
                ..Default::default()
            };
        }
        if !self.selecting {
            return None;
        }
        let local = cursor
            .position()
            .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y));
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.start = local;
                state.end = local;
                Some(iced::widget::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.start.is_some() => {
                state.end = local;
                Some(iced::widget::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let start = state.start.take()?;
                let end = local.or(state.end.take())?;
                let r = fitted(self.size, bounds);
                super::crop::map_selection(
                    self.size,
                    (r.x, r.y, r.width, r.height),
                    ((start.x, start.y), (end.x, end.y)),
                )
                .map(|rect| {
                    iced::widget::Action::publish(Message::CropSelected(self.session, rect))
                })
            }
            _ => None,
        }
    }
    fn draw(
        &self,
        state: &Drag,
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.draw_image(fitted(self.size, bounds), &self.handle);
        if self.selecting
            && state.key == Some((self.session, self.epoch))
            && let (Some(a), Some(b)) = (state.start, state.end)
        {
            let path = Path::rectangle(
                Point::new(a.x.min(b.x), a.y.min(b.y)),
                iced::Size::new((a.x - b.x).abs(), (a.y - b.y).abs()),
            );
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color::from_rgb(0., 0.6, 1.))
                    .with_width(2.),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas::Program;
    #[test]
    fn escape_then_reselect_cannot_finish_previous_drag() {
        let mut preview = Preview {
            handle: iced::widget::image::Handle::from_rgba(1, 1, vec![0; 4]),
            size: (100, 100),
            session: crate::jobs::JobId(1),
            selecting: true,
            epoch: 0,
        };
        let mut drag = Drag::default();
        let bounds = Rectangle::new(Point::ORIGIN, iced::Size::new(100., 100.));
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        preview.update(
            &mut drag,
            &press,
            bounds,
            mouse::Cursor::Available(Point::new(10., 10.)),
        );
        let escape = canvas::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::ORIGIN,
        });
        preview.selecting = false;
        preview.epoch += 1;
        preview.update(&mut drag, &escape, bounds, mouse::Cursor::Unavailable);
        preview.selecting = true;
        preview.epoch += 1;
        assert!(
            preview
                .update(
                    &mut drag,
                    &release,
                    bounds,
                    mouse::Cursor::Available(Point::new(90., 90.))
                )
                .is_none()
        );
        assert!(drag.start.is_none());
    }
}
