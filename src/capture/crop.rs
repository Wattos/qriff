#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
pub fn map_selection(
    size: (u32, u32),
    display: (f32, f32, f32, f32),
    drag: ((f32, f32), (f32, f32)),
) -> Option<PixelRect> {
    let (iw, ih) = size;
    let (x, y, w, h) = display;
    let ((ax, ay), (bx, by)) = drag;
    if iw == 0
        || ih == 0
        || w <= 0.
        || h <= 0.
        || ![x, y, w, h, ax, ay, bx, by].iter().all(|v| v.is_finite())
    {
        return None;
    }
    let left = ((ax.min(bx) - x) / w).clamp(0., 1.) * iw as f32;
    let right = ((ax.max(bx) - x) / w).clamp(0., 1.) * iw as f32;
    let top = ((ay.min(by) - y) / h).clamp(0., 1.) * ih as f32;
    let bottom = ((ay.max(by) - y) / h).clamp(0., 1.) * ih as f32;
    if right <= left || bottom <= top {
        return None;
    }
    let x = left.floor() as u32;
    let y = top.floor() as u32;
    Some(PixelRect {
        x,
        y,
        width: (right.ceil() as u32).min(iw) - x,
        height: (bottom.ceil() as u32).min(ih) - y,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_letterboxed_reverse_and_clamped() {
        let rect = PixelRect {
            x: 100,
            y: 50,
            width: 600,
            height: 300,
        };
        assert_eq!(
            map_selection(
                (1000, 500),
                (20., 30., 500., 250.),
                ((70., 55.), (370., 205.))
            ),
            Some(rect)
        );
        assert_eq!(
            map_selection(
                (1000, 500),
                (20., 30., 500., 250.),
                ((370., 205.), (70., 55.))
            ),
            Some(rect)
        );
        assert_eq!(
            map_selection(
                (1000, 500),
                (20., 30., 500., 250.),
                ((-90., 0.), (900., 900.))
            ),
            Some(PixelRect {
                x: 0,
                y: 0,
                width: 1000,
                height: 500
            })
        );
    }
    #[test]
    fn rejects_empty_and_invalid() {
        for drag in [
            ((0., 0.), (0., 0.)),
            ((f32::NAN, 0.), (2., 2.)),
            ((0., 0.), (f32::INFINITY, 2.)),
            ((-10., -10.), (-1., -1.)),
        ] {
            assert_eq!(map_selection((10, 10), (0., 0., 10., 10.), drag), None);
        }
        assert_eq!(
            map_selection((0, 10), (0., 0., 10., 10.), ((0., 0.), (2., 2.))),
            None
        );
    }
}
