//! mirx's icon, as code: original artwork, drawn on a 32-unit grid and
//! rasterized at every size the Windows 10/11 shell asks for.
//!
//! A navy speech bubble (mIRC colour 2) with a 1-device-pixel darker outline,
//! its tail pointing down-left, and a two-colour "x" inside: a yellow stroke
//! (mIRC colour 8) crossed by a white one. mIRC's colour numbers are the nod;
//! the shape is mirx's own.
//!
//! Each pixel takes 8x8 samples of signed-distance shapes; coverage becomes
//! alpha, and shapes are painted in order with straight-alpha "over". Only
//! `f32` arithmetic, so the output is the same on every build.

/// Square sizes in pixels: the shell's small and large icons at 100-200%
/// scaling, plus Explorer's 256 (stored as PNG).
pub const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

const GRID: f32 = 32.0;
const SAMPLES: u32 = 8;

const OUTLINE: Rgb = Rgb(0x00, 0x00, 0x3F);
const NAVY: Rgb = Rgb(0x00, 0x00, 0x7F);
const YELLOW: Rgb = Rgb(0xFF, 0xFF, 0x00);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

const STROKE_RADIUS: f32 = 1.75;
/// The "x": yellow from top right to bottom left, white from top left to
/// bottom right, painted last so it crosses over the yellow.
const YELLOW_STROKE: (Point, Point) = (Point(22.0, 7.0), Point(10.0, 19.0));
const WHITE_STROKE: (Point, Point) = (Point(10.0, 7.0), Point(22.0, 19.0));

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);

#[derive(Clone, Copy)]
struct Point(f32, f32);

/// A rendered size: straight (not premultiplied) RGBA, rows top-down.
pub struct Image {
    pub size: u32,
    pub rgba: Vec<u8>,
}

pub fn render_all() -> Vec<Image> {
    SIZES.iter().map(|&size| render(size)).collect()
}

fn render(size: u32) -> Image {
    let units_per_pixel = GRID / size as f32;
    let (yellow, white) = (snapped(YELLOW_STROKE, size), snapped(WHITE_STROKE, size));
    // Paint order: the bubble in the outline colour, the bubble inset by one
    // device pixel in navy, then the strokes.
    let layers: [(Rgb, &dyn Fn(Point) -> f32); 4] = [
        (OUTLINE, &bubble),
        (NAVY, &|p| bubble(p) + units_per_pixel),
        (YELLOW, &|p| capsule(p, yellow, STROKE_RADIUS)),
        (WHITE, &|p| capsule(p, white, STROKE_RADIUS)),
    ];
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut pixel = [0.0f32; 4];
            for (colour, shape) in &layers {
                let alpha = coverage(x, y, units_per_pixel, *shape);
                over(&mut pixel, *colour, alpha);
            }
            let at = ((y * size + x) * 4) as usize;
            for (channel, value) in pixel.iter().enumerate() {
                rgba[at + channel] = (value * 255.0).round() as u8;
            }
        }
    }
    Image { size, rgba }
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

fn coverage(x: u32, y: u32, units_per_pixel: f32, shape: &dyn Fn(Point) -> f32) -> f32 {
    let mut inside = 0u32;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * units_per_pixel;
            let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * units_per_pixel;
            if shape(Point(px, py)) <= 0.0 {
                inside += 1;
            }
        }
    }
    inside as f32 / (SAMPLES * SAMPLES) as f32
}

/// Straight-alpha "over" of a solid colour with coverage `alpha`.
fn over(pixel: &mut [f32; 4], colour: Rgb, alpha: f32) {
    let below = pixel[3] * (1.0 - alpha);
    let out = alpha + below;
    if out == 0.0 {
        return;
    }
    let source = [colour.0, colour.1, colour.2].map(|c| c as f32 / 255.0);
    for channel in 0..3 {
        pixel[channel] = (source[channel] * alpha + pixel[channel] * below) / out;
    }
    pixel[3] = out;
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

fn rounded_rect(p: Point, min: Point, max: Point, radius: f32) -> f32 {
    let centre = Point((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
    let qx = (p.0 - centre.0).abs() - ((max.0 - min.0) / 2.0 - radius);
    let qy = (p.1 - centre.1).abs() - ((max.1 - min.1) / 2.0 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// Signed distance to a triangle (negative inside), after Inigo Quilez.
fn triangle(p: Point, p0: Point, p1: Point, p2: Point) -> f32 {
    let sub = |a: Point, b: Point| Point(a.0 - b.0, a.1 - b.1);
    let dot = |a: Point, b: Point| a.0 * b.0 + a.1 * b.1;
    let cross = |a: Point, b: Point| a.0 * b.1 - a.1 * b.0;
    let edges = [(p0, sub(p1, p0)), (p1, sub(p2, p1)), (p2, sub(p0, p2))];
    let winding = cross(edges[0].1, edges[2].1).signum();
    let mut nearest = f32::MAX;
    let mut side = f32::MAX;
    for (start, edge) in edges {
        let v = sub(p, start);
        let t = (dot(v, edge) / dot(edge, edge)).clamp(0.0, 1.0);
        let to_edge = Point(v.0 - edge.0 * t, v.1 - edge.1 * t);
        nearest = nearest.min(dot(to_edge, to_edge));
        side = side.min(winding * cross(v, edge));
    }
    -nearest.sqrt() * side.signum()
}

/// A stroke with round ends: distance to the segment minus the radius.
fn capsule(p: Point, (a, b): (Point, Point), radius: f32) -> f32 {
    let (pax, pay) = (p.0 - a.0, p.1 - a.1);
    let (bax, bay) = (b.0 - a.0, b.1 - a.1);
    let t = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    (pax - bax * t).hypot(pay - bay * t) - radius
}
