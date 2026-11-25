//! Smoke test for the real sound device backend (`DeviceAudio`).
//!
//! CI and headless hosts have no audio hardware: the test then prints a skip
//! note and passes. When a device exists it plays 20 ms of a very quiet tone
//! (volume 1) and checks that the queue drains and the stream shuts down.

#![cfg(feature = "audio")]

use std::time::{Duration, Instant};

use basic_vm::{AudioBackend, AudioEvent, DeviceAudio, Tone, Waveform};

fn quiet_tone(ms: u64) -> AudioEvent {
    AudioEvent::Tone(Tone {
        frequency_hz: 440.0,
        duration_samples: 44_100 * ms / 1000,
        volume: 1,
        waveform: Waveform::Sine,
    })
}

fn open_or_skip(test: &str) -> Option<DeviceAudio> {
    match DeviceAudio::open() {
        Ok(device) => Some(device),
        Err(reason) => {
            println!("skipping {test}: no audio output available ({reason})");
            None
        }
    }
}

fn wait_until_empty(device: &DeviceAudio, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while device.queued_samples() > 0 {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

#[test]
fn a_short_quiet_tone_drains_from_the_device_queue() {
    let Some(mut device) = open_or_skip("a_short_quiet_tone_drains_from_the_device_queue") else {
        return;
    };
    assert!(device.is_real_time());
    assert_eq!(device.queued_samples(), 0);
    device.play(&[quiet_tone(20)]).unwrap();
    assert!(device.queued_samples() <= 882);
    assert!(
        wait_until_empty(&device, Duration::from_secs(5)),
        "the 20 ms tone was not consumed within 5 s ({} samples left)",
        device.queued_samples()
    );
    assert!(!device.is_playing());
    drop(device);
}

#[test]
fn stop_empties_the_queue_and_dropping_joins_the_stream() {
    let Some(mut device) = open_or_skip("stop_empties_the_queue_and_dropping_joins_the_stream")
    else {
        return;
    };
    // A long, silent-ish queue that is cut off at once.
    device.play(&[quiet_tone(60_000)]).unwrap();
    assert!(device.queued_samples() > 44_100 * 30);
    device.stop();
    assert_eq!(device.queued_samples(), 0);
    let started = Instant::now();
    drop(device);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "dropping the device took {:?}",
        started.elapsed()
    );
}

#[test]
fn drain_returns_once_the_sound_has_played() {
    let Some(mut device) = open_or_skip("drain_returns_once_the_sound_has_played") else {
        return;
    };
    device.play(&[quiet_tone(30)]).unwrap();
    let started = Instant::now();
    assert!(device.drain(3_000));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(device.queued_samples(), 0);
}
