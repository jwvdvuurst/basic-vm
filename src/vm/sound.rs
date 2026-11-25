//! Sound statements and functions: `BEEP`, `SOUND`, `PLAY`, `PLAYWAV`, `WAVE`,
//! `VOLUME`, `STOPSOUND`, `SOUNDWAIT`, `SOUNDPLAYING()`, `SOUNDQUEUE()`,
//! `NOTEFREQ()`, `MUSICLENGTH()` and `WAVLENGTH()`. The audio itself lives in
//! [`crate::audio`] and WAV decoding in [`crate::wav`]; this module
//! only validates arguments, builds events and talks to the backend.

use std::io::{Read, Write};

use super::media::who_error;
use super::{Vm, boolean, number};
use crate::Builtin;
use crate::audio::{
    AudioBackend, AudioEvent, BEEP_DURATION_MS, BEEP_FREQUENCY_HZ, DEFAULT_VOLUME,
    MAX_FREQUENCY_HZ, MAX_QUEUE_MS, MAX_QUEUE_SAMPLES, MAX_TONE_MS, MAX_VOLUME, MIN_FREQUENCY_HZ,
    MmlError, NullAudio, Tone, Waveform, mml_length_samples, note_frequency, parse_mml,
    samples_to_ms_round,
};
use crate::files::describe;
use crate::value::Value;
use crate::wav::{MAX_WAV_FILE_BYTES, decode_wav_samples, wav_info};

type Result<T> = std::result::Result<T, String>;

/// The error for sound that does not fit in the queue.
fn queue_full(who: &str) -> String {
    who_error(
        who,
        format!(
            "the sound queue is full (at most {} seconds may wait; use SOUNDWAIT)",
            MAX_QUEUE_MS / 1000
        ),
    )
}

fn music_error(who: &str, error: &MmlError) -> String {
    if error.too_long {
        return queue_full(who);
    }
    who_error(
        who,
        format!(
            "invalid music string at position {}: {}",
            error.position, error.message
        ),
    )
}

/// A number for an error message: as written when it is short, otherwise in
/// compact scientific form (`1e+300`, `-2.5e-7`), so that a huge value never
/// prints hundreds of digits or a saturated integer. Not-a-number and
/// infinities print as `NaN`, `inf` and `-inf`.
fn show_number(x: f64) -> String {
    let magnitude = x.abs();
    if !x.is_finite() || x == 0.0 || (1e-4..1e15).contains(&magnitude) {
        return x.to_string();
    }
    let text = format!("{x:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if !exponent.starts_with('-') => {
            format!("{mantissa}e+{exponent}")
        }
        _ => text,
    }
}

/// A whole number within `low..=high` (numbers truncate toward zero). Unlike
/// `media::ranged`, an error reports the value the program wrote, shown by
/// [`show_number`], and not the integer it saturates to.
fn whole_in_range(value: Value, who: &str, what: &str, low: i64, high: i64) -> Result<i64> {
    let out_of_range = |shown: String| {
        who_error(
            who,
            format!("{what} must be from {low} to {high}, got {shown}"),
        )
    };
    let n = match value {
        Value::Int(n) => n,
        other => {
            let x =
                number(other).map_err(|_| who_error(who, format!("{what} must be a number")))?;
            let whole = x.trunc();
            // Every range used here is far below 1e15, so a value this large
            // is out of range and is shown as the user wrote it.
            if !whole.is_finite() || whole.abs() >= 1e15 {
                return Err(out_of_range(show_number(x)));
            }
            whole as i64
        }
    };
    if !(low..=high).contains(&n) {
        return Err(out_of_range(n.to_string()));
    }
    Ok(n)
}

/// Longest slice a real-time `SOUNDWAIT` sleeps at once, in milliseconds, so
/// that the screen keeps being serviced while it waits.
const WAIT_SLICE_MS: u64 = 100;
/// Highest note number `NOTEFREQ` accepts (the top of the MIDI range).
const MAX_NOTE: f64 = 127.0;

pub(super) struct Sound {
    pub(super) backend: Box<dyn AudioBackend>,
    /// `VOLUME`: the volume of sounds queued without an explicit one.
    volume: u8,
    /// `WAVE`: the waveform of `SOUND` and `PLAY`.
    waveform: Waveform,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            backend: Box::new(NullAudio::new()),
            volume: DEFAULT_VOLUME,
            waveform: Waveform::default(),
        }
    }
}

impl Vm {
    /// Installs the backend that receives queued sound. Without one, sound
    /// statements are accepted and silently timed by a [`NullAudio`] sink, so a
    /// program never fails for lack of an audio device.
    pub fn set_audio_backend(&mut self, backend: Box<dyn AudioBackend>) {
        self.sound.backend = backend;
    }

    /// Milliseconds of queued sound still to play (rounded up). A host that
    /// plays in real time uses this after the program ends to let the last
    /// sounds finish; the VM itself never waits at the end of a run.
    pub fn audio_queued_ms(&self) -> u64 {
        self.sound.backend.queued_ms()
    }

    /// Ends the program's sound. With `wait` and a real-time backend the
    /// queue is first played out (at most its own length plus a short margin,
    /// and never longer than the queue limit), so a final `PLAY` is heard;
    /// then whatever is left is stopped. Virtual-clock backends never wait.
    pub fn finish_audio(&mut self, wait: bool) {
        if wait && self.sound.backend.is_real_time() {
            let limit = (self.sound.backend.queued_ms() + 2_000).min(MAX_QUEUE_MS + 2_000);
            self.sound.backend.drain(limit);
        }
        self.sound.backend.stop();
    }

    /// Hands `events` to the backend unless the queue would exceed
    /// [`MAX_QUEUE_MS`]; nothing is queued then (so `PLAY` is all or nothing).
    fn queue_audio(&mut self, who: &str, events: &[AudioEvent]) -> Result<()> {
        let added: u64 = events.iter().map(AudioEvent::samples).sum();
        if added == 0 {
            return Ok(());
        }
        if self.sound.backend.queued_samples() + added > MAX_QUEUE_SAMPLES {
            return Err(queue_full(who));
        }
        self.sound
            .backend
            .play(events)
            .map_err(|e| who_error(who, e))
    }

    fn text_argument(&self, value: Value, who: &str, what: &str) -> Result<String> {
        Ok(self.text_ref(value, who, what)?.to_string())
    }

    /// Like [`Vm::text_argument`] without copying the string.
    fn text_ref(&self, value: Value, who: &str, what: &str) -> Result<&str> {
        match value {
            Value::String(reference) => Ok(self.heap.get(reference)?),
            _ => Err(who_error(who, format!("{what} must be a string"))),
        }
    }

    /// Parses the music string argument. A string that is valid but plays for
    /// more than the whole queue holds is the same "queue is full" error as a
    /// `PLAY` that does not fit, and nothing is built for it.
    fn music(&self, value: Value, who: &str, volume: u8) -> Result<Vec<AudioEvent>> {
        let text = self.text_ref(value, who, "the music string")?;
        parse_mml(text, volume, self.sound.waveform).map_err(|e| music_error(who, &e))
    }

    pub(super) fn sound_builtin(
        &mut self,
        builtin: Builtin,
        argument_count: usize,
        output: &mut impl Write,
    ) -> Result<Option<Value>> {
        let args = self.pop_arguments(argument_count)?;
        let who = builtin.name();
        let value = match builtin {
            Builtin::Beep => {
                let tone = Tone::new(
                    BEEP_FREQUENCY_HZ,
                    BEEP_DURATION_MS,
                    self.sound.volume,
                    Waveform::Square,
                );
                self.queue_audio(who, &[AudioEvent::Tone(tone)])?;
                return Ok(None);
            }
            Builtin::Sound => {
                let frequency =
                    number(args[0]).map_err(|_| who_error(who, "frequency must be a number"))?;
                let valid =
                    frequency == 0.0 || (MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&frequency);
                if !valid {
                    return Err(who_error(
                        who,
                        format!(
                            "frequency must be 0 (a rest) or from {MIN_FREQUENCY_HZ} to \
                             {MAX_FREQUENCY_HZ} Hz, got {}",
                            show_number(frequency)
                        ),
                    ));
                }
                let duration =
                    whole_in_range(args[1], who, "duration", 0, i64::from(MAX_TONE_MS))? as u32;
                let volume = match args.get(2) {
                    Some(value) => {
                        whole_in_range(*value, who, "volume", 0, i64::from(MAX_VOLUME))? as u8
                    }
                    None => self.sound.volume,
                };
                let event = if frequency == 0.0 {
                    AudioEvent::rest_ms(duration)
                } else {
                    AudioEvent::Tone(Tone::new(frequency, duration, volume, self.sound.waveform))
                };
                self.queue_audio(who, &[event])?;
                return Ok(None);
            }
            Builtin::Play => {
                let events = self.music(args[0], who, self.sound.volume)?;
                self.queue_audio(who, &events)?;
                return Ok(None);
            }
            Builtin::Wave => {
                let name = self.text_argument(args[0], who, "the waveform")?;
                let Some(waveform) = Waveform::parse(name.trim()) else {
                    return Err(who_error(
                        who,
                        format!(
                            "unknown waveform \"{name}\" (use SQUARE, SINE, TRIANGLE, SAW or NOISE)"
                        ),
                    ));
                };
                self.sound.waveform = waveform;
                return Ok(None);
            }
            Builtin::Volume => {
                self.sound.volume =
                    whole_in_range(args[0], who, "volume", 0, i64::from(MAX_VOLUME))? as u8;
                return Ok(None);
            }
            Builtin::StopSound => {
                self.sound.backend.stop();
                return Ok(None);
            }
            Builtin::SoundWait => {
                // A real-time backend is waited for in short slices (so the
                // screen stays serviced) unless tests skip sleeps; every
                // other backend is on a virtual clock, which jumps to the end
                // of the queue at once.
                let real_time = self.sound.backend.is_real_time() && !self.test.no_sleep;
                loop {
                    let queued = self.sound.backend.queued_ms();
                    if queued == 0 {
                        break;
                    }
                    if real_time {
                        self.pause(queued.min(WAIT_SLICE_MS), output, false)?;
                    } else {
                        self.pause(queued, output, true)?;
                    }
                }
                return Ok(None);
            }
            Builtin::SoundPlaying => boolean(self.sound.backend.is_playing()),
            Builtin::SoundQueue => Value::Number(self.sound.backend.queued_ms() as f64),
            Builtin::NoteFreq => {
                // Fractions bend the pitch, so only the range is checked.
                let note = number(args[0]).map_err(|_| who_error(who, "note must be a number"))?;
                if !(0.0..=MAX_NOTE).contains(&note) {
                    return Err(who_error(
                        who,
                        format!(
                            "note must be from 0 to {MAX_NOTE}, got {}",
                            show_number(note)
                        ),
                    ));
                }
                Value::Number(note_frequency(note))
            }
            Builtin::MusicLength => {
                // The length is summed without building the events, so it
                // works (in constant memory) for strings far longer than
                // PLAY accepts; it is exact, not capped at the queue limit.
                let text = self.text_ref(args[0], who, "the music string")?;
                let samples = mml_length_samples(text).map_err(|e| music_error(who, &e))?;
                Value::Number(samples_to_ms_round(samples) as f64)
            }
            Builtin::PlayWav => {
                let path = self.text_argument(args[0], who, "the file name")?;
                let bytes = read_wav_file(who, &path)?;
                let samples = decode_wav_samples(&bytes)
                    .map_err(|e| who_error(who, format!("\"{path}\": {e}")))?;
                drop(bytes);
                self.queue_audio(who, &[AudioEvent::clip(samples, self.sound.volume)])?;
                return Ok(None);
            }
            Builtin::WavLength => {
                let path = self.text_argument(args[0], who, "the file name")?;
                let bytes = read_wav_file(who, &path)?;
                let info =
                    wav_info(&bytes).map_err(|e| who_error(who, format!("\"{path}\": {e}")))?;
                Value::Number(info.duration_ms() as f64)
            }
            other => return Err(format!("{}: not a sound routine", other.name())),
        };
        Ok(Some(value))
    }
}

/// Reads a WAV file for `PLAYWAV` and `WAVLENGTH`: a regular file of at most
/// [`MAX_WAV_FILE_BYTES`] (so a directory, a device or a huge file never
/// blocks or exhausts memory).
fn read_wav_file(who: &str, path: &str) -> Result<Vec<u8>> {
    let fail = |reason: String| who_error(who, format!("cannot read \"{path}\": {reason}"));
    if path.trim().is_empty() {
        return Err(who_error(who, "the file name is empty"));
    }
    let metadata = std::fs::metadata(path).map_err(|e| fail(describe(&e)))?;
    if metadata.is_dir() {
        return Err(fail("it is a directory".to_string()));
    }
    if !metadata.is_file() {
        return Err(fail("it is not a regular file".to_string()));
    }
    if metadata.len() > MAX_WAV_FILE_BYTES {
        return Err(fail(format!(
            "the file is too large ({} bytes; at most {} MiB are read)",
            metadata.len(),
            MAX_WAV_FILE_BYTES / (1024 * 1024)
        )));
    }
    let file = std::fs::File::open(path).map_err(|e| fail(describe(&e)))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    // The file may have grown since `metadata`; never read more than the cap.
    file.take(MAX_WAV_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail(describe(&e)))?;
    if bytes.len() as u64 > MAX_WAV_FILE_BYTES {
        return Err(fail(format!(
            "the file is too large (at most {} MiB are read)",
            MAX_WAV_FILE_BYTES / (1024 * 1024)
        )));
    }
    Ok(bytes)
}
