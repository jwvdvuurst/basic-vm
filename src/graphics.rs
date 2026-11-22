//! Headless, deterministic software graphics: a [`Canvas`] (framebuffer of
//! `0x00RRGGBB` pixels) with clipped drawing primitives, the
//! [`GraphicsBackend`] abstraction a window implementation plugs into, and a
//! [`HeadlessBackend`] that records frames and replays scripted events.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Largest canvas width or height in pixels.
pub const MAX_DIMENSION: usize = 4096;
/// Largest circle radius; bigger circles would take unbounded time.
pub const MAX_RADIUS: i64 = 1_000_000;
/// Coordinates are clamped to this magnitude so arithmetic cannot overflow.
pub const COORDINATE_LIMIT: i64 = 1 << 40;

/// The QBasic/VGA 16-colour palette (`0x00RRGGBB`).
pub const PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// A width x height framebuffer of packed `0x00RRGGBB` pixels, row-major,
/// origin at the top-left. Every drawing primitive clips to the canvas and
/// accepts any coordinates without panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
    dirty: bool,
}

impl Canvas {
    /// A black canvas; both sizes must be within `1..=MAX_DIMENSION`.
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        let valid = 1..=MAX_DIMENSION;
        if !valid.contains(&width) || !valid.contains(&height) {
            return Err(format!(
                "canvas size must be 1 to {MAX_DIMENSION} pixels in each direction, got {width} x {height}"
            ));
        }
        Ok(Self {
            width,
            height,
            pixels: vec![0; width * height],
            dirty: false,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The pixels in row-major order (`y * width + x`).
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Whether anything was drawn since the last [`Canvas::mark_clean`].
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// The pixel at `(x, y)`, or `None` outside the canvas.
    pub fn pixel(&self, x: i64, y: i64) -> Option<u32> {
        self.index(x, y).map(|index| self.pixels[index])
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        // Negative values wrap to huge unsigned ones and fail the bounds check.
        let (ux, uy) = (x as u64, y as u64);
        (ux < self.width as u64 && uy < self.height as u64)
            .then(|| uy as usize * self.width + ux as usize)
    }

    pub fn set_pixel(&mut self, x: i64, y: i64, color: u32) {
        if let Some(index) = self.index(x, y) {
            self.pixels[index] = color;
            self.dirty = true;
        }
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
        self.dirty = true;
    }

    /// A Bresenham line with inclusive endpoints. Only the part inside the
    /// canvas is visited, so far-away endpoints are cheap and the visible
    /// pixels equal those of the unclipped line.
    pub fn line(&mut self, x1: i64, y1: i64, x2: i64, y2: i64, color: u32) {
        // Canonical endpoint order makes the line symmetric under swapping.
        let ((x1, y1), (x2, y2)) = if (x1, y1) <= (x2, y2) {
            ((x1, y1), (x2, y2))
        } else {
            ((x2, y2), (x1, y1))
        };
        let (x1, y1, x2, y2) = (
            i128::from(x1),
            i128::from(y1),
            i128::from(x2),
            i128::from(y2),
        );
        let (dx, dy) = ((x2 - x1).abs(), (y2 - y1).abs());
        let (sx, sy) = (if x2 >= x1 { 1 } else { -1 }, if y2 >= y1 { 1 } else { -1 });
        let x_major = dx >= dy;
        let (major_start, major_step, minor_start, minor_step, major_length, minor_length, limit) =
            if x_major {
                (x1, sx, y1, sy, dx, dy, self.width as i128)
            } else {
                (y1, sy, x1, sx, dy, dx, self.height as i128)
            };
        // Steps whose major coordinate lies inside the canvas.
        let (mut first, mut last) = if major_step > 0 {
            (-major_start, limit - 1 - major_start)
        } else {
            (major_start - (limit - 1), major_start)
        };
        first = first.max(0);
        last = last.min(major_length);
        let mut step = first;
        while step <= last {
            let offset = if major_length == 0 {
                0
            } else {
                (2 * minor_length * step + major_length) / (2 * major_length)
            };
            let major = major_start + major_step * step;
            let minor = minor_start + minor_step * offset;
            let (x, y) = if x_major {
                (major, minor)
            } else {
                (minor, major)
            };
            self.set_pixel(x as i64, y as i64, color);
            step += 1;
        }
    }

    /// The outline of the box with opposite corners `(x1, y1)` and `(x2, y2)`.
    pub fn rect(&mut self, x1: i64, y1: i64, x2: i64, y2: i64, color: u32) {
        let (left, right) = (x1.min(x2), x1.max(x2));
        let (top, bottom) = (y1.min(y2), y1.max(y2));
        self.line(left, top, right, top, color);
        self.line(left, bottom, right, bottom, color);
        self.line(left, top, left, bottom, color);
        self.line(right, top, right, bottom, color);
    }

    /// The filled box with opposite corners `(x1, y1)` and `(x2, y2)`.
    pub fn fill_rect(&mut self, x1: i64, y1: i64, x2: i64, y2: i64, color: u32) {
        let left = x1.min(x2).max(0);
        let right = x1.max(x2).min(self.width as i64 - 1);
        let top = y1.min(y2).max(0);
        let bottom = y1.max(y2).min(self.height as i64 - 1);
        if left > right || top > bottom {
            return;
        }
        for y in top..=bottom {
            let start = y as usize * self.width;
            self.pixels[start + left as usize..=start + right as usize].fill(color);
        }
        self.dirty = true;
    }

    fn intersects_circle(&self, cx: i64, cy: i64, radius: i64) -> bool {
        cx + radius >= 0
            && cy + radius >= 0
            && cx - radius < self.width as i64
            && cy - radius < self.height as i64
    }

    /// A midpoint-algorithm circle outline; `radius` is clamped to
    /// `0..=MAX_RADIUS` (radius 0 is a single pixel).
    pub fn circle(&mut self, cx: i64, cy: i64, radius: i64, color: u32) {
        let radius = radius.clamp(0, MAX_RADIUS);
        if !self.intersects_circle(cx, cy, radius) {
            return;
        }
        let (mut x, mut y) = (radius, 0i64);
        let mut decision = 1 - radius;
        loop {
            for (px, py) in [
                (x, y),
                (y, x),
                (-x, y),
                (-y, x),
                (x, -y),
                (y, -x),
                (-x, -y),
                (-y, -x),
            ] {
                self.set_pixel(cx + px, cy + py, color);
            }
            if x <= y {
                break;
            }
            y += 1;
            if decision < 0 {
                decision += 2 * y + 1;
            } else {
                x -= 1;
                decision += 2 * (y - x) + 1;
            }
        }
    }

    /// A filled disc: horizontal spans plus the midpoint outline, so the fill
    /// always contains the outline.
    pub fn fill_circle(&mut self, cx: i64, cy: i64, radius: i64, color: u32) {
        let radius = radius.clamp(0, MAX_RADIUS);
        if !self.intersects_circle(cx, cy, radius) {
            return;
        }
        let top = (cy - radius).max(0);
        let bottom = (cy + radius).min(self.height as i64 - 1);
        for y in top..=bottom {
            let dy = y - cy;
            let half = (radius * radius - dy * dy).isqrt();
            self.fill_rect(cx - half, y, cx + half, y, color);
        }
        self.circle(cx, cy, radius, color);
    }

    /// Draws `text` with the built-in 8x8 font, top-left corner at `(x, y)`,
    /// 8 pixels per character; characters outside ASCII 32..126 show as `?`.
    pub fn text(&mut self, x: i64, y: i64, text: &str, color: u32) {
        for (position, ch) in text.chars().enumerate() {
            let left = x + 8 * position as i64;
            if left >= self.width as i64 {
                break;
            }
            if left + 8 <= 0 {
                continue;
            }
            let glyph = glyph(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for column in 0..8 {
                    if (bits >> column) & 1 == 1 {
                        self.set_pixel(left + column, y + row as i64, color);
                    }
                }
            }
        }
    }

    /// Overwrites this canvas with the pixels of `source`, which must have
    /// the same size. The dirty flag is set.
    pub fn copy_from(&mut self, source: &Canvas) {
        debug_assert_eq!((self.width, self.height), (source.width, source.height));
        self.pixels.copy_from_slice(&source.pixels);
        self.dirty = true;
    }

    /// The canvas encoded as `format`.
    pub fn encode(&self, format: ImageFormat) -> Vec<u8> {
        match format {
            ImageFormat::Ppm => self.to_ppm(),
            ImageFormat::Bmp => self.to_bmp(),
            ImageFormat::Png => self.to_png(),
        }
    }

    /// The canvas as a PNG image: 8-bit RGB, no interlacing, one `IDAT`
    /// chunk holding a zlib stream of *stored* (uncompressed) deflate blocks.
    /// Larger than a compressed PNG, but valid and dependency-free.
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity((self.width * 3 + 1) * self.height);
        for row in self.pixels.chunks(self.width) {
            raw.push(0); // filter type "None"
            for pixel in row {
                raw.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
            }
        }
        let mut zlib = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
        // CMF/FLG: deflate, 32 KiB window, no preset dictionary, check bits ok.
        zlib.extend_from_slice(&[0x78, 0x01]);
        let mut blocks = raw.chunks(STORED_BLOCK_MAX).peekable();
        while let Some(block) = blocks.next() {
            let is_last = blocks.peek().is_none();
            zlib.push(u8::from(is_last)); // BFINAL, BTYPE = 00 (stored)
            let length = block.len() as u16;
            zlib.extend_from_slice(&length.to_le_bytes());
            zlib.extend_from_slice(&(!length).to_le_bytes());
            zlib.extend_from_slice(block);
        }
        zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

        let mut png = Vec::with_capacity(zlib.len() + 64);
        png.extend_from_slice(&PNG_SIGNATURE);
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&(self.width as u32).to_be_bytes());
        header.extend_from_slice(&(self.height as u32).to_be_bytes());
        // 8 bits per channel, colour type 2 (RGB), deflate, filter 0, no interlace.
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        png_chunk(&mut png, b"IHDR", &header);
        png_chunk(&mut png, b"IDAT", &zlib);
        png_chunk(&mut png, b"IEND", &[]);
        png
    }

    /// The canvas as a binary PPM (`P6`) image.
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut bytes = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        for pixel in &self.pixels {
            bytes.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8]);
        }
        bytes
    }

    /// The canvas as a 24-bit uncompressed BMP image.
    pub fn to_bmp(&self) -> Vec<u8> {
        let row_bytes = (self.width * 3).next_multiple_of(4);
        let image_size = row_bytes * self.height;
        let mut bytes = Vec::with_capacity(54 + image_size);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&((54 + image_size) as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&(self.width as i32).to_le_bytes());
        bytes.extend_from_slice(&(self.height as i32).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&(image_size as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        for row in self.pixels.chunks(self.width).rev() {
            let start = bytes.len();
            for pixel in row {
                bytes.extend_from_slice(&[*pixel as u8, (pixel >> 8) as u8, (pixel >> 16) as u8]);
            }
            bytes.resize(start + row_bytes, 0);
        }
        bytes
    }
}

/// The image file formats a [`Canvas`] can be saved as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Ppm,
    Bmp,
    Png,
}

impl ImageFormat {
    /// The format selected by the file extension (case-insensitive:
    /// `.ppm`, `.bmp`, `.png`), or `None` for any other or no extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "ppm" => Some(Self::Ppm),
            "bmp" => Some(Self::Bmp),
            "png" => Some(Self::Png),
            _ => None,
        }
    }
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
/// Longest payload of one stored deflate block.
const STORED_BLOCK_MAX: usize = 65535;

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// The CRC-32 (ISO 3309, as used by PNG) of the concatenation of `parts`.
pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for byte in *part {
            crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
        }
    }
    !crc
}

/// The Adler-32 checksum of `bytes` (the zlib trailer).
pub fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65521;
    // The largest block for which the sums cannot overflow a u32.
    const BLOCK: usize = 5552;
    let (mut a, mut b) = (1u32, 0u32);
    for block in bytes.chunks(BLOCK) {
        for byte in block {
            a += u32::from(*byte);
            b += a;
        }
        a %= MODULUS;
        b %= MODULUS;
    }
    b << 16 | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

/// The 8x8 glyph of `ch`; bit 0 of each row byte is the leftmost pixel.
pub fn glyph(ch: char) -> &'static [u8; 8] {
    let index = if (' '..='~').contains(&ch) {
        ch as usize - 32
    } else {
        usize::from(b'?' - 32)
    };
    &FONT[index]
}

/// Public-domain 8x8 font ("font8x8_basic") for ASCII 32..=126.
#[rustfmt::skip]
const FONT: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // space
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // !
    [0x36, 0x36, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // "
    [0x36, 0x36, 0x7F, 0x36, 0x7F, 0x36, 0x36, 0x00], // #
    [0x0C, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x0C, 0x00], // $
    [0x00, 0x63, 0x33, 0x18, 0x0C, 0x66, 0x63, 0x00], // %
    [0x1C, 0x36, 0x1C, 0x6E, 0x3B, 0x33, 0x6E, 0x00], // &
    [0x06, 0x06, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00], // '
    [0x18, 0x0C, 0x06, 0x06, 0x06, 0x0C, 0x18, 0x00], // (
    [0x06, 0x0C, 0x18, 0x18, 0x18, 0x0C, 0x06, 0x00], // )
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // *
    [0x00, 0x0C, 0x0C, 0x3F, 0x0C, 0x0C, 0x00, 0x00], // +
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x06], // ,
    [0x00, 0x00, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x00], // -
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00], // .
    [0x60, 0x30, 0x18, 0x0C, 0x06, 0x03, 0x01, 0x00], // /
    [0x3E, 0x63, 0x73, 0x7B, 0x6F, 0x67, 0x3E, 0x00], // 0
    [0x0C, 0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x3F, 0x00], // 1
    [0x1E, 0x33, 0x30, 0x1C, 0x06, 0x33, 0x3F, 0x00], // 2
    [0x1E, 0x33, 0x30, 0x1C, 0x30, 0x33, 0x1E, 0x00], // 3
    [0x38, 0x3C, 0x36, 0x33, 0x7F, 0x30, 0x78, 0x00], // 4
    [0x3F, 0x03, 0x1F, 0x30, 0x30, 0x33, 0x1E, 0x00], // 5
    [0x1C, 0x06, 0x03, 0x1F, 0x33, 0x33, 0x1E, 0x00], // 6
    [0x3F, 0x33, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x00], // 7
    [0x1E, 0x33, 0x33, 0x1E, 0x33, 0x33, 0x1E, 0x00], // 8
    [0x1E, 0x33, 0x33, 0x3E, 0x30, 0x18, 0x0E, 0x00], // 9
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x00], // :
    [0x00, 0x0C, 0x0C, 0x00, 0x00, 0x0C, 0x0C, 0x06], // ;
    [0x18, 0x0C, 0x06, 0x03, 0x06, 0x0C, 0x18, 0x00], // <
    [0x00, 0x00, 0x3F, 0x00, 0x00, 0x3F, 0x00, 0x00], // =
    [0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00], // >
    [0x1E, 0x33, 0x30, 0x18, 0x0C, 0x00, 0x0C, 0x00], // ?
    [0x3E, 0x63, 0x7B, 0x7B, 0x7B, 0x03, 0x1E, 0x00], // @
    [0x0C, 0x1E, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x00], // A
    [0x3F, 0x66, 0x66, 0x3E, 0x66, 0x66, 0x3F, 0x00], // B
    [0x3C, 0x66, 0x03, 0x03, 0x03, 0x66, 0x3C, 0x00], // C
    [0x1F, 0x36, 0x66, 0x66, 0x66, 0x36, 0x1F, 0x00], // D
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x46, 0x7F, 0x00], // E
    [0x7F, 0x46, 0x16, 0x1E, 0x16, 0x06, 0x0F, 0x00], // F
    [0x3C, 0x66, 0x03, 0x03, 0x73, 0x66, 0x7C, 0x00], // G
    [0x33, 0x33, 0x33, 0x3F, 0x33, 0x33, 0x33, 0x00], // H
    [0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // I
    [0x78, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E, 0x00], // J
    [0x67, 0x66, 0x36, 0x1E, 0x36, 0x66, 0x67, 0x00], // K
    [0x0F, 0x06, 0x06, 0x06, 0x46, 0x66, 0x7F, 0x00], // L
    [0x63, 0x77, 0x7F, 0x7F, 0x6B, 0x63, 0x63, 0x00], // M
    [0x63, 0x67, 0x6F, 0x7B, 0x73, 0x63, 0x63, 0x00], // N
    [0x1C, 0x36, 0x63, 0x63, 0x63, 0x36, 0x1C, 0x00], // O
    [0x3F, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x0F, 0x00], // P
    [0x1E, 0x33, 0x33, 0x33, 0x3B, 0x1E, 0x38, 0x00], // Q
    [0x3F, 0x66, 0x66, 0x3E, 0x36, 0x66, 0x67, 0x00], // R
    [0x1E, 0x33, 0x07, 0x0E, 0x38, 0x33, 0x1E, 0x00], // S
    [0x3F, 0x2D, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // T
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x3F, 0x00], // U
    [0x33, 0x33, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // V
    [0x63, 0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x00], // W
    [0x63, 0x63, 0x36, 0x1C, 0x1C, 0x36, 0x63, 0x00], // X
    [0x33, 0x33, 0x33, 0x1E, 0x0C, 0x0C, 0x1E, 0x00], // Y
    [0x7F, 0x63, 0x31, 0x18, 0x4C, 0x66, 0x7F, 0x00], // Z
    [0x1E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x1E, 0x00], // [
    [0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00], // backslash
    [0x1E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1E, 0x00], // ]
    [0x08, 0x1C, 0x36, 0x63, 0x00, 0x00, 0x00, 0x00], // ^
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // _
    [0x0C, 0x0C, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00], // `
    [0x00, 0x00, 0x1E, 0x30, 0x3E, 0x33, 0x6E, 0x00], // a
    [0x07, 0x06, 0x06, 0x3E, 0x66, 0x66, 0x3B, 0x00], // b
    [0x00, 0x00, 0x1E, 0x33, 0x03, 0x33, 0x1E, 0x00], // c
    [0x38, 0x30, 0x30, 0x3E, 0x33, 0x33, 0x6E, 0x00], // d
    [0x00, 0x00, 0x1E, 0x33, 0x3F, 0x03, 0x1E, 0x00], // e
    [0x1C, 0x36, 0x06, 0x0F, 0x06, 0x06, 0x0F, 0x00], // f
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x1F], // g
    [0x07, 0x06, 0x36, 0x6E, 0x66, 0x66, 0x67, 0x00], // h
    [0x0C, 0x00, 0x0E, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // i
    [0x30, 0x00, 0x30, 0x30, 0x30, 0x33, 0x33, 0x1E], // j
    [0x07, 0x06, 0x66, 0x36, 0x1E, 0x36, 0x67, 0x00], // k
    [0x0E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x1E, 0x00], // l
    [0x00, 0x00, 0x33, 0x7F, 0x7F, 0x6B, 0x63, 0x00], // m
    [0x00, 0x00, 0x1F, 0x33, 0x33, 0x33, 0x33, 0x00], // n
    [0x00, 0x00, 0x1E, 0x33, 0x33, 0x33, 0x1E, 0x00], // o
    [0x00, 0x00, 0x3B, 0x66, 0x66, 0x3E, 0x06, 0x0F], // p
    [0x00, 0x00, 0x6E, 0x33, 0x33, 0x3E, 0x30, 0x78], // q
    [0x00, 0x00, 0x3B, 0x6E, 0x66, 0x06, 0x0F, 0x00], // r
    [0x00, 0x00, 0x3E, 0x03, 0x1E, 0x30, 0x1F, 0x00], // s
    [0x08, 0x0C, 0x3E, 0x0C, 0x0C, 0x2C, 0x18, 0x00], // t
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x33, 0x6E, 0x00], // u
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x1E, 0x0C, 0x00], // v
    [0x00, 0x00, 0x63, 0x6B, 0x7F, 0x7F, 0x36, 0x00], // w
    [0x00, 0x00, 0x63, 0x36, 0x1C, 0x36, 0x63, 0x00], // x
    [0x00, 0x00, 0x33, 0x33, 0x33, 0x3E, 0x30, 0x1F], // y
    [0x00, 0x00, 0x3F, 0x19, 0x0C, 0x26, 0x3F, 0x00], // z
    [0x38, 0x0C, 0x0C, 0x07, 0x0C, 0x0C, 0x38, 0x00], // {
    [0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x00], // |
    [0x07, 0x0C, 0x0C, 0x38, 0x0C, 0x0C, 0x07, 0x00], // }
    [0x6E, 0x3B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // ~
];

/// An input event reported by a graphics backend.
///
/// Key names are upper case: letters and digits as themselves (`"A"`, `"7"`),
/// punctuation as the character, and `SPACE`, `ENTER`, `ESC`, `TAB`,
/// `BACKSPACE`, `UP`, `DOWN`, `LEFT`, `RIGHT`, `HOME`, `END`, `PGUP`, `PGDN`,
/// `INS`, `DEL`, `F1`..`F12` for the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    KeyDown(String),
    KeyUp(String),
    /// The mouse moved or a button changed. `x`/`y` are canvas pixels and
    /// `buttons` is a bit mask: 1 left, 2 right, 4 middle.
    Mouse {
        x: i64,
        y: i64,
        buttons: u32,
    },
    /// The user asked to close the window.
    Close,
}

/// Where the VM sends frames and from where it receives input. A desktop
/// window implements this trait; the VM never blocks inside it except in
/// [`GraphicsBackend::sleep`].
pub trait GraphicsBackend {
    /// Opens (or re-opens) the output with a canvas of `width` x `height`.
    fn open(&mut self, width: usize, height: usize, title: &str) -> Result<(), String>;
    /// Like [`GraphicsBackend::open`], telling the backend how many pixel
    /// buffers the program uses (`SCREEN w, h, title$, buffers`: 1, or 2 for
    /// double buffering). The backend only ever receives *published* frames
    /// through [`GraphicsBackend::present`], so the default just opens.
    fn open_buffered(
        &mut self,
        width: usize,
        height: usize,
        title: &str,
        buffers: usize,
    ) -> Result<(), String> {
        let _ = buffers;
        self.open(width, height, title)
    }
    /// Closes the output (`SCREEN 0` or the end of the program).
    fn close(&mut self);
    /// Shows `canvas` (a complete frame; in double-buffered mode the buffer
    /// that was just published by `FLIP`).
    fn present(&mut self, canvas: &Canvas) -> Result<(), String>;
    /// Returns the events that arrived since the previous call, oldest
    /// first, without blocking.
    fn poll_events(&mut self) -> Vec<Event>;
    /// Pauses for `ms` milliseconds (frame pacing). A window backend should
    /// keep servicing its event loop meanwhile.
    fn sleep(&mut self, ms: u64);
}

impl GraphicsBackend for Box<dyn GraphicsBackend> {
    fn open(&mut self, width: usize, height: usize, title: &str) -> Result<(), String> {
        (**self).open(width, height, title)
    }
    fn open_buffered(
        &mut self,
        width: usize,
        height: usize,
        title: &str,
        buffers: usize,
    ) -> Result<(), String> {
        (**self).open_buffered(width, height, title, buffers)
    }
    fn close(&mut self) {
        (**self).close();
    }
    fn present(&mut self, canvas: &Canvas) -> Result<(), String> {
        (**self).present(canvas)
    }
    fn poll_events(&mut self) -> Vec<Event> {
        (**self).poll_events()
    }
    fn sleep(&mut self, ms: u64) {
        (**self).sleep(ms);
    }
}

/// Wraps a backend and reports [`Event::Close`] on every poll once a time
/// limit (measured from the last `open`) or a presented-frame limit is
/// reached. A bounded validation aid so automated runs of interactive
/// programs end; the program still decides what to do with the event.
pub struct AutoClose<B> {
    inner: B,
    after: Option<std::time::Duration>,
    max_frames: Option<u64>,
    opened_at: Option<std::time::Instant>,
    frames: u64,
}

impl<B: GraphicsBackend> AutoClose<B> {
    pub fn new(inner: B, after_ms: Option<u64>, max_frames: Option<u64>) -> Self {
        Self {
            inner,
            after: after_ms.map(std::time::Duration::from_millis),
            max_frames,
            opened_at: None,
            frames: 0,
        }
    }

    fn is_due(&self) -> bool {
        let timed_out = match (self.after, self.opened_at) {
            (Some(after), Some(opened)) => opened.elapsed() >= after,
            _ => false,
        };
        timed_out || self.max_frames.is_some_and(|max| self.frames >= max)
    }
}

impl<B: GraphicsBackend> GraphicsBackend for AutoClose<B> {
    fn open(&mut self, width: usize, height: usize, title: &str) -> Result<(), String> {
        self.open_buffered(width, height, title, 1)
    }

    fn open_buffered(
        &mut self,
        width: usize,
        height: usize,
        title: &str,
        buffers: usize,
    ) -> Result<(), String> {
        self.inner.open_buffered(width, height, title, buffers)?;
        self.opened_at = Some(std::time::Instant::now());
        self.frames = 0;
        Ok(())
    }

    fn close(&mut self) {
        self.inner.close();
    }

    fn present(&mut self, canvas: &Canvas) -> Result<(), String> {
        self.inner.present(canvas)?;
        self.frames += 1;
        Ok(())
    }

    fn poll_events(&mut self) -> Vec<Event> {
        let mut events = self.inner.poll_events();
        if self.is_due() {
            events.push(Event::Close);
        }
        events
    }

    fn sleep(&mut self, ms: u64) {
        self.inner.sleep(ms);
    }
}

/// What a [`HeadlessBackend`] recorded; shared by every clone.
#[derive(Debug, Default)]
pub struct HeadlessState {
    pub is_open: bool,
    pub opened: usize,
    pub title: String,
    pub size: (usize, usize),
    pub frames: usize,
    /// The most recently presented frame: what the display shows.
    pub last_frame: Option<Canvas>,
    /// The buffer count of the last open (1 or 2).
    pub buffers: usize,
    /// The last `history_limit` presented frames, oldest first.
    pub history: VecDeque<Canvas>,
    pub history_limit: usize,
    pub slept_ms: u64,
    pub polls: usize,
    /// Events returned (all at once) by the next poll.
    pub pending: Vec<Event>,
    /// One batch of events per poll, after `pending`.
    pub batches: VecDeque<Vec<Event>>,
}

/// A backend without a display: it stores the last presented frame, counts
/// frames, replays scripted events and never really sleeps. Clones share the
/// state, so a test keeps one clone and hands another to the VM.
#[derive(Debug, Clone, Default)]
pub struct HeadlessBackend {
    state: Rc<RefCell<HeadlessState>>,
}

impl HeadlessBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an event for the next poll.
    pub fn push_event(&self, event: Event) {
        self.state.borrow_mut().pending.push(event);
    }

    /// Queues events delivered by one poll of their own (after earlier ones).
    pub fn push_batch(&self, events: Vec<Event>) {
        self.state.borrow_mut().batches.push_back(events);
    }

    pub fn frames(&self) -> usize {
        self.state.borrow().frames
    }

    /// The front frame: the most recently presented one (what a display
    /// would show now), or `None` before anything was presented.
    pub fn last_frame(&self) -> Option<Canvas> {
        self.state.borrow().last_frame.clone()
    }

    /// Alias of [`HeadlessBackend::last_frame`] named after double buffering.
    pub fn front_frame(&self) -> Option<Canvas> {
        self.last_frame()
    }

    /// The buffer count given to the latest open (1 unless `SCREEN` asked
    /// for double buffering).
    pub fn buffers(&self) -> usize {
        self.state.borrow().buffers
    }

    /// Starts keeping copies of the last `limit` presented frames (default
    /// 0: none, because frames can be large). Retrieve them with
    /// [`HeadlessBackend::frame_history`].
    pub fn record_frames(&self, limit: usize) {
        let mut state = self.state.borrow_mut();
        state.history_limit = limit;
        while state.history.len() > limit {
            state.history.pop_front();
        }
    }

    /// The recorded presented frames, oldest first.
    pub fn frame_history(&self) -> Vec<Canvas> {
        self.state.borrow().history.iter().cloned().collect()
    }

    pub fn is_open(&self) -> bool {
        self.state.borrow().is_open
    }

    pub fn title(&self) -> String {
        self.state.borrow().title.clone()
    }

    pub fn size(&self) -> (usize, usize) {
        self.state.borrow().size
    }

    pub fn slept_ms(&self) -> u64 {
        self.state.borrow().slept_ms
    }

    pub fn polls(&self) -> usize {
        self.state.borrow().polls
    }

    pub fn opened(&self) -> usize {
        self.state.borrow().opened
    }
}

impl GraphicsBackend for HeadlessBackend {
    fn open(&mut self, width: usize, height: usize, title: &str) -> Result<(), String> {
        self.open_buffered(width, height, title, 1)
    }

    fn open_buffered(
        &mut self,
        width: usize,
        height: usize,
        title: &str,
        buffers: usize,
    ) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.is_open = true;
        state.opened += 1;
        state.size = (width, height);
        state.title = title.to_string();
        state.buffers = buffers;
        Ok(())
    }

    fn close(&mut self) {
        self.state.borrow_mut().is_open = false;
    }

    fn present(&mut self, canvas: &Canvas) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.frames += 1;
        state.last_frame = Some(canvas.clone());
        if state.history_limit > 0 {
            if state.history.len() == state.history_limit {
                state.history.pop_front();
            }
            state.history.push_back(canvas.clone());
        }
        Ok(())
    }

    fn poll_events(&mut self) -> Vec<Event> {
        let mut state = self.state.borrow_mut();
        state.polls += 1;
        let mut events = std::mem::take(&mut state.pending);
        if let Some(batch) = state.batches.pop_front() {
            events.extend(batch);
        }
        events
    }

    fn sleep(&mut self, ms: u64) {
        self.state.borrow_mut().slept_ms += ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_close_reports_close_after_the_frame_limit() {
        let headless = HeadlessBackend::new();
        let mut backend = AutoClose::new(headless.clone(), None, Some(2));
        backend.open(4, 4, "t").unwrap();
        let canvas = Canvas::new(4, 4).unwrap();
        backend.present(&canvas).unwrap();
        assert!(backend.poll_events().is_empty());
        backend.present(&canvas).unwrap();
        assert_eq!(backend.poll_events(), vec![Event::Close]);
        assert_eq!(backend.poll_events(), vec![Event::Close]);
        assert_eq!(headless.frames(), 2);
    }

    #[test]
    fn auto_close_reports_close_after_the_time_limit_only_once_open() {
        let mut backend = AutoClose::new(HeadlessBackend::new(), Some(0), None);
        assert!(backend.poll_events().is_empty());
        backend.open(4, 4, "t").unwrap();
        assert_eq!(backend.poll_events(), vec![Event::Close]);
    }

    #[test]
    fn font_covers_printable_ascii_and_falls_back_to_question_mark() {
        assert_eq!(FONT.len(), 95);
        assert_eq!(glyph('A')[0], 0x0C);
        assert_eq!(glyph('\u{e9}'), glyph('?'));
        assert_eq!(glyph(' '), &[0; 8]);
    }

    #[test]
    fn checksums_match_their_published_check_values() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
        // Long runs of 0xFF exercise the deferred modulo.
        let long = vec![0xFFu8; 100_000];
        let (mut a, mut b) = (1u64, 0u64);
        for byte in &long {
            a = (a + u64::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        assert_eq!(u64::from(adler32(&long)), b << 16 | a);
    }

    #[test]
    fn image_format_follows_the_extension() {
        use std::path::Path;
        assert_eq!(
            ImageFormat::from_path(Path::new("a.PNG")),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            ImageFormat::from_path(Path::new("dir/a.bmp")),
            Some(ImageFormat::Bmp)
        );
        assert_eq!(
            ImageFormat::from_path(Path::new("a.ppm")),
            Some(ImageFormat::Ppm)
        );
        assert_eq!(ImageFormat::from_path(Path::new("a.gif")), None);
        assert_eq!(ImageFormat::from_path(Path::new("png")), None);
    }

    #[test]
    fn bmp_and_ppm_headers_describe_the_canvas() {
        let mut canvas = Canvas::new(3, 2).unwrap();
        canvas.set_pixel(0, 0, 0x112233);
        assert!(canvas.to_ppm().starts_with(b"P6\n3 2\n255\n\x11\x22\x33"));
        let bmp = canvas.to_bmp();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 12 * 2);
        // The first row of the file is the bottom row of the image.
        assert_eq!(&bmp[54 + 12..54 + 15], &[0x33, 0x22, 0x11]);
    }
}
