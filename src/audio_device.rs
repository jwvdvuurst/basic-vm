//! Real-time audio output (feature `audio`): [`DeviceAudio`] plays the sound
//! queue through the default output device using `cpal` (WASAPI on Windows,
//! CoreAudio on macOS, ALSA on Linux).
//!
//! # Design
//!
//! `play` renders its events to mono 16-bit PCM at [`SAMPLE_RATE`] on the
//! calling thread and appends the result to a queue guarded by a mutex. The
//! device's own audio thread pulls from that queue in its data callback and
//! converts on the fly to whatever the device really runs at (often 48 kHz
//! stereo, `f32`): linear resampling in exact integer phase arithmetic, the
//! mono signal copied to every channel, silence when the queue is empty.
//!
//! * Everything queued plays **sequentially, in order**: `SOUND`, `PLAY` and
//!   `PLAYWAV` all go through the same queue and never overlap (the same
//!   semantic as the headless backends). `BEEP` and friends just append.
//! * `stop` empties the queue and cuts the sound immediately.
//! * [`AudioBackend::queued_samples`] is what the audio thread has not pulled
//!   yet, so `SOUNDPLAYING()`, `SOUNDQUEUE()` and `SOUNDWAIT` follow real time.
//! * The callback never panics and never blocks on anything but the queue's
//!   mutex (held only while copying). A fatal stream error (device unplugged
//!   and the like) or a stalled device marks the backend *dead*: later sound
//!   is dropped, the queue reads as empty so `SOUNDWAIT` cannot hang, and the
//!   program carries on silently.
//! * Dropping a [`DeviceAudio`] stops and drops the stream, which joins the
//!   host's audio thread: nothing is left running.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::audio::{AudioBackend, AudioEvent, MAX_QUEUE_SAMPLES, SAMPLE_RATE, render};

/// How long the audio thread may stay silent (not call back at all) while
/// sound is queued before the device is considered dead.
const STALL_MS: u64 = 5_000;
/// Extra time [`DeviceAudio::drain`] waits after the queue is empty so the
/// device's own buffer plays out too.
const TAIL_MS: u64 = 150;
/// Polling interval while waiting for the queue to drain.
const POLL_MS: u64 = 5;

/// One rendered chunk of the queue.
struct Segment {
    samples: Vec<i16>,
    position: usize,
}

/// The queue plus the resampler state; shared between the VM thread and the
/// audio thread behind a mutex. All of it is plain data, so it is tested
/// without a device.
struct Playback {
    queue: VecDeque<Segment>,
    /// Samples queued and not yet pulled by the resampler.
    remaining: u64,
    device_rate: u64,
    channels: usize,
    /// The source samples on either side of the current output position.
    before: i32,
    after: i32,
    /// Position between `before` and `after` in units of `1 / device_rate`.
    phase: u64,
}

impl Playback {
    fn new(device_rate: u32, channels: u16) -> Self {
        Self {
            queue: VecDeque::new(),
            remaining: 0,
            device_rate: u64::from(device_rate.max(1)),
            channels: usize::from(channels.max(1)),
            before: 0,
            after: 0,
            phase: 0,
        }
    }

    fn push(&mut self, samples: Vec<i16>) {
        self.remaining += samples.len() as u64;
        self.queue.push_back(Segment {
            samples,
            position: 0,
        });
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.remaining = 0;
        self.before = 0;
        self.after = 0;
        self.phase = 0;
    }

    /// The next source sample, or 0 when the queue is empty.
    fn pull(&mut self) -> i32 {
        while let Some(front) = self.queue.front_mut() {
            if let Some(sample) = front.samples.get(front.position) {
                front.position += 1;
                self.remaining -= 1;
                return i32::from(*sample);
            }
            self.queue.pop_front();
        }
        0
    }

    /// The next output frame (one mono value) on the 16-bit scale `-1..1`.
    fn next_frame(&mut self) -> f32 {
        self.phase += u64::from(SAMPLE_RATE);
        while self.phase >= self.device_rate {
            self.phase -= self.device_rate;
            self.before = self.after;
            self.after = self.pull();
        }
        let span = (self.after - self.before) as f32;
        let fraction = self.phase as f32 / self.device_rate as f32;
        (self.before as f32 + span * fraction) / 32_768.0
    }

    /// Fills an interleaved device buffer.
    fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        for frame in data.chunks_mut(self.channels) {
            frame.fill(T::from_sample(self.next_frame()));
        }
    }
}

/// State shared with the audio thread.
struct Shared {
    playback: Mutex<Playback>,
    /// A fatal stream error happened; the device is unusable.
    dead: AtomicBool,
    failure: Mutex<Option<String>>,
    epoch: Instant,
    /// Milliseconds since `epoch` of the last data callback.
    last_callback_ms: AtomicU64,
}

impl Shared {
    fn new(device_rate: u32, channels: u16) -> Self {
        Self {
            playback: Mutex::new(Playback::new(device_rate, channels)),
            dead: AtomicBool::new(false),
            failure: Mutex::new(None),
            epoch: Instant::now(),
            last_callback_ms: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Playback> {
        self.playback.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn mark_dead(&self, reason: String) {
        let mut failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
        failure.get_or_insert(reason);
        self.dead.store(true, Ordering::SeqCst);
    }

    fn is_dead(&self) -> bool {
        self.dead.load(Ordering::SeqCst)
    }

    /// Dead, or alive? A device that has stopped calling back while sound is
    /// queued counts as dead (and stays dead).
    fn check_alive(&self) -> bool {
        if self.is_dead() {
            return false;
        }
        let stalled = self
            .now_ms()
            .saturating_sub(self.last_callback_ms.load(Ordering::Relaxed))
            > STALL_MS;
        if stalled && self.lock().remaining > 0 {
            self.mark_dead("the audio device stopped responding".to_string());
            return false;
        }
        true
    }

    fn data_callback<T: SizedSample + FromSample<f32>>(&self, data: &mut [T]) {
        self.last_callback_ms
            .store(self.now_ms(), Ordering::Relaxed);
        self.lock().fill(data);
    }
}

/// Builds the output stream for one concrete sample type.
fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, cpal::Error> {
    let for_data = Arc::clone(shared);
    let for_errors = Arc::clone(shared);
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _| for_data.data_callback(data),
        move |error: cpal::Error| {
            use cpal::ErrorKind::{DeviceBusy, DeviceChanged, RealtimeDenied, Xrun};
            // Hiccups are not fatal; everything else is.
            if !matches!(
                error.kind(),
                DeviceBusy | DeviceChanged | RealtimeDenied | Xrun
            ) {
                for_errors.mark_dead(error.to_string());
            }
        },
        None,
    )
}

fn build_for_format(
    device: &cpal::Device,
    format: SampleFormat,
    config: StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, String> {
    let stream = match format {
        SampleFormat::F32 => build::<f32>(device, config, shared),
        SampleFormat::I16 => build::<i16>(device, config, shared),
        SampleFormat::U16 => build::<u16>(device, config, shared),
        SampleFormat::I32 => build::<i32>(device, config, shared),
        SampleFormat::F64 => build::<f64>(device, config, shared),
        SampleFormat::U8 => build::<u8>(device, config, shared),
        SampleFormat::I8 => build::<i8>(device, config, shared),
        SampleFormat::U32 => build::<u32>(device, config, shared),
        SampleFormat::I64 => build::<i64>(device, config, shared),
        SampleFormat::U64 => build::<u64>(device, config, shared),
        other => return Err(format!("unsupported device sample format {other:?}")),
    };
    stream.map_err(|error| format!("cannot open the output stream: {error}"))
}

/// The real-time backend: see the [module documentation](self).
pub struct DeviceAudio {
    shared: Arc<Shared>,
    /// `None` only while dropping.
    stream: Option<cpal::Stream>,
    description: String,
    /// Sound was queued since the last [`AudioBackend::drain`].
    pushed: bool,
}

impl DeviceAudio {
    /// Opens the default output device and starts its (silent) stream.
    ///
    /// # Errors
    ///
    /// A one-line reason when there is no device or the stream cannot be
    /// created or started. This never panics, even when the host's audio
    /// library does.
    pub fn open() -> Result<Self, String> {
        match catch_unwind(AssertUnwindSafe(Self::open_inner)) {
            Ok(result) => result,
            Err(_) => Err("the audio library failed unexpectedly".to_string()),
        }
    }

    fn open_inner() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("cannot query the output device: {error}"))?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let shared = Arc::new(Shared::new(config.sample_rate, config.channels));
        let stream = build_for_format(&device, format, config, &shared)?;
        stream
            .play()
            .map_err(|error| format!("cannot start the output stream: {error}"))?;
        let description = device
            .description()
            .map(|description| description.name().to_string())
            .unwrap_or_else(|_| "default output device".to_string());
        Ok(Self {
            shared,
            stream: Some(stream),
            description: format!(
                "{description}, {} Hz, {} channel(s)",
                config.sample_rate, config.channels
            ),
            pushed: false,
        })
    }

    /// The device name and its format, for diagnostics.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Why the device stopped working, once a fatal error happened.
    pub fn failure(&self) -> Option<String> {
        self.shared
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AudioBackend for DeviceAudio {
    fn play(&mut self, events: &[AudioEvent]) -> Result<(), String> {
        let samples = render(events);
        if samples.is_empty() || !self.shared.check_alive() {
            return Ok(());
        }
        let mut playback = self.shared.lock();
        if playback.remaining + samples.len() as u64 > MAX_QUEUE_SAMPLES {
            return Err("the audio queue is full".to_string());
        }
        playback.push(samples);
        self.pushed = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.lock().clear();
    }

    fn queued_samples(&self) -> u64 {
        if self.shared.check_alive() {
            self.shared.lock().remaining
        } else {
            0
        }
    }

    fn is_real_time(&self) -> bool {
        true
    }

    fn drain(&mut self, max_ms: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(max_ms);
        while self.queued_samples() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(POLL_MS));
        }
        let drained = self.queued_samples() == 0;
        if drained && self.pushed && !self.shared.is_dead() {
            // The last samples are still in the device's buffer.
            std::thread::sleep(Duration::from_millis(TAIL_MS));
        }
        self.pushed = false;
        drained
    }
}

impl Drop for DeviceAudio {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            // Pausing is a courtesy (it may be unsupported); dropping joins
            // the host's audio thread.
            let _ = stream.pause();
            drop(stream);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_f32(playback: &mut Playback, frames: usize) -> Vec<f32> {
        let mut data = vec![f32::NAN; frames * playback.channels];
        playback.fill(&mut data);
        data
    }

    fn scaled(values: &[i16]) -> Vec<f32> {
        values.iter().map(|v| f32::from(*v) / 32_768.0).collect()
    }

    fn assert_close(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (index, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (a - e).abs() < 1e-5,
                "frame {index}: {actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn at_the_native_rate_the_samples_pass_through_one_frame_late() {
        let mut playback = Playback::new(44_100, 1);
        playback.push(vec![1000, -2000, 3000]);
        assert_eq!(playback.remaining, 3);
        let out = fill_f32(&mut playback, 6);
        assert_close(&out, &scaled(&[0, 1000, -2000, 3000, 0, 0]));
        assert_eq!(playback.remaining, 0);
    }

    #[test]
    fn an_idle_queue_is_silence() {
        let mut playback = Playback::new(48_000, 2);
        let out = fill_f32(&mut playback, 100);
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(playback.remaining, 0);
    }

    #[test]
    fn segments_play_back_to_back() {
        let mut playback = Playback::new(44_100, 1);
        playback.push(vec![1, 2]);
        playback.push(vec![]);
        playback.push(vec![3]);
        let out = fill_f32(&mut playback, 5);
        assert_close(&out, &scaled(&[0, 1, 2, 3, 0]));
    }

    #[test]
    fn doubling_the_rate_interpolates_linearly() {
        let mut playback = Playback::new(88_200, 1);
        playback.push(vec![1000, 3000]);
        let out = fill_f32(&mut playback, 8);
        assert_close(&out, &scaled(&[0, 0, 500, 1000, 2000, 3000, 1500, 0]));
    }

    #[test]
    fn halving_the_rate_takes_every_other_sample() {
        let mut playback = Playback::new(22_050, 1);
        playback.push(vec![10, 20, 30, 40, 50, 60]);
        let out = fill_f32(&mut playback, 4);
        assert_close(&out, &scaled(&[10, 30, 50, 0]));
    }

    #[test]
    fn mono_is_copied_to_every_channel() {
        for channels in [1u16, 2, 6] {
            let mut playback = Playback::new(44_100, channels);
            playback.push(vec![8192, -8192]);
            let out = fill_f32(&mut playback, 3);
            assert_eq!(out.len(), 3 * usize::from(channels));
            for (frame, expected) in out.chunks(usize::from(channels)).zip([0.0, 0.25, -0.25]) {
                assert!(
                    frame.iter().all(|v| (*v - expected).abs() < 1e-6),
                    "{frame:?}"
                );
            }
        }
    }

    #[test]
    fn a_device_at_48_khz_gets_the_right_number_of_frames_and_pitch() {
        // 1 kHz for 100 ms at 44.1 kHz, played on a 48 kHz stereo device.
        let samples: Vec<i16> = (0..4410)
            .map(|i| {
                (10_000.0 * (std::f64::consts::TAU * 1000.0 * f64::from(i) / 44_100.0).sin()) as i16
            })
            .collect();
        let mut playback = Playback::new(48_000, 2);
        playback.push(samples);
        let out = fill_f32(&mut playback, 4800 + 8);
        assert_eq!(playback.remaining, 0);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let crossings = left
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0) && w[0] != 0.0)
            .count();
        assert!((198..=201).contains(&crossings), "{crossings}");
        let peak = left.iter().fold(0.0f32, |peak, v| peak.max(v.abs()));
        assert!((0.29..=0.31).contains(&peak), "{peak}");
        // The audio ends: the last frames are silent.
        assert!(left[4805..].iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn clearing_cuts_the_sound_at_once() {
        let mut playback = Playback::new(44_100, 1);
        playback.push(vec![5000; 100]);
        fill_f32(&mut playback, 10);
        assert!(playback.remaining > 0);
        playback.clear();
        assert_eq!(playback.remaining, 0);
        assert!(fill_f32(&mut playback, 10).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn integer_device_formats_are_converted() {
        let mut playback = Playback::new(44_100, 1);
        playback.push(vec![16_384, -16_384]);
        let mut out = vec![1i16; 4];
        playback.fill(&mut out);
        assert_eq!(out, vec![0, 16_384, -16_384, 0]);
        let mut playback = Playback::new(44_100, 1);
        playback.push(vec![16_384]);
        let mut out = vec![0u16; 3];
        playback.fill(&mut out);
        assert_eq!(out[0], 32_768); // silence is the midpoint
        assert_eq!(out[1], 49_152);
    }

    #[test]
    fn a_zero_rate_or_zero_channels_cannot_break_the_buffer_walk() {
        let mut playback = Playback::new(0, 0);
        playback.push(vec![1, 2, 3]);
        assert_eq!(fill_f32(&mut playback, 4).len(), 4);
    }

    #[test]
    fn a_stalled_device_is_declared_dead_only_with_sound_queued() {
        let shared = Shared::new(44_100, 1);
        // Pretend the last callback was long ago.
        shared.last_callback_ms.store(0, Ordering::Relaxed);
        let stalled = Shared {
            epoch: Instant::now() - Duration::from_millis(STALL_MS + 1000),
            ..Shared::new(44_100, 1)
        };
        assert!(stalled.check_alive(), "idle and silent is fine");
        stalled.lock().push(vec![1, 2, 3]);
        assert!(!stalled.check_alive());
        assert!(stalled.is_dead());
        assert_eq!(
            stalled.failure.lock().unwrap().as_deref(),
            Some("the audio device stopped responding")
        );
        assert!(shared.check_alive());
    }

    #[test]
    fn the_callback_marks_the_device_alive() {
        let shared = Shared {
            epoch: Instant::now() - Duration::from_millis(STALL_MS + 1000),
            ..Shared::new(44_100, 1)
        };
        shared.lock().push(vec![1, 2, 3]);
        let mut buffer = [0.0f32; 2];
        shared.data_callback(&mut buffer);
        assert!(shared.check_alive());
    }
}
