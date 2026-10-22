//! Small monochrome line icons, rendered by the existing canvas backend.
use super::Message;
use iced::{
    Point, Rectangle, Theme, mouse,
    widget::canvas::{self, Frame, Geometry, Path, Stroke},
};
#[derive(Clone, Copy)]
pub(super) enum Icon {
    Scan,
    Camera,
    Settings,
    Link,
    Phone,
    Wifi,
    Copy,
    Text,
}
impl canvas::Program<Message> for Icon {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let lines: &[&[(f32, f32)]] = match self {
            Self::Scan => &[
                &[(6., 2.), (2., 2.), (2., 6.)],
                &[(12., 2.), (16., 2.), (16., 6.)],
                &[(2., 12.), (2., 16.), (6., 16.)],
                &[(12., 16.), (16., 16.), (16., 12.)],
                &[(3., 9.), (15., 9.)],
            ],
            Self::Camera => &[&[
                (2., 5.),
                (6., 5.),
                (7., 2.),
                (11., 2.),
                (12., 5.),
                (16., 5.),
                (16., 15.),
                (2., 15.),
                (2., 5.),
            ]],
            Self::Settings => &[&[
                (6., 2.),
                (12., 2.),
                (12., 5.),
                (16., 6.),
                (16., 12.),
                (12., 13.),
                (12., 16.),
                (6., 16.),
                (6., 13.),
                (2., 12.),
                (2., 6.),
                (6., 5.),
                (6., 2.),
            ]],
            Self::Link => &[
                &[
                    (8., 5.),
                    (11., 2.),
                    (14., 2.),
                    (16., 4.),
                    (16., 7.),
                    (13., 10.),
                ],
                &[
                    (5., 8.),
                    (2., 11.),
                    (2., 14.),
                    (4., 16.),
                    (7., 16.),
                    (10., 13.),
                ],
                &[(6., 12.), (12., 6.)],
            ],
            Self::Phone => &[&[
                (4., 2.),
                (7., 6.),
                (5., 8.),
                (10., 13.),
                (12., 11.),
                (16., 14.),
                (14., 17.),
                (10., 16.),
                (4., 11.),
                (1., 6.),
                (2., 3.),
                (4., 2.),
            ]],
            Self::Wifi => &[
                &[(1., 5.), (5., 3.), (9., 2.), (13., 3.), (17., 5.)],
                &[(4., 9.), (7., 7.), (11., 7.), (14., 9.)],
                &[(7., 12.), (9., 11.), (11., 12.)],
            ],
            Self::Copy => &[
                &[(6., 6.), (16., 6.), (16., 17.), (6., 17.), (6., 6.)],
                &[(12., 6.), (12., 1.), (1., 1.), (1., 12.), (6., 12.)],
            ],
            Self::Text => &[
                &[
                    (3., 2.),
                    (12., 2.),
                    (16., 6.),
                    (16., 16.),
                    (3., 16.),
                    (3., 2.),
                ],
                &[(6., 7.), (12., 7.)],
                &[(6., 10.), (12., 10.)],
                &[(6., 13.), (10., 13.)],
            ],
        };
        let stroke = Stroke::default()
            .with_color(theme.palette().text)
            .with_width(1.4);
        for line in lines {
            let p = Path::new(|p| {
                p.move_to(Point::new(line[0].0, line[0].1));
                for (x, y) in &line[1..] {
                    p.line_to(Point::new(*x, *y));
                }
            });
            frame.stroke(&p, stroke);
        }
        if matches!(self, Self::Camera | Self::Settings) {
            frame.stroke(&Path::circle(Point::new(9., 9.), 3.), stroke);
        }
        if matches!(self, Self::Wifi) {
            frame.fill(&Path::circle(Point::new(9., 15.), 1.), theme.palette().text);
        }
        vec![frame.into_geometry()]
    }
}
pub(super) fn icon(icon: Icon) -> iced::Element<'static, Message> {
    iced::widget::canvas(icon).width(18).height(18).into()
}
