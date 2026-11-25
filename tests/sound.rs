//! Sound: `BEEP`, `SOUND`, `PLAY`, `WAVE`, `VOLUME`, `STOPSOUND`, `SOUNDWAIT`
//! and the sound functions, observed through the deterministic
//! [`HeadlessAudio`] backend (rendered PCM and a virtual clock).

mod common;

use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use basic_vm::{
    AudioEvent, HeadlessAudio, HeadlessBackend, MAX_QUEUE_MS, TestOptions, Tone, Vm, Waveform,
    compile, compile_file, render, run_tests, wav_bytes,
};
use common::{TempDir, decode_wav, examples};

struct Run {
    output: String,
    result: Result<(), basic_vm::Error>,
    audio: HeadlessAudio,
}

impl Run {
    fn error(&self) -> String {
        self.result.as_ref().unwrap_err().message.clone()
    }
    /// The printed lines.
    fn lines(&self) -> Vec<&str> {
        self.output.lines().collect()
    }
}

fn run(source: &str) -> Run {
    let program = compile(source).unwrap_or_else(|e| panic!("{e}\n{source}"));
    let mut vm = Vm::new(program).unwrap();
    let audio = HeadlessAudio::new();
    vm.set_audio_backend(Box::new(audio.clone()));
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    Run {
        output: String::from_utf8(output).unwrap(),
        result,
        audio,
    }
}

fn ok(source: &str) -> Run {
    let r = run(source);
    if let Err(e) = &r.result {
        panic!("{e}\n{source}");
    }
    r
}

/// The message of the error raised by `statement`, caught by `TRY`.
fn caught(statement: &str) -> String {
    let r = ok(&format!(
        "TRY\n{statement}\nPRINT \"no error\"\nCATCH e$\nPRINT e$\nEND TRY"
    ));
    assert_eq!(
        r.audio.events().len(),
        0,
        "{statement}: something was queued"
    );
    let message = r.output.trim_end().to_string();
    assert_ne!(message, "no error", "{statement} did not fail");
    message
}

fn tone(event: &AudioEvent) -> Tone {
    match event {
        AudioEvent::Tone(tone) => *tone,
        other => panic!("expected a tone, got {other:?}"),
    }
}

/// Sign changes, ignoring exact zeros (the fades start and end at zero).
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

// ------------------------------------------------------------------ SOUND

#[test]
fn sound_queues_exactly_the_requested_number_of_samples() {
    let r = ok("SOUND 440, 100");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 4410);
    let events = r.audio.events();
    assert_eq!(events.len(), 1);
    let t = tone(&events[0]);
    assert_eq!(t.frequency_hz, 440.0);
    assert_eq!(t.duration_samples, 4410);
    assert_eq!(t.volume, 50, "the default VOLUME");
    assert_eq!(t.waveform, Waveform::Square, "the default waveform");
    assert_eq!(r.audio.total_ms(), 100);
}

#[test]
fn peak_amplitude_is_proportional_to_volume() {
    for volume in [1, 10, 50, 100] {
        let r = ok(&format!("SOUND 440, 50, {volume}"));
        assert_eq!(peak(&r.audio.rendered_samples()), volume * 300, "{volume}");
    }
    let silent = ok("SOUND 440, 50, 0");
    let samples = silent.audio.rendered_samples();
    assert_eq!(samples.len(), 2205);
    assert_eq!(peak(&samples), 0);
}

#[test]
fn volume_sets_the_default_and_an_explicit_volume_overrides_it() {
    let r = ok("VOLUME 20\nSOUND 440, 20\nSOUND 440, 20, 80\nVOLUME 100\nSOUND 440, 20");
    let volumes: Vec<u8> = r.audio.events().iter().map(|e| tone(e).volume).collect();
    assert_eq!(volumes, vec![20, 80, 100]);
}

#[test]
fn sine_crosses_zero_twice_per_cycle() {
    let r = ok("WAVE SINE\nSOUND 440, 100, 100");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 4410);
    // 44 cycles in 100 ms: 88 crossings.
    let crossings = zero_crossings(&samples);
    assert!((87..=89).contains(&crossings), "{crossings} crossings");
    assert_eq!(peak(&samples), 30_000);
    // Twice the frequency, twice the crossings.
    let octave = ok("WAVE SINE\nSOUND 880, 100, 100");
    let crossings = zero_crossings(&octave.audio.rendered_samples());
    assert!((175..=177).contains(&crossings), "{crossings} crossings");
}

#[test]
fn square_has_exactly_two_levels_between_the_fades() {
    let r = ok("SOUND 440, 100");
    let samples = r.audio.rendered_samples();
    let steady = &samples[132..samples.len() - 132];
    let mut levels: Vec<i16> = steady.to_vec();
    levels.sort_unstable();
    levels.dedup();
    assert_eq!(levels, vec![-15_000, 15_000]);
    // The ends fade from and to silence instead of clicking.
    assert_eq!(samples[0], 0);
    assert_eq!(*samples.last().unwrap(), 0);
    assert!(samples[1].abs() < samples[100].abs());
}

#[test]
fn every_waveform_renders_what_the_library_renders() {
    for name in ["SQUARE", "SINE", "TRIANGLE", "SAW", "NOISE"] {
        let r = ok(&format!("WAVE {name}\nSOUND 300, 30, 70"));
        let wave = Waveform::parse(name).unwrap();
        let expected = render(&[AudioEvent::Tone(Tone::new(300.0, 30, 70, wave))]);
        assert_eq!(r.audio.rendered_samples(), expected, "{name}");
        assert!(peak(&expected) > 0, "{name} is silent");
    }
}

#[test]
fn noise_is_not_periodic_and_is_the_same_every_run() {
    let first = ok("WAVE NOISE\nSOUND 4000, 50, 100")
        .audio
        .rendered_samples();
    let second = ok("RANDOMIZE 7\nWAVE NOISE\nSOUND 4000, 50, 100")
        .audio
        .rendered_samples();
    assert_eq!(first, second, "noise must not depend on RND");
    let distinct: std::collections::BTreeSet<i16> = first.iter().copied().collect();
    assert!(distinct.len() > 100, "{} distinct levels", distinct.len());
}

#[test]
fn a_zero_frequency_is_a_silent_rest() {
    let r = ok("SOUND 0, 100");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 4410);
    assert!(samples.iter().all(|s| *s == 0));
    assert_eq!(
        r.audio.events(),
        vec![AudioEvent::Rest {
            duration_samples: 4410
        }]
    );
    assert_eq!(r.audio.total_ms(), 100);
}

#[test]
fn a_zero_duration_queues_nothing() {
    let r = ok("SOUND 440, 0\nPRINT SOUNDQUEUE(); SOUNDPLAYING()");
    assert_eq!(r.output, "00\n");
    assert_eq!(r.audio.rendered_samples().len(), 0);
}

#[test]
fn fractional_arguments_are_truncated_like_other_media_statements() {
    let r = ok("SOUND 440.5, 10.9, 60.7");
    let t = tone(&r.audio.events()[0]);
    assert_eq!(t.frequency_hz, 440.5, "a frequency may be fractional");
    assert_eq!(t.duration_samples, 441, "10 ms");
    assert_eq!(t.volume, 60);
}

#[test]
fn sounds_play_in_the_order_they_were_queued() {
    let r = ok("WAVE SINE\nSOUND 200, 100, 100\nSOUND 0, 50\nWAVE SQUARE\nSOUND 800, 100, 100");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 4410 + 2205 + 4410);
    let (first, rest) = samples.split_at(4410);
    let (gap, last) = rest.split_at(2205);
    assert!(gap.iter().all(|s| *s == 0));
    // 200 Hz sine: 20 cycles; 800 Hz square: 80 cycles.
    assert!((39..=41).contains(&zero_crossings(first)));
    assert!((159..=161).contains(&zero_crossings(last)));
    assert_eq!(r.audio.total_ms(), 250);
    let events = r.audio.events();
    assert_eq!(tone(&events[0]).waveform, Waveform::Sine);
    assert_eq!(tone(&events[2]).waveform, Waveform::Square);
}

#[test]
fn beep_is_a_short_square_tone_at_the_current_volume() {
    let r = ok("WAVE SINE\nVOLUME 30\nBEEP");
    let t = tone(&r.audio.events()[0]);
    assert_eq!(t.frequency_hz, 800.0);
    assert_eq!(t.waveform, Waveform::Square, "BEEP ignores WAVE");
    assert_eq!(t.volume, 30);
    assert_eq!(r.audio.rendered_samples().len(), 8820, "200 ms");
    assert_eq!(peak(&r.audio.rendered_samples()), 9_000);
}

// ------------------------------------------------------------------- WAVE

#[test]
fn wave_accepts_keywords_strings_and_expressions_in_any_case() {
    let source = "\
WAVE SINE
SOUND 100, 1
WAVE \"Triangle\"
SOUND 100, 1
w$ = \"saw\"
WAVE w$
SOUND 100, 1
WAVE \"no\" + \"ise\"
SOUND 100, 1
wave Square
SOUND 100, 1
WAVE (\"SINE\")
SOUND 100, 1
";
    let r = ok(source);
    let waves: Vec<Waveform> = r.audio.events().iter().map(|e| tone(e).waveform).collect();
    assert_eq!(
        waves,
        vec![
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Noise,
            Waveform::Square,
            Waveform::Sine
        ]
    );
}

#[test]
fn wave_rejects_unknown_waveforms_and_non_strings() {
    assert!(caught("WAVE \"sinus\"").starts_with("WAVE: unknown waveform \"sinus\""));
    assert!(caught("WAVE \"\"").contains("unknown waveform"));
    assert!(caught("WAVE 3").contains("WAVE: the waveform must be a string"));
}

#[test]
fn a_variable_named_like_a_waveform_needs_parentheses() {
    // A bare word is the keyword; `(SAW)` is the variable's value.
    let r = ok("DIM SAW\nSAW = \"noise\"\nWAVE (SAW)\nSOUND 100, 1\nWAVE SAW\nSOUND 100, 1");
    let waves: Vec<Waveform> = r.audio.events().iter().map(|e| tone(e).waveform).collect();
    assert_eq!(waves, vec![Waveform::Noise, Waveform::Saw]);
}

// ------------------------------------------------------------------- PLAY

#[test]
fn play_durations_are_exact_at_various_tempos() {
    // (music, milliseconds, samples)
    let cases = [
        ("T120 L4 C", 500, 22_050),
        ("C", 500, 22_050),
        ("T60 C", 1000, 44_100),
        ("T240 C", 250, 11_025),
        ("T120 L8 C", 250, 11_025),
        ("T120 C2", 1000, 44_100),
        ("T120 C1", 2000, 88_200),
        ("T120 C4.", 750, 33_075),
        ("T120 C4..", 875, 38_588),
        ("T120 L16 C", 125, 5_513),
        ("T120 L64 C", 31, 1_378),
        ("T32 C4", 1875, 82_688),
        ("T255 C4", 235, 10_376),
        ("T120 P4 R4", 1000, 44_100),
        ("T120 N40", 500, 22_050),
        ("T120 N0", 500, 22_050),
        ("T120 C D E F", 2000, 88_200),
        ("T120 C T240 D", 750, 33_075),
        ("T120 L3 C", 667, 29_400),
        ("", 0, 0),
    ];
    for (music, milliseconds, samples) in cases {
        let r = ok(&format!(
            "PLAY \"{music}\"\nPRINT MUSICLENGTH(\"{music}\"); \" \"; SOUNDQUEUE()"
        ));
        assert_eq!(r.audio.rendered_samples().len(), samples, "{music:?}");
        // MUSICLENGTH rounds to the nearest millisecond, SOUNDQUEUE up.
        let queued = (samples * 1000).div_ceil(44_100);
        assert_eq!(r.output, format!("{milliseconds} {queued}\n"), "{music:?}");
    }
}

#[test]
fn play_renders_the_notes_at_the_right_pitches() {
    // Octave 3 contains middle C and A = 440 Hz; the default octave is 4.
    let r = ok("PLAY \"O3 A ML\"\nPLAY \"O3 C\"\nPLAY \"A\"\nPLAY \"N46\"");
    let hz: Vec<f64> = r
        .audio
        .events()
        .iter()
        .filter_map(|e| match e {
            AudioEvent::Tone(t) => Some(t.frequency_hz),
            _ => None,
        })
        .collect();
    assert_eq!(hz.len(), 4);
    assert_eq!(hz[0], 440.0);
    assert!((hz[1] - 261.6256).abs() < 1e-3);
    assert_eq!(hz[2], 880.0);
    assert_eq!(hz[3], 440.0);
}

#[test]
fn play_rendered_pcm_has_the_expected_pitch_and_articulation() {
    // One legato A at 440 Hz for a quarter note: 500 ms = 220 cycles.
    let r = ok("WAVE SINE\nVOLUME 100\nPLAY \"ML O3 A\"");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 22_050);
    let crossings = zero_crossings(&samples);
    assert!((439..=441).contains(&crossings), "{crossings}");
    // Normal articulation: the last eighth of the note is silent.
    let r = ok("WAVE SINE\nVOLUME 100\nPLAY \"MN O3 A\"");
    let samples = r.audio.rendered_samples();
    assert_eq!(samples.len(), 22_050);
    assert!(samples[19_294..].iter().all(|s| *s == 0));
    assert!(peak(&samples[..19_294]) > 29_000);
    // Staccato: a quarter of the note is silent.
    let r = ok("WAVE SINE\nVOLUME 100\nPLAY \"MS O3 A\"");
    let samples = r.audio.rendered_samples();
    assert!(samples[16_538..].iter().all(|s| *s == 0));
    assert!(peak(&samples[..16_538]) > 29_000);
}

#[test]
fn play_volume_commands_and_the_volume_statement() {
    let r = ok("VOLUME 10\nPLAY \"C V90 D\"\nVOLUME 70\nPLAY \"E\"");
    let volumes: Vec<u8> = r
        .audio
        .events()
        .iter()
        .filter_map(|e| match e {
            AudioEvent::Tone(t) => Some(t.volume),
            _ => None,
        })
        .collect();
    assert_eq!(volumes, vec![10, 90, 70], "V is local to its string");
}

#[test]
fn play_names_the_position_of_an_error_and_queues_nothing() {
    let message = caught("PLAY \"CDE X\"");
    assert_eq!(
        message,
        "PLAY: invalid music string at position 5: unknown command 'X'"
    );
    assert!(caught("PLAY \"T300\"").contains("position 1: tempo must be from 32 to 255, got 300"));
    assert!(caught("PLAY \"C D O7\"").contains("position 5: octave must be from 0 to 6"));
    assert!(caught("PLAY \"CDEFGAB C65\"").contains("position 9: length"));
    assert!(caught("PLAY \"V101\"").contains("volume must be from 0 to 100"));
    assert!(caught("PLAY \"N85\"").contains("0 to 84"));
    assert!(caught("PLAY \"MX\"").contains("M must be followed by"));
    assert!(caught("PLAY 5").contains("PLAY: the music string must be a string"));
    // Valid notes before the error are not played (checked by `caught`).
    let r = ok("TRY\nPLAY \"CCCC T0\"\nCATCH e$\nEND TRY\nPRINT SOUNDPLAYING()");
    assert_eq!(r.output, "0\n");
}

#[test]
fn musiclength_validates_like_play() {
    let r = ok("PRINT MUSICLENGTH(\"T90 L4 CDEF\")");
    assert_eq!(r.output, "2667\n", "4 quarters of 666.67 ms");
    let r = ok("TRY\nPRINT MUSICLENGTH(\"CQ\")\nCATCH e$\nPRINT e$\nEND TRY");
    assert_eq!(
        r.output,
        "MUSICLENGTH: invalid music string at position 2: unknown command 'Q'\n"
    );
    assert_eq!(r.audio.events().len(), 0);
}

// ------------------------------------------------------- NOTEFREQ / errors

#[test]
fn notefreq_uses_midi_numbering_and_equal_temperament() {
    let source = "\
ASSERTEQ 440, NOTEFREQ(69)
ASSERTEQ 880, NOTEFREQ(81)
ASSERTEQ 220, NOTEFREQ(57)
ASSERTNEAR 261.6256, NOTEFREQ(60), 0.001
ASSERTNEAR 8.1758, NOTEFREQ(0), 0.001
ASSERTNEAR 12543.854, NOTEFREQ(127), 0.01
ASSERTNEAR 466.1638, NOTEFREQ(70), 0.001
ASSERTNEAR 453.0, NOTEFREQ(69.5) - 0, 1
ASSERT NOTEFREQ(70) > NOTEFREQ(69.5) AND NOTEFREQ(69.5) > NOTEFREQ(69)
PRINT \"ok\"
";
    assert_eq!(ok(source).output, "ok\n");
    assert!(
        caught("PRINT NOTEFREQ(128)").contains("NOTEFREQ: note must be from 0 to 127, got 128")
    );
    assert!(caught("PRINT NOTEFREQ(-1)").contains("NOTEFREQ: note must be from 0 to 127"));
    assert!(caught("PRINT NOTEFREQ(\"A\")").contains("NOTEFREQ"));
}

#[test]
fn invalid_sound_arguments_are_catchable_runtime_errors() {
    assert!(
        caught("SOUND 10, 100")
            .contains("SOUND: frequency must be 0 (a rest) or from 20 to 20000 Hz, got 10")
    );
    assert!(caught("SOUND 20001, 100").contains("frequency"));
    assert!(caught("SOUND -440, 100").contains("frequency"));
    assert!(caught("SOUND 440, -1").contains("SOUND: duration must be from 0 to 60000, got -1"));
    assert!(
        caught("SOUND 440, 60001").contains("SOUND: duration must be from 0 to 60000, got 60001")
    );
    assert!(caught("SOUND 440, 100, 101").contains("SOUND: volume must be from 0 to 100, got 101"));
    assert!(caught("SOUND 440, 100, -1").contains("volume"));
    assert!(caught("SOUND \"a\", 100").contains("SOUND: frequency must be a number"));
    assert!(caught("VOLUME 101").contains("VOLUME: volume must be from 0 to 100, got 101"));
    assert!(caught("VOLUME -1").contains("VOLUME"));
    // The limits themselves are valid.
    let r = ok("SOUND 20, 0\nSOUND 20000, 1, 100\nSOUND 440, 60000, 0\nVOLUME 0\nVOLUME 100");
    assert_eq!(r.audio.total_ms(), 60_001);
}

#[test]
fn wrong_argument_counts_are_compile_errors() {
    for (source, message) in [
        ("BEEP 1", "BEEP requires 0 arguments, got 1"),
        ("SOUND 440", "SOUND requires 2 to 3 arguments, got 1"),
        (
            "SOUND 440, 1, 2, 3",
            "SOUND requires 2 to 3 arguments, got 4",
        ),
        ("PLAY \"C\", \"D\"", "PLAY requires 1 arguments, got 2"),
        ("VOLUME 1, 2", "VOLUME requires 1 arguments, got 2"),
        ("STOPSOUND 1", "STOPSOUND requires 0 arguments, got 1"),
        ("SOUNDWAIT 1", "SOUNDWAIT requires 0 arguments, got 1"),
        (
            "PRINT SOUNDPLAYING(1)",
            "SOUNDPLAYING requires 0 arguments, got 1",
        ),
        (
            "PRINT SOUNDQUEUE(1)",
            "SOUNDQUEUE requires 0 arguments, got 1",
        ),
        ("PRINT NOTEFREQ()", "NOTEFREQ requires 1 arguments, got 0"),
        (
            "PRINT NOTEFREQ(1, 2)",
            "NOTEFREQ requires 1 arguments, got 2",
        ),
        (
            "PRINT MUSICLENGTH()",
            "MUSICLENGTH requires 1 arguments, got 0",
        ),
        (
            "PRINT MUSICLENGTH(\"C\", 1)",
            "MUSICLENGTH requires 1 arguments, got 2",
        ),
    ] {
        let error = compile(source).expect_err(source).to_string();
        assert!(error.contains(message), "{source}: {error}");
    }
    // Missing arguments entirely are syntax errors.
    for source in ["PLAY", "WAVE", "VOLUME", "SOUND"] {
        assert!(compile(source).is_err(), "{source}");
    }
}

// ----------------------------------------------------------- queue / clock

#[test]
fn the_queue_is_observed_on_a_virtual_clock() {
    let source = "\
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
SOUND 440, 1000
SOUND 880, 500
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
SLEEP 20
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
FOR i = 1 TO 10
  DOEVENTS
NEXT
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
";
    let r = ok(source);
    assert_eq!(r.lines(), vec!["0 0", "1 1500", "1 1480", "1 1470"]);
    assert_eq!(r.audio.clock_ms(), 30);
    assert_eq!(r.audio.queued_ms(), 1470);
    assert!(r.audio.is_playing());
}

#[test]
fn a_polling_loop_terminates_because_doevents_advances_the_clock() {
    let r = ok("\
SOUND 440, 500
n = 0
DO WHILE SOUNDPLAYING()
  DOEVENTS
  n = n + 1
LOOP
PRINT n");
    assert_eq!(r.output, "500\n");
    assert!(!r.audio.is_playing());
}

#[test]
fn soundwait_jumps_to_the_end_of_the_queue_without_waiting() {
    let started = Instant::now();
    let r = ok("\
SOUND 440, 30000
SOUND 880, 30000
t = TIMER()
SOUNDWAIT
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
ASSERT TIMER() - t >= 60, \"TIMER accounts for the wait\"
SOUNDWAIT
PRINT \"done\"");
    assert_eq!(r.lines(), vec!["0 0", "done"]);
    assert_eq!(r.audio.clock_ms(), 60_000);
    assert!(
        started.elapsed().as_secs() < 10,
        "SOUNDWAIT really waited: {:?}",
        started.elapsed()
    );
}

#[test]
fn soundwait_on_an_empty_queue_does_nothing() {
    let r = ok("SOUNDWAIT\nPRINT SOUNDQUEUE()");
    assert_eq!(r.output, "0\n");
    assert_eq!(r.audio.clock_ms(), 0);
}

#[test]
fn stopsound_empties_the_queue_and_cuts_the_audio_at_the_current_time() {
    let r = ok("\
SOUND 440, 1000
FOR i = 1 TO 100
  DOEVENTS
NEXT
STOPSOUND
PRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()
SOUND 880, 10
PRINT SOUNDQUEUE()");
    assert_eq!(r.lines(), vec!["0 0", "10"]);
    assert_eq!(r.audio.stops(), 1);
    // 100 ms of the first sound were heard, then the second one.
    let audible = r.audio.audible_events();
    assert_eq!(audible.len(), 2);
    assert_eq!(audible[0].samples(), 4410);
    assert_eq!(audible[1].samples(), 441);
    assert_eq!(r.audio.rendered_samples().len(), 4410 + 441);
    // The whole history is still available.
    assert_eq!(r.audio.events().len(), 2);
    assert_eq!(r.audio.events()[0].samples(), 44_100);
}

#[test]
fn stopsound_on_an_empty_queue_is_harmless() {
    let r = ok("STOPSOUND\nPRINT SOUNDPLAYING()");
    assert_eq!(r.output, "0\n");
}

#[test]
fn a_tone_cut_by_stopsound_is_an_abrupt_prefix_of_the_whole_tone() {
    let r = ok("VOLUME 100\nSOUND 440, 1000\nSLEEP 300\nSTOPSOUND");
    let whole = render(&[AudioEvent::Tone(Tone::new(
        440.0,
        1000,
        100,
        Waveform::default(),
    ))]);
    let heard = r.audio.rendered_samples();
    assert_eq!(heard.len(), 13_230);
    assert_eq!(heard[..], whole[..13_230]);
    // The device cuts abruptly: no fade-out was applied at the cut.
    assert_eq!(i32::from(heard[heard.len() - 1]).abs(), 30_000);
    // The recorded event is still the shortened tone.
    assert_eq!(r.audio.audible_events()[0].samples(), 13_230);
}

#[test]
fn out_of_range_sound_arguments_are_reported_compactly_as_written() {
    for (statement, expected) in [
        (
            "SOUND 440, 1E300",
            "SOUND: duration must be from 0 to 60000, got 1e+300",
        ),
        (
            "SOUND 440, -1E300",
            "SOUND: duration must be from 0 to 60000, got -1e+300",
        ),
        (
            "SOUND 440, 100, 1E300",
            "SOUND: volume must be from 0 to 100, got 1e+300",
        ),
        (
            "VOLUME 1E300",
            "VOLUME: volume must be from 0 to 100, got 1e+300",
        ),
        (
            "SOUND 1E300, 100",
            "SOUND: frequency must be 0 (a rest) or from 20 to 20000 Hz, got 1e+300",
        ),
        (
            "SOUND 1E-300, 100",
            "SOUND: frequency must be 0 (a rest) or from 20 to 20000 Hz, got 1e-300",
        ),
        (
            "PRINT NOTEFREQ(1E300)",
            "NOTEFREQ: note must be from 0 to 127, got 1e+300",
        ),
        // Ordinary values keep their plain form.
        (
            "SOUND 440, 60000.5 * 2",
            "SOUND: duration must be from 0 to 60000, got 120001",
        ),
        (
            "SOUND 25000.5, 100",
            "SOUND: frequency must be 0 (a rest) or from 20 to 20000 Hz, got 25000.5",
        ),
    ] {
        let message = caught(statement);
        assert!(message.contains(expected), "{statement}: {message}");
        assert!(message.len() < 200, "{statement}: {message}");
    }
}

#[test]
fn a_huge_music_string_is_refused_quickly_and_measured_without_events() {
    // 2^21 = 2097152 quarter notes: about 1.05e9 ms, far beyond the 10 minute queue.
    let started = Instant::now();
    let r = ok("\
s$ = \"C\"
FOR i = 1 TO 21
  s$ = s$ + s$
NEXT
TRY
  PLAY s$
  PRINT \"no error\"
CATCH e$
  PRINT e$
END TRY
PRINT MUSICLENGTH(s$)
t$ = s$ + \"X\"
TRY
  PLAY t$
CATCH e$
  PRINT e$
END TRY
TRY
  PRINT MUSICLENGTH(t$)
CATCH e$
  PRINT e$
END TRY");
    let lines = r.lines();
    assert!(
        lines[0].starts_with("PLAY: the sound queue is full"),
        "{lines:?}"
    );
    // The exact length, not capped at the queue limit.
    assert_eq!(lines[1], "1048576000");
    // A syntax error far into the string is still reported first, with its position.
    let invalid = "invalid music string at position 2097153: unknown command 'X'";
    assert!(lines[2].contains(invalid), "{lines:?}");
    assert!(lines[3].contains(invalid), "{lines:?}");
    assert_eq!(r.audio.events().len(), 0);
    assert!(started.elapsed().as_secs() < 20);
}

#[test]
fn the_queue_holds_at_most_ten_minutes_and_a_refused_sound_queues_nothing() {
    assert_eq!(MAX_QUEUE_MS, 600_000);
    let mut source = String::new();
    for _ in 0..10 {
        source.push_str("SOUND 440, 60000\n");
    }
    // Exactly full is fine.
    let full = ok(&source);
    assert_eq!(full.audio.queued_ms(), 600_000);
    // One millisecond more is refused with an explicit error.
    let over = run(&format!("{source}SOUND 440, 1\n"));
    let message = over.error();
    assert!(
        message.starts_with("SOUND: the sound queue is full"),
        "{message}"
    );
    assert!(message.contains("600 seconds"), "{message}");
    assert_eq!(over.audio.queued_ms(), 600_000);
    assert_eq!(over.audio.events().len(), 10);
    // PLAY is all or nothing: a string that does not fit queues none of it.
    let play = run(&format!("{source}PLAY \"C\"\n"));
    assert!(play.error().starts_with("PLAY: the sound queue is full"));
    assert_eq!(play.audio.events().len(), 10);
    let other = run(&format!("{source}BEEP\n"));
    assert!(other.error().starts_with("BEEP: the sound queue is full"));
    // Playing time frees room: SOUNDWAIT drains the queue.
    let drained = ok(&format!(
        "{source}SOUNDWAIT\nSOUND 440, 60000\nPRINT SOUNDQUEUE()"
    ));
    assert_eq!(drained.output, "60000\n");
    // The error can be caught and the program carries on.
    let caught_full = ok(&format!(
        "{source}TRY\nSOUND 440, 1\nCATCH e$\nPRINT \"full\"\nEND TRY\nSTOPSOUND\nSOUND 440, 1\nPRINT SOUNDQUEUE()"
    ));
    assert_eq!(caught_full.lines(), vec!["full", "1"]);
}

#[test]
fn a_long_play_string_can_exceed_the_queue() {
    // 650 whole notes at T32 are far more than ten minutes.
    let message = caught(&format!("PLAY \"T32 L1 {}\"", "C".repeat(100)));
    assert!(
        message.starts_with("PLAY: the sound queue is full"),
        "{message}"
    );
}

// ------------------------------------------------------------- environment

#[test]
fn sound_works_without_a_backend_and_without_a_screen() {
    let program = compile(
        "BEEP\nSOUND 440, 100\nPLAY \"CDE\"\nPRINT SOUNDPLAYING(); \" \"; SOUNDQUEUE()\nSOUNDWAIT\nPRINT SOUNDPLAYING()\nSTOPSOUND",
    )
    .unwrap();
    let mut vm = Vm::new(program).unwrap();
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "1 1800\n0\n");
    assert_eq!(vm.audio_queued_ms(), 0);
}

#[test]
fn the_vm_reports_the_queue_left_when_the_program_ends() {
    let mut vm = Vm::new(compile("PLAY \"C\"").unwrap()).unwrap();
    vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap();
    assert_eq!(vm.audio_queued_ms(), 500, "the VM does not wait at the end");
}

#[test]
fn sound_and_graphics_work_together() {
    let mut vm = Vm::new(
        compile(
            "SCREEN 16, 16\nSOUND 440, 200\nFLIP\nSOUNDWAIT\nPSET 1, 1, 15\nPRINT SOUNDQUEUE()",
        )
        .unwrap(),
    )
    .unwrap();
    let audio = HeadlessAudio::new();
    vm.set_audio_backend(Box::new(audio.clone()));
    vm.set_graphics_backend(Box::new(HeadlessBackend::new()));
    let mut output = Vec::new();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "0\n");
    assert_eq!(audio.clock_ms(), 200);
    assert_eq!(vm.canvas().unwrap().pixel(1, 1), Some(0xFFFFFF));
}

#[test]
fn sound_words_remain_usable_as_variable_names() {
    let source = "\
BEEP = 1
SOUND = 2
PLAY = 3
WAVE = 4
VOLUME = 5
STOPSOUND = 6
SOUNDWAIT = 7
SOUNDPLAYING = 8
SOUNDQUEUE = 9
NOTEFREQ = 10
MUSICLENGTH = 11
PRINT BEEP + SOUND + PLAY + WAVE + VOLUME + STOPSOUND + SOUNDWAIT
PRINT SOUNDPLAYING + SOUNDQUEUE + NOTEFREQ + MUSICLENGTH
VOLUME = VOLUME + 1
PRINT VOLUME
VOLUME 20
SOUND 440, 10
PRINT VOLUME, SOUNDQUEUE()
PRINT NOTEFREQ(69)
";
    let r = ok(source);
    assert_eq!(r.lines(), vec!["28", "38", "6", "6\t10", "440"]);
    assert_eq!(tone(&r.audio.events()[0]).volume, 20);
}

#[test]
fn sound_words_work_as_array_variables() {
    let r = ok("\
PLAY = ARRAY(10, 20)
PLAY[1] = 5
PRINT PLAY[1] + PLAY[2]
SOUND = LIST(\"a\")
PRINT LEN(SOUND)
");
    assert_eq!(r.lines(), vec!["25", "1"]);
    assert_eq!(r.audio.events().len(), 0);
}

#[test]
fn user_functions_shadow_the_sound_functions() {
    let r = ok("FUNCTION NOTEFREQ(PARAMETER n)\n  NOTEFREQ = n * 2\nFEND\nPRINT NOTEFREQ(4)");
    assert_eq!(r.output, "8\n");
}

#[test]
fn sound_state_is_per_vm_run() {
    let first = ok("WAVE SINE\nVOLUME 5\nSOUND 440, 10");
    let second = ok("SOUND 440, 10");
    assert_eq!(tone(&first.audio.events()[0]).volume, 5);
    assert_eq!(tone(&second.audio.events()[0]).volume, 50);
    assert_eq!(tone(&second.audio.events()[0]).waveform, Waveform::Square);
}

// ------------------------------------------------------------------- WAV

#[test]
fn rendered_sound_round_trips_through_a_wav_file() {
    let r = ok("WAVE SINE\nPLAY \"T240 L8 CDEFG\"");
    let samples = r.audio.rendered_samples();
    let dir = TempDir::new("sound-wav");
    let path = dir.file("tune.wav");
    std::fs::write(&path, wav_bytes(&samples)).unwrap();
    let (rate, decoded) = decode_wav(&std::fs::read(&path).unwrap());
    assert_eq!(rate, 44_100);
    assert_eq!(decoded, samples);
    // Five eighths at T240 are 5512.5 -> 5513 samples each.
    assert_eq!(decoded.len(), 5 * 5_513);
    assert!(peak(&decoded) > 14_000);
}

// --------------------------------------------------------- TEST interplay

fn count_of(report: &basic_vm::TestReport, status: &str) -> usize {
    report
        .results
        .iter()
        .filter(|r| r.line_text("").starts_with(status))
        .count()
}

#[test]
fn sound_in_test_blocks_never_waits_and_does_not_leak_between_tests() {
    let source = "\
TEST Queue_then_wait
  SOUND 440, 60000
  PLAY \"T32 L1 CCCC\"
  t = TIMER()
  SOUNDWAIT
  ASSERTEQ 0, SOUNDQUEUE()
  ASSERT SOUNDPLAYING() = 0, \"stopped\"
  ASSERT TIMER() - t >= 90, \"virtual time passed\"
END TEST
TEST Fresh_state
  ASSERTEQ 0, SOUNDQUEUE()
  SOUND 440, 100
  WAVE SINE
  VOLUME 7
END TEST
TEST Defaults_restored
  ASSERTEQ 0, SOUNDQUEUE()
  SOUND 440, 100
  ASSERTEQ 100, SOUNDQUEUE()
END TEST
TEST Errors_fail_the_test
  SOUND 5, 100
END TEST
";
    let started = Instant::now();
    let program = compile(source).unwrap();
    let report = run_tests(&program, &TestOptions::default());
    let lines: Vec<String> = report.results.iter().map(|r| r.line_text("")).collect();
    assert_eq!(count_of(&report, "PASS"), 3, "{lines:?}");
    assert_eq!(count_of(&report, "ERROR"), 1, "{lines:?}");
    assert!(
        lines[3].contains("SOUND: frequency must be 0 (a rest) or from 20 to 20000 Hz"),
        "{lines:?}"
    );
    assert!(
        started.elapsed().as_secs() < 10,
        "sound slept in a test: {:?}",
        started.elapsed()
    );
}

// --------------------------------------------------------------- example

#[test]
fn the_example_runs_instantly_with_the_library_default_and_headless_audio() {
    let program = compile_file(&examples("sound.bas")).unwrap();
    let mut vm = Vm::new(program).unwrap();
    let audio = HeadlessAudio::new();
    vm.set_audio_backend(Box::new(audio.clone()));
    let mut output = Vec::new();
    let started = Instant::now();
    vm.run(&mut Cursor::new(""), &mut output).unwrap();
    assert!(started.elapsed().as_secs() < 10);
    let lines: Vec<String> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        lines,
        vec![
            "Melody queued: 3429 ms",
            "Queued: 4379 ms, playing: 1",
            "Queued after SOUNDWAIT: 0 ms, playing: 0",
            "After STOPSOUND, playing: 0",
        ]
    );
    // 3429 + 10 * 60 + 100 + 250 ms were heard in the first part.
    assert!(audio.total_ms() >= 4379);
    assert_eq!(audio.stops(), 1);
    assert!(!audio.is_playing());
}

#[test]
fn the_command_line_runs_the_example_without_a_device() {
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--headless-audio")
        .arg(Path::new(&examples("sound.bas")))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Melody queued: 3429 ms"), "{stdout}");
    assert!(
        stdout.ends_with("After STOPSOUND, playing: 0\n"),
        "{stdout}"
    );
}

// ----------------------------------------------------- PLAYWAV / WAVLENGTH

/// Writes `bytes` to `name` in `dir` and returns the path as BASIC text.
fn stored(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    std::fs::write(dir.file(name), bytes).unwrap();
    slashed(dir, name)
}

/// The path of `name` in `dir` with forward slashes, safe inside a BASIC
/// string literal on every platform.
fn slashed(dir: &TempDir, name: &str) -> String {
    dir.text(name).replace('\\', "/")
}

/// `sample * percent / 100`, halves away from zero.
fn at_volume(sample: i16, percent: i32) -> i16 {
    (f64::from(sample) * f64::from(percent) / 100.0).round() as i16
}

fn is_clip(event: &AudioEvent) -> bool {
    matches!(event, AudioEvent::Clip { .. })
}

#[test]
fn playwav_queues_the_decoded_clip_scaled_by_the_volume() {
    let dir = TempDir::new("sound-playwav-volume");
    let clip: Vec<i16> = vec![0, 1000, -1000, 32_767, -32_768, 333, -333, 7, 1, -1];
    let path = stored(&dir, "clip.wav", &wav_bytes(&clip));
    for percent in [100, 99, 50, 10, 1, 0] {
        let r = ok(&format!("VOLUME {percent}\nPLAYWAV \"{path}\""));
        let expected: Vec<i16> = clip.iter().map(|s| at_volume(*s, percent)).collect();
        assert_eq!(r.audio.rendered_samples(), expected, "VOLUME {percent}");
        assert_eq!(r.audio.events().len(), 1);
        assert!(is_clip(&r.audio.events()[0]));
    }
    // The default volume is 50: half of the file's level.
    let r = ok(&format!("PLAYWAV \"{path}\""));
    let half: Vec<i16> = clip.iter().map(|s| at_volume(*s, 50)).collect();
    assert_eq!(r.audio.rendered_samples(), half);
    // Only the VOLUME statement counts (SOUND's own volume argument does not).
    let r = ok(&format!("VOLUME 100\nSOUND 440, 5, 1\nPLAYWAV \"{path}\""));
    let rendered = r.audio.rendered_samples();
    assert_eq!(&rendered[rendered.len() - clip.len()..], &clip[..]);
}

#[test]
fn playwav_accepts_a_string_expression_and_a_relative_path() {
    let dir = TempDir::new("sound-playwav-expr");
    stored(&dir, "x.wav", &wav_bytes(&[5, 6, 7]));
    let base = slashed(&dir, "x");
    let r = ok(&format!(
        "n$ = \"{base}\"\nVOLUME 100\nPLAYWAV n$ + \".wav\""
    ));
    assert_eq!(r.audio.rendered_samples(), vec![5, 6, 7]);
}

#[test]
fn wavlength_reports_the_duration_in_milliseconds_and_queues_nothing() {
    let dir = TempDir::new("sound-wavlength");
    let mono_441 = stored(
        &dir,
        "a.wav",
        &wav_bytes(&common::sine_samples(440.0, 500, 44_100, 1000.0)),
    );
    let mono_8k = stored(&dir, "b.wav", &common::pcm_wav(1, 8_000, 8, &[128; 4_000]));
    let stereo_48k = stored(
        &dir,
        "c.wav",
        &common::pcm_wav(2, 48_000, 16, &[0; 480 * 4]),
    );
    let empty = stored(&dir, "d.wav", &wav_bytes(&[]));
    let one_frame = stored(&dir, "e.wav", &wav_bytes(&[1]));
    let long_one = stored(
        &dir,
        "f.wav",
        &common::pcm_wav(1, 8_000, 8, &vec![128; 8_000 * 700]),
    );
    let source = format!(
        "PRINT WAVLENGTH(\"{mono_441}\")\nPRINT WAVLENGTH(\"{mono_8k}\")\n\
         PRINT WAVLENGTH(\"{stereo_48k}\")\nPRINT WAVLENGTH(\"{empty}\")\n\
         PRINT WAVLENGTH(\"{one_frame}\")\nPRINT WAVLENGTH(\"{long_one}\")\n\
         PRINT SOUNDQUEUE()"
    );
    let r = ok(&source);
    // A 700-second file is too long to play but its length can be asked.
    assert_eq!(r.lines(), vec!["500", "500", "10", "0", "0", "700000", "0"]);
    assert!(r.audio.events().is_empty());
}

#[test]
fn playwav_is_queued_in_order_with_sound_and_play() {
    let dir = TempDir::new("sound-playwav-order");
    let clip = common::sine_samples(1000.0, 50, 44_100, 8_000.0);
    let path = stored(&dir, "c.wav", &wav_bytes(&clip));
    let r = ok(&format!(
        "VOLUME 100\nSOUND 440, 10\nPLAYWAV \"{path}\"\nPLAY \"L32 C\"\nPRINT SOUNDQUEUE()"
    ));
    let events = r.audio.events();
    assert!(matches!(events[0], AudioEvent::Tone(_)));
    assert!(is_clip(&events[1]));
    assert!(matches!(events[2], AudioEvent::Tone(_)));
    let rendered = r.audio.rendered_samples();
    assert_eq!(rendered, render(&events));
    assert_eq!(&rendered[441..441 + clip.len()], &clip[..]);
    let total_ms = (rendered.len() as f64 * 1000.0 / 44_100.0).ceil();
    assert_eq!(r.output, format!("{total_ms}\n"));
}

#[test]
fn playwav_converts_channels_sample_size_and_rate() {
    let dir = TempDir::new("sound-playwav-convert");
    // 24-bit stereo at 48 kHz: left 0x400000 (half of full scale = 16384 in
    // 16 bits), right 0; the average of the two channels is 8192.
    let mut data = Vec::new();
    for _ in 0..480 {
        data.extend_from_slice(&[0x00, 0x00, 0x40]);
        data.extend_from_slice(&[0x00, 0x00, 0x00]);
    }
    let path = stored(&dir, "s.wav", &common::pcm_wav(2, 48_000, 24, &data));
    let r = ok(&format!(
        "VOLUME 100\nPLAYWAV \"{path}\"\nPRINT SOUNDQUEUE()"
    ));
    let rendered = r.audio.rendered_samples();
    assert_eq!(rendered.len(), 441);
    assert!(rendered.iter().all(|s| *s == 8192), "{:?}", &rendered[..8]);
    assert_eq!(r.output, "10\n");
}

#[test]
fn playwav_obeys_soundwait_stopsound_and_the_virtual_clock() {
    let dir = TempDir::new("sound-playwav-clock");
    let clip: Vec<i16> = (1..=44_100).map(|v| (v % 30_000) as i16).collect();
    let path = stored(&dir, "one-second.wav", &wav_bytes(&clip));
    let r = ok(&format!(
        "VOLUME 100\nPLAYWAV \"{path}\"\nPRINT SOUNDQUEUE(); \" \"; SOUNDPLAYING()\n\
         SOUNDWAIT\nPRINT SOUNDQUEUE(); \" \"; SOUNDPLAYING()"
    ));
    assert_eq!(r.lines(), vec!["1000 1", "0 0"]);
    assert_eq!(r.audio.rendered_samples(), clip);
    // A stopped clip keeps only what was played (20 ms = 882 samples).
    let r = ok(&format!(
        "VOLUME 100\nPLAYWAV \"{path}\"\nSLEEP 20\nSTOPSOUND\nPRINT SOUNDQUEUE()"
    ));
    assert_eq!(r.output, "0\n");
    assert_eq!(r.audio.rendered_samples(), clip[..882].to_vec());
    assert_eq!(r.audio.events().len(), 1, "events keeps everything queued");
}

#[test]
fn playwav_counts_against_the_queue_limit() {
    let dir = TempDir::new("sound-playwav-limit");
    let nine_minutes = common::pcm_wav(1, 8_000, 8, &vec![128; 8_000 * 540]);
    let path = stored(&dir, "nine.wav", &nine_minutes);
    let r = ok(&format!(
        "PLAYWAV \"{path}\"\nTRY\nPLAYWAV \"{path}\"\nPRINT \"no error\"\nCATCH e$\nPRINT e$\nEND TRY\n\
         PRINT SOUNDQUEUE()"
    ));
    let lines = r.lines();
    assert!(
        lines[0].starts_with("PLAYWAV: the sound queue is full"),
        "{lines:?}"
    );
    assert_eq!(lines[1], "540000");
    assert_eq!(r.audio.events().len(), 1);
}

#[test]
fn playwav_and_wavlength_errors_are_catchable_and_name_the_file() {
    let dir = TempDir::new("sound-playwav-errors");
    let missing = slashed(&dir, "missing.wav");
    let folder = slashed(&dir, "folder");
    std::fs::create_dir(dir.file("folder")).unwrap();
    let garbage = stored(&dir, "garbage.wav", b"this is not a wave file at all");
    let tiny = stored(&dir, "tiny.wav", b"RIFF");
    let float = stored(&dir, "float.wav", &{
        let mut bytes = common::pcm_wav(1, 44_100, 32, &[0; 16]);
        bytes[20] = 3; // IEEE float
        bytes
    });
    let too_long = stored(
        &dir,
        "long.wav",
        &common::pcm_wav(1, 8_000, 8, &vec![128; 8_000 * 601]),
    );
    for who in ["PLAYWAV", "WAVLENGTH"] {
        let wrap = |argument: &str| {
            if who == "PLAYWAV" {
                format!("PLAYWAV {argument}")
            } else {
                format!("x = WAVLENGTH({argument})")
            }
        };
        assert_eq!(
            caught(&wrap(&format!("\"{missing}\""))),
            format!("{who}: cannot read \"{missing}\": file not found")
        );
        assert_eq!(
            caught(&wrap(&format!("\"{folder}\""))),
            format!("{who}: cannot read \"{folder}\": it is a directory")
        );
        assert_eq!(
            caught(&wrap("\"\"")),
            format!("{who}: the file name is empty")
        );
        assert_eq!(
            caught(&wrap("\"  \"")),
            format!("{who}: the file name is empty")
        );
        assert_eq!(
            caught(&wrap(&format!("\"{garbage}\""))),
            format!("{who}: \"{garbage}\": not a WAV file (no RIFF header)")
        );
        assert_eq!(
            caught(&wrap(&format!("\"{tiny}\""))),
            format!("{who}: \"{tiny}\": too short to be a WAV file (4 bytes)")
        );
        let message = caught(&wrap(&format!("\"{float}\"")));
        assert!(
            message.starts_with(&format!(
                "{who}: \"{float}\": unsupported WAV format: IEEE floating point"
            )),
            "{message}"
        );
        assert_eq!(
            caught(&wrap("42")),
            format!("{who}: the file name must be a string")
        );
    }
    // Only PLAYWAV has to decode (and so refuses audio longer than the queue).
    let message = caught(&format!("PLAYWAV \"{too_long}\""));
    assert!(
        message.contains("too long to play (601 seconds; the limit is 600 seconds)"),
        "{message}"
    );
    // Argument counts are compile errors like the other sound routines.
    for (source, message) in [
        ("PLAYWAV", ""),
        (
            "PLAYWAV \"a\", \"b\"",
            "PLAYWAV requires 1 arguments, got 2",
        ),
        ("x = WAVLENGTH()", "WAVLENGTH requires 1 arguments, got 0"),
    ] {
        let error = compile(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}: {error}");
    }
    // An uncaught error carries the usual line prefix.
    let r = run(&format!("PRINT 1\nPLAYWAV \"{missing}\""));
    assert_eq!(
        r.result.unwrap_err().to_string(),
        format!("line 2: PLAYWAV: cannot read \"{missing}\": file not found")
    );
}

#[test]
fn playwav_refuses_files_over_the_size_cap_without_reading_them() {
    let dir = TempDir::new("sound-playwav-big");
    let path = slashed(&dir, "big.wav");
    let file = std::fs::File::create(dir.file("big.wav")).unwrap();
    file.set_len(basic_vm::MAX_WAV_FILE_BYTES + 1).unwrap();
    drop(file);
    let message = caught(&format!("PLAYWAV \"{path}\""));
    assert!(
        message.starts_with(&format!(
            "PLAYWAV: cannot read \"{path}\": the file is too large"
        )),
        "{message}"
    );
    assert!(message.contains("at most 32 MiB"), "{message}");
}

#[test]
fn a_wav_file_made_by_the_program_s_own_writer_plays_back_identically() {
    // Render a melody, store it as WAV, play it back: same samples at full
    // volume (the closed loop behind --save-audio and wavdemo.bas).
    let dir = TempDir::new("sound-playwav-roundtrip");
    let first = ok("WAVE SINE\nVOLUME 80\nPLAY \"T200 L8 CDEFGAB\"");
    let melody = first.audio.rendered_samples();
    let path = stored(&dir, "melody.wav", &wav_bytes(&melody));
    let second = ok(&format!("VOLUME 100\nPLAYWAV \"{path}\""));
    assert_eq!(second.audio.rendered_samples(), melody);
}
