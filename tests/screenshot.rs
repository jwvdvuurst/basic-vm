//! `SCREENSHOT file$ [, VISIBLE]` and the PNG/BMP/PPM encoders behind it.

mod common;

use std::io::Cursor;

use basic_vm::{Canvas, HeadlessBackend, Vm, compile};
use common::{TempDir, decode_bmp, decode_file, decode_png, decode_ppm, png_chunks};

const BASE: u32 = 0x0000AA;
const SPOT: u32 = 0x0A141E;
const RED: u32 = 0xAA0000;
const GREEN: u32 = 0x00AA00;

struct Run {
    output: String,
    result: Result<(), basic_vm::Error>,
    backend: HeadlessBackend,
}

impl Run {
    fn error(&self) -> String {
        self.result.as_ref().unwrap_err().message.clone()
    }
}

fn run(source: &str) -> Run {
    let mut vm = Vm::new(compile(source).unwrap_or_else(|e| panic!("{e}\n{source}"))).unwrap();
    let backend = HeadlessBackend::new();
    vm.set_graphics_backend(Box::new(backend.clone()));
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    Run {
        output: String::from_utf8(output).unwrap(),
        result,
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

/// The pixel colour the scene drawn by `SCENE` has at (x, y).
fn expected(x: usize, y: usize) -> u32 {
    match (x, y) {
        (0, 0) => SPOT,
        (1..=4, 1) => RED,
        (5, 3) => GREEN,
        _ => BASE,
    }
}

const SCENE: &str =
    "SCREEN 6, 4\nGCLS 1\nPSET 0, 0, RGB(10, 20, 30)\nLINE 1, 1, 4, 1, 4\nPSET 5, 3, 2\n";

fn assert_scene(image: &common::Image) {
    assert_eq!((image.width, image.height), (6, 4));
    for y in 0..4 {
        for x in 0..6 {
            assert_eq!(image.at(x, y), expected(x, y), "pixel ({x}, {y})");
        }
    }
}

#[test]
fn ppm_bmp_and_png_are_pixel_exact() {
    let dir = TempDir::new("shot-formats");
    for name in ["s.ppm", "s.bmp", "s.png", "UPPER.PNG", "Mixed.Bmp"] {
        ok(&format!("{SCENE}SCREENSHOT \"{}\"", dir.text(name)));
        assert_scene(&decode_file(&dir.file(name)));
    }
    // The BASIC statement writes exactly what the encoders produce.
    let mut canvas = Canvas::new(6, 4).unwrap();
    canvas.clear(BASE);
    canvas.set_pixel(0, 0, SPOT);
    canvas.line(1, 1, 4, 1, RED);
    canvas.set_pixel(5, 3, GREEN);
    assert_eq!(std::fs::read(dir.file("s.ppm")).unwrap(), canvas.to_ppm());
    assert_eq!(std::fs::read(dir.file("s.bmp")).unwrap(), canvas.to_bmp());
    assert_eq!(std::fs::read(dir.file("s.png")).unwrap(), canvas.to_png());
}

#[test]
fn png_has_the_documented_structure() {
    let dir = TempDir::new("shot-png");
    ok(&format!("{SCENE}SCREENSHOT \"{}\"", dir.text("s.png")));
    let data = std::fs::read(dir.file("s.png")).unwrap();
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
    let chunks = png_chunks(&data); // verifies every CRC
    let kinds: Vec<&[u8]> = chunks.iter().map(|(kind, _)| &kind[..]).collect();
    assert_eq!(kinds, [&b"IHDR"[..], b"IDAT", b"IEND"]);
    let header = &chunks[0].1;
    assert_eq!(&header[0..4], &6u32.to_be_bytes());
    assert_eq!(&header[4..8], &4u32.to_be_bytes());
    assert_eq!(&header[8..], &[8, 2, 0, 0, 0]);
    // 4 rows of (1 filter byte + 6 * 3 colour bytes) in stored blocks.
    assert_eq!(common::inflate_stored(&chunks[1].1).len(), 4 * 19);
}

#[test]
fn png_images_larger_than_one_stored_block_round_trip() {
    // 300 x 100 RGB needs 90,100 raw bytes: two stored deflate blocks.
    let mut canvas = Canvas::new(300, 100).unwrap();
    for y in 0..100 {
        for x in 0..300 {
            canvas.set_pixel(
                x,
                y,
                ((x * 7 + y * 3) as u32) << 8 | (x ^ y) as u32 & 0xFF | 0x010000,
            );
        }
    }
    let png = canvas.to_png();
    let image = decode_png(&png);
    assert_eq!((image.width, image.height), (300, 100));
    assert_eq!(image.pixels, canvas.pixels());
    let chunks = png_chunks(&png);
    let zlib = &chunks[1].1;
    assert_eq!(zlib[2] & 1, 0, "the first of several blocks is not final");
    // One-pixel and one-row extremes.
    for (w, h) in [(1, 1), (1, 40), (4096, 1)] {
        let mut small = Canvas::new(w, h).unwrap();
        small.set_pixel(0, 0, 0xFF8001);
        let decoded = decode_png(&small.to_png());
        assert_eq!(decoded.pixels, small.pixels(), "{w}x{h}");
    }
}

#[test]
fn screenshot_saves_the_active_buffer_and_visible_saves_the_front_buffer() {
    let dir = TempDir::new("shot-double");
    let program = format!(
        "SCREEN 6, 4, \"T\", 2\nGCLS 1\nPSET 0, 0, 4\nFLIP\nPSET 5, 3, 2\n\
         SCREENSHOT \"{active}\"\nSCREENSHOT \"{front}\", VISIBLE\n\
         SCREENSHOT \"{lower}\", visible\n",
        active = dir.text("active.ppm"),
        front = dir.text("front.bmp"),
        lower = dir.text("lower.png"),
    );
    let r = ok(&program);
    // Taking screenshots reads the framebuffers; it presents nothing.
    assert_eq!(r.backend.frames(), 1);
    let active = decode_ppm(&std::fs::read(dir.file("active.ppm")).unwrap());
    let front = decode_bmp(&std::fs::read(dir.file("front.bmp")).unwrap());
    // Active = the hidden back buffer (black, one green pixel).
    for y in 0..4 {
        for x in 0..6 {
            assert_eq!(active.at(x, y), if (x, y) == (5, 3) { GREEN } else { 0 });
            let want = match (x, y) {
                (0, 0) => RED,
                _ => BASE,
            };
            assert_eq!(front.at(x, y), want, "front ({x}, {y})");
        }
    }
    assert_eq!(decode_file(&dir.file("lower.png")), front);
    // The front screenshot is exactly the frame the display received.
    let shown = r.backend.front_frame().unwrap();
    assert_eq!(front.pixels, shown.pixels());
}

#[test]
fn buffer_selection_changes_what_screenshot_captures() {
    let dir = TempDir::new("shot-buffer");
    let a = dir.text("one.ppm");
    let b = dir.text("two.ppm");
    ok(&format!(
        "SCREEN 4, 4, \"T\", 2\nBUFFER 1\nPSET 0, 0, 4\nBUFFER 2\nPSET 1, 1, 2\n\
         BUFFER 1\nSCREENSHOT \"{a}\"\nBUFFER 2\nSCREENSHOT \"{b}\"\nSCREENSHOT \"{a}\", VISIBLE"
    ));
    // `a` was overwritten by the visible (buffer 1) capture; `b` is buffer 2.
    let one = decode_file(&dir.file("one.ppm"));
    let two = decode_file(&dir.file("two.ppm"));
    assert_eq!(one.at(0, 0), RED);
    assert_eq!(one.at(1, 1), 0);
    assert_eq!(two.at(0, 0), 0);
    assert_eq!(two.at(1, 1), GREEN);
}

#[test]
fn single_buffer_visible_is_the_same_canvas() {
    let dir = TempDir::new("shot-single");
    ok(&format!(
        "{SCENE}SCREENSHOT \"{}\"\nSCREENSHOT \"{}\", VISIBLE",
        dir.text("a.png"),
        dir.text("b.png")
    ));
    assert_scene(&decode_file(&dir.file("a.png")));
    assert_eq!(
        std::fs::read(dir.file("a.png")).unwrap(),
        std::fs::read(dir.file("b.png")).unwrap()
    );
}

#[test]
fn screenshot_works_in_graphics_state_without_presenting() {
    let dir = TempDir::new("shot-nopresent");
    let r = ok(&format!(
        "{SCENE}SCREENSHOT \"{}\"\nSCREENSHOT \"{}\"",
        dir.text("a.png"),
        dir.text("b.bmp")
    ));
    // Only the end-of-run present happened.
    assert_eq!(r.backend.frames(), 1);
}

#[test]
fn unknown_or_missing_extensions_are_explicit_errors() {
    let dir = TempDir::new("shot-ext");
    for name in ["a.gif", "a", "a.png.txt", "png", "a."] {
        let path = dir.text(name);
        let r = run(&format!("SCREEN 4, 4\nSCREENSHOT \"{path}\""));
        assert!(
            r.error().contains("SCREENSHOT") && r.error().contains(".ppm, .bmp or .png"),
            "{name}: {}",
            r.error()
        );
        assert!(!dir.file(name).exists(), "{name} must not be created");
    }
    let r = run("SCREEN 4, 4\nSCREENSHOT \"\"");
    assert!(r.error().contains(".ppm, .bmp or .png"), "{}", r.error());
}

#[test]
fn missing_directory_is_a_catchable_runtime_error() {
    let dir = TempDir::new("shot-missing");
    let path = dir.text("nope/deeper/a.png");
    let r = ok(&format!(
        "{SCENE}TRY\nSCREENSHOT \"{path}\"\nPRINT \"not reached\"\nCATCH\nPRINT \"caught\"\n\
         PRINT ERR$()\nEND TRY\nPRINT \"after\""
    ));
    let lines: Vec<&str> = r.output.lines().collect();
    assert_eq!(lines[0], "caught");
    assert!(
        lines[1].starts_with("SCREENSHOT: cannot write"),
        "{}",
        lines[1]
    );
    assert_eq!(lines[2], "after");
    assert_eq!(lines.len(), 3);
    // Uncaught, it is an ordinary runtime error.
    let r = run(&format!("{SCENE}SCREENSHOT \"{path}\""));
    assert!(r.error().contains("cannot write"), "{}", r.error());
    assert!(!dir.file("nope").exists());
}

#[test]
fn writing_to_a_directory_path_is_an_error() {
    let dir = TempDir::new("shot-isdir");
    std::fs::create_dir(dir.file("folder.png")).unwrap();
    let r = run(&format!("{SCENE}SCREENSHOT \"{}\"", dir.text("folder.png")));
    assert!(r.error().contains("cannot write"), "{}", r.error());
}

#[test]
fn an_existing_file_is_overwritten_completely() {
    let dir = TempDir::new("shot-overwrite");
    let path = dir.text("o.png");
    std::fs::write(dir.file("o.png"), vec![0xEE; 100_000]).unwrap();
    ok(&format!("{SCENE}SCREENSHOT \"{path}\""));
    assert_scene(&decode_file(&dir.file("o.png")));
    // A second, smaller screenshot replaces the first without leftovers.
    ok(&format!(
        "SCREEN 2, 2\nGCLS 4\nSCREENSHOT \"{path}\"\nPSET 0, 0, 2\nSCREENSHOT \"{path}\""
    ));
    let image = decode_file(&dir.file("o.png"));
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.pixels, [GREEN, RED, RED, RED]);
    let leftovers = std::fs::read_dir(&dir.0).unwrap().count();
    assert_eq!(leftovers, 1, "only the screenshot remains in the directory");
}

#[test]
fn a_file_open_through_open_cannot_be_overwritten() {
    let dir = TempDir::new("shot-open");
    let path = dir.text("busy.png");
    let r = run(&format!(
        "{SCENE}OPEN \"{path}\" FOR OUTPUT AS #1\nSCREENSHOT \"{path}\""
    ));
    assert!(r.error().contains("open as file #1"), "{}", r.error());
    // Once closed it can be written.
    ok(&format!(
        "{SCENE}OPEN \"{path}\" FOR OUTPUT AS #1\nCLOSE #1\nSCREENSHOT \"{path}\""
    ));
    assert_scene(&decode_file(&dir.file("busy.png")));
}

#[test]
fn screenshot_without_a_screen_is_an_explicit_error() {
    let dir = TempDir::new("shot-noscreen");
    for statement in [
        format!("SCREENSHOT \"{}\"", dir.text("a.png")),
        format!("SCREENSHOT \"{}\", VISIBLE", dir.text("a.png")),
        "SCREENSHOT \"bad.gif\"".to_string(),
        "SCREENSHOT 5".to_string(),
    ] {
        let r = run(&statement);
        assert!(
            r.error().contains("no graphics screen"),
            "{statement}: {}",
            r.error()
        );
    }
    let r = run(&format!(
        "SCREEN 4, 4\nSCREEN 0\nSCREENSHOT \"{}\"",
        dir.text("a.png")
    ));
    assert!(r.error().contains("no graphics screen"));
    assert!(!dir.file("a.png").exists());
}

#[test]
fn screenshot_arguments_are_checked() {
    assert!(
        run("SCREEN 4, 4\nSCREENSHOT 5")
            .error()
            .contains("file name must be a string")
    );
    let error = |source: &str| match compile(source) {
        Err(error) => error.message,
        Ok(_) => panic!("expected a compile error for {source}"),
    };
    assert!(error("SCREENSHOT \"a.png\", FULL").contains("flag must be VISIBLE"));
    assert!(error("SCREENSHOT \"a.png\", 1").contains("flag must be VISIBLE"));
    assert!(compile("SCREENSHOT").is_err());
    assert!(compile("SCREENSHOT \"a.png\", VISIBLE, VISIBLE").is_err());
    // The path may be any string expression.
    let dir = TempDir::new("shot-expr");
    ok(&format!(
        "SCREEN 2, 2\nN$ = \"{}\"\nSCREENSHOT N$ + \"/x\" + \".png\"",
        dir.text("").trim_end_matches(['/', '\\'])
    ));
    assert!(dir.file("x.png").exists());
}

#[test]
fn screenshot_is_usable_as_a_variable_name_in_assignments() {
    let r = ok("SCREENSHOT = 3\nPRINT SCREENSHOT");
    assert_eq!(r.output, "3\n");
    let r = ok("BUFFER = 1\nPRINT BUFFER");
    assert_eq!(r.output, "1\n");
}
