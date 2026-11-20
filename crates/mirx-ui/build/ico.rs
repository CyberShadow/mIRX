//! The ICO container: 32-bpp BMP entries for the small sizes, a PNG entry for
//! 256 (Windows Vista and later read PNG entries; a 256 BMP would be 256 KiB).

use crate::icon::Image;

const PNG_SIZE: u32 = 256;

pub fn encode(images: &[Image]) -> Vec<u8> {
    let entries: Vec<Vec<u8>> = images
        .iter()
        .map(|image| {
            if image.size == PNG_SIZE {
                png(image)
            } else {
                bmp(image)
            }
        })
        .collect();

    let count = u16::try_from(images.len()).expect("an icon has few sizes");
    let mut out = Vec::new();
    // ICONDIR: reserved, type 1 (icon), count.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (image, data) in images.iter().zip(&entries) {
        // ICONDIRENTRY; a width or height of 0 means 256.
        let side = u8::try_from(image.size % 256).expect("icon sizes are at most 256");
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        let len = u32::try_from(data.len()).expect("an icon entry is small");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += len;
    }
    for data in &entries {
        out.extend_from_slice(data);
    }
    out
}

/// A `BITMAPINFOHEADER` (height doubled for the mask), BGRA rows bottom-up,
/// then the 1-bpp AND mask (set where the pixel is fully transparent), its
/// rows padded to 32 bits.
fn bmp(image: &Image) -> Vec<u8> {
    let size = image.size;
    let mask_stride = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&(2 * size as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(size * size * 4 + mask_stride * size).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution, colours used, important
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = pixel(image, x, y);
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    for y in (0..size).rev() {
        let mut row = vec![0u8; mask_stride as usize];
        for x in 0..size {
            if pixel(image, x, y)[3] == 0 {
                row[(x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    out
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.size + x) * 4) as usize;
    image.rgba[at..at + 4].try_into().expect("four channels")
}

/// An RGBA PNG: one unfiltered IDAT, zlib-compressed by miniz_oxide.
fn png(image: &Image) -> Vec<u8> {
    let stride = (image.size * 4) as usize;
    let mut raw = Vec::with_capacity((stride + 1) * image.size as usize);
    for row in image.rgba.chunks_exact(stride) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut header = Vec::new();
    header.extend_from_slice(&image.size.to_be_bytes());
    header.extend_from_slice(&image.size.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, deflate, no interlace

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 10),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).expect("a PNG chunk is small");
    out.extend_from_slice(&len.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// CRC-32 (ISO 3309, as PNG uses it), bit by bit: a few KiB of input.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
