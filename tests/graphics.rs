//! Console statements, the software graphics core and event handlers.

use std::io::Cursor;

use basic_vm::{
    AudioEvent, AutoClose, Event, HeadlessAudio, HeadlessBackend, ScriptedKeys, Vm, compile,
};

const WHITE: u32 = 0xFFFFFF;

struct Run {
    output: String,
    result: Result<(), basic_vm::Error>,
    vm: Vm,
    backend: HeadlessBackend,
    keys: ScriptedKeys,
}

impl Run {
    fn pixel(&self, x: i64, y: i64) -> Option<u32> {
        self.vm.canvas().and_then(|canvas| canvas.pixel(x, y))
    }
    fn count(&self, color: u32) -> usize {
        self.vm
            .canvas()
            .unwrap()
            .pixels()
            .iter()
            .filter(|pixel| **pixel == color)
            .count()
    }
    fn error(&self) -> String {
        self.result.as_ref().unwrap_err().message.clone()
    }
}

fn run_with(source: &str, setup: impl FnOnce(&HeadlessBackend, &ScriptedKeys)) -> Run {
    let mut vm = Vm::new(compile(source).unwrap_or_else(|e| panic!("{e}\n{source}"))).unwrap();
    let backend = HeadlessBackend::new();
    let keys = ScriptedKeys::new();
    setup(&backend, &keys);
    vm.set_graphics_backend(Box::new(backend.clone()));
    vm.set_key_provider(Box::new(keys.clone()));
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    Run {
        output: String::from_utf8(output).unwrap(),
        result,
        vm,
        backend,
        keys,
    }
}

fn run(source: &str) -> Run {
    run_with(source, |_, _| {})
}

fn ok(source: &str) -> Run {
    let r = run(source);
    if let Err(e) = &r.result {
        panic!("{e}\n{source}");
    }
    r
}

#[test]
fn console_sequences_are_exact() {
    let r = ok("CLS\nCOLOR 14\nCOLOR 15, 1\nLOCATE 5, 10\nLOCATE 3\nPRINT \"x\"");
    assert_eq!(
        r.output,
        "\x1b[2J\x1b[H\x1b[93m\x1b[97;44m\x1b[5;10H\x1b[3;1Hx\n\x1b[0m"
    );
}

#[test]
fn console_arguments_are_validated() {
    assert!(run("COLOR 16").error().contains("COLOR"));
    assert!(run("LOCATE 0, 1").error().contains("LOCATE"));
}

#[test]
fn inkey_reads_scripted_keys_without_blocking() {
    let r = run_with(
        "PRINT \"[\" + INKEY$() + \"]\"\nPRINT \"[\" + INKEY$() + \"]\"",
        |_, keys| keys.push("a"),
    );
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "[a]\n[]\n");
}

#[test]
fn sleep_uses_the_provider_and_is_bounded() {
    let r = ok("SLEEP 250\nSLEEP 0");
    assert_eq!(r.keys.sleeps(), vec![250, 0]);
    assert!(run("SLEEP -1").result.is_err());
    assert!(run("SLEEP 100000000").result.is_err());
}

#[test]
fn pset_point_and_palette() {
    let r = ok(
        "SCREEN 10, 10\nPSET 3, 4, 4\nPSET 5, 5, RGB(1, 2, 3)\nPSET 6, 6\n\
         PRINT POINT(3, 4); POINT(5, 5); POINT(6, 6); POINT(-1, 0); POINT(10, 0); POINT(0, 0)\n\
         PRINT RGB(255, 128, 1)",
    );
    assert_eq!(
        r.output,
        format!("{}{}{}-1-10\n{}\n", 0xAA0000, 0x010203, WHITE, 0xFF8001)
    );
}

#[test]
fn gcolor_sets_default_and_gcls_clears() {
    let r = ok("SCREEN 4, 4\nGCLS 1\nGCOLOR 2\nPSET 1, 1\nGCOLOR &H102030\nPSET 2, 2");
    assert_eq!(r.pixel(0, 0), Some(0x0000AA));
    assert_eq!(r.pixel(1, 1), Some(0x00AA00));
    assert_eq!(r.pixel(2, 2), Some(0x102030));
}

#[test]
fn lines_have_inclusive_endpoints_and_exact_counts() {
    let r = ok("SCREEN 20, 20\nLINE 2, 3, 11, 3\nLINE 15, 0, 15, 19");
    assert_eq!(r.count(WHITE), 10 + 20);
    let r = ok("SCREEN 20, 20\nLINE 1, 1, 8, 8\nLINE 8, 1, 1, 8");
    assert_eq!(r.pixel(1, 1), Some(WHITE));
    assert_eq!(r.pixel(8, 8), Some(WHITE));
    assert_eq!(r.pixel(8, 1), Some(WHITE));
    assert_eq!(r.pixel(1, 8), Some(WHITE));
    assert_eq!(r.pixel(4, 4), Some(WHITE));
    assert_eq!(r.count(WHITE), 16);
    // Reversed endpoints draw the same pixels.
    let a = ok("SCREEN 30, 30\nLINE 2, 3, 25, 11");
    let b = ok("SCREEN 30, 30\nLINE 25, 11, 2, 3");
    assert_eq!(
        a.vm.canvas().unwrap().pixels(),
        b.vm.canvas().unwrap().pixels()
    );
    assert_eq!(a.count(WHITE), 24);
}

#[test]
fn boxes_outline_and_fill() {
    let r = ok("SCREEN 20, 20\nLINE 2, 2, 11, 6, 4, B");
    assert_eq!(r.count(0xAA0000), 10 * 5 - 8 * 3);
    assert_eq!(r.pixel(5, 4), Some(0));
    let r = ok("SCREEN 20, 20\nLINE 2, 2, 11, 6, 4, BF");
    assert_eq!(r.count(0xAA0000), 50);
    // Omitted colour uses the current one.
    let r = ok("SCREEN 20, 20\nGCOLOR 2\nLINE 2, 2, 5, 5, , BF");
    assert_eq!(r.count(0x00AA00), 16);
    // Reversed corners are normalised.
    let r = ok("SCREEN 20, 20\nLINE 11, 6, 2, 2, , BF");
    assert_eq!(r.count(WHITE), 50);
}

#[test]
fn circles_are_symmetric_and_fill_is_bounded() {
    let r = ok("SCREEN 41, 41\nCIRCLE 20, 20, 10");
    assert_eq!(r.pixel(30, 20), Some(WHITE));
    assert_eq!(r.pixel(10, 20), Some(WHITE));
    assert_eq!(r.pixel(20, 10), Some(WHITE));
    assert_eq!(r.pixel(20, 30), Some(WHITE));
    assert_eq!(r.pixel(20, 20), Some(0));
    for y in 0..41 {
        for x in 0..41 {
            assert_eq!(r.pixel(x, y), r.pixel(40 - x, y));
            assert_eq!(r.pixel(x, y), r.pixel(x, 40 - y));
            assert_eq!(r.pixel(x, y), r.pixel(y, x));
        }
    }
    let r = ok("SCREEN 41, 41\nCIRCLE 20, 20, 10, 4, FILL");
    let area = r.count(0xAA0000) as f64;
    let ideal = std::f64::consts::PI * 100.0;
    assert!((area - ideal).abs() < ideal * 0.12, "area {area}");
    assert_eq!(r.pixel(20, 20), Some(0xAA0000));
    assert_eq!(r.pixel(0, 0), Some(0));
}

#[test]
fn drawing_is_clipped_and_never_panics() {
    let r = ok("SCREEN 10, 10\nLINE -1000000000, -5, 1000000000, 14\n\
         LINE -99999999999999, -99999999999999, 99999999999999, 99999999999999\n\
         LINE -50, -50, 500, 500, 1, BF\nCIRCLE 1000000, 1000000, 5\n\
         CIRCLE 5, 5, 1000000\nCIRCLE 5, 5, 1000000, 2, FILL\nPSET 1E300, -1E300\n\
         TEXT -100, -100, \"hi\"\nTEXT 1000000, 3, \"hi\"");
    r.result.as_ref().unwrap();
    assert_eq!(r.pixel(0, 0), Some(0x00AA00));
}

#[test]
fn non_integers_truncate_toward_zero() {
    let r = ok("SCREEN 10, 10\nPSET 2.9, 3.9\nPSET -0.5, 0.5");
    assert_eq!(r.pixel(2, 3), Some(WHITE));
    assert_eq!(r.pixel(0, 0), Some(WHITE));
}

#[test]
fn text_draws_glyph_pixels_in_its_cell() {
    let r = ok("SCREEN 40, 20\nTEXT 8, 4, \"A\", 4");
    // Row 0 of 'A' in the 8x8 font is 0x0C: columns 2 and 3.
    assert_eq!(r.pixel(8 + 2, 4), Some(0xAA0000));
    assert_eq!(r.pixel(8 + 3, 4), Some(0xAA0000));
    assert_eq!(r.pixel(8, 4), Some(0));
    assert!(r.count(0xAA0000) > 10);
    for (x, y, p) in
        r.vm.canvas()
            .unwrap()
            .pixels()
            .iter()
            .enumerate()
            .map(|(i, p)| ((i % 40) as i64, (i / 40) as i64, *p))
    {
        if p != 0 {
            assert!((8..16).contains(&x) && (4..12).contains(&y));
        }
    }
    let blank = ok("SCREEN 40, 20\nTEXT 0, 0, \" \"");
    assert_eq!(blank.count(0), 800);
}

#[test]
fn screen_zero_resets_and_open_close_are_visible() {
    let r = ok("PRINT ISGRAPHICS(); WINDOWOPEN()\nSCREEN 8, 6, \"T\"\n\
         PRINT ISGRAPHICS(); WINDOWOPEN(); SCREENW(); SCREENH()\nSCREEN 0\n\
         PRINT ISGRAPHICS(); WINDOWOPEN()");
    assert_eq!(r.output, "00\n1186\n00\n");
    assert_eq!(r.backend.title(), "T");
    assert!(!r.backend.is_open());
    assert_eq!(r.backend.size(), (8, 6));
}

#[test]
fn drawing_without_screen_is_an_explicit_error() {
    for statement in [
        "PSET 1, 1",
        "GCLS",
        "LINE 0, 0, 1, 1",
        "CIRCLE 1, 1, 1",
        "TEXT 0, 0, \"a\"",
        "FLIP",
        "GCOLOR 1",
        "PRINT POINT(0, 0)",
    ] {
        let r = run(statement);
        assert!(
            r.error().contains("no graphics screen"),
            "{statement}: {}",
            r.error()
        );
    }
    let r = run("SCREEN 5, 5\nSCREEN 0\nPSET 1, 1");
    assert!(r.error().contains("no graphics screen"));
}

#[test]
fn argument_errors_are_explicit() {
    assert!(run("SCREEN 0, 5").result.is_err());
    assert!(run("SCREEN 5000, 5").result.is_err());
    assert!(run("SCREEN 5, 5\nPSET \"a\", 1").error().contains("PSET"));
    assert!(run("SCREEN 5, 5\nPSET 1, 1, -3").result.is_err());
    assert!(run("SCREEN 5, 5\nPSET 1, 1, 17000000").result.is_err());
    assert!(run("SCREEN 5, 5\nPRINT RGB(256, 0, 0)").result.is_err());
    assert!(run("SCREEN 5, 5\nCIRCLE 1, 1, -1").result.is_err());
}

#[test]
fn screen_without_backend_reports_how_to_enable_one() {
    let mut vm = Vm::new(compile("SCREEN 5, 5").unwrap()).unwrap();
    let error = vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap_err();
    assert!(error.message.contains("graphics"));
}

#[test]
fn frames_are_presented_and_final_frame_is_kept() {
    let r = ok("SCREEN 4, 4\nPSET 1, 1\nFLIP\nPSET 2, 2\nPRESENT");
    assert!(r.backend.frames() >= 2);
    assert_eq!(r.backend.last_frame().unwrap().pixel(2, 2), Some(WHITE));
    let r = ok("SCREEN 4, 4\nPSET 1, 1");
    assert_eq!(r.backend.last_frame().unwrap().pixel(1, 1), Some(WHITE));
}

const HANDLERS: &str = "\
FUNCTION OnKey(PARAMETER K$)\nPRINT \"key \" + K$\nFEND\n\
FUNCTION OnMouse(PARAMETER X, PARAMETER Y, PARAMETER B)\nPRINT \"mouse\"; X; Y; B\nFEND\n\
FUNCTION OnClose()\nPRINT \"close\"\nFEND\n\
ON KEY CALL OnKey\nON MOUSE CALL OnMouse\nON CLOSE CALL OnClose\n";

fn key(name: &str) -> Event {
    Event::KeyDown(name.to_string())
}

#[test]
fn handlers_dispatch_in_order_with_arguments() {
    let source = format!("{HANDLERS}SCREEN 8, 8\nDOEVENTS\nPRINT \"closed=\"; CLOSED()");
    let r = run_with(&source, |backend, _| {
        backend.push_batch(vec![
            key("A"),
            Event::Mouse {
                x: 3,
                y: 4,
                buttons: 1,
            },
            key("ENTER"),
            Event::Close,
        ]);
    });
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "key A\nmouse341\nkey ENTER\nclose\nclosed=1\n");
}

#[test]
fn doevents_without_events_is_a_no_op_and_close_flag_has_no_handler_effect() {
    let r = ok(&format!("{HANDLERS}SCREEN 8, 8\nDOEVENTS\nPRINT CLOSED()"));
    assert_eq!(r.output, "0\n");
    let r = run_with("SCREEN 8, 8\nDOEVENTS\nPRINT CLOSED()", |backend, _| {
        backend.push_event(Event::Close)
    });
    assert_eq!(r.output, "1\n");
}

#[test]
fn handler_state_changes_drawn_pixels() {
    let source = "\
FUNCTION OnKey(PARAMETER K$)\nPSET N, 0\nN = N + 1\nFEND\n\
ON KEY CALL OnKey\nSCREEN 8, 2\nN = 0\nDOEVENTS\nDOEVENTS";
    let r = run_with(source, |backend, _| {
        backend.push_batch(vec![key("A"), key("B")]);
        backend.push_batch(vec![key("C")]);
    });
    r.result.as_ref().unwrap();
    assert_eq!(r.pixel(0, 0), Some(WHITE));
    assert_eq!(r.pixel(1, 0), Some(WHITE));
    assert_eq!(r.pixel(2, 0), Some(WHITE));
    assert_eq!(r.pixel(3, 0), Some(0));
}

#[test]
fn polling_functions_see_keys_and_mouse() {
    let r = run_with(
        "SCREEN 8, 8\nFLIP\nPRINT MOUSEX(); MOUSEY(); MOUSEB(); KEYDOWN(\"left\")\n\
         PRINT GETKEY$() + \"|\" + GETKEY$() + \"|\" + GETKEY$()",
        |backend, _| {
            backend.push_batch(vec![
                Event::KeyDown("LEFT".into()),
                Event::KeyDown("X".into()),
                Event::Mouse {
                    x: 7,
                    y: 2,
                    buttons: 2,
                },
            ]);
        },
    );
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "7221\nLEFT|X|\n");
    let r = run_with(
        "SCREEN 8, 8\nFLIP\nFLIP\nPRINT KEYDOWN(\"A\")",
        |backend, _| {
            backend.push_batch(vec![key("A")]);
            backend.push_batch(vec![Event::KeyUp("A".into())]);
        },
    );
    assert_eq!(r.output, "0\n");
}

#[test]
fn errors_in_handlers_propagate_and_can_be_trapped() {
    let source = "FUNCTION Bad(PARAMETER K$)\nPRINT 1 / 0\nFEND\nON KEY CALL Bad\nSCREEN 4, 4\n";
    let r = run_with(
        &format!("{source}DOEVENTS\nPRINT \"after\""),
        |backend, _| backend.push_event(key("A")),
    );
    assert!(r.error().to_lowercase().contains("zero") || !r.error().is_empty());
    assert!(!r.output.contains("after"));
    let r = run_with(
        &format!(
            "{source}TRY\nDOEVENTS\nCATCH\nPRINT \"caught\"\nEND TRY\nDOEVENTS\nPRINT \"after\""
        ),
        |backend, _| backend.push_event(key("A")),
    );
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "caught\nafter\n");
}

#[test]
fn handler_may_end_the_program() {
    let source = "FUNCTION OnClose()\nEND\nFEND\nON CLOSE CALL OnClose\nSCREEN 4, 4\nDOEVENTS\nPRINT \"never\"";
    let r = run_with(source, |backend, _| backend.push_event(Event::Close));
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "");
}

#[test]
fn nested_doevents_and_bad_handlers_are_rejected() {
    let r = run_with(
        "FUNCTION H(PARAMETER K$)\nDOEVENTS\nFEND\nON KEY CALL H\nSCREEN 4, 4\nDOEVENTS",
        |backend, _| backend.push_event(key("A")),
    );
    assert!(r.error().contains("DOEVENTS"));
    let wrong = |source: &str| compile(source).expect_err("compile error").message;
    assert!(
        wrong("ON KEY CALL Missing").contains("unknown function"),
        "{}",
        wrong("ON KEY CALL Missing")
    );
    assert!(wrong("FUNCTION F()\nFEND\nON KEY CALL F").contains("argument"));
    assert!(wrong("FUNCTION F(PARAMETER A)\nFEND\nON CLOSE CALL F").contains("argument"));
}

#[test]
fn on_off_removes_handlers() {
    let source = format!("{HANDLERS}ON KEY OFF\nSCREEN 4, 4\nDOEVENTS\nPRINT \"done\"");
    let r = run_with(&source, |backend, _| backend.push_event(key("A")));
    r.result.as_ref().unwrap();
    assert_eq!(r.output, "done\n");
}

#[test]
fn gc_is_safe_inside_handlers() {
    let source = "FUNCTION OnKey(PARAMETER K$)\nLOCAL L\nL = LIST(\"a\", K$)\nGC\nS$ = S$ + L[2]\nFEND\n\
ON KEY CALL OnKey\nSCREEN 4, 4\nS$ = \"\"\nDOEVENTS\nPRINT S$";
    let r = run_with(source, |backend, _| {
        backend.push_batch((0..200).map(|_| key("Z")).collect());
    });
    r.result.as_ref().unwrap();
    assert_eq!(r.output, format!("{}\n", "Z".repeat(200)));
}

#[test]
fn sleep_in_graphics_mode_uses_backend_and_presents() {
    let r = ok("SCREEN 4, 4\nPSET 0, 0\nSLEEP 40");
    assert_eq!(r.backend.slept_ms(), 40);
    assert!(r.backend.frames() >= 1);
}

#[test]
fn maze_game_moves_draws_and_plays_collection_sound() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("maze.bas");
    let source = std::fs::read_to_string(path).unwrap();
    let program = compile(&source).unwrap();
    let mut vm = Vm::new(program).unwrap();
    let graphics = HeadlessBackend::new();
    graphics.push_event(Event::KeyDown("RIGHT".into()));
    vm.set_graphics_backend(Box::new(AutoClose::new(graphics.clone(), None, Some(40))));
    let audio = HeadlessAudio::new();
    vm.set_audio_backend(Box::new(audio.clone()));
    vm.run(&mut Cursor::new(""), &mut Vec::new()).unwrap();

    let frame = graphics.last_frame().unwrap();
    assert_eq!(frame.pixel(30, 62), Some(0));
    assert!(
        (50..=230).any(|x| frame.pixel(x, 62) == Some(0xFFFF55)),
        "player should be drawn further along the corridor"
    );
    assert!(
        audio
            .events()
            .iter()
            .any(|event| matches!(event, AudioEvent::Tone(tone) if tone.frequency_hz == 700.0)),
        "collecting a dot should play a tone"
    );
}

#[test]
fn line_is_symmetric_for_swapped_endpoints() {
    for (x1, y1, x2, y2) in [
        (2, 3, 25, 11),
        (0, 0, 7, 4),
        (3, 9, 11, 2),
        (5, 1, 6, 28),
        (10, 10, 3, 10),
    ] {
        let a = ok(&format!("SCREEN 30, 30\nLINE {x1}, {y1}, {x2}, {y2}"));
        let b = ok(&format!("SCREEN 30, 30\nLINE {x2}, {y2}, {x1}, {y1}"));
        assert_eq!(
            a.vm.canvas().unwrap().pixels(),
            b.vm.canvas().unwrap().pixels(),
            "{x1},{y1}-{x2},{y2}"
        );
    }
}

#[test]
fn clamped_values_are_not_reported_and_bad_flags_are_clear() {
    let error = run("SLEEP 99999999999999999999").error();
    assert!(!error.contains("got 4"), "{error}");
    let error = run("SCREEN 99999999999999999999, 5").error();
    assert!(!error.contains("got 4"), "{error}");
    let source = |flag: &str| format!("SCREEN 5, 5\nLINE 0, 0, 3, 3, 3, {flag}");
    let error = match compile(&source("XYZ")) {
        Err(error) => error.message,
        Ok(_) => panic!("expected a compile error"),
    };
    assert!(error.contains("LINE flag must be B or BF"), "{error}");
    assert!(compile(&source("\"B\"")).is_err());
}
