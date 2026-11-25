//! Sound core: a deterministic software synthesizer, the Music Macro Language
//! (`PLAY`), and the audio backend abstraction behind `BEEP`, `SOUND`, `PLAY`
//! and friends.
//!
//! The design mirrors the graphics side. The VM turns statements into a list of
//! [`AudioEvent`]s (tones and rests) and hands them to an [`AudioBackend`],
//! which queues them for playback. [`HeadlessAudio`] is the deterministic
//! backend used by tests: it records the events and runs a *virtual clock*, so
//! `SOUNDPLAYING()`, `SOUNDQUEUE()` and `SOUNDWAIT` are fully reproducible and
//! nothing ever has to wait. [`NullAudio`] is the same virtual queue without any
//! recording (the VM's default). A real device backend implements the same trait
//! and renders the events with [`render`].
//!
//! # Audio format
//!
//! [`SAMPLE_RATE`] is 44 100 Hz, mono, signed 16-bit PCM. A time is stored as a
//! whole number of *samples* (never as floating point milliseconds), so music
//! timing is exact and rounded exactly once per event.
//!
//! # Determinism
//!
//! Rendering uses only IEEE `+ - * /`, `floor` and `round` (the sine is a
//! polynomial, not libm). The phase of every sample is computed from the sample
//! index, so nothing accumulates, and noise comes from a fixed-seed xorshift
//! generator that is independent of BASIC's `RND`. The output is therefore the
//! same on every platform, and rendering is *concatenative*:
//! `render(a ++ b) == render(a) ++ render(b)`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

/// Sample rate of all synthesized audio, in Hz (mono, signed 16-bit PCM).
pub const SAMPLE_RATE: u32 = 44_100;
/// Largest volume; volumes are whole numbers from 0 to this.
pub const MAX_VOLUME: u8 = 100;
/// The volume `VOLUME` starts at.
pub const DEFAULT_VOLUME: u8 = 50;
/// Peak sample value per volume step: volume 100 peaks at 30 000 (about
/// -0.8 dBFS) and volume 50 at 15 000, exactly proportional.
pub const AMPLITUDE_PER_VOLUME: u16 = 300;
/// Length of the linear fade-in and fade-out of every tone, in milliseconds.
pub const ENVELOPE_MS: u32 = 3;
/// [`ENVELOPE_MS`] in samples (132 at 44.1 kHz).
pub const ENVELOPE_SAMPLES: u64 = (SAMPLE_RATE as u64) * (ENVELOPE_MS as u64) / 1000;
/// Lowest frequency `SOUND` accepts (other than 0, a rest), in Hz.
pub const MIN_FREQUENCY_HZ: f64 = 20.0;
/// Highest frequency `SOUND` accepts, in Hz.
pub const MAX_FREQUENCY_HZ: f64 = 20_000.0;
/// Longest single `SOUND`, in milliseconds.
pub const MAX_TONE_MS: u32 = 60_000;
/// Most audio that may wait in the queue, in milliseconds (10 minutes).
pub const MAX_QUEUE_MS: u64 = 600_000;
/// [`MAX_QUEUE_MS`] in samples.
pub const MAX_QUEUE_SAMPLES: u64 = MAX_QUEUE_MS * (SAMPLE_RATE as u64) / 1000;
/// Frequency of `BEEP`, in Hz.
pub const BEEP_FREQUENCY_HZ: f64 = 800.0;
/// Duration of `BEEP`, in milliseconds.
pub const BEEP_DURATION_MS: u32 = 200;

/// Default MML tempo: quarter notes per minute.
pub const DEFAULT_TEMPO: u32 = 120;
/// Slowest and fastest MML tempo.
pub const MIN_TEMPO: u32 = 32;
pub const MAX_TEMPO: u32 = 255;
/// Default MML octave (octave 3 contains middle C, as in classic BASIC).
pub const DEFAULT_OCTAVE: u32 = 4;
/// Highest MML octave; the lowest is 0.
pub const MAX_OCTAVE: u32 = 6;
/// Default MML note length: a quarter note.
pub const DEFAULT_LENGTH: u32 = 4;
/// Longest MML note length denominator (a 64th note).
pub const MAX_LENGTH: u32 = 64;
/// Most dots after one MML length.
pub const MAX_DOTS: u32 = 3;
/// Highest MML note number (`N84` is B in octave 6); `N0` is a rest.
pub const MAX_NOTE_NUMBER: u32 = 84;

/// Seed of the noise generator, restarted at the beginning of every tone.
const NOISE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// The shape of a tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Waveform {
    /// +1 for the first half of each cycle, -1 for the second (the default).
    #[default]
    Square,
    Sine,
    /// Rises from 0 to +1, falls to -1, returns to 0.
    Triangle,
    /// A rising ramp from -1 to +1 in every cycle.
    Saw,
    /// White noise held for one cycle of the tone's frequency, so a higher
    /// frequency sounds brighter.
    Noise,
}

impl Waveform {
    pub const ALL: [Waveform; 5] = [
        Waveform::Square,
        Waveform::Sine,
        Waveform::Triangle,
        Waveform::Saw,
        Waveform::Noise,
    ];

    /// The lower-case name accepted by `WAVE`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Sine => "sine",
            Self::Triangle => "triangle",
            Self::Saw => "saw",
            Self::Noise => "noise",
        }
    }

    /// Parses a waveform name, ignoring ASCII case.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|wave| wave.name().eq_ignore_ascii_case(text))
    }
}

/// One tone: a waveform of a given frequency, loudness and length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Frequency in Hz. A frequency that is not positive and finite renders as
    /// silence.
    pub frequency_hz: f64,
    /// Length in samples (see [`SAMPLE_RATE`]).
    pub duration_samples: u64,
    /// Volume from 0 to [`MAX_VOLUME`].
    pub volume: u8,
    pub waveform: Waveform,
}

impl Tone {
    /// A tone lasting `duration_ms` milliseconds (rounded to whole samples).
    pub fn new(frequency_hz: f64, duration_ms: u32, volume: u8, waveform: Waveform) -> Self {
        Self {
            frequency_hz,
            duration_samples: ms_to_samples(u64::from(duration_ms)),
            volume,
            waveform,
        }
    }

    /// The length in milliseconds (fractional when the length is not a whole
    /// number of milliseconds).
    pub fn duration_ms(&self) -> f64 {
        self.duration_samples as f64 * 1000.0 / f64::from(SAMPLE_RATE)
    }
}

/// One entry of the playback queue.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AudioEvent {
    Tone(Tone),
    /// Silence.
    Rest {
        duration_samples: u64,
    },
    /// Decoded PCM (a `PLAYWAV` file): mono 16-bit at [`SAMPLE_RATE`], played
    /// at `volume` percent of its own level (`MAX_VOLUME` is unchanged). The
    /// clip is shared, so cloning an event is cheap. When `duration_samples`
    /// is shorter than the clip it is cut off there, when longer it is padded
    /// with silence (this is how a stopped clip is trimmed).
    Clip {
        samples: Arc<[i16]>,
        volume: u8,
        duration_samples: u64,
    },
}

impl AudioEvent {
    /// A rest of `duration_ms` milliseconds (rounded to whole samples).
    pub fn rest_ms(duration_ms: u32) -> Self {
        Self::Rest {
            duration_samples: ms_to_samples(u64::from(duration_ms)),
        }
    }

    /// A clip of decoded samples played at `volume` percent
    /// (`sample * volume / 100`, rounded half away from zero).
    pub fn clip(samples: impl Into<Arc<[i16]>>, volume: u8) -> Self {
        let samples = samples.into();
        Self::Clip {
            duration_samples: samples.len() as u64,
            samples,
            volume: volume.min(MAX_VOLUME),
        }
    }

    /// The length in samples.
    pub fn samples(&self) -> u64 {
        match self {
            Self::Tone(tone) => tone.duration_samples,
            Self::Rest { duration_samples }
            | Self::Clip {
                duration_samples, ..
            } => *duration_samples,
        }
    }

    /// The same event, shortened (or lengthened) to `samples`.
    fn with_samples(&self, samples: u64) -> Self {
        match self {
            Self::Tone(tone) => Self::Tone(Tone {
                duration_samples: samples,
                ..*tone
            }),
            Self::Rest { .. } => Self::Rest {
                duration_samples: samples,
            },
            Self::Clip {
                samples: clip,
                volume,
                ..
            } => Self::Clip {
                samples: Arc::clone(clip),
                volume: *volume,
                duration_samples: samples,
            },
        }
    }
}

/// One clip sample at `volume` percent: `sample * volume / 100`, rounded half
/// away from zero (exact for 100 and 0).
fn scale_clip_sample(sample: i16, volume: u8) -> i16 {
    let scaled = i32::from(sample) * i32::from(volume);
    let magnitude = (scaled.abs() + 50) / 100;
    (if scaled < 0 { -magnitude } else { magnitude }) as i16
}

/// Milliseconds to whole samples, rounding half up.
pub fn ms_to_samples(ms: u64) -> u64 {
    (u128::from(ms) * u128::from(SAMPLE_RATE) + 500).div_euclid(1000) as u64
}

/// Samples to milliseconds, rounding up (a non-empty queue never reports 0).
pub fn samples_to_ms_ceil(samples: u64) -> u64 {
    (u128::from(samples) * 1000).div_ceil(u128::from(SAMPLE_RATE)) as u64
}

/// Samples to milliseconds, rounding half up.
pub fn samples_to_ms_round(samples: u64) -> u64 {
    ((u128::from(samples) * 1000 + u128::from(SAMPLE_RATE) / 2) / u128::from(SAMPLE_RATE)) as u64
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

/// `sin(2 pi t)` for a phase `t` in turns (`0 <= t < 1`), as a polynomial so
/// the result does not depend on the platform's math library. The error is
/// below 1e-7.
fn sine_turns(t: f64) -> f64 {
    // Fold into [-1/4, 1/4] turns, where the series converges quickly.
    let mut x = if t > 0.5 { t - 1.0 } else { t };
    if x > 0.25 {
        x = 0.5 - x;
    } else if x < -0.25 {
        x = -0.5 - x;
    }
    let x = x * std::f64::consts::TAU;
    let x2 = x * x;
    x * (1.0
        + x2 * (-1.0 / 6.0
            + x2 * (1.0 / 120.0
                + x2 * (-1.0 / 5040.0 + x2 * (1.0 / 362_880.0 + x2 * (-1.0 / 39_916_800.0))))))
}

/// xorshift64: tiny, fixed and entirely separate from BASIC's `RND`.
struct NoiseGenerator(u64);

impl NoiseGenerator {
    /// The next value in `[-1, 1)`.
    fn draw(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// Appends the samples of one tone.
fn render_tone(tone: &Tone, out: &mut Vec<i16>) {
    let count = tone.duration_samples;
    let audible = tone.frequency_hz.is_finite() && tone.frequency_hz > 0.0 && tone.volume > 0;
    if !audible {
        out.resize(out.len() + count as usize, 0);
        return;
    }
    let amplitude = f64::from(tone.volume.min(MAX_VOLUME)) * f64::from(AMPLITUDE_PER_VOLUME);
    let mut noise = NoiseGenerator(NOISE_SEED);
    let mut noise_cycle = u64::MAX;
    let mut noise_value = 0.0;
    for index in 0..count {
        // The phase comes from the sample index: no accumulated error.
        let cycles = tone.frequency_hz * index as f64 / f64::from(SAMPLE_RATE);
        let whole = cycles.floor();
        let phase = cycles - whole;
        let wave = match tone.waveform {
            Waveform::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Sine => sine_turns(phase),
            Waveform::Triangle => {
                if phase < 0.25 {
                    4.0 * phase
                } else if phase < 0.75 {
                    2.0 - 4.0 * phase
                } else {
                    4.0 * phase - 4.0
                }
            }
            Waveform::Saw => 2.0 * phase - 1.0,
            Waveform::Noise => {
                if noise_cycle != whole as u64 {
                    noise_cycle = whole as u64;
                    noise_value = noise.draw();
                }
                noise_value
            }
        };
        // Linear fade-in and fade-out: zero at both ends of the tone.
        let edge = index.min(count - 1 - index).min(ENVELOPE_SAMPLES);
        let gain = edge as f64 / ENVELOPE_SAMPLES as f64;
        let sample = (wave * amplitude * gain).round();
        out.push(sample.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16);
    }
}

/// Appends the samples of one event.
fn render_event(event: &AudioEvent, out: &mut Vec<i16>) {
    match event {
        AudioEvent::Tone(tone) => render_tone(tone, out),
        AudioEvent::Rest { duration_samples } => {
            out.resize(out.len() + *duration_samples as usize, 0);
        }
        AudioEvent::Clip {
            samples,
            volume,
            duration_samples,
        } => {
            let count = *duration_samples as usize;
            let shown = count.min(samples.len());
            out.extend(
                samples[..shown]
                    .iter()
                    .map(|s| scale_clip_sample(*s, *volume)),
            );
            out.resize(out.len() + (count - shown), 0);
        }
    }
}

/// Appends the first `kept` samples of one event as the device would have
/// played them when it was cut off there. A tone is rendered in full and then
/// sliced, so the cut is abrupt (the fade-out belongs to the end of the full
/// tone, not to wherever playback stopped); rests and clips have no envelope,
/// so shortening them is the same thing.
fn render_part(event: &AudioEvent, kept: u64, out: &mut Vec<i16>) {
    if kept >= event.samples() {
        render_event(event, out);
    } else if let AudioEvent::Tone(_) = event {
        let start = out.len();
        render_event(event, out);
        out.truncate(start + kept as usize);
    } else {
        render_event(&event.with_samples(kept), out);
    }
}

/// Renders events back to back into 16-bit mono PCM at [`SAMPLE_RATE`].
/// Every tone starts at phase 0 with a fresh noise generator and fades in and
/// out over [`ENVELOPE_MS`], so the result of rendering a list equals the
/// concatenated results of rendering its parts. A clip is its samples scaled
/// by `volume / 100`.
pub fn render(events: &[AudioEvent]) -> Vec<i16> {
    let total: u64 = events.iter().map(AudioEvent::samples).sum();
    let mut out = Vec::with_capacity(total.min(MAX_QUEUE_SAMPLES) as usize);
    for event in events {
        render_event(event, &mut out);
    }
    out
}

/// Adds `source` into `target` sample by sample with saturation (the sum is
/// clamped to the 16-bit range instead of wrapping); `target` is extended with
/// silence when `source` is longer.
pub fn mix_into(target: &mut Vec<i16>, source: &[i16]) {
    if target.len() < source.len() {
        target.resize(source.len(), 0);
    }
    for (mixed, added) in target.iter_mut().zip(source) {
        *mixed = mixed.saturating_add(*added);
    }
}

/// The 44-byte header of a PCM, mono, 16-bit, [`SAMPLE_RATE`] WAV file with
/// `sample_count` samples, or `None` when that does not fit in the 4 GiB a WAV
/// file can describe.
fn wav_header(sample_count: u64) -> Option<[u8; 44]> {
    let data_bytes = u32::try_from(sample_count.checked_mul(2)?)
        .ok()
        .filter(|bytes| *bytes <= u32::MAX - 36)?;
    let mut header = [0u8; 44];
    let mut put = |at: usize, bytes: &[u8]| header[at..at + bytes.len()].copy_from_slice(bytes);
    put(0, b"RIFF");
    put(4, &(36 + data_bytes).to_le_bytes());
    put(8, b"WAVE");
    put(12, b"fmt ");
    put(16, &16u32.to_le_bytes());
    put(20, &1u16.to_le_bytes()); // PCM
    put(22, &1u16.to_le_bytes()); // mono
    put(24, &SAMPLE_RATE.to_le_bytes());
    put(28, &(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    put(32, &2u16.to_le_bytes()); // bytes per frame
    put(34, &16u16.to_le_bytes()); // bits per sample
    put(36, b"data");
    put(40, &data_bytes.to_le_bytes());
    Some(header)
}

/// A RIFF/WAVE file (PCM, mono, 16-bit, [`SAMPLE_RATE`]) holding `samples`.
///
/// # Panics
///
/// When the audio does not fit in the 4 GiB a WAV file can describe.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let header = wav_header(samples.len() as u64).expect("audio too long for a WAV file");
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(&header);
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Renders `events` back to back and writes them to `out` as a WAV file (the
/// format of [`wav_bytes`]), one event at a time so a long program never
/// holds all of its audio in memory twice. Returns the number of samples
/// written.
///
/// # Errors
///
/// A write error, or `InvalidInput` (before anything is written) when the
/// audio would not fit in a WAV file.
pub fn write_wav(events: &[AudioEvent], out: &mut impl std::io::Write) -> std::io::Result<u64> {
    write_wav_parts(events.iter().map(|event| (event, event.samples())), out)
}

/// [`write_wav`] for events of which only the first `kept` samples are
/// wanted (see [`render_part`]).
fn write_wav_parts<'a>(
    parts: impl Iterator<Item = (&'a AudioEvent, u64)> + Clone,
    out: &mut impl std::io::Write,
) -> std::io::Result<u64> {
    let total: u64 = parts.clone().map(|(_, kept)| kept).sum();
    let header = wav_header(total).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the audio is too long for a WAV file",
        )
    })?;
    out.write_all(&header)?;
    let mut samples = Vec::new();
    let mut bytes = Vec::new();
    for (event, kept) in parts {
        samples.clear();
        render_part(event, kept, &mut samples);
        bytes.clear();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        out.write_all(&bytes)?;
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

/// MIDI notes 0..=11 (octave -1) in Hz, as literals so every frequency is a
/// product of one literal and a power of two: exact and platform independent.
const PITCH_CLASS_HZ: [f64; 12] = [
    8.175798915643707,
    8.661957218027252,
    9.177023997418987,
    9.722718241315029,
    10.300861153527185,
    10.913382232281371,
    11.562325709738575,
    12.249857374429665,
    12.978271799373285,
    13.75,
    14.56761754744031,
    15.433853164253879,
];

/// The frequency of MIDI-style note number `note` in 12-tone equal
/// temperament with A4 = 69 = 440 Hz and middle C (C4) = 60 = 261.63 Hz. A
/// fraction bends the pitch between notes. Values are clamped to 0..=127.
pub fn note_frequency(note: f64) -> f64 {
    let note = note.clamp(0.0, 127.0);
    let whole = note.floor();
    let fraction = note - whole;
    let index = whole as usize;
    let base = PITCH_CLASS_HZ[index % 12] * f64::from(1u32 << (index / 12));
    if fraction == 0.0 {
        base
    } else {
        base * (fraction / 12.0).exp2()
    }
}

// ---------------------------------------------------------------------------
// Music Macro Language
// ---------------------------------------------------------------------------

/// Why a music string was rejected, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmlError {
    /// 1-based character position of the offending command in the string
    /// (white space counts).
    pub position: usize,
    pub message: String,
    /// `true` when the string is valid but plays for more than
    /// [`MAX_QUEUE_SAMPLES`] (`position` is then the command that crossed the
    /// limit). Such a string could never be queued, so `parse_mml` gives up
    /// on it without keeping its events.
    pub too_long: bool,
}

impl fmt::Display for MmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "position {}: {}", self.position, self.message)
    }
}

impl std::error::Error for MmlError {}

#[derive(Clone, Copy)]
enum Articulation {
    /// `MN`: 7/8 of the note sounds, the rest is silence.
    Normal,
    /// `ML`: the whole note sounds.
    Legato,
    /// `MS`: 3/4 of the note sounds.
    Staccato,
}

/// Reads the characters of a music string lazily (no copy of the text) and
/// counts how many it has consumed, which is what error positions report.
struct MmlParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    /// Characters consumed so far (the 0-based index of the next one).
    index: usize,
}

impl<'a> MmlParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.chars().peekable(),
            index: 0,
        }
    }

    /// Consumes the next character.
    fn advance(&mut self) {
        if self.chars.next().is_some() {
            self.index += 1;
        }
    }

    fn skip_space(&mut self) {
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.advance();
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.chars.peek().copied()
    }

    /// An unsigned number following the command, if there is one. White space
    /// may precede the number but not split it.
    fn number(&mut self) -> Option<u32> {
        self.skip_space();
        let mut value: Option<u32> = None;
        while let Some(digit) = self.chars.peek().and_then(|c| c.to_digit(10)) {
            // Saturate: anything this large is out of range anyway.
            value = Some((value.unwrap_or(0) * 10 + digit).min(1_000_000));
            self.advance();
        }
        value
    }

    /// A required number within `low..=high`.
    fn bounded(
        &mut self,
        position: usize,
        command: char,
        what: &str,
        low: u32,
        high: u32,
    ) -> Result<u32, MmlError> {
        match self.number() {
            None => Err(mml_error(
                position,
                format!("{command} needs a number ({what} from {low} to {high})"),
            )),
            Some(n) if !(low..=high).contains(&n) => Err(mml_error(
                position,
                format!("{what} must be from {low} to {high}, got {n}"),
            )),
            Some(n) => Ok(n),
        }
    }

    /// An optional length (1..=64) and dots; `default` when omitted.
    fn length_and_dots(&mut self, position: usize, default: u32) -> Result<(u32, u32), MmlError> {
        let length = match self.number() {
            None => default,
            Some(n) if (1..=MAX_LENGTH).contains(&n) => n,
            Some(n) => {
                return Err(mml_error(
                    position,
                    format!("length must be from 1 to {MAX_LENGTH}, got {n}"),
                ));
            }
        };
        let mut dots = 0;
        while self.peek() == Some('.') {
            self.advance();
            dots += 1;
            if dots > MAX_DOTS {
                return Err(mml_error(
                    position,
                    format!("a length takes at most {MAX_DOTS} dots"),
                ));
            }
        }
        Ok((length, dots))
    }
}

fn mml_error(position: usize, message: impl Into<String>) -> MmlError {
    MmlError {
        position,
        message: message.into(),
        too_long: false,
    }
}

/// The samples of a note or rest of `1/length` (plus dots) at `tempo` quarter
/// notes per minute, rounded to the nearest sample once.
fn length_samples(length: u32, dots: u32, tempo: u32) -> u64 {
    // A whole note is four quarters of 60/tempo seconds each.
    const WHOLE_NOTE: u64 = 4 * 60 * SAMPLE_RATE as u64;
    // Each dot adds half of the previous addition: 2 - 1/2^dots in total.
    let numerator = WHOLE_NOTE * ((2u64 << dots) - 1);
    let denominator = (u64::from(length) * u64::from(tempo)) << dots;
    (2 * numerator + denominator) / (2 * denominator)
}

/// Receives the events of a music string one by one and keeps the running
/// length (saturating), so nothing needs to be collected to know how long a
/// string is.
struct MmlSink<F: FnMut(usize, AudioEvent, u64)> {
    waveform: Waveform,
    /// Total samples so far.
    total: u64,
    /// Called with the position of the command, the event and the total
    /// including that event.
    emit: F,
}

impl<F: FnMut(usize, AudioEvent, u64)> MmlSink<F> {
    fn push(&mut self, position: usize, event: AudioEvent) {
        self.total = self.total.saturating_add(event.samples());
        (self.emit)(position, event, self.total);
    }

    /// A note of `total` samples, split into sound and silence by
    /// `articulation`.
    fn note(
        &mut self,
        position: usize,
        number: u32,
        total: u64,
        articulation: Articulation,
        volume: u8,
    ) {
        let sounded = match articulation {
            Articulation::Normal => (total * 7 + 4) / 8,
            Articulation::Staccato => (total * 3 + 2) / 4,
            Articulation::Legato => total,
        };
        self.push(
            position,
            AudioEvent::Tone(Tone {
                frequency_hz: note_frequency(f64::from(number + 23)),
                duration_samples: sounded,
                volume,
                waveform: self.waveform,
            }),
        );
        if total > sounded {
            self.push(
                position,
                AudioEvent::Rest {
                    duration_samples: total - sounded,
                },
            );
        }
    }
}

/// Parses a Music Macro Language string into events. The whole string is
/// validated first: on error nothing is returned.
///
/// Every string starts from the defaults (tempo 120, octave 4, length 4,
/// normal articulation, `volume`); nothing carries over between strings.
/// Notes use `waveform`. See `docs/statements.md` for the command reference.
///
/// A string that plays for more than [`MAX_QUEUE_SAMPLES`] can never be
/// queued, so its events are dropped as soon as the running length crosses
/// that limit (memory stays bounded by the limit, not by the string). The
/// rest of the string is still checked, so a syntax error is reported first;
/// a valid one gives an error with [`MmlError::too_long`] set.
pub fn parse_mml(text: &str, volume: u8, waveform: Waveform) -> Result<Vec<AudioEvent>, MmlError> {
    let mut events = Vec::new();
    let mut too_long_at: Option<usize> = None;
    scan_mml(text, volume, waveform, |position, event, total| {
        if total > MAX_QUEUE_SAMPLES {
            if too_long_at.is_none() {
                too_long_at = Some(position);
                events = Vec::new();
            }
        } else {
            events.push(event);
        }
    })?;
    match too_long_at {
        None => Ok(events),
        Some(position) => Err(MmlError {
            position,
            message: format!(
                "the music plays for more than {} seconds",
                MAX_QUEUE_MS / 1000
            ),
            too_long: true,
        }),
    }
}

/// The exact length in samples of a music string (saturating at
/// `u64::MAX`), validated like [`parse_mml`] but without building any events:
/// O(n) time and O(1) memory however long the string is, and no queue limit.
pub fn mml_length_samples(text: &str) -> Result<u64, MmlError> {
    scan_mml(text, DEFAULT_VOLUME, Waveform::default(), |_, _, _| {})
}

/// The one MML interpreter behind [`parse_mml`] and [`mml_length_samples`]:
/// validates `text` and hands every event to `emit`. Returns the total length.
fn scan_mml(
    text: &str,
    volume: u8,
    waveform: Waveform,
    emit: impl FnMut(usize, AudioEvent, u64),
) -> Result<u64, MmlError> {
    let mut parser = MmlParser::new(text);
    let mut tempo = DEFAULT_TEMPO;
    let mut octave = DEFAULT_OCTAVE;
    let mut length = DEFAULT_LENGTH;
    let mut articulation = Articulation::Normal;
    let mut volume = volume.min(MAX_VOLUME);
    let mut sink = MmlSink {
        waveform,
        total: 0,
        emit,
    };

    while let Some(command) = parser.peek() {
        let position = parser.index + 1;
        parser.advance();
        match command.to_ascii_uppercase() {
            letter @ 'A'..='G' => {
                let semitone = match letter {
                    'C' => 0,
                    'D' => 2,
                    'E' => 4,
                    'F' => 5,
                    'G' => 7,
                    'A' => 9,
                    _ => 11,
                };
                let accidental = match parser.peek() {
                    Some('#' | '+') => {
                        parser.advance();
                        1
                    }
                    Some('-') => {
                        parser.advance();
                        -1
                    }
                    _ => 0,
                };
                let (note_length, dots) = parser.length_and_dots(position, length)?;
                let number = (12 * octave + semitone + 1) as i64 + accidental;
                if !(1..=i64::from(MAX_NOTE_NUMBER)).contains(&number) {
                    return Err(mml_error(
                        position,
                        format!(
                            "the note is outside the playable range (octave 0 C to octave {MAX_OCTAVE} B)"
                        ),
                    ));
                }
                let total = length_samples(note_length, dots, tempo);
                sink.note(position, number as u32, total, articulation, volume);
            }
            'N' => {
                let number = parser.bounded(position, 'N', "note number", 0, MAX_NOTE_NUMBER)?;
                let total = length_samples(length, 0, tempo);
                if number == 0 {
                    sink.push(
                        position,
                        AudioEvent::Rest {
                            duration_samples: total,
                        },
                    );
                } else {
                    sink.note(position, number, total, articulation, volume);
                }
            }
            'P' | 'R' => {
                let (rest_length, dots) = parser.length_and_dots(position, length)?;
                sink.push(
                    position,
                    AudioEvent::Rest {
                        duration_samples: length_samples(rest_length, dots, tempo),
                    },
                );
            }
            'O' => octave = parser.bounded(position, 'O', "octave", 0, MAX_OCTAVE)?,
            '<' | '>' => {
                let shifted = if command == '>' {
                    octave + 1
                } else {
                    octave.wrapping_sub(1)
                };
                if shifted > MAX_OCTAVE {
                    return Err(mml_error(
                        position,
                        format!("'{command}' leaves the octave range 0 to {MAX_OCTAVE}"),
                    ));
                }
                octave = shifted;
            }
            'L' => length = parser.bounded(position, 'L', "length", 1, MAX_LENGTH)?,
            'T' => tempo = parser.bounded(position, 'T', "tempo", MIN_TEMPO, MAX_TEMPO)?,
            'V' => {
                volume = parser.bounded(position, 'V', "volume", 0, u32::from(MAX_VOLUME))? as u8;
            }
            'M' => {
                articulation = match parser.peek().map(|c| c.to_ascii_uppercase()) {
                    Some('N') => Articulation::Normal,
                    Some('L') => Articulation::Legato,
                    Some('S') => Articulation::Staccato,
                    _ => {
                        return Err(mml_error(
                            position,
                            "M must be followed by N (normal), L (legato) or S (staccato)",
                        ));
                    }
                };
                parser.advance();
            }
            _ => {
                return Err(mml_error(position, format!("unknown command '{command}'")));
            }
        }
    }
    Ok(sink.total)
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// Where the VM sends sound. The VM never blocks inside a backend: `play`
/// only *queues*, and the backend plays the queue in order, asynchronously.
///
/// Time is in samples ([`SAMPLE_RATE`]); the VM converts with
/// [`samples_to_ms_ceil`]. Implemented by [`HeadlessAudio`] and [`NullAudio`]
/// (virtual time) and, later, by a device backend (real time: report `true`
/// from [`AudioBackend::is_real_time`]).
pub trait AudioBackend {
    /// Appends `events` to the end of the queue. The VM has already checked
    /// the queue limit ([`MAX_QUEUE_SAMPLES`]) and the value ranges. Events
    /// are meant to be rendered with [`render`] (or an equivalent).
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String>;
    /// Silences the output immediately and empties the queue.
    fn stop(&mut self);
    /// Audio still waiting to play, in samples: 0 when nothing is playing.
    fn queued_samples(&self) -> u64;
    /// [`AudioBackend::queued_samples`] in milliseconds, rounded up.
    fn queued_ms(&self) -> u64 {
        samples_to_ms_ceil(self.queued_samples())
    }
    /// Whether anything is playing or waiting.
    fn is_playing(&self) -> bool {
        self.queued_samples() > 0
    }
    /// The program let `ms` milliseconds pass (`SLEEP`, `DOEVENTS`,
    /// `SOUNDWAIT`). A backend with a virtual clock plays that much of the
    /// queue; a real-time backend ignores it because time passes by itself.
    fn elapse(&mut self, ms: u64) {
        let _ = ms;
    }
    /// Whether the queue drains in real time. `SOUNDWAIT` then really sleeps
    /// (keeping the window responsive); otherwise it advances the virtual
    /// clock at once.
    fn is_real_time(&self) -> bool {
        false
    }
    /// Lets the queue play out, waiting at most `max_ms` milliseconds (the
    /// end of a program that wants its last sound heard). Returns whether the
    /// queue is empty afterwards. A virtual-clock backend just advances its
    /// clock; a real-time backend really waits.
    fn drain(&mut self, max_ms: u64) -> bool {
        let wait = self.queued_ms().min(max_ms);
        self.elapse(wait);
        !self.is_playing()
    }
}

impl AudioBackend for Box<dyn AudioBackend> {
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String> {
        (**self).play(events)
    }
    fn stop(&mut self) {
        (**self).stop();
    }
    fn queued_samples(&self) -> u64 {
        (**self).queued_samples()
    }
    fn queued_ms(&self) -> u64 {
        (**self).queued_ms()
    }
    fn is_playing(&self) -> bool {
        (**self).is_playing()
    }
    fn elapse(&mut self, ms: u64) {
        (**self).elapse(ms);
    }
    fn is_real_time(&self) -> bool {
        (**self).is_real_time()
    }
    fn drain(&mut self, max_ms: u64) -> bool {
        (**self).drain(max_ms)
    }
}

/// What opens a real backend for [`DeferredAudio`].
pub type AudioOpener = Box<dyn FnOnce() -> Result<Box<dyn AudioBackend>, String>>;

/// Called once with the reason when [`DeferredAudio`] cannot open its backend.
pub type AudioFailureHandler = Box<dyn FnOnce(&str)>;

/// A backend that opens the real one only when the program first makes a
/// sound, so a program without sound never touches the audio device (and
/// never prints a warning about a missing one).
///
/// When the opener fails, `on_failure` is called once with the reason and
/// every later sound goes to a [`NullAudio`]: the program runs on with
/// silent, virtual-clock sound.
pub struct DeferredAudio {
    opener: Option<AudioOpener>,
    on_failure: Option<AudioFailureHandler>,
    backend: Box<dyn AudioBackend>,
    opened: bool,
}

impl DeferredAudio {
    pub fn new(opener: AudioOpener, on_failure: AudioFailureHandler) -> Self {
        Self {
            opener: Some(opener),
            on_failure: Some(on_failure),
            backend: Box::new(NullAudio::new()),
            opened: false,
        }
    }

    /// Whether the real backend was opened (false before the first sound and
    /// after a failure).
    pub fn is_open(&self) -> bool {
        self.opened
    }

    fn open_now(&mut self) {
        let Some(opener) = self.opener.take() else {
            return;
        };
        match opener() {
            Ok(backend) => {
                self.backend = backend;
                self.opened = true;
            }
            Err(reason) => {
                if let Some(report) = self.on_failure.take() {
                    report(&reason);
                }
            }
        }
    }
}

impl AudioBackend for DeferredAudio {
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String> {
        self.open_now();
        self.backend.play(events)
    }
    fn stop(&mut self) {
        self.backend.stop();
    }
    fn queued_samples(&self) -> u64 {
        self.backend.queued_samples()
    }
    fn queued_ms(&self) -> u64 {
        self.backend.queued_ms()
    }
    fn is_playing(&self) -> bool {
        self.backend.is_playing()
    }
    fn elapse(&mut self, ms: u64) {
        self.backend.elapse(ms);
    }
    fn is_real_time(&self) -> bool {
        self.backend.is_real_time()
    }
    fn drain(&mut self, max_ms: u64) -> bool {
        self.backend.drain(max_ms)
    }
}

/// The virtual playback queue shared by the headless backends.
#[derive(Debug, Clone, Copy, Default)]
struct VirtualQueue {
    /// Samples still to play.
    pending: u64,
    /// Time already elapsed towards the next sample, in thousandths of a
    /// sample (so millisecond steps never drift: 1 ms is 44.1 samples).
    carry: u64,
}

impl VirtualQueue {
    fn push(&mut self, samples: u64) {
        self.pending = self.pending.saturating_add(samples);
    }

    fn stop(&mut self) {
        *self = Self::default();
    }

    /// Plays `ms` milliseconds of the queue; an idle queue ignores time.
    fn advance(&mut self, ms: u64) {
        if self.pending == 0 {
            return;
        }
        let credit = u128::from(self.carry) + u128::from(ms) * u128::from(SAMPLE_RATE);
        let consumed = (credit / 1000).min(u128::from(self.pending)) as u64;
        self.pending -= consumed;
        self.carry = if self.pending == 0 {
            0
        } else {
            (credit % 1000) as u64
        };
    }
}

/// A silent sink with a virtual clock: it accepts everything, remembers
/// nothing and plays the queue only as the program lets time pass. This is the
/// backend a [`crate::Vm`] starts with, so sound statements never fail just
/// because no audio output exists.
#[derive(Debug, Clone, Default)]
pub struct NullAudio {
    queue: VirtualQueue,
}

impl NullAudio {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioBackend for NullAudio {
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String> {
        self.queue
            .push(events.iter().map(AudioEvent::samples).sum());
        Ok(())
    }
    fn stop(&mut self) {
        self.queue.stop();
    }
    fn queued_samples(&self) -> u64 {
        self.queue.pending
    }
    fn elapse(&mut self, ms: u64) {
        self.queue.advance(ms);
    }
}

/// What a [`HeadlessAudio`] recorded; shared by every clone.
#[derive(Debug, Default)]
struct HeadlessState {
    queue: VirtualQueue,
    /// Everything ever accepted, in order.
    events: Vec<AudioEvent>,
    /// What a listener would have heard: `events`, each with the number of
    /// its samples that were played (all of them, unless `stop` cut it).
    audible: Vec<(AudioEvent, u64)>,
    audible_samples: u64,
    clock_ms: u64,
    stops: usize,
}

/// A deterministic audio backend for tests: it records the queued events and
/// plays them against a *virtual clock* that only moves when the program
/// sleeps, calls `DOEVENTS` or `SOUNDWAIT`, or the test calls
/// [`HeadlessAudio::advance`]. Nothing ever really waits. Clones share the
/// state, so a test keeps one clone and hands another to the VM.
#[derive(Debug, Clone, Default)]
pub struct HeadlessAudio {
    state: Rc<RefCell<HeadlessState>>,
}

impl HeadlessAudio {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every event accepted so far, in order, including ones later discarded
    /// by `STOPSOUND`.
    pub fn events(&self) -> Vec<AudioEvent> {
        self.state.borrow().events.clone()
    }

    /// The events a listener would have heard: [`HeadlessAudio::events`] minus
    /// what `stop` discarded before it played (a tone cut in the middle is
    /// shortened to the played part). Rendering this list re-applies the
    /// fade-out to a shortened tone; [`HeadlessAudio::rendered_samples`] and
    /// [`HeadlessAudio::write_wav`] do not.
    pub fn audible_events(&self) -> Vec<AudioEvent> {
        self.state
            .borrow()
            .audible
            .iter()
            .map(|(event, kept)| {
                if *kept < event.samples() {
                    event.with_samples(*kept)
                } else {
                    event.clone()
                }
            })
            .collect()
    }

    /// The PCM of [`HeadlessAudio::audible_events`] back to back, as the
    /// device would have played it: a tone cut by `stop` is a plain prefix of
    /// the tone rendered in full (abrupt, with no fade-out at the cut). Time
    /// the queue spent idle is not included.
    pub fn rendered_samples(&self) -> Vec<i16> {
        let state = self.state.borrow();
        let mut out = Vec::with_capacity(state.audible_samples.min(MAX_QUEUE_SAMPLES) as usize);
        for (event, kept) in &state.audible {
            render_part(event, *kept, &mut out);
        }
        out
    }

    /// Streams [`HeadlessAudio::rendered_samples`] to `out` as a WAV file
    /// without building the whole PCM in memory; returns the sample count.
    ///
    /// # Errors
    ///
    /// As [`write_wav`].
    pub fn write_wav(&self, out: &mut impl std::io::Write) -> std::io::Result<u64> {
        let state = self.state.borrow();
        write_wav_parts(
            state.audible.iter().map(|(event, kept)| (event, *kept)),
            out,
        )
    }

    /// Length of the audible audio in milliseconds, rounded to the nearest.
    pub fn total_ms(&self) -> u64 {
        samples_to_ms_round(self.state.borrow().audible_samples)
    }

    /// Audio still waiting to play, in samples.
    pub fn queued_samples(&self) -> u64 {
        self.state.borrow().queue.pending
    }

    /// Audio still waiting to play, in milliseconds (rounded up).
    pub fn queued_ms(&self) -> u64 {
        samples_to_ms_ceil(self.queued_samples())
    }

    pub fn is_playing(&self) -> bool {
        self.queued_samples() > 0
    }

    /// Lets `ms` virtual milliseconds pass.
    pub fn advance(&self, ms: u64) {
        let mut state = self.state.borrow_mut();
        state.clock_ms = state.clock_ms.saturating_add(ms);
        state.queue.advance(ms);
    }

    /// Total virtual time that has passed, in milliseconds.
    pub fn clock_ms(&self) -> u64 {
        self.state.borrow().clock_ms
    }

    /// How many times playback was stopped.
    pub fn stops(&self) -> usize {
        self.state.borrow().stops
    }
}

/// Keeps the first `keep` samples of `events`; the event that straddles the
/// limit stays whole but records how much of it was played.
fn trim_events(events: &mut Vec<(AudioEvent, u64)>, keep: u64) {
    let mut remaining = keep;
    let mut cut = events.len();
    for (index, (_, kept)) in events.iter_mut().enumerate() {
        if remaining >= *kept {
            remaining -= *kept;
        } else {
            if remaining > 0 {
                *kept = remaining;
                cut = index + 1;
            } else {
                cut = index;
            }
            break;
        }
    }
    events.truncate(cut);
}

impl AudioBackend for HeadlessAudio {
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        for event in events.iter().filter(|event| event.samples() > 0) {
            state.events.push(event.clone());
            state.audible.push((event.clone(), event.samples()));
            state.audible_samples += event.samples();
            state.queue.push(event.samples());
        }
        Ok(())
    }

    fn stop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.stops += 1;
        let played = state.audible_samples - state.queue.pending;
        trim_events(&mut state.audible, played);
        state.audible_samples = played;
        state.queue.stop();
    }

    fn queued_samples(&self) -> u64 {
        HeadlessAudio::queued_samples(self)
    }

    fn elapse(&mut self, ms: u64) {
        self.advance(ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(frequency_hz: f64, ms: u32, volume: u8) -> Tone {
        Tone::new(frequency_hz, ms, volume, Waveform::Square)
    }

    fn pcm(tone: Tone) -> Vec<i16> {
        render(&[AudioEvent::Tone(tone)])
    }

    fn mml(text: &str) -> Vec<AudioEvent> {
        parse_mml(text, 50, Waveform::Square).unwrap_or_else(|e| panic!("{text:?}: {e}"))
    }

    fn mml_samples(text: &str) -> u64 {
        mml(text).iter().map(AudioEvent::samples).sum()
    }

    fn mml_error_of(text: &str) -> MmlError {
        parse_mml(text, 50, Waveform::Square).unwrap_err()
    }

    fn tone_of(event: &AudioEvent) -> Tone {
        match event {
            AudioEvent::Tone(tone) => *tone,
            other => panic!("expected a tone, got {other:?}"),
        }
    }

    // --- units and constants ---

    #[test]
    fn constants_have_the_documented_values() {
        assert_eq!(SAMPLE_RATE, 44_100);
        assert_eq!(ENVELOPE_SAMPLES, 132);
        assert_eq!(MAX_QUEUE_SAMPLES, 26_460_000);
        assert_eq!(ms_to_samples(100), 4410);
        assert_eq!(ms_to_samples(1), 44);
        assert_eq!(ms_to_samples(3), 132);
        assert_eq!(ms_to_samples(0), 0);
        assert_eq!(samples_to_ms_ceil(4410), 100);
        assert_eq!(samples_to_ms_ceil(1), 1);
        assert_eq!(samples_to_ms_ceil(0), 0);
        assert_eq!(samples_to_ms_round(22_050), 500);
        assert_eq!(samples_to_ms_round(22), 0);
        assert_eq!(samples_to_ms_round(23), 1);
    }

    #[test]
    fn waveform_names_round_trip_ignoring_case() {
        for wave in Waveform::ALL {
            assert_eq!(Waveform::parse(wave.name()), Some(wave));
            assert_eq!(Waveform::parse(&wave.name().to_uppercase()), Some(wave));
        }
        assert_eq!(Waveform::parse("Saw"), Some(Waveform::Saw));
        assert_eq!(Waveform::parse("sinus"), None);
        assert_eq!(Waveform::parse(""), None);
        assert_eq!(Waveform::default(), Waveform::Square);
    }

    // --- equal temperament ---

    #[test]
    fn note_frequencies_follow_equal_temperament() {
        assert_eq!(note_frequency(69.0), 440.0);
        assert_eq!(note_frequency(81.0), 880.0);
        assert_eq!(note_frequency(57.0), 220.0);
        assert!((note_frequency(60.0) - 261.6256).abs() < 1e-3);
        assert!((note_frequency(0.0) - 8.1758).abs() < 1e-3);
        assert!((note_frequency(127.0) - 12_543.854).abs() < 1e-2);
        for note in 0..=127u32 {
            let expected = 440.0 * ((f64::from(note) - 69.0) / 12.0).exp2();
            let actual = note_frequency(f64::from(note));
            assert!(
                (actual - expected).abs() / expected < 1e-12,
                "note {note}: {actual} vs {expected}"
            );
            if note >= 12 {
                // An octave is exactly a factor of two.
                assert_eq!(actual, 2.0 * note_frequency(f64::from(note - 12)));
            }
        }
    }

    #[test]
    fn fractional_notes_bend_between_semitones_and_values_clamp() {
        let quarter_tone = note_frequency(69.5);
        assert!((quarter_tone - 440.0 * 2f64.powf(0.5 / 12.0)).abs() < 1e-9);
        assert!(quarter_tone > 440.0 && quarter_tone < note_frequency(70.0));
        assert_eq!(note_frequency(-5.0), note_frequency(0.0));
        assert_eq!(note_frequency(500.0), note_frequency(127.0));
    }

    // --- synthesis ---

    #[test]
    fn tone_length_is_exact_in_samples() {
        assert_eq!(pcm(square(440.0, 100, 50)).len(), 4410);
        assert_eq!(pcm(square(440.0, 1, 50)).len(), 44);
        assert_eq!(pcm(square(440.0, 0, 50)).len(), 0);
        assert_eq!(render(&[AudioEvent::rest_ms(250)]).len(), 11_025);
    }

    #[test]
    fn square_has_two_levels_and_a_linear_envelope() {
        let samples = pcm(square(100.0, 100, 100));
        let amplitude = 30_000;
        assert_eq!(samples[0], 0);
        assert_eq!(*samples.last().unwrap(), 0);
        // Fade-in: sample 66 is half way up, sample 132 is at full level.
        assert_eq!(samples[66], amplitude / 2);
        assert_eq!(samples[132], amplitude);
        // Fade-out mirrors it: 4277 is phase 0.698 of a 441-sample cycle.
        let n = samples.len();
        assert_eq!(samples[n - 1 - 132], -amplitude);
        assert_eq!(samples[n - 1 - 66], -amplitude / 2);
        // Between the fades only +A and -A occur.
        let levels: std::collections::BTreeSet<i16> =
            samples[132..n - 132].iter().copied().collect();
        assert_eq!(
            levels.into_iter().collect::<Vec<_>>(),
            vec![-amplitude, amplitude]
        );
        // 100 Hz at 44.1 kHz: a half cycle is exactly 220.5 samples.
        assert_eq!(samples[220], amplitude);
        assert_eq!(samples[221], -amplitude);
    }

    #[test]
    fn short_tones_get_a_triangular_envelope_not_a_click() {
        // 2 ms = 88 samples, shorter than two envelopes.
        let samples = pcm(square(100.0, 2, 100));
        assert_eq!(samples.len(), 88);
        assert_eq!(samples[0], 0);
        assert_eq!(samples[87], 0);
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak < 30_000 && peak > 0, "peak {peak}");
        // Strictly smaller than a full-level sample all over.
        assert!(samples[1].unsigned_abs() < samples[40].unsigned_abs());
    }

    #[test]
    fn peak_amplitude_is_proportional_to_volume() {
        for volume in [1u8, 10, 25, 50, 100] {
            let peak = pcm(square(200.0, 50, volume))
                .iter()
                .map(|s| i32::from(*s).abs())
                .max()
                .unwrap();
            assert_eq!(peak, i32::from(volume) * 300);
        }
        assert!(pcm(square(200.0, 50, 0)).iter().all(|s| *s == 0));
    }

    #[test]
    fn sine_matches_the_real_sine_within_rounding() {
        let amplitude = 30_000.0;
        let tone = Tone::new(441.0, 100, 100, Waveform::Sine);
        let samples = pcm(tone);
        for (index, sample) in samples.iter().enumerate() {
            let edge = index.min(samples.len() - 1 - index).min(132);
            let gain = edge as f64 / 132.0;
            let ideal =
                (std::f64::consts::TAU * 441.0 * index as f64 / 44_100.0).sin() * amplitude * gain;
            assert!(
                (f64::from(*sample) - ideal).abs() <= 1.0,
                "sample {index}: {sample} vs {ideal}"
            );
        }
    }

    #[test]
    fn sine_polynomial_is_accurate_over_a_full_turn() {
        for step in 0..1000 {
            let t = f64::from(step) / 1000.0;
            let ideal = (std::f64::consts::TAU * t).sin();
            assert!((sine_turns(t) - ideal).abs() < 1e-7, "t = {t}");
        }
    }

    #[test]
    fn triangle_and_saw_have_their_shapes() {
        // 4410 Hz: exactly ten samples per cycle.
        let triangle = pcm(Tone::new(4410.0, 20, 100, Waveform::Triangle));
        let samples = &triangle[200..220];
        let near = |actual: i16, expected: i16| {
            assert!(
                (i32::from(actual) - i32::from(expected)).abs() <= 1,
                "{actual} vs {expected}"
            );
        };
        // Phases 0, .1, .2, ... of the cycle.
        for (index, expected) in [
            (0, 0),
            (1, 12_000),
            (2, 24_000),
            (3, 24_000),
            (4, 12_000),
            (5, 0),
            (6, -12_000),
            (7, -24_000),
            (8, -24_000),
            (9, -12_000),
            (10, 0),
        ] {
            near(samples[index], expected);
        }
        let saw = pcm(Tone::new(4410.0, 20, 100, Waveform::Saw));
        let cycle = &saw[200..210];
        near(cycle[0], -30_000);
        near(cycle[5], 0);
        assert!(cycle.windows(2).all(|pair| pair[1] > pair[0]));
        // A saw falls back to the bottom at the start of every cycle.
        near(saw[210], -30_000);
    }

    #[test]
    fn noise_is_deterministic_bounded_and_not_constant() {
        let tone = Tone::new(8_000.0, 100, 100, Waveform::Noise);
        let first = pcm(tone);
        assert_eq!(first, pcm(tone));
        let distinct: std::collections::BTreeSet<i16> = first.iter().copied().collect();
        assert!(distinct.len() > 1000, "only {} levels", distinct.len());
        assert!(first.iter().any(|s| *s > 10_000) && first.iter().any(|s| *s < -10_000));
        assert!(first.iter().all(|s| s.unsigned_abs() <= 30_000));
        // The mean of white noise is near zero.
        let mean = first.iter().map(|s| f64::from(*s)).sum::<f64>() / first.len() as f64;
        assert!(mean.abs() < 1500.0, "mean {mean}");
        // Noise is held for a cycle of the frequency: 4410 Hz holds 10 samples.
        let held = pcm(Tone::new(4410.0, 100, 100, Waveform::Noise));
        assert_eq!(held[1000], held[1009]);
        assert_ne!(held[1009], held[1010]);
    }

    #[test]
    fn noise_restarts_for_every_tone_and_ignores_basic_rnd() {
        let tone = Tone::new(5_000.0, 20, 100, Waveform::Noise);
        let twice = render(&[AudioEvent::Tone(tone), AudioEvent::Tone(tone)]);
        let (a, b) = twice.split_at(twice.len() / 2);
        assert_eq!(a, b);
    }

    #[test]
    fn rendering_is_concatenative() {
        let events = [
            AudioEvent::Tone(Tone::new(440.0, 30, 80, Waveform::Sine)),
            AudioEvent::rest_ms(10),
            AudioEvent::Tone(Tone::new(1000.0, 20, 30, Waveform::Noise)),
            AudioEvent::Tone(Tone::new(330.0, 25, 60, Waveform::Triangle)),
        ];
        let whole = render(&events);
        let parts: Vec<i16> = events
            .iter()
            .flat_map(|e| render(std::slice::from_ref(e)))
            .collect();
        assert_eq!(whole, parts);
        assert_eq!(whole, render(&events));
    }

    #[test]
    fn invalid_frequencies_render_as_silence() {
        for frequency in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let samples = pcm(square(frequency, 10, 100));
            assert_eq!(samples.len(), 441);
            assert!(samples.iter().all(|s| *s == 0), "{frequency}");
        }
    }

    #[test]
    fn phase_does_not_drift_over_long_tones() {
        // 440 Hz is exactly 22 cycles per 2205 samples, so a sample and the one
        // 2_205_000 later (a thousand repeats on) have the same phase; computing
        // the phase from the index means a minute of audio has not drifted.
        let tone = Tone::new(440.0, 60_000, 100, Waveform::Sine);
        let samples = pcm(tone);
        assert_eq!(samples.len(), 2_646_000);
        for start in [1_000usize, 1_234, 5_000] {
            let early = i32::from(samples[start]);
            let late = i32::from(samples[start + 2_205_000]);
            assert!((early - late).abs() <= 1, "{early} vs {late}");
        }
    }

    // --- mixing and WAV ---

    #[test]
    fn mixing_saturates_instead_of_wrapping() {
        let mut target = vec![30_000, -30_000, 100, 0];
        mix_into(&mut target, &[30_000, -30_000, -50, 7, 9]);
        assert_eq!(target, vec![i16::MAX, i16::MIN, 50, 7, 9]);
        let mut empty = Vec::new();
        mix_into(&mut empty, &[1, 2]);
        assert_eq!(empty, vec![1, 2]);
    }

    #[test]
    fn wav_header_and_data_are_exact() {
        let bytes = wav_bytes(&[1, -2, 300]);
        let mut expected = Vec::new();
        expected.extend_from_slice(b"RIFF");
        expected.extend_from_slice(&[42, 0, 0, 0]); // 36 + 6
        expected.extend_from_slice(b"WAVEfmt ");
        expected.extend_from_slice(&[16, 0, 0, 0]);
        expected.extend_from_slice(&[1, 0, 1, 0]); // PCM, mono
        expected.extend_from_slice(&[0x44, 0xAC, 0, 0]); // 44100
        expected.extend_from_slice(&[0x88, 0x58, 0x01, 0]); // 88200
        expected.extend_from_slice(&[2, 0, 16, 0]);
        expected.extend_from_slice(b"data");
        expected.extend_from_slice(&[6, 0, 0, 0]);
        expected.extend_from_slice(&[1, 0, 0xFE, 0xFF, 0x2C, 0x01]);
        assert_eq!(bytes, expected);
        assert_eq!(wav_bytes(&[]).len(), 44);
    }

    // --- MML ---

    #[test]
    fn default_quarter_note_is_exactly_500_ms() {
        // Normal articulation: 7/8 sounding plus the 1/8 gap.
        let events = mml("C");
        assert_eq!(events.len(), 2);
        assert_eq!(tone_of(&events[0]).duration_samples, 19_294);
        assert_eq!(
            events[1],
            AudioEvent::Rest {
                duration_samples: 2_756
            }
        );
        assert_eq!(mml_samples("C"), 22_050);
        assert_eq!(mml_samples("T120 L4 C"), 22_050);
    }

    #[test]
    fn lengths_dots_and_tempo_arithmetic_is_exact() {
        for (text, samples) in [
            ("C1", 88_200),
            ("C2", 44_100),
            ("C4", 22_050),
            ("C8", 11_025),
            ("C16", 5_513),   // 5512.5 rounds half up
            ("C32", 2_756),   // 2756.25
            ("C4.", 33_075),  // 1.5 quarter notes
            ("C4..", 38_588), // 1.75 quarter notes = 38587.5, rounded half up
            ("T60 C4", 44_100),
            ("T240 C4", 11_025),
            ("T32 C1", 330_750),
            ("T255 C64", 649), // 648.5
            ("L8 C", 11_025),
            ("L8 C4", 22_050),
            ("C6", 14_700), // a quarter-note triplet
            ("P4", 22_050),
            ("R2.", 66_150),
            ("L16 P", 5_513),
            ("N0", 22_050),
        ] {
            assert_eq!(mml_samples(text), samples, "{text}");
        }
    }

    #[test]
    fn the_whole_string_length_is_the_sum_of_its_notes() {
        assert_eq!(mml_samples("C D E F"), 4 * 22_050);
        assert_eq!(mml_samples("CDEFGAB>C"), 8 * 22_050);
        assert_eq!(mml_samples("T120 L8 CDEF"), 4 * 11_025);
        assert_eq!(mml_samples("T60 C T120 C"), 44_100 + 22_050);
        assert_eq!(mml_samples(""), 0);
        assert_eq!(mml_samples("  \t "), 0);
    }

    #[test]
    fn articulation_splits_notes_without_changing_their_length() {
        let legato = mml("ML C");
        assert_eq!(legato.len(), 1);
        assert_eq!(tone_of(&legato[0]).duration_samples, 22_050);
        let staccato = mml("MS C");
        assert_eq!(tone_of(&staccato[0]).duration_samples, 16_538); // 3/4, rounded
        assert_eq!(staccato[1].samples(), 22_050 - 16_538);
        let normal = mml("ML MN C");
        assert_eq!(tone_of(&normal[0]).duration_samples, 19_294);
        // Rests are never split.
        assert_eq!(
            mml("MS P4"),
            vec![AudioEvent::Rest {
                duration_samples: 22_050
            }]
        );
        // The articulation stays in force until changed.
        assert_eq!(mml("MS CDE").len(), 6);
        assert_eq!(mml("MS C ML D").len(), 3);
    }

    #[test]
    fn pitches_follow_the_classic_octave_numbering() {
        let hz = |text: &str| tone_of(&mml(text)[0]).frequency_hz;
        // Octave 3 contains middle C and A = 440 Hz.
        assert_eq!(hz("O3 A"), 440.0);
        assert!((hz("O3 C") - 261.6256).abs() < 1e-3);
        assert_eq!(hz("O4 A"), 880.0);
        assert_eq!(hz("A"), 880.0, "the default octave is 4");
        assert_eq!(hz("O2 A"), 220.0);
        assert!((hz("O0 C") - 32.7032).abs() < 1e-3);
        assert!((hz("O6 B") - 3951.066).abs() < 1e-2);
        // Octave shifts.
        assert_eq!(hz("<A"), 440.0);
        assert_eq!(hz(">A"), 1760.0);
        assert_eq!(hz("O1 > > A"), 440.0);
        // Accidentals.
        assert_eq!(hz("C#"), hz("C+"));
        assert_eq!(hz("C#"), hz("D-"));
        assert_eq!(hz("E#"), hz("F"));
        assert_eq!(hz("B#"), hz(">C"));
        assert_eq!(hz("O4 C-"), hz("O3 B"));
        assert!(hz("C#") > hz("C") && hz("C#") < hz("D"));
        // Case and white space are ignored.
        assert_eq!(hz("o3 a"), 440.0);
        assert_eq!(hz("O 3 A"), 440.0);
        assert_eq!(hz("a #4"), hz("A#4"));
    }

    #[test]
    fn note_numbers_cover_84_notes_from_c0() {
        let hz = |text: &str| tone_of(&mml(text)[0]).frequency_hz;
        assert_eq!(hz("N1"), hz("O0 C"));
        assert_eq!(hz("N37"), hz("O3 C"));
        assert_eq!(hz("N46"), 440.0);
        assert_eq!(hz("N84"), hz("O6 B"));
    }

    #[test]
    fn note_number_zero_is_a_rest_and_85_is_an_error() {
        assert_eq!(
            mml("N0"),
            vec![AudioEvent::Rest {
                duration_samples: 22_050
            }]
        );
        let error = mml_error_of("N85");
        assert_eq!(error.position, 1);
        assert!(error.message.contains("0 to 84"), "{error}");
    }

    #[test]
    fn rests_and_volume_and_waveform_are_carried_into_events() {
        let events = parse_mml("V20 C P8 V80 ML D", 50, Waveform::Sine).unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(tone_of(&events[0]).volume, 20);
        assert_eq!(tone_of(&events[0]).waveform, Waveform::Sine);
        assert_eq!(events[2].samples(), 11_025, "an eighth note rest");
        assert_eq!(events[3].samples(), 22_050, "legato quarter note");
        assert!(matches!(events[1], AudioEvent::Rest { .. }));
        assert_eq!(tone_of(&events[3]).volume, 80);
        // The default volume is the caller's.
        let default = parse_mml("C", 33, Waveform::Square).unwrap();
        assert_eq!(tone_of(&default[0]).volume, 33);
        // The state does not leak out of a string.
        let again = parse_mml("C", 33, Waveform::Square).unwrap();
        assert_eq!(tone_of(&again[0]).duration_samples, 19_294);
    }

    #[test]
    fn errors_name_the_position_of_the_offending_command() {
        let cases = [
            ("C X", 3, "unknown command 'X'"),
            ("CDE 4 4", 7, "unknown command '4'"),
            ("T300", 1, "tempo must be from 32 to 255, got 300"),
            ("T31", 1, "tempo"),
            ("C D T0", 5, "tempo"),
            ("O7", 1, "octave must be from 0 to 6, got 7"),
            ("O", 1, "O needs a number"),
            ("L0", 1, "length must be from 1 to 64, got 0"),
            ("L65", 1, "length"),
            ("  C65", 3, "length must be from 1 to 64, got 65"),
            ("C4....", 1, "at most 3 dots"),
            ("V101", 1, "volume must be from 0 to 100, got 101"),
            ("MX", 1, "M must be followed by"),
            ("M", 1, "M must be followed by"),
            ("<<<<<", 5, "leaves the octave range"),
            (">>>", 3, "leaves the octave range"),
            ("O6 >", 4, "leaves the octave range"),
            ("O0 C-", 4, "outside the playable range"),
            ("O6 B#", 4, "outside the playable range"),
            ("C--", 3, "unknown command '-'"),
            ("\u{e9}", 1, "unknown command"),
            ("N", 1, "N needs a number"),
            ("N4.", 3, "unknown command '.'"),
            ("L4.", 3, "unknown command '.'"),
            ("C99999999999", 1, "length"),
        ];
        for (text, position, fragment) in cases {
            let error = mml_error_of(text);
            assert_eq!(error.position, position, "{text:?}: {error}");
            assert!(error.message.contains(fragment), "{text:?}: {error}");
        }
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        let error = mml_error_of("C\u{e9}C Z");
        assert_eq!(error.position, 2);
        let error = mml_error_of("\u{3b1}\u{3b2} X");
        assert_eq!(error.position, 1);
        let error = mml_error_of("C \u{1F3B5} X");
        assert_eq!(error.position, 3);
    }

    #[test]
    fn mml_error_display_includes_the_position() {
        assert_eq!(
            mml_error_of("Z").to_string(),
            "position 1: unknown command 'Z'"
        );
    }

    #[test]
    fn the_length_only_scan_agrees_with_the_parser() {
        for text in [
            "",
            "C",
            "T200 L8 C.. D#4 ML E MS F N40 N0 R2.. P8 < > O2 V10 A- B+",
            "t90 l16 c d e f g a b > c",
        ] {
            assert_eq!(
                mml_length_samples(text).unwrap(),
                mml_samples(text),
                "{text:?}"
            );
        }
        // The same errors at the same positions.
        for text in [
            "T300", "C D O7", "MX", "ZZ", "L0", "C....", "V101", "N85", "C é Z",
        ] {
            assert_eq!(
                mml_length_samples(text).unwrap_err(),
                mml_error_of(text),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_string_longer_than_the_queue_is_given_up_on_at_the_limit() {
        // A default quarter note is 22050 samples: exactly 1200 fit.
        let fits = "C".repeat(1200);
        assert_eq!(
            mml(&fits).iter().map(AudioEvent::samples).sum::<u64>(),
            MAX_QUEUE_SAMPLES
        );
        let too_long = "C".repeat(1201);
        let error = mml_error_of(&too_long);
        assert!(error.too_long);
        assert_eq!(error.position, 1201);
        // Syntax errors still win, wherever they are in the string.
        let invalid = format!("{too_long}Z");
        let error = mml_error_of(&invalid);
        assert!(!error.too_long);
        assert_eq!(error.to_string(), "position 1202: unknown command 'Z'");
        assert!(!mml_error_of("T300").too_long);
    }

    #[test]
    fn two_million_notes_are_refused_and_measured_without_building_events() {
        let started = std::time::Instant::now();
        let text = "C".repeat(2_000_000);
        let error = mml_error_of(&text);
        assert!(error.too_long);
        assert_eq!(error.position, 1201);
        assert_eq!(mml_length_samples(&text).unwrap(), 2_000_000 * 22_050);
        assert!(started.elapsed().as_secs() < 10);
    }

    // --- headless backend ---
    #[test]
    fn virtual_queue_never_drifts() {
        let mut queue = VirtualQueue::default();
        queue.push(4410);
        // 1 ms = 44.1 samples: ten 1 ms steps play exactly 441 samples.
        for _ in 0..10 {
            queue.advance(1);
        }
        assert_eq!(queue.pending, 4410 - 441);
        for _ in 0..90 {
            queue.advance(1);
        }
        assert_eq!(queue.pending, 0);
        // An idle queue ignores time and keeps no credit.
        queue.advance(1000);
        queue.push(100);
        queue.advance(1);
        assert_eq!(queue.pending, 100 - 44);
    }

    #[test]
    fn headless_audio_tracks_a_virtual_clock() {
        let audio = HeadlessAudio::new();
        let mut backend: Box<dyn AudioBackend> = Box::new(audio.clone());
        assert!(!audio.is_playing());
        backend
            .play(&[AudioEvent::Tone(square(440.0, 100, 50))])
            .unwrap();
        assert!(audio.is_playing());
        assert!(backend.is_playing());
        assert_eq!(audio.queued_ms(), 100);
        assert_eq!(audio.queued_samples(), 4410);
        backend.elapse(30);
        assert_eq!(audio.queued_ms(), 70);
        assert_eq!(audio.queued_samples(), 3087);
        audio.advance(69);
        // 99 ms = 4365.9 samples: 4365 have played, 45 remain (just over 1 ms).
        assert_eq!(audio.queued_samples(), 45);
        assert_eq!(audio.queued_ms(), 2);
        assert!(audio.is_playing());
        audio.advance(1);
        assert!(!audio.is_playing());
        assert_eq!(audio.clock_ms(), 100);
        assert!(!backend.is_real_time());
        // Queued events play after one another.
        backend
            .play(&[
                AudioEvent::Tone(square(440.0, 100, 50)),
                AudioEvent::rest_ms(50),
            ])
            .unwrap();
        assert_eq!(audio.queued_ms(), 150);
        assert_eq!(audio.events().len(), 3);
        assert_eq!(audio.total_ms(), 250);
    }

    #[test]
    fn headless_stop_cuts_the_audible_audio_at_the_played_position() {
        let audio = HeadlessAudio::new();
        let mut backend = audio.clone();
        backend
            .play(&[
                AudioEvent::Tone(square(440.0, 100, 50)),
                AudioEvent::Tone(square(880.0, 100, 50)),
                AudioEvent::rest_ms(100),
            ])
            .unwrap();
        audio.advance(150);
        backend.stop();
        assert_eq!(audio.stops(), 1);
        assert!(!audio.is_playing());
        assert_eq!(audio.events().len(), 3, "history is kept");
        let audible = audio.audible_events();
        assert_eq!(audible.len(), 2);
        assert_eq!(audible[0].samples(), 4410);
        assert_eq!(audible[1].samples(), 2205);
        assert_eq!(tone_of(&audible[1]).frequency_hz, 880.0);
        assert_eq!(audio.rendered_samples().len(), 6615);
        // Nothing left to stop: a second stop changes nothing audible.
        backend.stop();
        assert_eq!(audio.rendered_samples().len(), 6615);
        // Stopping before anything played removes the whole event.
        backend
            .play(&[AudioEvent::Tone(square(440.0, 10, 50))])
            .unwrap();
        backend.stop();
        assert_eq!(audio.audible_events().len(), 2);
        assert_eq!(audio.events().len(), 4);
    }

    #[test]
    fn a_stopped_tone_renders_as_an_abrupt_prefix_and_streams_the_same_wav() {
        let audio = HeadlessAudio::new();
        let mut backend = audio.clone();
        let tone = square(440.0, 1000, 100);
        backend.play(&[AudioEvent::Tone(tone)]).unwrap();
        audio.advance(300);
        backend.stop();
        let whole = pcm(tone);
        let heard = audio.rendered_samples();
        assert_eq!(heard, whole[..13_230]);
        assert_eq!(heard[13_229].abs(), 30_000, "no fade at the cut");
        // The event list is shortened, as documented.
        assert_eq!(audio.audible_events()[0].samples(), 13_230);
        let mut wav = Vec::new();
        assert_eq!(audio.write_wav(&mut wav).unwrap(), 13_230);
        assert_eq!(wav, wav_bytes(&heard));
    }

    #[test]
    fn headless_ignores_zero_length_events() {
        let mut audio = HeadlessAudio::new();
        audio
            .play(&[AudioEvent::Rest {
                duration_samples: 0,
            }])
            .unwrap();
        assert!(audio.events().is_empty());
        assert!(!audio.is_playing());
    }

    #[test]
    fn null_audio_has_the_same_clock_but_remembers_nothing() {
        let mut audio = NullAudio::new();
        audio
            .play(&[AudioEvent::Tone(square(440.0, 100, 50))])
            .unwrap();
        assert_eq!(audio.queued_ms(), 100);
        audio.elapse(40);
        assert_eq!(audio.queued_ms(), 60);
        assert!(audio.is_playing());
        audio.stop();
        assert!(!audio.is_playing());
        assert!(!audio.is_real_time());
    }

    #[test]
    fn rendered_samples_of_the_headless_backend_equal_the_render_function() {
        let events = mml("T180 L8 CDEFG");
        let mut audio = HeadlessAudio::new();
        audio.play(&events).unwrap();
        assert_eq!(audio.rendered_samples(), render(&events));
        let wav = wav_bytes(&audio.rendered_samples());
        assert_eq!(wav.len(), 44 + 2 * audio.rendered_samples().len());
    }

    // --- clips ---

    #[test]
    fn a_clip_is_its_samples_scaled_by_the_volume_percent() {
        let samples: Vec<i16> = vec![0, 1, -1, 100, -100, 32_767, -32_768, 333];
        let full = render(&[AudioEvent::clip(samples.clone(), 100)]);
        assert_eq!(full, samples);
        let silent = render(&[AudioEvent::clip(samples.clone(), 0)]);
        assert_eq!(silent, vec![0; samples.len()]);
        let half = render(&[AudioEvent::clip(samples.clone(), 50)]);
        // Halves round away from zero: 1 -> 0.5 -> 1, -1 -> -1, 333 -> 166.5 -> 167.
        assert_eq!(half, vec![0, 1, -1, 50, -50, 16_384, -16_384, 167]);
        let tenth = render(&[AudioEvent::clip(samples, 10)]);
        assert_eq!(tenth, vec![0, 0, 0, 10, -10, 3_277, -3_277, 33]);
    }

    #[test]
    fn a_clip_volume_above_the_maximum_is_clamped() {
        let event = AudioEvent::clip(vec![1000i16], 250);
        assert_eq!(render(&[event]), vec![1000]);
    }

    #[test]
    fn clips_are_cut_and_padded_like_any_event() {
        let event = AudioEvent::clip(vec![10i16, 20, 30, 40], 100);
        assert_eq!(event.samples(), 4);
        assert_eq!(render(&[event.with_samples(2)]), vec![10, 20]);
        assert_eq!(render(&[event.with_samples(6)]), vec![10, 20, 30, 40, 0, 0]);
        assert_eq!(render(&[event.with_samples(0)]), Vec::<i16>::new());
    }

    #[test]
    fn rendering_with_clips_is_still_concatenative() {
        let tone = AudioEvent::Tone(Tone::new(440.0, 20, 50, Waveform::Sine));
        let clip = AudioEvent::clip(vec![5i16, -5, 7], 100);
        let both = render(&[
            tone.clone(),
            clip.clone(),
            AudioEvent::rest_ms(1),
            tone.clone(),
        ]);
        let mut parts = render(std::slice::from_ref(&tone));
        parts.extend(render(&[clip]));
        parts.extend(render(&[AudioEvent::rest_ms(1)]));
        parts.extend(render(&[tone]));
        assert_eq!(both, parts);
    }

    #[test]
    fn the_headless_backend_trims_a_stopped_clip() {
        let mut audio = HeadlessAudio::new();
        let clip = AudioEvent::clip((1..=441).map(|v| v as i16).collect::<Vec<_>>(), 100);
        audio.play(&[clip]).unwrap();
        audio.advance(4); // 176.4 samples
        audio.stop();
        assert_eq!(audio.rendered_samples(), (1..=176).collect::<Vec<i16>>());
        assert_eq!(audio.queued_samples(), 0);
    }

    // --- WAV writing ---

    #[test]
    fn write_wav_matches_wav_bytes() {
        let events = mml("T200 L16 CDE");
        let mut streamed = Vec::new();
        let count = write_wav(&events, &mut streamed).unwrap();
        let expected = render(&events);
        assert_eq!(count, expected.len() as u64);
        assert_eq!(streamed, wav_bytes(&expected));
        let mut empty = Vec::new();
        assert_eq!(write_wav(&[], &mut empty).unwrap(), 0);
        assert_eq!(empty, wav_bytes(&[]));
        assert_eq!(empty.len(), 44);
    }

    #[test]
    fn write_wav_reports_write_errors_and_oversized_audio() {
        struct Failing;
        impl std::io::Write for Failing {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let error = write_wav(&mine(), &mut Failing).unwrap_err();
        assert_eq!(error.to_string(), "disk full");
        // 3 GiB of silence in two events does not fit; nothing is written.
        let huge = AudioEvent::Rest {
            duration_samples: 1_200_000_000,
        };
        let mut sink = Vec::new();
        let error = write_wav(&[huge.clone(), huge], &mut sink).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(sink.is_empty());
    }

    fn mine() -> Vec<AudioEvent> {
        mml("CDE")
    }

    // --- drain ---

    #[test]
    fn draining_a_virtual_backend_plays_the_queue_out_at_once() {
        let mut audio = HeadlessAudio::new();
        audio.play(&[AudioEvent::rest_ms(500)]).unwrap();
        assert!(audio.drain(10_000));
        assert_eq!(audio.queued_samples(), 0);
        assert_eq!(audio.clock_ms(), 500);
        let mut null = NullAudio::new();
        null.play(&[AudioEvent::rest_ms(500)]).unwrap();
        assert!(!null.drain(100));
        assert_eq!(null.queued_ms(), 400);
        assert!(null.drain(400));
    }

    // --- deferred opening ---

    #[test]
    fn a_deferred_backend_opens_on_the_first_sound_only() {
        let opened = Rc::new(RefCell::new(0));
        let failures = Rc::new(RefCell::new(Vec::<String>::new()));
        let shared = HeadlessAudio::new();
        let probe = shared.clone();
        let count = Rc::clone(&opened);
        let mut audio = DeferredAudio::new(
            Box::new(move || {
                *count.borrow_mut() += 1;
                Ok(Box::new(shared) as Box<dyn AudioBackend>)
            }),
            Box::new({
                let failures = Rc::clone(&failures);
                move |reason| failures.borrow_mut().push(reason.to_string())
            }),
        );
        // Nothing opens for queries, stop, elapse or drain.
        assert_eq!(audio.queued_samples(), 0);
        audio.stop();
        audio.elapse(10);
        assert!(audio.drain(10));
        assert!(!audio.is_open());
        assert_eq!(*opened.borrow(), 0);
        audio.play(&[AudioEvent::rest_ms(100)]).unwrap();
        audio.play(&[AudioEvent::rest_ms(100)]).unwrap();
        assert!(audio.is_open());
        assert_eq!(*opened.borrow(), 1);
        assert_eq!(probe.events().len(), 2);
        assert_eq!(audio.queued_ms(), 200);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_failing_open_warns_once_and_falls_back_to_silent_virtual_time() {
        let failures = Rc::new(RefCell::new(Vec::<String>::new()));
        let mut audio = DeferredAudio::new(
            Box::new(|| Err("no device".to_string())),
            Box::new({
                let failures = Rc::clone(&failures);
                move |reason| failures.borrow_mut().push(reason.to_string())
            }),
        );
        for _ in 0..3 {
            audio.play(&[AudioEvent::rest_ms(1000)]).unwrap();
        }
        assert_eq!(*failures.borrow(), vec!["no device".to_string()]);
        assert!(!audio.is_open());
        assert!(!audio.is_real_time());
        assert_eq!(audio.queued_ms(), 3000);
        audio.elapse(1000);
        assert_eq!(audio.queued_ms(), 2000);
        assert!(audio.drain(5000));
    }
}
