//! The rasterizer mirx's icons are drawn with: shapes as signed distance
//! functions (negative inside) on a square grid of units, painted in order.
//!
//! Each pixel takes 8x8 samples of a shape; coverage becomes alpha, and
//! shapes are painted with straight-alpha "over". Only `f32` arithmetic, so
//! the output is the same on every build.

const SAMPLES: u32 = 8;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A point in grid units: x right, y down.
#[derive(Clone, Copy)]
pub struct Point(pub f32, pub f32);

/// A rendered size: straight (not premultiplied) RGBA, rows top-down.
pub struct Image {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// A shape: its signed distance at a point, in grid units.
pub type Shape<'a> = &'a dyn Fn(Point) -> f32;

/// Paints `layers` in order on a transparent `size` x `size` image, the
/// grid's `grid` units covering it.
pub fn render(size: u32, grid: f32, layers: &[(Rgb, Shape)]) -> Image {
    let units_per_pixel = grid / size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut pixel = [0.0f32; 4];
            for (colour, shape) in layers {
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

fn coverage(x: u32, y: u32, units_per_pixel: f32, shape: Shape) -> f32 {
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

pub fn rounded_rect(p: Point, min: Point, max: Point, radius: f32) -> f32 {
    let centre = Point((min.0 + max.0) / 2.0, (min.1 + max.1) / 2.0);
    let qx = (p.0 - centre.0).abs() - ((max.0 - min.0) / 2.0 - radius);
    let qy = (p.1 - centre.1).abs() - ((max.1 - min.1) / 2.0 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// An axis-aligned rectangle from `min` to `max`.
pub fn rect(p: Point, min: Point, max: Point) -> f32 {
    rounded_rect(p, min, max, 0.0)
}

/// Signed distance to a triangle (negative inside), after Inigo Quilez.
pub fn triangle(p: Point, p0: Point, p1: Point, p2: Point) -> f32 {
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

/// Signed distance to a simple polygon (negative inside), after Inigo
/// Quilez: the nearest edge's distance, its sign from the crossing count.
pub fn polygon(p: Point, vertices: &[Point]) -> f32 {
    let sub = |a: Point, b: Point| Point(a.0 - b.0, a.1 - b.1);
    let dot = |a: Point, b: Point| a.0 * b.0 + a.1 * b.1;
    let mut nearest = f32::MAX;
    let mut inside = false;
    for (index, &start) in vertices.iter().enumerate() {
        let end = vertices[(index + 1) % vertices.len()];
        let edge = sub(end, start);
        let v = sub(p, start);
        let t = (dot(v, edge) / dot(edge, edge)).clamp(0.0, 1.0);
        let to_edge = Point(v.0 - edge.0 * t, v.1 - edge.1 * t);
        nearest = nearest.min(dot(to_edge, to_edge));
        // Whether a ray to the right of p crosses the edge.
        if (start.1 > p.1) != (end.1 > p.1)
            && p.0 < start.0 + (p.1 - start.1) / (end.1 - start.1) * edge.0
        {
            inside = !inside;
        }
    }
    let distance = nearest.sqrt();
    if inside { -distance } else { distance }
}

/// A stroke with round ends: distance to the segment minus the radius.
pub fn capsule(p: Point, (a, b): (Point, Point), radius: f32) -> f32 {
    let (pax, pay) = (p.0 - a.0, p.1 - a.1);
    let (bax, bay) = (b.0 - a.0, b.1 - a.1);
    let t = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    (pax - bax * t).hypot(pay - bay * t) - radius
}

pub fn circle(p: Point, centre: Point, radius: f32) -> f32 {
    (p.0 - centre.0).hypot(p.1 - centre.1) - radius
}

/// A circle's outline, `half_width` either side of `radius`.
pub fn ring(p: Point, centre: Point, radius: f32, half_width: f32) -> f32 {
    circle(p, centre, radius).abs() - half_width
}
