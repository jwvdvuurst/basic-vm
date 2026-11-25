//! A defensive RIFF/WAVE reader for `PLAYWAV` and `WAVLENGTH`.
//!
//! Everything here is a pure function of the file's bytes, so it is tested
//! without a device or a file system. The reader never panics and never
//! allocates more than the documented caps allow, whatever the header claims.
//!
//! # What is accepted
//!
//! * Uncompressed PCM (format tag 1) of 8-bit unsigned, 16-bit, 24-bit and
//!   32-bit signed samples, and `WAVE_FORMAT_EXTENSIBLE` (tag `0xFFFE`) whose
//!   sub-format is PCM. Floating-point, ADPCM, A-law/µ-law, MP3 and every other
//!   format is refused with an error that names the format.
//! * 1 to [`MAX_WAV_CHANNELS`] channels, any sample rate from 1 Hz up.
//! * Chunks in any order, with unknown chunks (`LIST`, `fact`, `cue `, ...)
//!   skipped and the pad byte after odd-sized chunks honoured. The RIFF size
//!   field is ignored (many writers get it wrong) and a `data` chunk that
//!   claims more bytes than the file holds is clamped to what is there, which
//!   also covers streamed files with a size of `0xFFFFFFFF`. A trailing
//!   partial frame is dropped.
//!
//! # Conversion to the playback format
//!
//! The playback format is mono 16-bit [`SAMPLE_RATE`]. Samples are first
//! converted to the 16-bit scale (24- and 32-bit samples are rounded to the
//! nearest 16-bit value, 8-bit samples are `(byte - 128) * 256`), the channels
//! of a frame are averaged (rounding half away from zero), and the result is
//! resampled to 44.1 kHz with linear interpolation in exact integer
//! arithmetic (rounding half away from zero). There is no anti-aliasing
//! filter, so downsampling material above 22 kHz folds back; this is meant
//! for sound effects and music clips, not for mastering.

use std::fmt;

use crate::audio::{MAX_QUEUE_SAMPLES, SAMPLE_RATE};

/// Largest WAV file `PLAYWAV` and `WAVLENGTH` read, in bytes (32 MiB).
pub const MAX_WAV_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Most channels a WAV file may have.
pub const MAX_WAV_CHANNELS: u16 = 64;

/// Why a WAV file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavError {
    message: String,
}

impl WavError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The human-readable reason.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WavError {}

type Result<T> = std::result::Result<T, WavError>;

/// What the headers of a WAV file say about its audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    /// Sample rate of the file, in Hz.
    pub sample_rate: u32,
    pub channels: u16,
    /// Bits per sample of the file (8, 16, 24 or 32).
    pub bits_per_sample: u16,
    /// Whole frames (one sample per channel) in the data that is present.
    pub frames: u64,
}

impl WavInfo {
    /// The length of the file's own audio in milliseconds, rounded to the
    /// nearest (what `WAVLENGTH` reports).
    pub fn duration_ms(&self) -> u64 {
        let ms = (u128::from(self.frames) * 1000 + u128::from(self.sample_rate) / 2)
            / u128::from(self.sample_rate);
        ms as u64
    }

    /// The number of samples after conversion to [`SAMPLE_RATE`], rounded to
    /// the nearest.
    pub fn output_samples(&self) -> u64 {
        let rate = u128::from(self.sample_rate);
        ((u128::from(self.frames) * u128::from(SAMPLE_RATE) + rate / 2) / rate) as u64
    }
}

/// The sub-format GUID of every standard format tag, after the 2-byte tag
/// (`xxxxxxxx-0000-0010-8000-00AA00389B71`).
const GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

const TAG_PCM: u16 = 1;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

fn format_name(tag: u16) -> String {
    match tag {
        0x0002 => "Microsoft ADPCM".into(),
        0x0003 => "IEEE floating point".into(),
        0x0006 => "A-law".into(),
        0x0007 => "µ-law".into(),
        0x0011 => "IMA ADPCM".into(),
        0x0055 => "MPEG layer 3 (MP3)".into(),
        0x00FF | 0x1610 => "AAC".into(),
        0x0160..=0x0163 => "Windows Media Audio".into(),
        other => format!("format tag 0x{other:04X}"),
    }
}

/// The parsed `fmt ` chunk.
#[derive(Debug, Clone, Copy)]
struct Format {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
}

fn parse_format(body: &[u8]) -> Result<Format> {
    if body.len() < 16 {
        return Err(WavError::new(format!(
            "the fmt chunk is too short ({} bytes, at least 16 needed)",
            body.len()
        )));
    }
    let u16_at = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]);
    let mut tag = u16_at(0);
    let channels = u16_at(2);
    let sample_rate = u32_at(4);
    let bits_per_sample = u16_at(14);
    if tag == TAG_EXTENSIBLE {
        // cbSize, valid bits, channel mask, then the sub-format GUID.
        if body.len() < 40 {
            return Err(WavError::new(
                "the extensible fmt chunk is too short (40 bytes needed)",
            ));
        }
        if body[26..40] != GUID_TAIL {
            return Err(WavError::new(
                "unsupported WAVE_FORMAT_EXTENSIBLE sub-format (only PCM is supported)",
            ));
        }
        tag = u16_at(24);
    }
    if tag != TAG_PCM {
        return Err(WavError::new(format!(
            "unsupported WAV format: {} (only uncompressed PCM is supported)",
            format_name(tag)
        )));
    }
    if channels == 0 || channels > MAX_WAV_CHANNELS {
        return Err(WavError::new(format!(
            "unsupported channel count {channels} (1 to {MAX_WAV_CHANNELS} channels are supported)"
        )));
    }
    if sample_rate == 0 {
        return Err(WavError::new("the sample rate is 0 Hz"));
    }
    if !matches!(bits_per_sample, 8 | 16 | 24 | 32) {
        return Err(WavError::new(format!(
            "unsupported sample size of {bits_per_sample} bits (8, 16, 24 and 32 are supported)"
        )));
    }
    Ok(Format {
        channels,
        sample_rate,
        bits_per_sample,
    })
}

/// The headers of a file and where its sample data lies.
struct Layout {
    info: WavInfo,
    data_start: usize,
    data_end: usize,
}

/// Walks the chunks of `bytes` and returns the format and the position of the
/// data. Never reads outside `bytes`.
fn parse_layout(bytes: &[u8]) -> Result<Layout> {
    if bytes.len() < 12 {
        return Err(WavError::new(format!(
            "too short to be a WAV file ({} bytes)",
            bytes.len()
        )));
    }
    match &bytes[0..4] {
        b"RIFF" => {}
        b"RIFX" => return Err(WavError::new("big-endian RIFX files are not supported")),
        b"RF64" | b"BW64" => {
            return Err(WavError::new(
                "RF64/BW64 (64-bit) WAV files are not supported",
            ));
        }
        _ => return Err(WavError::new("not a WAV file (no RIFF header)")),
    }
    if &bytes[8..12] != b"WAVE" {
        return Err(WavError::new("not a WAV file (the RIFF type is not WAVE)"));
    }
    let length = bytes.len() as u64;
    let mut format: Option<Format> = None;
    let mut data: Option<(usize, usize)> = None;
    let mut position = 12u64;
    while position + 8 <= length {
        let at = position as usize;
        let id = &bytes[at..at + 4];
        let size = u64::from(u32::from_le_bytes([
            bytes[at + 4],
            bytes[at + 5],
            bytes[at + 6],
            bytes[at + 7],
        ]));
        let body = position + 8;
        let available = length - body;
        let clamped = size.min(available);
        match id {
            b"fmt " if format.is_none() => {
                if size > available {
                    return Err(WavError::new("the fmt chunk is truncated"));
                }
                format = Some(parse_format(&bytes[body as usize..(body + size) as usize])?);
            }
            b"data" if data.is_none() => {
                data = Some((body as usize, (body + clamped) as usize));
            }
            _ => {}
        }
        if format.is_some() && data.is_some() {
            break;
        }
        // Chunks are padded to an even size; the pad is not part of the size.
        position = body.saturating_add(size).saturating_add(size & 1);
    }
    let Some(format) = format else {
        return Err(WavError::new("no fmt chunk (truncated or not a WAV file)"));
    };
    let Some((data_start, data_end)) = data else {
        return Err(WavError::new("no data chunk (truncated file)"));
    };
    let frame_bytes = usize::from(format.channels) * usize::from(format.bits_per_sample / 8);
    Ok(Layout {
        info: WavInfo {
            sample_rate: format.sample_rate,
            channels: format.channels,
            bits_per_sample: format.bits_per_sample,
            frames: ((data_end - data_start) / frame_bytes) as u64,
        },
        data_start,
        data_end,
    })
}

/// What the headers of `bytes` say, without decoding any audio.
pub fn wav_info(bytes: &[u8]) -> Result<WavInfo> {
    parse_layout(bytes).map(|layout| layout.info)
}

/// `numerator / denominator` rounded to the nearest, halves away from zero.
fn div_round(numerator: i64, denominator: i64) -> i64 {
    let half = denominator / 2;
    if numerator >= 0 {
        (numerator + half) / denominator
    } else {
        -((-numerator + half) / denominator)
    }
}

/// One sample on the 16-bit scale.
fn sample_16(bytes: &[u8], bits: u16) -> i32 {
    match bits {
        8 => (i32::from(bytes[0]) - 128) << 8,
        16 => i32::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        24 => {
            let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
            ((value + 128) >> 8).min(i32::from(i16::MAX))
        }
        _ => {
            let value = i64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            ((value + 32_768) >> 16).min(i64::from(i16::MAX)) as i32
        }
    }
}

/// The frames of the data as mono 16-bit samples at the file's own rate.
fn downmix(data: &[u8], format: WavInfo) -> Vec<i16> {
    let bytes_per_sample = usize::from(format.bits_per_sample / 8);
    let channels = usize::from(format.channels);
    let frame_bytes = bytes_per_sample * channels;
    data.chunks_exact(frame_bytes)
        .map(|frame| {
            let sum: i64 = frame
                .chunks_exact(bytes_per_sample)
                .map(|sample| i64::from(sample_16(sample, format.bits_per_sample)))
                .sum();
            div_round(sum, i64::from(format.channels)) as i16
        })
        .collect()
}

/// Linear-interpolation resampling of `mono` from `rate` to [`SAMPLE_RATE`]
/// with `count` output samples. Output sample `i` sits at source position
/// `i * rate / SAMPLE_RATE`; the end of the clip repeats its last sample.
fn resample(mono: &[i16], rate: u32, count: usize) -> Vec<i16> {
    if mono.is_empty() {
        return Vec::new();
    }
    if rate == SAMPLE_RATE {
        return mono.to_vec();
    }
    let last = mono.len() - 1;
    let output_rate = u64::from(SAMPLE_RATE);
    (0..count as u64)
        .map(|index| {
            let position = index * u64::from(rate);
            let whole = (position / output_rate) as usize;
            let fraction = (position % output_rate) as i64;
            let before = i64::from(mono[whole.min(last)]);
            let after = i64::from(mono[(whole + 1).min(last)]);
            let weighted = before * (output_rate as i64 - fraction) + after * fraction;
            div_round(weighted, output_rate as i64) as i16
        })
        .collect()
}

/// Decodes a WAV file into mono 16-bit samples at [`SAMPLE_RATE`] (see the
/// module documentation for the exact conversion).
///
/// A file whose converted audio would be longer than the playback queue
/// ([`MAX_QUEUE_SAMPLES`], 10 minutes) is an error; this is checked before
/// anything is allocated for the result.
pub fn decode_wav_samples(bytes: &[u8]) -> Result<Vec<i16>> {
    let layout = parse_layout(bytes)?;
    let info = layout.info;
    let count = info.output_samples();
    if count > MAX_QUEUE_SAMPLES {
        return Err(WavError::new(format!(
            "too long to play ({} seconds; the limit is {} seconds)",
            info.duration_ms().div_ceil(1000),
            MAX_QUEUE_SAMPLES / u64::from(SAMPLE_RATE)
        )));
    }
    let mono = downmix(&bytes[layout.data_start..layout.data_end], info);
    Ok(resample(&mono, info.sample_rate, count as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::wav_bytes;

    /// Builds a file from a list of chunks, each padded to an even length.
    fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        for (id, data) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn fmt_chunk(tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let block = channels * bits.div_ceil(8);
        let mut out = Vec::new();
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out
    }

    fn extensible_chunk(sub_tag: u16, channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let mut out = fmt_chunk(TAG_EXTENSIBLE, channels, rate, bits);
        out.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        out.extend_from_slice(&bits.to_le_bytes()); // valid bits
        out.extend_from_slice(&3u32.to_le_bytes()); // channel mask
        out.extend_from_slice(&sub_tag.to_le_bytes());
        out.extend_from_slice(&GUID_TAIL);
        out
    }

    fn pcm16(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn wav16(channels: u16, rate: u32, values: &[i16]) -> Vec<u8> {
        riff(&[
            (b"fmt ", fmt_chunk(1, channels, rate, 16)),
            (b"data", pcm16(values)),
        ])
    }

    fn decode(bytes: &[u8]) -> Vec<i16> {
        decode_wav_samples(bytes).unwrap()
    }

    fn error_of(bytes: &[u8]) -> String {
        decode_wav_samples(bytes).unwrap_err().to_string()
    }

    // --- formats ---

    #[test]
    fn the_crate_s_own_wav_files_round_trip_exactly() {
        let samples: Vec<i16> = (0..1000)
            .map(|i| (i * 37 % 20_000 - 10_000) as i16)
            .collect();
        assert_eq!(decode(&wav_bytes(&samples)), samples);
        let info = wav_info(&wav_bytes(&samples)).unwrap();
        assert_eq!(info.sample_rate, 44_100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.frames, 1000);
        assert!(decode(&wav_bytes(&[])).is_empty());
    }

    #[test]
    fn eight_bit_samples_are_unsigned_with_silence_at_128() {
        let bytes = riff(&[
            (b"fmt ", fmt_chunk(1, 1, 44_100, 8)),
            (b"data", vec![128, 255, 0, 129, 127]),
        ]);
        assert_eq!(decode(&bytes), vec![0, 127 << 8, -128 << 8, 256, -256]);
    }

    #[test]
    fn twenty_four_bit_samples_round_to_sixteen_bits() {
        let samples: [i32; 6] = [0, 0x7F_FFFF, -0x80_0000, 0x00_0080, 0x00_007F, -0x00_0080];
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| s.to_le_bytes()[..3].to_vec())
            .collect();
        let bytes = riff(&[(b"fmt ", fmt_chunk(1, 1, 44_100, 24)), (b"data", data)]);
        // 0x7FFFFF rounds up to 32768, which clamps to 32767.
        assert_eq!(decode(&bytes), vec![0, 32_767, -32_768, 1, 0, 0]);
    }

    #[test]
    fn thirty_two_bit_samples_round_to_sixteen_bits() {
        let samples: [i32; 6] = [0, i32::MAX, i32::MIN, 0x8000, 0x7FFF, 0x0001_2345];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let bytes = riff(&[(b"fmt ", fmt_chunk(1, 1, 44_100, 32)), (b"data", data)]);
        assert_eq!(decode(&bytes), vec![0, 32_767, -32_768, 1, 0, 1]);
    }

    #[test]
    fn sixteen_bit_samples_are_kept_exactly() {
        let values = [0, 1, -1, 32_767, -32_768, 12_345];
        assert_eq!(decode(&wav16(1, 44_100, &values)), values);
    }

    #[test]
    fn extensible_pcm_is_accepted() {
        let bytes = riff(&[
            (b"fmt ", extensible_chunk(1, 1, 44_100, 16)),
            (b"data", pcm16(&[5, -6, 7])),
        ]);
        assert_eq!(decode(&bytes), vec![5, -6, 7]);
        assert_eq!(wav_info(&bytes).unwrap().bits_per_sample, 16);
    }

    #[test]
    fn extensible_float_and_foreign_guids_are_refused() {
        let float = riff(&[
            (b"fmt ", extensible_chunk(3, 1, 44_100, 32)),
            (b"data", vec![0; 8]),
        ]);
        assert!(
            error_of(&float).contains("IEEE floating point"),
            "{}",
            error_of(&float)
        );
        let mut foreign = extensible_chunk(1, 1, 44_100, 16);
        foreign[30] ^= 0xFF;
        let bytes = riff(&[(b"fmt ", foreign), (b"data", vec![0; 4])]);
        assert!(
            error_of(&bytes).contains("sub-format"),
            "{}",
            error_of(&bytes)
        );
        // The extensible header must be complete.
        let mut short = extensible_chunk(1, 1, 44_100, 16);
        short.truncate(30);
        let bytes = riff(&[(b"fmt ", short), (b"data", vec![0; 4])]);
        assert!(
            error_of(&bytes).contains("extensible"),
            "{}",
            error_of(&bytes)
        );
    }

    #[test]
    fn unsupported_formats_are_named() {
        for (tag, name) in [
            (3u16, "IEEE floating point"),
            (2, "Microsoft ADPCM"),
            (0x11, "IMA ADPCM"),
            (6, "A-law"),
            (7, "µ-law"),
            (0x55, "MP3"),
            (0x1234, "0x1234"),
        ] {
            let bytes = riff(&[
                (b"fmt ", fmt_chunk(tag, 1, 44_100, 16)),
                (b"data", vec![0; 8]),
            ]);
            let message = error_of(&bytes);
            assert!(message.contains(name), "{tag}: {message}");
            assert!(message.contains("only uncompressed PCM"), "{message}");
        }
    }

    #[test]
    fn odd_bit_depths_zero_channels_and_zero_rates_are_refused() {
        for bits in [0u16, 4, 12, 20, 48, 64] {
            let bytes = riff(&[
                (b"fmt ", fmt_chunk(1, 1, 44_100, bits)),
                (b"data", vec![0; 8]),
            ]);
            assert!(error_of(&bytes).contains("sample size"), "{bits}");
        }
        let bytes = riff(&[
            (b"fmt ", fmt_chunk(1, 0, 44_100, 16)),
            (b"data", vec![0; 8]),
        ]);
        assert!(error_of(&bytes).contains("channel count 0"));
        let bytes = riff(&[
            (b"fmt ", fmt_chunk(1, 65, 44_100, 16)),
            (b"data", vec![0; 8]),
        ]);
        assert!(error_of(&bytes).contains("channel count 65"));
        let bytes = riff(&[(b"fmt ", fmt_chunk(1, 1, 0, 16)), (b"data", vec![0; 8])]);
        assert!(error_of(&bytes).contains("0 Hz"));
    }

    // --- channels ---

    #[test]
    fn stereo_is_averaged_with_rounding_away_from_zero() {
        // (10, 20) -> 15; (1, 2) -> 1.5 -> 2; (-1, -2) -> -1.5 -> -2;
        // (-3, 4) -> 0.5 -> 1; (32767, 32767) -> 32767.
        let bytes = wav16(2, 44_100, &[10, 20, 1, 2, -1, -2, -3, 4, 32_767, 32_767]);
        assert_eq!(decode(&bytes), vec![15, 2, -2, 1, 32_767]);
        assert_eq!(wav_info(&bytes).unwrap().frames, 5);
    }

    #[test]
    fn many_channels_are_averaged() {
        let bytes = wav16(4, 44_100, &[100, 200, 300, 400, -4, -4, -4, -4]);
        assert_eq!(decode(&bytes), vec![250, -4]);
        let six = wav16(6, 44_100, &[6, 0, 0, 0, 0, 0]);
        assert_eq!(decode(&six), vec![1]);
    }

    #[test]
    fn a_trailing_partial_frame_is_dropped() {
        let mut bytes = wav16(2, 44_100, &[1, 1, 2, 2]);
        // Add three stray bytes to the data chunk (and fix its size).
        bytes.extend_from_slice(&[9, 9, 9]);
        let size_at = bytes.len() - 3 - 8 - 4;
        bytes[size_at..size_at + 4].copy_from_slice(&11u32.to_le_bytes());
        assert_eq!(decode(&bytes), vec![1, 2]);
    }

    // --- resampling ---

    #[test]
    fn the_output_length_follows_the_rate_ratio() {
        for (rate, frames, expected) in [
            (44_100u32, 441usize, 441usize),
            (22_050, 100, 200),
            (11_025, 100, 400),
            (8_000, 8_000, 44_100),
            (48_000, 480, 441),
            (96_000, 960, 441),
            (48_000, 1, 1),
            (48_000, 0, 0),
        ] {
            let bytes = wav16(1, rate, &vec![1000; frames]);
            assert_eq!(decode(&bytes).len(), expected, "{rate} Hz, {frames} frames");
            assert_eq!(
                wav_info(&bytes).unwrap().output_samples(),
                expected as u64,
                "{rate} Hz"
            );
        }
    }

    #[test]
    fn upsampling_interpolates_linearly() {
        // 22.05 kHz -> 44.1 kHz: every other sample is the average.
        let bytes = wav16(1, 22_050, &[0, 100, 300, 301]);
        assert_eq!(decode(&bytes), vec![0, 50, 100, 200, 300, 301, 301, 301]);
    }

    #[test]
    fn a_constant_signal_stays_constant_at_any_rate() {
        for rate in [8_000, 11_025, 22_050, 32_000, 48_000, 96_000, 192_000, 1] {
            let samples = decode(&wav16(1, rate, &vec![-1234; 5000.min(rate as usize * 2)]));
            assert!(samples.iter().all(|s| *s == -1234), "{rate} Hz");
        }
    }

    #[test]
    fn downsampling_picks_interpolated_positions() {
        // 88.2 kHz -> 44.1 kHz: exactly every second sample.
        let bytes = wav16(1, 88_200, &[10, 20, 30, 40, 50, 60]);
        assert_eq!(decode(&bytes), vec![10, 30, 50]);
        // 48 kHz: output 1 sits at source position 48000/44100 = 1.088...
        let bytes = wav16(1, 48_000, &[0, 1000, 2000, 3000]);
        let out = decode(&bytes);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 1088); // 1000 + 0.0884 * 1000 -> 1088
    }

    #[test]
    fn a_sine_keeps_its_frequency_and_level_when_resampled() {
        let rate = 48_000u32;
        let values: Vec<i16> = (0..4800)
            .map(|i| {
                (10_000.0 * (std::f64::consts::TAU * 1000.0 * f64::from(i) / f64::from(rate)).sin())
                    as i16
            })
            .collect();
        let out = decode(&wav16(1, rate, &values));
        assert_eq!(out.len(), 4410);
        // 1 kHz for 100 ms: 100 cycles = 200 sign changes.
        let crossings = out
            .windows(2)
            .filter(|w| (w[0] < 0) != (w[1] < 0) && w[0] != 0)
            .count();
        assert!((198..=201).contains(&crossings), "{crossings}");
        let peak = out.iter().map(|s| i32::from(*s).abs()).max().unwrap();
        assert!((9_900..=10_000).contains(&peak), "{peak}");
    }

    // --- chunk walking ---

    #[test]
    fn unknown_chunks_are_skipped_before_and_after_the_data() {
        let bytes = riff(&[
            (b"LIST", b"INFOISFT\x05\0\0\0Lavf\0".to_vec()),
            (b"fmt ", fmt_chunk(1, 1, 44_100, 16)),
            (b"fact", vec![1, 0, 0, 0]),
            (b"junk", vec![7; 3]), // odd size: followed by a pad byte
            (b"data", pcm16(&[4, 5, 6])),
            (b"cue ", vec![0; 10]),
        ]);
        assert_eq!(decode(&bytes), vec![4, 5, 6]);
    }

    #[test]
    fn an_odd_sized_data_chunk_is_followed_by_a_pad_byte() {
        // 8-bit mono with an odd length, then another chunk after the pad.
        let bytes = riff(&[
            (b"fmt ", fmt_chunk(1, 1, 44_100, 8)),
            (b"data", vec![128, 129, 130]),
            (b"LIST", vec![0; 4]),
        ]);
        assert_eq!(decode(&bytes), vec![0, 256, 512]);
    }

    #[test]
    fn data_before_fmt_is_found() {
        let bytes = riff(&[
            (b"data", pcm16(&[1, 2])),
            (b"fmt ", fmt_chunk(1, 1, 44_100, 16)),
        ]);
        assert_eq!(decode(&bytes), vec![1, 2]);
    }

    #[test]
    fn the_first_fmt_and_data_chunks_win() {
        let bytes = riff(&[
            (b"fmt ", fmt_chunk(1, 1, 44_100, 16)),
            (b"data", pcm16(&[1, 2])),
            (b"data", pcm16(&[3, 4])),
        ]);
        assert_eq!(decode(&bytes), vec![1, 2]);
    }

    #[test]
    fn the_riff_size_field_is_ignored() {
        for claimed in [0u32, 4, 0xFFFF_FFFF] {
            let mut bytes = wav16(1, 44_100, &[1, 2, 3]);
            bytes[4..8].copy_from_slice(&claimed.to_le_bytes());
            assert_eq!(decode(&bytes), vec![1, 2, 3], "{claimed}");
        }
    }

    #[test]
    fn a_data_size_larger_than_the_file_is_clamped() {
        for claimed in [1_000u32, 0xFFFF_FFFF, 0x7FFF_FFFF] {
            let mut bytes = wav16(1, 44_100, &[1, 2, 3]);
            let size_at = bytes.len() - 6 - 4;
            bytes[size_at..size_at + 4].copy_from_slice(&claimed.to_le_bytes());
            assert_eq!(decode(&bytes), vec![1, 2, 3], "{claimed}");
        }
    }

    #[test]
    fn a_data_size_of_zero_is_an_empty_clip() {
        let bytes = wav16(1, 44_100, &[]);
        assert!(decode(&bytes).is_empty());
        let info = wav_info(&bytes).unwrap();
        assert_eq!((info.frames, info.duration_ms()), (0, 0));
    }

    #[test]
    fn a_zero_sized_unknown_chunk_does_not_stall_the_walk() {
        let bytes = riff(&[
            (b"zero", vec![]),
            (b"fmt ", fmt_chunk(1, 1, 44_100, 16)),
            (b"zero", vec![]),
            (b"data", pcm16(&[9])),
        ]);
        assert_eq!(decode(&bytes), vec![9]);
    }

    // --- bad input ---

    #[test]
    fn garbage_and_truncated_files_are_errors_not_panics() {
        assert!(error_of(b"").contains("too short"));
        assert!(error_of(b"RIFF").contains("too short"));
        assert!(error_of(b"RIFF\0\0\0\0WAVE").contains("no fmt chunk"));
        assert!(error_of(b"this is certainly not a wave file").contains("no RIFF header"));
        assert!(error_of(b"RIFF\x10\0\0\0AVI LIST\0\0\0\0").contains("not WAVE"));
        assert!(error_of(b"RIFX\0\0\0\0WAVE").contains("big-endian"));
        assert!(error_of(b"RF64\0\0\0\0WAVE").contains("RF64"));
        assert!(error_of(&[0xFF; 100]).contains("no RIFF header"));
        // A valid file cut at every length: never a panic, and an error until
        // the data chunk header has been read.
        let bytes = wav16(2, 22_050, &[1, 2, 3, 4, 5, 6]);
        for cut in 0..bytes.len() {
            let result = decode_wav_samples(&bytes[..cut]);
            if cut < 44 {
                assert!(result.is_err(), "cut at {cut}");
            }
        }
        let message = error_of(&bytes[..30]);
        assert!(message.contains("fmt chunk is truncated"), "{message}");
        let message = error_of(&bytes[..36]);
        assert!(message.contains("no data chunk"), "{message}");
        // Cut inside the data: what is there plays.
        // One stereo frame at 22.05 kHz becomes two samples at 44.1 kHz.
        assert_eq!(decode_wav_samples(&bytes[..44 + 4]).unwrap().len(), 2);
    }

    #[test]
    fn every_corruption_of_the_header_is_survivable() {
        let original = wav16(2, 22_050, &[1, 2, 3, 4, 5, 6, 7, 8]);
        for index in 0..44 {
            for value in [0x00u8, 0x01, 0x7F, 0x80, 0xFF] {
                let mut bytes = original.clone();
                bytes[index] = value;
                // Whatever happens, there is no panic and no huge result.
                if let Ok(samples) = decode_wav_samples(&bytes) {
                    assert!(samples.len() <= MAX_QUEUE_SAMPLES as usize);
                }
            }
        }
    }

    #[test]
    fn chunk_sizes_near_the_integer_limits_do_not_overflow() {
        for size in [
            0xFFFF_FFFFu32,
            0xFFFF_FFFE,
            0x8000_0000,
            0x7FFF_FFFF,
            0xFFFF_FFF0,
        ] {
            let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
            bytes.extend_from_slice(b"junk");
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend_from_slice(&[0; 40]);
            assert!(decode_wav_samples(&bytes).is_err());
            let mut bytes = wav16(1, 44_100, &[1, 2]);
            bytes.extend_from_slice(b"junk");
            bytes.extend_from_slice(&size.to_le_bytes());
            assert_eq!(decode(&bytes), vec![1, 2]);
        }
    }

    // --- caps ---

    #[test]
    fn audio_longer_than_the_queue_is_refused_before_allocating() {
        // One hour of 8 kHz audio in the header; only a little data present
        // is not a problem, but what is present is far over the cap here.
        let seconds = 601u64;
        let rate = 8_000u32;
        let data = vec![128u8; (seconds * u64::from(rate)) as usize];
        let bytes = riff(&[(b"fmt ", fmt_chunk(1, 1, rate, 8)), (b"data", data)]);
        let message = error_of(&bytes);
        assert!(message.contains("too long to play"), "{message}");
        assert!(message.contains("601 seconds"), "{message}");
        assert!(message.contains("limit is 600 seconds"), "{message}");
        // WAVLENGTH-style queries still work on it.
        assert_eq!(wav_info(&bytes).unwrap().duration_ms(), 601_000);
        // Exactly ten minutes is fine.
        let data = vec![128u8; (600 * u64::from(rate)) as usize];
        let bytes = riff(&[(b"fmt ", fmt_chunk(1, 1, rate, 8)), (b"data", data)]);
        assert_eq!(decode(&bytes).len() as u64, MAX_QUEUE_SAMPLES);
    }

    #[test]
    fn a_tiny_sample_rate_cannot_inflate_a_small_file() {
        // 1 Hz with 1000 frames would be 44.1 million samples.
        let bytes = wav16(1, 1, &vec![1; 1000]);
        assert!(error_of(&bytes).contains("too long to play"));
        let bytes = wav16(1, 1, &[1, 2]);
        assert_eq!(decode(&bytes).len(), 88_200);
    }

    // --- durations ---

    #[test]
    fn durations_are_reported_in_rounded_milliseconds() {
        let info = |rate, frames| WavInfo {
            sample_rate: rate,
            channels: 1,
            bits_per_sample: 16,
            frames,
        };
        assert_eq!(info(44_100, 44_100).duration_ms(), 1000);
        assert_eq!(info(44_100, 22).duration_ms(), 0);
        assert_eq!(info(44_100, 23).duration_ms(), 1);
        assert_eq!(info(8_000, 4_000).duration_ms(), 500);
        assert_eq!(info(48_000, 1).duration_ms(), 0);
        assert_eq!(info(1, 3).duration_ms(), 3000);
        assert_eq!(info(u32::MAX, u64::MAX).duration_ms(), 4_294_967_297_000);
    }
}
