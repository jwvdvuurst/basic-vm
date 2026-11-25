//! Helpers shared by the integration tests: unique scratch directories and
//! independent decoders for the image formats the crate writes (so the tests
//! do not trust the encoder's own checksum code).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// A unique scratch directory removed (with its contents) on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("basic-vm-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The path as text, for embedding in BASIC string literals.
    pub fn text(&self, name: &str) -> String {
        self.file(name).display().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn examples(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

/// Decodes a 16-bit mono PCM WAV file written by the crate, checking every
/// header field independently of the encoder. Returns `(sample_rate, samples)`.
pub fn decode_wav(data: &[u8]) -> (u32, Vec<i16>) {
    let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    assert_eq!(&data[0..4], b"RIFF");
    assert_eq!(u32_at(4) as usize, data.len() - 8, "RIFF size");
    assert_eq!(&data[8..16], b"WAVEfmt ");
    assert_eq!(u32_at(16), 16, "fmt chunk size");
    assert_eq!(u16_at(20), 1, "PCM");
    assert_eq!(u16_at(22), 1, "mono");
    let rate = u32_at(24);
    assert_eq!(u32_at(28), rate * 2, "byte rate");
    assert_eq!(u16_at(32), 2, "block align");
    assert_eq!(u16_at(34), 16, "bits per sample");
    assert_eq!(&data[36..40], b"data");
    let length = u32_at(40) as usize;
    assert_eq!(data.len(), 44 + length, "data size");
    let samples = (0..length / 2)
        .map(|index| u16_at(44 + 2 * index) as i16)
        .collect();
    (rate, samples)
}

/// A PCM WAV file with an arbitrary format (hand-built headers, independent of
/// the crate's own writer); `data` is the raw, already interleaved sample bytes.
pub fn pcm_wav(channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    let block = channels * bits / 8;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A sine of `hz` Hz lasting `ms` milliseconds at `rate` Hz, as 16-bit
/// samples with the given peak.
pub fn sine_samples(hz: f64, ms: u32, rate: u32, peak: f64) -> Vec<i16> {
    let count = u64::from(rate) * u64::from(ms) / 1000;
    (0..count)
        .map(|i| {
            (peak * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin()).round() as i16
        })
        .collect()
}

/// A decoded image: `pixels[y * width + x]` is `0xRRGGBB`.
#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Image {
    pub fn at(&self, x: usize, y: usize) -> u32 {
        self.pixels[y * self.width + x]
    }
}

fn rgb(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2])
}

pub fn decode_ppm(data: &[u8]) -> Image {
    let mut fields = Vec::new();
    let mut position = 0;
    // "P6", width, height, maxval: four whitespace-terminated tokens.
    while fields.len() < 4 {
        let start = position;
        while !data[position].is_ascii_whitespace() {
            position += 1;
        }
        fields.push(String::from_utf8(data[start..position].to_vec()).unwrap());
        position += 1;
    }
    assert_eq!(fields[0], "P6");
    assert_eq!(fields[3], "255");
    let (width, height): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    let body = &data[position..];
    assert_eq!(body.len(), width * height * 3);
    Image {
        width,
        height,
        pixels: body.chunks(3).map(rgb).collect(),
    }
}

pub fn decode_bmp(data: &[u8]) -> Image {
    assert_eq!(&data[0..2], b"BM");
    let width = i32::from_le_bytes(data[18..22].try_into().unwrap()) as usize;
    let height = i32::from_le_bytes(data[22..26].try_into().unwrap()) as usize;
    assert_eq!(u16::from_le_bytes(data[28..30].try_into().unwrap()), 24);
    let offset = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
    let row_bytes = (width * 3).next_multiple_of(4);
    assert_eq!(data.len(), offset + row_bytes * height);
    let mut pixels = vec![0; width * height];
    for row in 0..height {
        // Rows are stored bottom-up, pixels as BGR.
        let start = offset + (height - 1 - row) * row_bytes;
        for column in 0..width {
            let p = &data[start + column * 3..start + column * 3 + 3];
            pixels[row * width + column] = rgb(&[p[2], p[1], p[0]]);
        }
    }
    Image {
        width,
        height,
        pixels,
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

/// The chunks (type, data) of a PNG file after verifying the signature and
/// every chunk's CRC.
pub fn png_chunks(data: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(
        &data[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let mut chunks = Vec::new();
    let mut position = 8;
    while position < data.len() {
        let length = u32::from_be_bytes(data[position..position + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = data[position + 4..position + 8].try_into().unwrap();
        let body = &data[position + 8..position + 8 + length];
        let stored = u32::from_be_bytes(
            data[position + 8 + length..position + 12 + length]
                .try_into()
                .unwrap(),
        );
        let mut checked = kind.to_vec();
        checked.extend_from_slice(body);
        assert_eq!(
            stored,
            crc32(&checked),
            "bad CRC in chunk {}",
            String::from_utf8_lossy(&kind)
        );
        chunks.push((kind, body.to_vec()));
        position += 12 + length;
    }
    assert_eq!(position, data.len(), "trailing bytes after the last chunk");
    chunks
}

/// Decodes a zlib stream made only of stored deflate blocks, verifying the
/// header, block length complements and the Adler-32 trailer.
pub fn inflate_stored(zlib: &[u8]) -> Vec<u8> {
    assert_eq!(zlib[0] & 0x0F, 8, "deflate method");
    assert_eq!((u32::from(zlib[0]) * 256 + u32::from(zlib[1])) % 31, 0);
    assert_eq!(zlib[1] & 0x20, 0, "no preset dictionary");
    let mut out = Vec::new();
    let mut position = 2;
    loop {
        let header = zlib[position];
        assert_eq!(header & 0b110, 0, "block type must be stored");
        let length = u16::from_le_bytes(zlib[position + 1..position + 3].try_into().unwrap());
        let complement = u16::from_le_bytes(zlib[position + 3..position + 5].try_into().unwrap());
        assert_eq!(length, !complement);
        position += 5;
        out.extend_from_slice(&zlib[position..position + length as usize]);
        position += length as usize;
        if header & 1 == 1 {
            break;
        }
    }
    let trailer = u32::from_be_bytes(zlib[position..position + 4].try_into().unwrap());
    assert_eq!(trailer, adler32(&out), "Adler-32");
    assert_eq!(position + 4, zlib.len());
    out
}

pub fn decode_png(data: &[u8]) -> Image {
    let chunks = png_chunks(data);
    assert_eq!(&chunks[0].0, b"IHDR");
    assert_eq!(&chunks.last().unwrap().0, b"IEND");
    assert!(chunks.last().unwrap().1.is_empty());
    let header = &chunks[0].1;
    assert_eq!(header.len(), 13);
    let width = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
    let height = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    // 8 bits, colour type 2 (RGB), deflate, filter method 0, no interlace.
    assert_eq!(&header[8..], &[8, 2, 0, 0, 0]);
    let idat: Vec<u8> = chunks
        .iter()
        .filter(|(kind, _)| kind == b"IDAT")
        .flat_map(|(_, body)| body.clone())
        .collect();
    let raw = inflate_stored(&idat);
    assert_eq!(raw.len(), (width * 3 + 1) * height);
    let mut pixels = Vec::with_capacity(width * height);
    for row in raw.chunks(width * 3 + 1) {
        assert_eq!(row[0], 0, "filter type None");
        pixels.extend(row[1..].chunks(3).map(rgb));
    }
    Image {
        width,
        height,
        pixels,
    }
}

/// Decodes a file by its extension.
pub fn decode_file(path: &Path) -> Image {
    let data = std::fs::read(path).unwrap();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap()
        .to_ascii_lowercase();
    match extension.as_str() {
        "ppm" => decode_ppm(&data),
        "bmp" => decode_bmp(&data),
        "png" => decode_png(&data),
        other => panic!("unknown image extension {other}"),
    }
}
