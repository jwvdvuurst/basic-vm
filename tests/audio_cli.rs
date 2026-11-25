//! The command-line audio flags: `--save-audio` renders everything the
//! program queued into a WAV file on the virtual clock, `--headless-audio`
//! discards it. Neither opens a sound device, so these tests are silent and
//! need no hardware.

mod common;

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use common::{TempDir, decode_wav, pcm_wav, sine_samples};

fn basic(args: &[&dyn AsRef<std::ffi::OsStr>]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_basic-vm"));
    for arg in args {
        command.arg(arg.as_ref());
    }
    command.output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Writes a program into `dir` and returns its path.
fn program(dir: &TempDir, source: &str) -> std::path::PathBuf {
    std::fs::write(dir.file("program.bas"), source).unwrap();
    dir.file("program.bas")
}

fn slashed(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Sign changes, ignoring exact zeros.
fn zero_crossings(samples: &[i16]) -> usize {
    let mut crossings = 0;
    let mut last = 0i16;
    for sample in samples.iter().copied().filter(|s| *s != 0) {
        if last != 0 && (last < 0) != (sample < 0) {
            crossings += 1;
        }
        last = sample;
    }
    crossings
}

fn peak(samples: &[i16]) -> i32 {
    samples
        .iter()
        .map(|s| i32::from(*s).abs())
        .max()
        .unwrap_or(0)
}

fn saved(dir: &TempDir, name: &str) -> Vec<i16> {
    let (rate, samples) = decode_wav(&std::fs::read(dir.file(name)).unwrap());
    assert_eq!(rate, 44_100);
    samples
}

// ------------------------------------------------------------ --save-audio

#[test]
fn save_audio_writes_the_rendered_sound_as_a_16_bit_mono_wav() {
    let dir = TempDir::new("audio-cli-save");
    let source = program(
        &dir,
        "WAVE SINE\nVOLUME 100\nSOUND 440, 200\nPRINT \"queued\"\n",
    );
    let started = Instant::now();
    let output = basic(&[&"--save-audio", &dir.file("out.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "queued\n");
    assert_eq!(stderr(&output), "");
    assert!(started.elapsed() < Duration::from_secs(10));
    let samples = saved(&dir, "out.wav");
    assert_eq!(samples.len(), 8_820);
    assert_eq!(peak(&samples), 30_000);
    let crossings = zero_crossings(&samples);
    assert!((175..=177).contains(&crossings), "{crossings}");
}

#[test]
fn save_audio_concatenates_play_sound_and_playwav_in_program_order() {
    let dir = TempDir::new("audio-cli-order");
    let clip = sine_samples(1000.0, 100, 44_100, 10_000.0);
    std::fs::write(dir.file("clip.wav"), basic_vm::wav_bytes(&clip)).unwrap();
    let wav_path = slashed(&dir.file("clip.wav"));
    let source = program(
        &dir,
        &format!(
            "WAVE SINE\nVOLUME 100\nSOUND 440, 100\nPLAYWAV \"{wav_path}\"\nPLAY \"L16 C\"\nSOUND 880, 50\n"
        ),
    );
    let output = basic(&[&"--save-audio", &dir.file("all.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    let samples = saved(&dir, "all.wav");
    let tone_100 = 4_410;
    assert_eq!(&samples[tone_100..tone_100 + clip.len()], &clip[..]);
    let before = &samples[..tone_100];
    assert!((87..=89).contains(&zero_crossings(before)));
    let last = &samples[samples.len() - 2_205..];
    assert!((87..=89).contains(&zero_crossings(last)));
    // 100 ms + 100 ms + one sixteenth note at the default tempo + 50 ms.
    assert_eq!(samples.len(), 4_410 + 4_410 + 5_513 + 2_205);
}

#[test]
fn save_audio_keeps_only_what_was_played_before_stopsound() {
    let dir = TempDir::new("audio-cli-stop");
    let source = program(
        &dir,
        "VOLUME 100\nSOUND 440, 1000\nSLEEP 100\nSTOPSOUND\nSOUND 880, 50\n",
    );
    let output = basic(&[&"--save-audio", &dir.file("stopped.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    // 100 ms of the first tone (the virtual clock), then the second tone.
    assert_eq!(saved(&dir, "stopped.wav").len(), 4_410 + 2_205);
}

#[test]
fn save_audio_cuts_a_stopped_tone_abruptly_like_the_device() {
    let dir = TempDir::new("audio-cli-abrupt");
    let source = program(&dir, "VOLUME 100\nSOUND 440, 1000\nSLEEP 300\nSTOPSOUND\n");
    let output = basic(&[&"--save-audio", &dir.file("cut.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    let whole = basic_vm::render(&[basic_vm::AudioEvent::Tone(basic_vm::Tone::new(
        440.0,
        1000,
        100,
        basic_vm::Waveform::default(),
    ))]);
    let samples = saved(&dir, "cut.wav");
    assert_eq!(samples, whole[..13_230]);
    assert_eq!(peak(&samples[13_200..]), 30_000, "the cut is not faded");
}

#[test]
fn save_audio_does_not_wait_for_the_sound() {
    let dir = TempDir::new("audio-cli-fast");
    let source = program(&dir, "SOUND 440, 60000\nSOUNDWAIT\nSOUND 440, 60000\n");
    let started = Instant::now();
    let output = basic(&[&"--save-audio", &dir.file("long.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(saved(&dir, "long.wav").len(), 44_100 * 120);
}

#[test]
fn save_audio_overwrites_an_existing_file() {
    let dir = TempDir::new("audio-cli-overwrite");
    std::fs::write(dir.file("out.wav"), vec![0xAB; 100_000]).unwrap();
    let source = program(&dir, "SOUND 440, 10\n");
    let output = basic(&[&"--save-audio", &dir.file("out.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(saved(&dir, "out.wav").len(), 441);
}

#[test]
fn a_program_without_sound_gives_an_empty_wav_and_a_warning() {
    let dir = TempDir::new("audio-cli-empty");
    let source = program(&dir, "PRINT \"quiet\"\n");
    let output = basic(&[&"--save-audio", &dir.file("empty.wav"), &source]);
    assert!(output.status.success());
    assert!(
        stderr(&output).contains("produced no sound"),
        "{}",
        stderr(&output)
    );
    let bytes = std::fs::read(dir.file("empty.wav")).unwrap();
    assert_eq!(bytes.len(), 44);
    assert!(saved(&dir, "empty.wav").is_empty());
}

#[test]
fn save_audio_reports_an_unwritable_path_with_exit_code_1() {
    let dir = TempDir::new("audio-cli-badpath");
    let source = program(&dir, "SOUND 440, 10\nPRINT \"ran\"\n");
    let bad = dir.file("no-such-folder").join("out.wav");
    let output = basic(&[&"--save-audio", &bad, &source]);
    assert_eq!(output.status.code(), Some(1));
    let message = stderr(&output);
    assert!(message.contains("--save-audio"), "{message}");
    assert!(message.contains("out.wav"), "{message}");
    assert_eq!(stdout(&output), "ran\n", "the program itself still ran");
    // A directory is not a file either.
    let output = basic(&[&"--save-audio", &dir.file(""), &source]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(stderr(&output).contains("--save-audio"));
}

#[test]
fn save_audio_without_a_file_argument_is_an_error() {
    // Exit 1, like a missing --screenshot, --auto-close-ms or --max-frames
    // argument: only --test mode misuse uses the usage status 2.
    let output = basic(&[&"--save-audio"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("--save-audio"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn save_audio_still_writes_the_sound_when_the_program_fails() {
    let dir = TempDir::new("audio-cli-failure");
    let source = program(&dir, "SOUND 440, 100\nx = 1 / 0\nSOUND 880, 100\n");
    let output = basic(&[&"--save-audio", &dir.file("partial.wav"), &source]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("line 2"), "{}", stderr(&output));
    assert_eq!(saved(&dir, "partial.wav").len(), 4_410);
}

#[test]
fn save_audio_works_with_the_headless_graphics_flags() {
    let dir = TempDir::new("audio-cli-graphics");
    let source = program(
        &dir,
        "SCREEN 64, 64, \"audio\"\nVOLUME 100\nSOUND 440, 100\n",
    );
    let output = basic(&[
        &"--headless-graphics",
        &"--screenshot",
        &dir.file("shot.png"),
        &"--save-audio",
        &dir.file("sound.wav"),
        &source,
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(saved(&dir, "sound.wav").len(), 4_410);
    assert!(dir.file("shot.png").exists());
    // The graphics example makes no sound at all.
    let output = basic(&[
        &"--headless-graphics",
        &"--save-audio",
        &dir.file("none.wav"),
        &common::examples("graphics.bas"),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(saved(&dir, "none.wav").is_empty());
}

#[test]
fn save_audio_renders_the_sound_example_and_playwav_plays_it_back() {
    let dir = TempDir::new("audio-cli-example");
    let output = basic(&[
        &"--save-audio",
        &dir.file("example.wav"),
        &common::examples("sound.bas"),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let samples = saved(&dir, "example.wav");
    assert!(samples.len() > 44_100 * 3, "{}", samples.len());
    assert!(peak(&samples) > 1_000);
    // Feeding it to wavdemo.bas through ARG$(1) reports the same length.
    let output = basic(&[
        &"--save-audio",
        &dir.file("again.wav"),
        &common::examples("wavdemo.bas"),
        &dir.file("example.wav"),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let again = saved(&dir, "again.wav");
    assert_eq!(again.len(), samples.len());
    assert!(stdout(&output).contains(&format!("{} ms", (samples.len() * 1000 + 22_050) / 44_100)));
}

#[test]
fn a_hand_made_wav_of_another_format_round_trips_through_save_audio() {
    let dir = TempDir::new("audio-cli-formats");
    // 8-bit stereo at 22.05 kHz: 0.1 s is 2205 frames, 4410 output samples.
    let mut data = Vec::new();
    for _ in 0..2_205 {
        data.extend_from_slice(&[255, 255]);
    }
    std::fs::write(dir.file("in.wav"), pcm_wav(2, 22_050, 8, &data)).unwrap();
    let source = program(
        &dir,
        &format!("VOLUME 100\nPLAYWAV \"{}\"\n", slashed(&dir.file("in.wav"))),
    );
    let output = basic(&[&"--save-audio", &dir.file("out.wav"), &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    let samples = saved(&dir, "out.wav");
    assert_eq!(samples.len(), 4_410);
    assert!(samples.iter().all(|s| *s == 32_512), "{:?}", &samples[..4]);
}

// -------------------------------------------------------- --headless-audio

#[test]
fn headless_audio_runs_immediately_without_waiting_for_the_sound() {
    let dir = TempDir::new("audio-cli-headless");
    let source = program(
        &dir,
        "SOUND 440, 30000\nPRINT SOUNDQUEUE()\nSOUNDWAIT\nPRINT SOUNDPLAYING()\nPLAY \"CDE\"\n",
    );
    let started = Instant::now();
    let output = basic(&[&"--headless-audio", &source]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "30000\n0\n");
    assert_eq!(stderr(&output), "");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn headless_audio_leaves_the_exit_code_of_the_program_alone() {
    let dir = TempDir::new("audio-cli-exit");
    let source = program(&dir, "SOUND 440, 10\nx = 1 / 0\n");
    let output = basic(&[&"--headless-audio", &source]);
    assert_eq!(output.status.code(), Some(1));
    let source = program(&dir, "SOUND 440, 10\nEND\n");
    assert!(basic(&[&"--headless-audio", &source]).status.success());
}

#[test]
fn the_audio_flags_are_documented_in_the_usage_text() {
    let output = basic(&[]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("--save-audio"), "{text}");
    assert!(text.contains("--headless-audio"), "{text}");
}
