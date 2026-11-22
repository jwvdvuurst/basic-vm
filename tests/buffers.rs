//! Double-buffered graphics: SCREEN's buffers argument, FLIP, BUFFER,
//! COPYBUFFER and the buffer query functions.

use std::io::Cursor;

use basic_vm::{HeadlessBackend, Vm, compile};

const RED: u32 = 0xAA0000;
const GREEN: u32 = 0x00AA00;
const BLUE: u32 = 0x0000AA;
const WHITE: u32 = 0xFFFFFF;

struct Run {
    output: String,
    result: Result<(), basic_vm::Error>,
    vm: Vm,
    backend: HeadlessBackend,
}

impl Run {
    fn error(&self) -> String {
        self.result.as_ref().unwrap_err().message.clone()
    }
    /// The pixel of the frame the display shows (the last presented one).
    fn shown(&self, x: i64, y: i64) -> Option<u32> {
        self.backend.front_frame().and_then(|c| c.pixel(x, y))
    }
}

fn run(source: &str) -> Run {
    let mut vm = Vm::new(compile(source).unwrap_or_else(|e| panic!("{e}\n{source}"))).unwrap();
    let backend = HeadlessBackend::new();
    backend.record_frames(16);
    vm.set_graphics_backend(Box::new(backend.clone()));
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    Run {
        output: String::from_utf8(output).unwrap(),
        result,
        vm,
        backend,
    }
}

fn ok(source: &str) -> Run {
    let r = run(source);
    if let Err(e) = &r.result {
        panic!("{e}\n{source}");
    }
    r
}

#[test]
fn back_buffer_drawing_is_not_shown_until_flip() {
    let r = ok("SCREEN 8, 8, \"T\", 2\nPSET 1, 1, 4\nDOEVENTS\nSLEEP 10\nDOEVENTS");
    assert_eq!(r.backend.buffers(), 2);
    assert_eq!(r.backend.frames(), 0, "nothing was published");
    assert!(r.backend.front_frame().is_none());
    // Drawing went to the active (back) page; the visible page is untouched.
    assert_eq!(r.vm.canvas().unwrap().pixel(1, 1), Some(RED));
    assert_eq!(r.vm.visible_canvas().unwrap().pixel(1, 1), Some(0));

    let r = ok("SCREEN 8, 8, \"T\", 2\nPSET 1, 1, 4\nFLIP\nSLEEP 10");
    assert_eq!(r.backend.frames(), 1);
    assert_eq!(r.shown(1, 1), Some(RED));
    assert_eq!(r.vm.visible_canvas().unwrap().pixel(1, 1), Some(RED));
    // Drawing moved to the other page.
    assert_eq!(r.vm.canvas().unwrap().pixel(1, 1), Some(0));
}

#[test]
fn every_drawing_statement_targets_the_back_buffer() {
    let scene = "GCLS 1\nLINE 0, 0, 15, 15, 4\nLINE 2, 2, 9, 5, 2, BF\nCIRCLE 8, 8, 5, 14\n\
                 CIRCLE 8, 8, 2, 13, FILL\nTEXT 1, 1, \"A\", 15\nPSET 15, 0, 3\n\
                 PRINT POINT(0, 0)";
    let hidden = ok(&format!("SCREEN 16, 16, \"T\", 2\n{scene}\nDOEVENTS"));
    assert_eq!(
        hidden.output,
        format!("{RED}\n"),
        "POINT reads the back page"
    );
    assert_eq!(hidden.backend.frames(), 0);
    let visible = hidden.vm.visible_canvas().unwrap();
    assert!(visible.pixels().iter().all(|p| *p == 0), "front untouched");
    let back = hidden.vm.canvas().unwrap().clone();
    assert_eq!(back.pixel(0, 0), Some(RED));
    assert_eq!(back.pixel(15, 0), Some(0x00AAAA));
    assert_eq!(back.pixel(9, 2), Some(GREEN));

    let flipped = ok(&format!("SCREEN 16, 16, \"T\", 2\n{scene}\nFLIP"));
    assert_eq!(flipped.backend.frames(), 1);
    assert_eq!(
        flipped.backend.front_frame().unwrap().pixels(),
        back.pixels()
    );
}

#[test]
fn flip_swaps_roles_and_keeps_the_previous_frame_in_the_new_back_buffer() {
    let r = ok("SCREEN 4, 4, \"T\", 2\n\
         PRINT ACTIVEBUFFER(); VISIBLEBUFFER(); BUFFERS()\n\
         PSET 0, 0, 4\n\
         FLIP\n\
         PRINT ACTIVEBUFFER(); VISIBLEBUFFER()\n\
         PRINT POINT(0, 0)\n\
         PSET 1, 1, 2\n\
         FLIP\n\
         PRINT ACTIVEBUFFER(); VISIBLEBUFFER()\n\
         PRINT POINT(0, 0); \"|\"; POINT(1, 1)");
    // Buffer 1 starts visible, buffer 2 is the first back buffer.
    assert_eq!(
        r.output,
        format!("212\n12\n0\n21\n{RED}|0\n"),
        "after two flips the back buffer holds the frame from two flips ago"
    );
    let history = r.backend.frame_history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].pixel(0, 0), Some(RED));
    assert_eq!(history[0].pixel(1, 1), Some(0));
    assert_eq!(history[1].pixel(0, 0), Some(0));
    assert_eq!(history[1].pixel(1, 1), Some(GREEN));
}

#[test]
fn flip_clear_blanks_the_new_back_buffer() {
    let r = ok(
        "SCREEN 4, 4, \"T\", 2\nGCLS 1\nPSET 0, 0, 4\nFLIP\nPSET 3, 3, 4\nFLIP\n\
         PRINT POINT(0, 0)\nFLIP CLEAR\nPRINT POINT(0, 0); POINT(3, 3)\n\
         PRESENT CLEAR\nPRINT POINT(0, 0)",
    );
    // Two plain flips leave frame 1 in the back buffer; FLIP CLEAR empties it.
    assert_eq!(r.output, format!("{RED}\n00\n0\n"));
    assert_eq!(r.backend.frames(), 4);
    let history = r.backend.frame_history();
    // The third frame was the back buffer that held frame 1 (blue + red dot).
    assert_eq!(history[2].pixel(0, 0), Some(RED));
    assert_eq!(history[2].pixel(2, 2), Some(BLUE));
    assert_eq!(history[2].pixel(3, 3), Some(BLUE));
}

#[test]
fn buffer_selects_the_drawing_target_and_visible_page_draws_are_auto_presented() {
    let r = ok("SCREEN 4, 4, \"T\", 2\n\
         BUFFER 1\nPRINT ACTIVEBUFFER(); VISIBLEBUFFER()\nPSET 0, 0, 4\n\
         BUFFER 2\nPRINT ACTIVEBUFFER()\nPSET 1, 1, 2\n\
         DOEVENTS");
    assert_eq!(r.output, "11\n2\n");
    // Only the visible page (buffer 1, changed by the first PSET) was
    // presented by DOEVENTS; the buffer 2 pixel stays hidden.
    assert_eq!(r.backend.frames(), 1);
    assert_eq!(r.shown(0, 0), Some(RED));
    assert_eq!(r.shown(1, 1), Some(0));
    assert_eq!(r.vm.canvas().unwrap().pixel(1, 1), Some(GREEN));
}

#[test]
fn flip_publishes_whatever_buffer_is_active() {
    let r = ok(
        "SCREEN 4, 4, \"T\", 2\nBUFFER 1\nPSET 0, 0, 4\nFLIP\nPRINT ACTIVEBUFFER(); VISIBLEBUFFER()",
    );
    // Buffer 1 was already visible: it is published again and drawing moves on.
    assert_eq!(r.output, "21\n");
    assert_eq!(r.shown(0, 0), Some(RED));
}

#[test]
fn copybuffer_copies_whole_buffers() {
    let r = ok("SCREEN 4, 4, \"T\", 2\n\
         BUFFER 1\nPSET 0, 0, 4\nBUFFER 2\nPSET 1, 1, 2\n\
         COPYBUFFER 1, 2\nPRINT POINT(0, 0); \"|\"; POINT(1, 1)\n\
         PSET 2, 2, 1\nCOPYBUFFER 2, 1\nBUFFER 1\nPRINT POINT(2, 2); \"|\"; POINT(0, 0)\n\
         COPYBUFFER 1, 1\nPRINT POINT(2, 2)");
    assert_eq!(r.output, format!("{RED}|0\n{BLUE}|{RED}\n{BLUE}\n"));
    // Copying into the visible buffer shows up at the next automatic present.
    let r = ok("SCREEN 4, 4, \"T\", 2\nPSET 3, 3, 4\nCOPYBUFFER 2, 1\nDOEVENTS");
    assert_eq!(r.backend.frames(), 1);
    assert_eq!(r.shown(3, 3), Some(RED));
}

#[test]
fn buffer_arguments_are_validated() {
    for statement in [
        "BUFFER 0",
        "BUFFER 3",
        "BUFFER -1",
        "COPYBUFFER 1, 3",
        "COPYBUFFER 0, 1",
    ] {
        let r = run(&format!("SCREEN 4, 4, \"T\", 2\n{statement}"));
        assert!(r.error().contains("buffer"), "{statement}: {}", r.error());
    }
    assert!(
        run("SCREEN 4, 4, \"T\", 2\nBUFFER \"a\"")
            .error()
            .contains("BUFFER")
    );
}

#[test]
fn single_buffer_mode_is_unchanged_and_rejects_buffer_two() {
    let r = ok("SCREEN 4, 4\nPSET 1, 1\nDOEVENTS\n\
         PRINT BUFFERS(); ACTIVEBUFFER(); VISIBLEBUFFER()\nBUFFER 1\nCOPYBUFFER 1, 1");
    assert_eq!(r.output, "111\n");
    assert_eq!(r.backend.buffers(), 1);
    assert_eq!(r.backend.frames(), 1, "DOEVENTS presents in single mode");
    assert_eq!(r.shown(1, 1), Some(WHITE));
    let explicit = ok("SCREEN 4, 4, \"T\", 1\nPSET 1, 1\nSLEEP 5");
    assert_eq!(
        explicit.backend.frames(),
        1,
        "SLEEP presents in single mode"
    );
    assert_eq!(explicit.backend.buffers(), 1);
    // Both views are the one canvas.
    assert_eq!(explicit.vm.canvas(), explicit.vm.visible_canvas());
    for statement in ["BUFFER 2", "COPYBUFFER 1, 2", "COPYBUFFER 2, 1"] {
        let r = run(&format!("SCREEN 4, 4\n{statement}"));
        assert!(
            r.error().contains("single-buffered"),
            "{statement}: {}",
            r.error()
        );
    }
    let r = run("SCREEN 4, 4\nFLIP CLEAR");
    assert!(r.error().contains("double-buffered"), "{}", r.error());
    // FLIP in single mode still presents the one canvas.
    let r = ok("SCREEN 4, 4\nPSET 2, 2\nFLIP\nFLIP");
    assert_eq!(r.backend.frames(), 2);
}

#[test]
fn screen_buffer_count_is_validated() {
    for bad in ["0", "3", "-1", "4", "\"2\""] {
        let r = run(&format!("SCREEN 4, 4, \"T\", {bad}"));
        assert!(r.result.is_err(), "buffers {bad} must fail");
        if bad != "\"2\"" {
            assert!(
                r.error().contains("buffers must be 1"),
                "{bad}: {}",
                r.error()
            );
        }
    }
    assert!(run("SCREEN 4, 4, 5, 2").error().contains("title"));
    assert!(compile("SCREEN 4, 4, \"T\", 2, 1").is_err());
    assert!(!run("SCREEN 4, 4, \"T\", 3").backend.is_open());
}

#[test]
fn screen_zero_releases_every_buffer() {
    let r = ok("SCREEN 4, 4, \"T\", 2\nPSET 0, 0, 4\nFLIP\nSCREEN 0\nPRINT ISGRAPHICS()");
    assert_eq!(r.output, "0\n");
    assert!(!r.backend.is_open());
    assert!(r.vm.canvas().is_none());
    assert!(r.vm.visible_canvas().is_none());
    for statement in [
        "PRINT BUFFERS()",
        "BUFFER 1",
        "PRINT ACTIVEBUFFER()",
        "FLIP",
    ] {
        let r = run(&format!("SCREEN 4, 4, \"T\", 2\nSCREEN 0\n{statement}"));
        assert!(
            r.error().contains("no graphics screen"),
            "{statement}: {}",
            r.error()
        );
    }
    // Re-opening starts from blank, role-reset buffers, with the new count.
    let r = ok(
        "SCREEN 4, 4, \"T\", 2\nPSET 0, 0, 4\nFLIP\nPSET 1, 1, 4\nSCREEN 4, 4\n\
         PRINT BUFFERS(); ACTIVEBUFFER(); VISIBLEBUFFER(); POINT(0, 0); POINT(1, 1)\n\
         SCREEN 4, 4, \"T\", 2\nPRINT ACTIVEBUFFER(); VISIBLEBUFFER(); POINT(0, 0)",
    );
    assert_eq!(r.output, "11100\n210\n");
    assert_eq!(r.backend.opened(), 3);
}

#[test]
fn buffer_statements_without_a_screen_are_explicit_errors() {
    for statement in [
        "BUFFER 1",
        "COPYBUFFER 1, 2",
        "PRINT ACTIVEBUFFER()",
        "PRINT VISIBLEBUFFER()",
        "PRINT BUFFERS()",
        "FLIP CLEAR",
    ] {
        let r = run(statement);
        assert!(
            r.error().contains("no graphics screen"),
            "{statement}: {}",
            r.error()
        );
    }
}

#[test]
fn double_buffer_errors_are_catchable_and_handlers_still_run() {
    let r = ok(
        "FUNCTION OnKey(PARAMETER K$)\nPSET 0, 0, 4\nFEND\nON KEY CALL OnKey\n\
         SCREEN 4, 4, \"T\", 2\nTRY\nBUFFER 7\nCATCH\nPRINT \"caught\"\nEND TRY\n\
         DOEVENTS\nFLIP\nPRINT \"done\"",
    );
    assert_eq!(r.output, "caught\ndone\n");
}

#[test]
fn final_presented_frame_is_the_last_flipped_one() {
    let r = ok("SCREEN 4, 4, \"T\", 2\nPSET 0, 0, 4\nFLIP\nGCLS 1\nPSET 1, 1, 2\nDOEVENTS");
    // The unfinished second frame is never shown, not even at the end.
    assert_eq!(r.backend.frames(), 1);
    assert_eq!(r.shown(0, 0), Some(RED));
    assert_eq!(r.shown(1, 1), Some(0));
    assert_eq!(r.shown(3, 3), Some(0));
}

#[test]
fn animation_frames_are_whole_frames() {
    // Each flipped frame must contain exactly the ball of its own iteration.
    let r =
        ok("SCREEN 20, 4, \"T\", 2\nFOR I = 0 TO 4\nGCLS\nPSET I * 4, 1, 15\nFLIP\nDOEVENTS\nNEXT");
    let history = r.backend.frame_history();
    assert_eq!(history.len(), 5);
    for (i, frame) in history.iter().enumerate() {
        let lit: Vec<usize> = frame
            .pixels()
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == WHITE)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(lit, vec![20 + i * 4], "frame {i}");
    }
}

#[test]
fn geometry_functions_report_size_centre_and_bounds() {
    let r = ok(concat!(
        "SCREEN 640, 480, \"T\", 2\n",
        "PRINT WIDTH(); \" \"; HEIGHT(); \" \"; CENTERX(); \" \"; CENTERY()\n",
        "C = CENTER()\n",
        "PRINT LEN(C); \" \"; C[1]; \" \"; C[2]\n",
        "PRINT INSIDE(0, 0); INSIDE(639, 479); INSIDE(640, 0); INSIDE(0, 480); INSIDE(-1, 5); INSIDE(5, -1)\n",
        "PRINT INSIDE(639.9, 479.9); INSIDE(-0.5, 0)\n",
        "SCREEN 5, 3\n",
        "PRINT WIDTH(); \" \"; HEIGHT(); \" \"; CENTERX(); \" \"; CENTERY()\n",
    ));
    assert_eq!(
        r.output,
        "640 480 320 240\n2 320 240\n110000\n11\n5 3 2 1\n"
    );
}

#[test]
fn geometry_functions_need_a_screen_and_checked_arguments() {
    let r = run("PRINT WIDTH()");
    assert!(r.error().contains("no graphics screen"), "{}", r.error());
    let r = run("PRINT INSIDE(1, 1)");
    assert!(r.error().contains("no graphics screen"), "{}", r.error());
    let r = run("SCREEN 4, 4\nPRINT INSIDE(\"a\", 1)");
    assert!(r.result.is_err());
    assert!(compile("SCREEN 4, 4\nPRINT INSIDE(1)").is_err());
    assert!(compile("SCREEN 4, 4\nPRINT WIDTH(1)").is_err());
}

#[test]
fn width_and_height_remain_usable_as_plain_variable_names() {
    let r = ok("WIDTH = 3\nHEIGHT = 4\nPRINT WIDTH * HEIGHT");
    assert_eq!(r.output, "12\n");
}
