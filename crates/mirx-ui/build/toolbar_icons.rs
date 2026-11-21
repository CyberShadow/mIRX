//! The toolbar's glyphs, as code: mirx's own drawings of the subjects of
//! mIRC's toolbar buttons, on a 16-unit grid, in the 16-colour palette's
//! hues. Bold shapes with a one-device-pixel darker outline, as the app
//! icon, so each reads at 16 px.
//!
//! Each is an icon group whose resource id `win32/app_icon.rs` has too
//! (`TOOLBAR_ICON_IDS`), at the toolbar's sizes at 100-200% scaling;
//! `LoadImageW` scales the nearest one for any other.

use crate::raster::{self, Image, Point, Rgb, capsule, circle, polygon, rect, ring};

pub const SIZES: [u32; 4] = [16, 20, 24, 32];

const GRID: f32 = 16.0;

const BLACK: Rgb = Rgb(0x00, 0x00, 0x00);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const GREY: Rgb = Rgb(0xC0, 0xC0, 0xC0);
const DARK_GREY: Rgb = Rgb(0x60, 0x60, 0x60);
const NAVY: Rgb = Rgb(0x00, 0x00, 0x7F);
const NAVY_OUTLINE: Rgb = Rgb(0x00, 0x00, 0x3F);
const YELLOW: Rgb = Rgb(0xFF, 0xFF, 0x00);
const GOLD: Rgb = Rgb(0x7F, 0x5F, 0x00);
const FOLDER: Rgb = Rgb(0xFF, 0xD8, 0x50);
const RED: Rgb = Rgb(0xFF, 0x00, 0x00);
const DARK_RED: Rgb = Rgb(0x7F, 0x00, 0x00);
const GREEN: Rgb = Rgb(0x00, 0x93, 0x00);
const DARK_GREEN: Rgb = Rgb(0x00, 0x4F, 0x00);
const BLUE: Rgb = Rgb(0x00, 0x00, 0xFC);
const BROWN: Rgb = Rgb(0xA0, 0x60, 0x20);
const DARK_BROWN: Rgb = Rgb(0x50, 0x30, 0x10);

/// A glyph: its icon group's resource id, the `.ico` file's stem, and its
/// drawing.
pub struct Glyph {
    pub id: u16,
    pub stem: &'static str,
    draw: fn(Pen) -> Vec<Layer>,
}

pub const GLYPHS: [Glyph; 12] = [
    glyph(101, "connect", connect),
    glyph(102, "disconnect", disconnect),
    glyph(103, "options", options),
    glyph(104, "list", list),
    glyph(105, "join", join),
    glyph(106, "colors", colors),
    glyph(107, "sendfile", send_file),
    glyph(108, "cascade", cascade),
    glyph(109, "tileh", tile_horizontal),
    glyph(110, "tilev", tile_vertical),
    glyph(111, "arrange", arrange_icons),
    glyph(112, "help", help),
];

const fn glyph(id: u16, stem: &'static str, draw: fn(Pen) -> Vec<Layer>) -> Glyph {
    Glyph { id, stem, draw }
}

impl Glyph {
    pub fn render_all(&self) -> Vec<Image> {
        SIZES
            .iter()
            .map(|&size| {
                let layers = (self.draw)(Pen {
                    units_per_pixel: GRID / size as f32,
                });
                let layers = layers
                    .iter()
                    .map(|(colour, shape)| (*colour, shape.as_ref()))
                    .collect::<Vec<_>>();
                raster::render(size, GRID, &layers)
            })
            .collect()
    }
}

type Layer = (Rgb, Box<dyn Fn(Point) -> f32>);

/// Draws at one size: what a device pixel is in grid units.
#[derive(Clone, Copy)]
struct Pen {
    units_per_pixel: f32,
}

impl Pen {
    /// To the nearest pixel edge, so straight edges stay crisp at every
    /// size.
    fn snap(self, v: f32) -> f32 {
        (v / self.units_per_pixel).round() * self.units_per_pixel
    }

    /// A rectangle with pixel-aligned edges.
    fn rect(
        self,
        (x0, y0): (f32, f32),
        (x1, y1): (f32, f32),
    ) -> impl Fn(Point) -> f32 + Copy + 'static {
        let (min, max) = (
            Point(self.snap(x0), self.snap(y0)),
            Point(self.snap(x1), self.snap(y1)),
        );
        move |p| rect(p, min, max)
    }

    /// `shape` in `fill`, edged by one device pixel of `outline`.
    fn outlined(
        self,
        outline: Rgb,
        fill: Rgb,
        shape: impl Fn(Point) -> f32 + Copy + 'static,
    ) -> Vec<Layer> {
        let inset = self.units_per_pixel;
        vec![
            (outline, Box::new(shape)),
            (fill, Box::new(move |p| shape(p) + inset)),
        ]
    }

    /// A window: a dark frame, a white body and a navy title bar.
    fn window(self, (x0, y0): (f32, f32), (x1, y1): (f32, f32)) -> Vec<Layer> {
        let inset = self.units_per_pixel;
        let frame = self.rect((x0, y0), (x1, y1));
        let title = self.rect((x0, y0), (x1, y0 + 3.0));
        let mut layers = self.outlined(BLACK, WHITE, self.rect((x0, y0), (x1, y1)));
        layers.push((NAVY, Box::new(move |p| title(p).max(frame(p) + inset))));
        layers
    }
}

/// mIRC's best-known button: a yellow lightning bolt.
fn bolt(p: Point) -> f32 {
    polygon(
        p,
        &[
            Point(8.0, 0.5),
            Point(14.5, 0.5),
            Point(10.0, 5.5),
            Point(13.5, 5.5),
            Point(3.5, 15.5),
            Point(6.5, 8.5),
            Point(2.5, 8.5),
        ],
    )
}

fn connect(pen: Pen) -> Vec<Layer> {
    pen.outlined(GOLD, YELLOW, bolt)
}

/// The bolt gone grey, struck through in red.
fn disconnect(pen: Pen) -> Vec<Layer> {
    let stroke = (Point(2.5, 2.5), Point(13.5, 13.5));
    let mut layers = pen.outlined(DARK_GREY, GREY, bolt);
    layers.extend(pen.outlined(DARK_RED, RED, move |p| capsule(p, stroke, 1.25)));
    layers
}

/// A hammer: a brown handle and a grey head across its end.
fn options(pen: Pen) -> Vec<Layer> {
    let handle = (Point(2.0, 14.0), Point(10.0, 6.0));
    let head = |p| {
        polygon(
            p,
            &[
                Point(8.0, 0.0),
                Point(15.0, 7.0),
                Point(12.0, 10.0),
                Point(5.0, 3.0),
            ],
        )
    };
    let mut layers = pen.outlined(DARK_BROWN, BROWN, move |p| capsule(p, handle, 1.4));
    layers.extend(pen.outlined(BLACK, GREY, head));
    layers
}

/// A page, with `lines` grey text lines from `top`, every three units.
fn page(pen: Pen, (x0, y0): (f32, f32), (x1, y1): (f32, f32), lines: &[f32]) -> Vec<Layer> {
    let mut layers = pen.outlined(DARK_GREY, WHITE, pen.rect((x0, y0), (x1, y1)));
    for &y in lines {
        layers.push((NAVY, Box::new(pen.rect((x0 + 2.0, y), (x1 - 2.0, y + 1.0)))));
    }
    layers
}

/// mIRC's Channels List: a page of lines.
fn list(pen: Pen) -> Vec<Layer> {
    page(pen, (3.0, 1.0), (13.0, 15.0), &[4.0, 7.0, 10.0])
}

/// mIRC's Channels Folder: a yellow folder.
fn join(pen: Pen) -> Vec<Layer> {
    let tab = pen.rect((1.0, 2.0), (7.0, 5.0));
    let body = pen.rect((1.0, 4.0), (15.0, 14.0));
    let mut layers = pen.outlined(GOLD, FOLDER, move |p| tab(p).min(body(p)));
    // The open front's edge.
    layers.push((GOLD, Box::new(pen.rect((1.0, 6.0), (15.0, 7.0)))));
    layers
}

/// Four of mIRC's colours in a grid.
fn colors(pen: Pen) -> Vec<Layer> {
    let mut layers: Vec<Layer> = vec![(BLACK, Box::new(pen.rect((1.0, 1.0), (15.0, 15.0))))];
    for (colour, min, max) in [
        (RED, (2.0, 2.0), (8.0, 8.0)),
        (GREEN, (8.0, 2.0), (14.0, 8.0)),
        (BLUE, (2.0, 8.0), (8.0, 14.0)),
        (YELLOW, (8.0, 8.0), (14.0, 14.0)),
    ] {
        layers.push((colour, Box::new(pen.rect(min, max))));
    }
    layers
}

/// DCC Send: a page going out, a green arrow.
fn send_file(pen: Pen) -> Vec<Layer> {
    let arrow = |p| {
        polygon(
            p,
            &[
                Point(5.0, 9.0),
                Point(10.0, 9.0),
                Point(10.0, 6.5),
                Point(15.5, 11.0),
                Point(10.0, 15.5),
                Point(10.0, 13.0),
                Point(5.0, 13.0),
            ],
        )
    };
    let mut layers = page(pen, (2.0, 1.0), (11.0, 14.0), &[4.0, 7.0]);
    layers.extend(pen.outlined(DARK_GREEN, GREEN, arrow));
    layers
}

fn cascade(pen: Pen) -> Vec<Layer> {
    let mut layers = pen.window((1.0, 1.0), (11.0, 11.0));
    layers.extend(pen.window((5.0, 5.0), (15.0, 15.0)));
    layers
}

fn tile_horizontal(pen: Pen) -> Vec<Layer> {
    let mut layers = pen.window((1.0, 1.0), (15.0, 8.0));
    layers.extend(pen.window((1.0, 8.0), (15.0, 15.0)));
    layers
}

fn tile_vertical(pen: Pen) -> Vec<Layer> {
    let mut layers = pen.window((1.0, 1.0), (8.0, 15.0));
    layers.extend(pen.window((8.0, 1.0), (15.0, 15.0)));
    layers
}

/// Arrange Icons: a window above three minimized title bars in a row.
fn arrange_icons(pen: Pen) -> Vec<Layer> {
    let mut layers = pen.window((3.0, 1.0), (13.0, 9.0));
    for x in [1.0, 6.0, 11.0] {
        layers.extend(pen.outlined(BLACK, NAVY, pen.rect((x, 11.0), (x + 4.0, 15.0))));
    }
    layers
}

/// A navy disc with a white question mark.
fn help(pen: Pen) -> Vec<Layer> {
    let centre = Point(8.0, 5.8);
    // The hook: a ring with its lower left quarter cut away.
    let hook = move |p: Point| ring(p, centre, 2.4, 1.1).max((centre.0 - p.0).min(p.1 - centre.1));
    let stem = (Point(8.0, 8.2), Point(8.0, 9.6));
    let mut layers = pen.outlined(NAVY_OUTLINE, NAVY, |p| circle(p, Point(8.0, 8.0), 7.5));
    layers.push((
        WHITE,
        Box::new(move |p| {
            hook(p)
                .min(capsule(p, stem, 1.1))
                .min(circle(p, Point(8.0, 12.6), 1.2))
        }),
    ));
    layers
}
