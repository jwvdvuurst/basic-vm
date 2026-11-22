use std::path::Path;
use std::process::{Command, Stdio};

mod common;

#[test]
fn runs_collection_example_and_dumps_bytecode() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("collections.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Score: 10\nScore: 25\nScore: 30\nName: Ada\nName: Linus\n\
         Pears: 4\nCount: 3\nCount: 4\n\
         Indexed score: 10\nIndexed score: 25\nIndexed score: 30\n\
         Countdown: 3\nCountdown: 2\nCountdown: 1\n"
    );
    let dump = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--dump-bytecode")
        .arg(&path)
        .output()
        .unwrap();
    assert!(dump.status.success());
    let dump = String::from_utf8(dump.stdout).unwrap();
    assert!(dump.contains("JumpIfFalse"));
    assert!(dump.contains("Jump("));
    assert!(dump.contains("EachStart"));
    assert!(dump.contains("ForStart"));
    assert!(!dump.contains("Score: 10\n"));
}

#[test]
fn runs_function_example() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("functions.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Square: 49\nFactorial: 720\nHello, Ada!\n\
         Hello from a function without parameters!\nValue: 1\nValue: 2\nValue: 3\n"
    );
}

#[test]
fn runs_control_flow_example() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("control.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "I = one\nI = three to five: 3\nI = three to five: 5\nI = seven or more\n\
         N = 3\nshort-circuit ok\nlogic ok\nFirst multiple of 7 above 20: 21\n"
    );
}

#[test]
fn reads_console_input_and_reports_usage_errors() {
    use std::io::Write;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("input.bas");
    let mut child = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"Ada\n21\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "What is your name?\nHello, Ada!\nEnter a number:\nTwice your number is 42\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("usage:"));
}

const STDLIB_EXAMPLE_OUTPUT: &str = "Math: 3 4 9 2.57 -3 -2 1024\n\
    HELLO, WORLD|hello, world|Hello|World|World\n\
    8 HeLLo, WorLd\n\
    apple-fig-pear\n\
    pear fig apple kiwi / popped kiwi / left 3\n\
    x+y 1 0 1\n\
    same seed, same number\n\
    13.5 7!\n";

#[test]
fn runs_stdlib_example_and_passes_program_arguments() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("stdlib.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{STDLIB_EXAMPLE_OUTPUT}Arguments: 0\n")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .args(["one", "--dump-bytecode"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{STDLIB_EXAMPLE_OUTPUT}Arguments: 2\n")
    );
}

fn temp_program(name: &str, source: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("basic-vm-{}-{name}.bas", std::process::id()));
    std::fs::write(&path, source).unwrap();
    path
}

#[test]
fn exposes_arguments_and_environment_to_programs() {
    let path = temp_program(
        "args-env",
        "PRINT ARGC(); \"|\"; ARG$(1); \"|\"; ARG$(2); \"|\"; ENV$(\"BASIC_VM_TEST_VALUE\"); \"|\"; LEN(ENV$(\"BASIC_VM_TEST_UNSET\"))\n\
         PRINT ARG$(3)\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .args(["h\u{e9}llo", "two words"])
        .env("BASIC_VM_TEST_VALUE", "v\u{e4}lue \u{1f600}")
        .env_remove("BASIC_VM_TEST_UNSET")
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "2|h\u{e9}llo|two words|v\u{e4}lue \u{1f600}|0\n"
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("line 2") && stderr.contains("ARG$: index 3 out of range (ARGC is 2)"),
        "{stderr}"
    );
}

#[test]
fn dump_bytecode_rejects_program_arguments_and_shows_builtins() {
    let path = temp_program("dump", "PRINT ABS(-1)\n");
    let rejected = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--dump-bytecode")
        .arg(&path)
        .arg("extra")
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("usage:")
    );
    let dump = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--dump-bytecode")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(String::from_utf8(dump.stdout).unwrap().contains("Builtin"));
}

#[test]
fn runs_file_io_example_in_a_scratch_directory_and_cleans_up() {
    let directory = std::env::temp_dir().join(format!("basic-vm-fileio-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("fileio.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .current_dir(&directory)
        .output()
        .unwrap();
    let leftovers = std::fs::read_dir(&directory).unwrap().count();
    std::fs::remove_dir(&directory).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "read: alpha\nread: beta,42\nread: gamma\n3 bytes: 0 255 65\nsize: 20\n\
         caught: 1\ncleanup done\nscratch file exists: 0\n"
    );
    assert_eq!(leftovers, 0);
}

#[test]
fn uncaught_errors_run_finally_report_the_function_and_exit_unsuccessfully() {
    let path = temp_program(
        "uncaught",
        "FUNCTION F\nF = 1 / 0\nFEND\nTRY\nPRINT \"start\"\nPRINT F()\nFINALLY\n\
         PRINT \"finally\"\nEND TRY\nPRINT \"unreachable\"\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "start\nfinally\n"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("line 2: division by zero (in function F)"),
        "{stderr}"
    );
}

#[test]
fn files_are_flushed_when_the_cli_program_fails() {
    let data = std::env::temp_dir().join(format!("basic-vm-cli-flush-{}.txt", std::process::id()));
    let path = temp_program(
        "flush-on-error",
        &format!(
            "OPEN \"{}\" FOR OUTPUT AS #1\nPRINT #1, \"kept\"\nTHROW \"stop\"\n",
            data.display()
        ),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(&data).unwrap(), "kept\n");
    std::fs::remove_file(&data).unwrap();
}

#[test]
fn runs_modules_example_from_another_working_directory() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("modules.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .current_dir(std::env::temp_dir())
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "mathutil loaded\nmain starts\nSquare(7) = 49\nScaled(2) = 60\n\
         Scaled(2, factor := 5) = 100\nSCALE = 10\nafter swap: 2 1\npoint: 3,4\n\
         origin: 0,0\ncalls so far: 3\ncalls (global): 3\n"
    );
}

#[test]
fn graphics_example_writes_screenshots_headless() {
    let dir = std::env::temp_dir();
    let ppm = dir.join(format!("basic-vm-{}.ppm", std::process::id()));
    let bmp = dir.join(format!("basic-vm-{}.bmp", std::process::id()));
    for file in [&ppm, &bmp] {
        let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
            .args(["--headless-graphics", "--screenshot"])
            .arg(file)
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/graphics.bas"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let data = std::fs::read(&ppm).unwrap();
    assert!(data.starts_with(b"P6\n160 120\n255\n"));
    let pixels = &data[data.len() - 160 * 120 * 3..];
    // (80, 80) was set to RGB(255, 0, 255).
    let offset = (80 * 160 + 80) * 3;
    assert_eq!(&pixels[offset..offset + 3], &[255, 0, 255]);
    // (1, 118) is inside the green ground strip's lower part.
    assert_eq!(&pixels[0..3], &[0, 0, 64]);
    let bitmap = std::fs::read(&bmp).unwrap();
    assert!(bitmap.starts_with(b"BM"));
    std::fs::remove_file(ppm).ok();
    std::fs::remove_file(bmp).ok();
}

#[test]
fn bad_screenshot_extension_fails() {
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/graphics.bas");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--screenshot", "out.gif"])
        .arg(&program)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".ppm, .bmp or .png"), "{stderr}");
    assert!(!Path::new("out.gif").exists());
}

#[test]
fn cli_screenshot_accepts_png() {
    let dir = common::TempDir::new("cli-png");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--headless-graphics", "--screenshot"])
        .arg(dir.file("scene.png"))
        .arg(common::examples("graphics.bas"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let png = common::decode_file(&dir.file("scene.png"));
    assert_eq!((png.width, png.height), (160, 120));
    assert_eq!(png.at(80, 80), 0xFF00FF);
    assert_eq!(png.at(0, 0), 0x000040);
    // Same pixels as the PPM written for the same run.
    let ppm_output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--screenshot")
        .arg(dir.file("scene.PPM"))
        .arg(common::examples("graphics.bas"))
        .output()
        .unwrap();
    assert!(ppm_output.status.success());
    assert_eq!(png, common::decode_file(&dir.file("scene.PPM")));
}

#[test]
fn animation_example_ends_on_the_last_flipped_frame() {
    let dir = common::TempDir::new("cli-animation");
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--headless-graphics", "--screenshot"])
        .arg(dir.file("last.png"))
        .arg(common::examples("animation.bas"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Animation done\n"
    );
    let last = common::decode_file(&dir.file("last.png"));
    assert_eq!((last.width, last.height), (160, 120));
    // Frame 60: the ball (frame index 59) is at (128, 99), and nothing of the
    // earlier positions remains because every frame starts with GCLS.
    assert_eq!(last.at(128, 99), 0xFFFF55);
    assert_eq!(last.at(10, 40), 0x0000AA);
    assert_eq!(last.at(69, 69), 0x0000AA);
    assert_eq!(last.at(80, 115), 0x00AA00);
    // The frame counter text is drawn near the top-left corner.
    let text_pixels = (0..8)
        .flat_map(|y| (4..4 + 8 * 8).map(move |x| (x, y + 4)))
        .filter(|(x, y)| last.at(*x, *y) == 0xFFFFFF)
        .count();
    assert!(text_pixels > 20, "{text_pixels}");
}

fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("basic-vm-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn screenshots_have_correct_headers_and_pixels() {
    let dir = test_dir("shots");
    let program = dir.join("p.bas");
    std::fs::write(
        &program,
        "SCREEN 5, 3\nGCLS 1\nPSET 2, 1, RGB(10, 20, 30)\n",
    )
    .unwrap();
    for name in ["s.ppm", "s.bmp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
            .arg("--screenshot")
            .arg(dir.join(name))
            .arg(&program)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
    }
    let ppm = std::fs::read(dir.join("s.ppm")).unwrap();
    assert!(ppm.starts_with(b"P6\n5 3\n255\n"));
    let pixels = &ppm[ppm.len() - 5 * 3 * 3..];
    assert_eq!(&pixels[0..3], &[0, 0, 0xAA]);
    let offset = (5 + 2) * 3;
    assert_eq!(&pixels[offset..offset + 3], &[10, 20, 30]);
    let bmp = std::fs::read(dir.join("s.bmp")).unwrap();
    assert_eq!(&bmp[0..2], b"BM");
    assert_eq!(i32::from_le_bytes(bmp[18..22].try_into().unwrap()), 5);
    assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), 3);
    assert_eq!(u16::from_le_bytes(bmp[28..30].try_into().unwrap()), 24);
    // Rows are stored bottom-up, 16 bytes each (5 * 3 padded to 4), BGR order.
    let offset = 54 + 16 + 2 * 3;
    assert_eq!(&bmp[offset..offset + 3], &[30, 20, 10]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn runtime_error_is_reported_before_a_screenshot_failure() {
    let dir = test_dir("mask");
    let program = dir.join("p.bas");
    std::fs::write(&program, "PRINT 1 / 0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .arg("--screenshot")
        .arg(dir.join("x.ppm"))
        .arg(&program)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let runtime = stderr.find("division by zero").expect(&stderr);
    let shot = stderr.find("--screenshot").expect(&stderr);
    assert!(runtime < shot, "{stderr}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unknown_option_is_reported() {
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--bogus", "x.bas"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option --bogus"));
}
