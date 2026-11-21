//! mirx's icon, as code: original artwork, drawn on a 32-unit grid and
//! rasterized at every size the Windows 10/11 shell asks for.
//!
//! A navy speech bubble (mIRC colour 2) with a 1-device-pixel darker outline,
//! its tail pointing down-left, and a two-colour "x" inside: a yellow stroke
//! (mIRC colour 8) crossed by a white one. mIRC's colour numbers are the nod;
//! the shape is mirx's own.
//!
//! Rasterized by `raster`.

use crate::raster::{self, Image, Point, Rgb, capsule, rounded_rect, triangle};

/// Square sizes in pixels: the shell's small and large icons at 100-200%
/// scaling, plus Explorer's 256 (stored as PNG).
pub const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

const GRID: f32 = 32.0;

const OUTLINE: Rgb = Rgb(0x00, 0x00, 0x3F);
const NAVY: Rgb = Rgb(0x00, 0x00, 0x7F);
const YELLOW: Rgb = Rgb(0xFF, 0xFF, 0x00);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

const STROKE_RADIUS: f32 = 1.75;
/// The "x": yellow from top right to bottom left, white from top left to
/// bottom right, painted last so it crosses over the yellow.
const YELLOW_STROKE: (Point, Point) = (Point(22.0, 7.0), Point(10.0, 19.0));
const WHITE_STROKE: (Point, Point) = (Point(10.0, 7.0), Point(22.0, 19.0));

pub fn render_all() -> Vec<Image> {
    SIZES.iter().map(|&size| render(size)).collect()
}

fn render(size: u32) -> Image {
    let units_per_pixel = GRID / size as f32;
    let (yellow, white) = (snapped(YELLOW_STROKE, size), snapped(WHITE_STROKE, size));
    // Paint order: the bubble in the outline colour, the bubble inset by one
    // device pixel in navy, then the strokes.
    let layers: [(Rgb, raster::Shape); 4] = [
        (OUTLINE, &bubble),
        (NAVY, &|p| bubble(p) + units_per_pixel),
        (YELLOW, &|p| capsule(p, yellow, STROKE_RADIUS)),
        (WHITE, &|p| capsule(p, white, STROKE_RADIUS)),
    ];
    raster::render(size, GRID, &layers)
}

/// At 16 and 20 px a stroke is under two pixels wide, so its ends move to
/// pixel centres: the diagonals then run through whole pixels and stay crisp
/// instead of smearing over three.
fn snapped((a, b): (Point, Point), size: u32) -> (Point, Point) {
    if size > 20 {
        return (a, b);
    }
    let units_per_pixel = GRID / size as f32;
    let snap = |v: f32| ((v / units_per_pixel).floor() + 0.5) * units_per_pixel;
    (Point(snap(a.0), snap(a.1)), Point(snap(b.0), snap(b.1)))
}

/// The bubble: a rounded rectangle (1,3)-(31,23), radius 5, and its tail.
fn bubble(p: Point) -> f32 {
    rounded_rect(p, Point(1.0, 3.0), Point(31.0, 23.0), 5.0).min(triangle(
        p,
        Point(6.0, 21.0),
        Point(13.0, 21.0),
        Point(4.0, 30.0),
    ))
}
