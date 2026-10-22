use crate::decode::Detection;
use iced::{Point, Rectangle, Size};

/// Uniform aspect-preserving image fit, including letterbox offsets.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    image: [u32; 2],
    scale: f32,
    offset: Point,
}
impl Transform {
    pub fn new(image: [u32; 2], surface: Size) -> Option<Self> {
        if image.contains(&0)
            || !surface.width.is_finite()
            || !surface.height.is_finite()
            || surface.width <= 0.0
            || surface.height <= 0.0
        {
            return None;
        }
        let scale = (surface.width / image[0] as f32).min(surface.height / image[1] as f32);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Some(Self {
            image,
            scale,
            offset: Point::new(
                (surface.width - image[0] as f32 * scale) / 2.0,
                (surface.height - image[1] as f32 * scale) / 2.0,
            ),
        })
    }
    pub fn to_surface(&self, p: [f32; 2]) -> Point {
        Point::new(
            p[0] * self.scale + self.offset.x,
            p[1] * self.scale + self.offset.y,
        )
    }
    pub fn to_image(&self, p: Point) -> Option<[f32; 2]> {
        let p = [
            (p.x - self.offset.x) / self.scale,
            (p.y - self.offset.y) / self.scale,
        ];
        (p.iter().all(|v| v.is_finite())
            && p[0] >= 0.0
            && p[1] >= 0.0
            && p[0] <= self.image[0] as f32
            && p[1] <= self.image[1] as f32)
            .then_some(p)
    }
}

/// Returns the first detector region containing the point; edges are included.
pub fn hit_test(detections: &[Detection], transform: &Transform, point: Point) -> Option<usize> {
    let p = transform.to_image(point)?;
    detections.iter().position(|d| {
        if !d.corners.iter().flatten().all(|v| v.is_finite()) {
            return false;
        }
        let mut positive = false;
        let mut negative = false;
        let mut area = 0.0;
        for i in 0..4 {
            let a = d.corners[i];
            let b = d.corners[(i + 1) % 4];
            let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
            positive |= cross > 0.0;
            negative |= cross < 0.0;
            area += a[0] * b[1] - a[1] * b[0];
        }
        area.abs() > f32::EPSILON && !(positive && negative)
    })
}

/// Places a panel below the highlight where possible and clamps it to the viewport.
/// Callers render large contents in a scrollable within these bounds.
pub fn panel_bounds(highlight: Rectangle, requested: Size, viewport: Size) -> Rectangle {
    fn extent(v: f32) -> f32 {
        if v.is_finite() { v.max(0.0) } else { 0.0 }
    }
    let vw = extent(viewport.width);
    let vh = extent(viewport.height);
    let width = extent(requested.width).min(vw);
    let height = extent(requested.height).min(vh);
    let x = extent(highlight.x).min(vw - width);
    let below = highlight.y + highlight.height;
    let y = if below.is_finite() && below + height <= vh {
        below.max(0.0)
    } else {
        extent(highlight.y - height).min(vh - height)
    };
    Rectangle {
        x,
        y,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_coordinates_round_trip() {
        let t = Transform::new([1400, 1400], iced::Size::new(1000., 1000.)).unwrap();
        assert_eq!(t.to_surface([140., 280.]), iced::Point::new(100., 200.));
        assert_eq!(t.to_image(iced::Point::new(100., 200.)), Some([140., 280.]));
        assert!(t.to_image(iced::Point::new(-1., 0.)).is_none());
        assert!(t.to_image(iced::Point::new(f32::NAN, 0.)).is_none());
    }
    #[test]
    fn invalid_dimensions_are_rejected() {
        for s in [
            iced::Size::new(0., 1.),
            iced::Size::new(f32::NAN, 1.),
            iced::Size::new(f32::INFINITY, 1.),
        ] {
            assert!(Transform::new([10, 10], s).is_none());
        }
        assert!(Transform::new([0, 10], iced::Size::new(10., 10.)).is_none());
    }
    #[test]
    fn hit_testing_uses_quadrilateral() {
        let t = Transform::new([100, 100], iced::Size::new(100., 100.)).unwrap();
        let d = [crate::decode::Detection {
            result_index: 3,
            corners: [[50., 10.], [90., 50.], [50., 90.], [10., 50.]],
        }];
        assert_eq!(hit_test(&d, &t, iced::Point::new(50., 50.)), Some(0));
        assert_eq!(hit_test(&d, &t, iced::Point::new(11., 11.)), None);
        assert_eq!(hit_test(&d, &t, iced::Point::new(50., 10.)), Some(0));
    }
    #[test]
    fn panels_stay_inside_viewport() {
        for (x, y) in [(0., 0.), (99., 0.), (0., 99.), (99., 99.)] {
            for size in [iced::Size::new(40., 30.), iced::Size::new(200., 200.)] {
                let r = panel_bounds(
                    iced::Rectangle {
                        x,
                        y,
                        width: 1.,
                        height: 1.,
                    },
                    size,
                    iced::Size::new(100., 100.),
                );
                assert!(r.x >= 0. && r.y >= 0. && r.x + r.width <= 100. && r.y + r.height <= 100.);
            }
        }
    }
}
