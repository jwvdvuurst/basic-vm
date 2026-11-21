//! CLI runs of the interactive example with the auto-close validation aids.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn paint() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("paint.bas")
}

#[test]
fn headless_paint_ends_after_the_frame_limit() {
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--headless-graphics", "--max-frames", "3"])
        .arg(paint())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "Goodbye\n");
}

#[test]
fn rejects_a_bad_auto_close_value() {
    let output = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--auto-close-ms", "soon"])
        .arg(paint())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--auto-close-ms"));
}

/// Opens a real window and lets `--auto-close-ms` end the program. Skips
/// (passes) when no window can be created, e.g. on a machine without a
/// display or in a build without the `window` feature.
#[test]
fn real_window_opens_and_closes_itself() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_basic-vm"))
        .args(["--auto-close-ms", "500"])
        .arg(paint())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("window run (pid {pid}) did not end within 30 s and was killed");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !status.success() {
        if stderr.contains("cannot open the graphics screen")
            || stderr.contains("no window backend")
        {
            eprintln!("skipped: no window available: {stderr}");
            return;
        }
        panic!("window run failed: {status}: {stderr}");
    }
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Goodbye\n");
}
