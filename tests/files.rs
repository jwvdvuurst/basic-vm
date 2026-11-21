//! File I/O: text and binary access, error cases and deterministic cleanup.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use basic_vm::{Error, Vm, compile};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A test-owned path in the temp directory that is removed on drop.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!(
            "basic-vm-files-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn with_contents(name: &str, contents: &[u8]) -> Self {
        let file = Self::new(name);
        fs::write(&file.0, contents).unwrap();
        file
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for TempFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Runs a program and keeps the VM alive so tests can inspect the file system
/// while it still exists.
fn run(source: &str) -> (String, Result<(), Error>, Vm) {
    let mut vm = Vm::new(compile(source).unwrap()).unwrap();
    let mut output = Vec::new();
    let result = vm.run(&mut Cursor::new(""), &mut output);
    (String::from_utf8(output).unwrap(), result, vm)
}

fn run_ok(source: &str) -> String {
    let (output, result, _vm) = run(source);
    result.unwrap_or_else(|error| panic!("{error}\n{source}"));
    output
}

fn run_error(source: &str) -> Error {
    run(source).1.unwrap_err()
}

#[test]
fn text_round_trip_append_and_eof() {
    let file = TempFile::new("text.txt");
    let output = run_ok(&format!(
        "P$ = \"{file}\"\n\
         OPEN P$ FOR OUTPUT AS #1\n\
         PRINT #1, \"alpha\"\n\
         PRINT #1, \"n=\"; 42, \"tab\"\n\
         PRINT #1, \"no newline\";\n\
         PRINT #1, \" joined\"\n\
         PRINT #1\n\
         CLOSE #1\n\
         OPEN P$ FOR APPEND AS #1\n\
         PRINT #1, \"appended\"\n\
         CLOSE #1\n\
         OPEN P$ FOR INPUT AS #1\n\
         N = 0\n\
         DO UNTIL EOF(1)\n\
         LINE INPUT #1, L$\n\
         N = N + 1\n\
         PRINT N; \":\"; L$\n\
         LOOP\n\
         PRINT EOF(1)\n\
         CLOSE #1\n"
    ));
    assert_eq!(
        output,
        "1:alpha\n2:n=42\ttab\n3:no newline joined\n4:\n5:appended\n1\n"
    );
    assert_eq!(
        fs::read_to_string(file.path()).unwrap(),
        "alpha\nn=42\ttab\nno newline joined\n\nappended\n"
    );
}

#[test]
fn output_mode_truncates_and_unicode_survives() {
    let file = TempFile::with_contents("trunc.txt", b"old contents that are long\n");
    let output = run_ok(&format!(
        "OPEN \"{file}\" FOR OUTPUT AS #3\nPRINT #3, \"h\u{e9}llo \u{1f600}\"\nCLOSE #3\n\
         OPEN \"{file}\" FOR INPUT AS #3\nLINE INPUT #3, A$\nPRINT A$; \" \"; LEN(A$)\nCLOSE #3\n"
    ));
    assert_eq!(output, "h\u{e9}llo \u{1f600} 7\n");
}

#[test]
fn line_input_handles_crlf_and_a_final_line_without_newline() {
    let file = TempFile::with_contents("crlf.txt", b"one\r\ntwo\r\nthree");
    let output = run_ok(&format!(
        "OPEN \"{file}\" FOR INPUT AS #1\n\
         WHILE NOT EOF(1)\n\
         LINE INPUT #1, L$\n\
         PRINT \"[\"; L$; \"]\"; LEN(L$)\n\
         WEND\n\
         CLOSE #1\n"
    ));
    assert_eq!(output, "[one]3\n[two]3\n[three]5\n");

    let numbers = TempFile::with_contents("numbers.txt", b"12\r\n3.5\n");
    let source = format!(
        "OPEN \"{numbers}\" FOR INPUT AS #1\nINPUT #1, A\nINPUT #1, B\nPRINT A + B; \" \"; EOF(1)\n"
    );
    assert_eq!(run_ok(&source), "15.5 1\n");
    let error = run_error(&format!("{source}INPUT #1, C$\n"));
    assert!(
        error.message.contains("reading past the end of file #1"),
        "{error}"
    );
    assert_eq!(error.line, 5);
}

#[test]
fn binary_round_trip_covers_every_byte_value() {
    let file = TempFile::new("bytes.bin");
    let output = run_ok(&format!(
        "OPEN \"{file}\" FOR BINARY AS #1\n\
         B = LIST()\n\
         FOR I = 0 TO 255\n\
         APPEND B, I\n\
         NEXT\n\
         PUT #1, B\n\
         PRINT LOF(1); \" \"; LOC(1)\n\
         SEEK #1, 0\n\
         PRINT EOF(1)\n\
         R = GET(1, 300)\n\
         PRINT LEN(R); \" \"; R[1]; \" \"; R[256]; \" \"; EOF(1); \" \"; LOC(1)\n\
         PRINT LEN(GET(1, 10))\n\
         SEEK #1, 250\n\
         R3 = GET(1, 10)\n\
         PRINT LEN(R3); \" \"; R3[1]\n\
         SEEK #1, 2\n\
         PUT #1, ARRAY(7, 8)\n\
         PRINT \"text:\"; LOF(1)\n\
         PRINT #1, \"\u{e9}\"\n\
         CLOSE #1\n"
    ));
    assert_eq!(output, "256 256\n0\n256 0 255 1 256\n0\n6 250\ntext:256\n");
    let bytes = fs::read(file.path()).unwrap();
    let mut expected: Vec<u8> = (0..=255).collect();
    expected[2] = 7;
    expected[3] = 8;
    // PRINT # appended UTF-8 text plus a newline at the file position (4).
    expected.splice(4..4 + "\u{e9}\n".len(), "\u{e9}\n".bytes());
    assert_eq!(bytes, expected);
}

#[test]
fn binary_mode_keeps_existing_contents() {
    let file = TempFile::with_contents("keep.bin", b"hello");
    let output = run_ok(&format!(
        "OPEN \"{file}\" FOR BINARY AS #1\nPRINT LOF(1); \" \"; GET(1, 2)[2]\nCLOSE #1\n"
    ));
    assert_eq!(output, "5 101\n");
    assert_eq!(fs::read(file.path()).unwrap(), b"hello");
}

#[test]
fn file_errors_are_explicit() {
    let existing = TempFile::with_contents("existing.txt", b"x\ny\n");
    let missing = TempFile::new("missing.txt");
    let out = TempFile::new("out.bin");
    let invalid = TempFile::with_contents("invalid.txt", b"\xff\xfe\n");
    for (source, message) in [
        (
            format!("OPEN \"{missing}\" FOR INPUT AS #1"),
            "file not found",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nOPEN \"{existing}\" FOR INPUT AS #1"),
            "file #1 is already open",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nPRINT #1, \"x\""),
            "file #1 is open for INPUT",
        ),
        (
            format!("OPEN \"{out}\" FOR OUTPUT AS #1\nLINE INPUT #1, A$"),
            "file #1 is not open for INPUT",
        ),
        ("PRINT #2, \"x\"".into(), "file #2 is not open"),
        ("A = EOF(1)".into(), "file #1 is not open"),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nCLOSE #1\nCLOSE #1"),
            "file #1 is not open",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nCLOSE #1\nINPUT #1, A$"),
            "file #1 is not open",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nPUT #1, LIST(1)"),
            "file #1 is not open for BINARY",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nA = GET(1, 1)"),
            "file #1 is not open for BINARY",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nSEEK #1, 0"),
            "file #1 is not open for BINARY",
        ),
        (
            format!(
                "OPEN \"{existing}\" FOR INPUT AS #1\nLINE INPUT #1, A$\nLINE INPUT #1, A$\n\
                 LINE INPUT #1, A$"
            ),
            "reading past the end of file #1",
        ),
        (
            format!("OPEN \"{invalid}\" FOR INPUT AS #1\nLINE INPUT #1, A$"),
            "file #1 contains invalid UTF-8",
        ),
        (
            "PRINT #0, \"x\"".into(),
            "file number must be a whole number from 1 to 255",
        ),
        (
            "PRINT #256, \"x\"".into(),
            "file number must be a whole number from 1 to 255",
        ),
        (
            "CLOSE #1.5".into(),
            "file number must be a whole number from 1 to 255",
        ),
        (
            format!("OPEN \"{existing}\" FOR INPUT AS #1\nKILL \"{existing}\""),
            "is open as file #1",
        ),
        (format!("KILL \"{missing}\""), "file not found"),
        (
            format!("OPEN \"{out}\" FOR OUTPUT AS #1\nA = EOF(1)"),
            "not open for INPUT or BINARY",
        ),
        (
            format!("OPEN \"{out}\" FOR BINARY AS #1\nPUT #1, LIST(1, 256)"),
            "element 2 is not a whole number from 0 to 255",
        ),
        (
            format!("OPEN \"{out}\" FOR BINARY AS #1\nPUT #1, LIST(\"a\")"),
            "element 1 is not a whole number",
        ),
        (
            format!("OPEN \"{out}\" FOR BINARY AS #1\nA = GET(1, 65537)"),
            "must not exceed 65536",
        ),
        (
            format!("OPEN \"{out}\" FOR BINARY AS #1\nSEEK #1, -1"),
            "position must be a non-negative whole number",
        ),
    ] {
        let error = run_error(&source);
        assert!(error.message.contains(message), "{source}: {error}");
    }
    assert!(!missing.path().exists());
}

#[test]
fn file_statements_report_compile_errors() {
    for (source, message) in [
        ("OPEN \"a\" FOR READ AS #1", "OPEN mode must be"),
        ("OPEN \"a\" FOR INPUT AS 1", "expected Hash"),
        ("OPEN \"a\" FOR INPUT #1", "expected Word(\"AS\")"),
        ("LINE INPUT #1, A", "requires a string variable"),
        ("PRINT #1 5", "expected ','"),
        ("CLOSE 1", "expected Hash"),
    ] {
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, 1, "{source}");
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn all_file_numbers_can_be_used_and_freefile_reports_exhaustion() {
    // Each number needs its own file: opening one path twice is an error.
    let files: Vec<TempFile> = (1..=255)
        .map(|n| TempFile::with_contents(&format!("many{n}.txt"), b"x\n"))
        .collect();
    let opens: String = files
        .iter()
        .enumerate()
        .map(|(index, file)| format!("OPEN \"{file}\" FOR INPUT AS #{}\n", index + 1))
        .collect();
    let first = &files[0];
    let output = run_ok(&format!(
        "PRINT FREEFILE()\n\
         OPEN \"{first}\" FOR INPUT AS #1\n\
         PRINT FREEFILE(); ISOPEN(1); ISOPEN(2)\n\
         CLOSE #1\n\
         {opens}\
         CLOSE #7\n\
         PRINT FREEFILE()\n\
         CLOSE\n\
         PRINT FREEFILE(); ISOPEN(255)\n"
    ));
    assert_eq!(output, "1\n210\n7\n10\n");
    let error = run_error(&format!("{opens}A = FREEFILE()"));
    assert!(error.message.contains("all file numbers are in use"));
}

#[test]
fn helpers_check_files_and_delete_them() {
    let file = TempFile::with_contents("helper.txt", b"x");
    let output = run_ok(&format!(
        "PRINT FILEEXISTS(\"{file}\")\nKILL \"{file}\"\nPRINT FILEEXISTS(\"{file}\")\n"
    ));
    assert_eq!(output, "1\n0\n");
    assert!(!file.path().exists());
}

#[test]
fn handles_are_flushed_and_released_when_the_program_ends() {
    let text = TempFile::new("exit.txt");
    let binary = TempFile::new("exit.bin");
    let (_, result, _vm) = run(&format!(
        "OPEN \"{text}\" FOR OUTPUT AS #1\nPRINT #1, \"unflushed\"\n\
         OPEN \"{binary}\" FOR BINARY AS #2\nPUT #2, LIST(1, 2)\nEND\n"
    ));
    result.unwrap();
    assert_released(&text, b"unflushed\n");
    assert_released(&binary, &[1, 2]);
}

#[test]
fn handles_are_flushed_and_released_when_the_program_fails() {
    let text = TempFile::new("error.txt");
    let (_, result, _vm) = run(&format!(
        "OPEN \"{text}\" FOR OUTPUT AS #4\nPRINT #4, \"before the error\"\nA = 1 / 0\n"
    ));
    assert!(result.unwrap_err().message.contains("division by zero"));
    assert_released(&text, b"before the error\n");
}

#[test]
fn handles_are_released_after_an_uncaught_error_inside_a_function_with_a_finally() {
    let text = TempFile::new("nested.txt");
    let (output, result, _vm) = run(&format!(
        "FUNCTION F\n\
         OPEN \"{text}\" FOR OUTPUT AS #1\n\
         PRINT #1, \"data\"\n\
         F = 1 / 0\n\
         FEND\n\
         TRY\n\
         PRINT F()\n\
         FINALLY\n\
         PRINT \"finally; still open: \"; ISOPEN(1)\n\
         END TRY\n"
    ));
    assert_eq!(output, "finally; still open: 1\n");
    assert!(result.is_err());
    assert_released(&text, b"data\n");
}

/// The file has its expected contents and can be deleted and re-created even
/// though the VM that wrote it is still alive.
fn assert_released(file: &TempFile, expected: &[u8]) {
    assert_eq!(fs::read(file.path()).unwrap(), expected);
    fs::remove_file(file.path()).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file.path())
        .unwrap();
}

#[test]
fn close_flushes_so_the_file_can_be_read_back_immediately() {
    let file = TempFile::new("flush.txt");
    let output = run_ok(&format!(
        "OPEN \"{file}\" FOR OUTPUT AS #1\nPRINT #1, \"abc\"\nPRINT LOF(1)\nCLOSE #1\n\
         OPEN \"{file}\" FOR INPUT AS #2\nPRINT LOF(2)\nLINE INPUT #2, A$\nPRINT A$\nCLOSE\n"
    ));
    assert_eq!(output, "4\n4\nabc\n");
}

#[test]
fn opening_an_already_open_path_is_an_error_and_does_not_truncate() {
    let file = TempFile::with_contents("dup.txt", b"keep me\n");
    let error = run_error(&format!(
        "OPEN \"{file}\" FOR INPUT AS #1\nOPEN \"{file}\" FOR OUTPUT AS #2"
    ));
    assert!(
        error.message.contains("is already open as file #1"),
        "{error}"
    );
    // Paths are shown plainly: no Debug-escaped (doubled) backslashes.
    assert!(
        error.message.contains(&format!("\"{file}\"")),
        "{}",
        error.message
    );
    assert_eq!(fs::read(file.path()).unwrap(), b"keep me\n");
}

#[test]
fn close_with_several_numbers_is_all_or_nothing() {
    let a = TempFile::with_contents("a.txt", b"a");
    let b = TempFile::with_contents("b.txt", b"b");
    let source = format!(
        "OPEN \"{a}\" FOR INPUT AS #1\nOPEN \"{b}\" FOR INPUT AS #2\nTRY\nCLOSE #1, #3\n\
         CATCH E$\nPRINT E$\nEND TRY\nPRINT ISOPEN(1); ISOPEN(2)\n\
         TRY\nCLOSE #2, #2\nCATCH E$\nPRINT E$\nEND TRY\nPRINT ISOPEN(2)"
    );
    assert_eq!(
        run_ok(&source),
        "CLOSE: file #3 is not open\n11\nCLOSE: file #2 is listed twice\n1\n"
    );
}

#[test]
fn seek_to_an_unrepresentable_offset_is_a_friendly_error() {
    let file = TempFile::new("seek.bin");
    let error = run_error(&format!("OPEN \"{file}\" FOR BINARY AS #1\nSEEK #1, 1E19"));
    assert!(
        error.message.contains("SEEK") && error.message.contains("too large"),
        "{error}"
    );
}

#[cfg(windows)]
#[test]
fn nul_device_can_be_opened_for_output() {
    assert_eq!(
        run_ok("OPEN \"NUL\" FOR OUTPUT AS #1\nPRINT #1, \"x\"\nCLOSE #1\nPRINT \"ok\""),
        "ok\n"
    );
}

#[test]
fn binary_falls_back_to_read_only_when_the_file_is_read_only() {
    let file = TempFile::with_contents("ro.bin", &[7, 8, 9]);
    let mut permissions = fs::metadata(file.path()).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(file.path(), permissions).unwrap();
    let source = format!(
        "OPEN \"{file}\" FOR BINARY AS #1\nPRINT GET(1, 3)[2]\nTRY\nPUT #1, LIST(1)\n\
         CATCH E$\nPRINT \"put failed\"\nEND TRY\nCLOSE #1"
    );
    let output = std::panic::catch_unwind(|| run_ok(&source));
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let mut permissions = fs::metadata(file.path()).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(file.path(), permissions).unwrap();
    }
    assert_eq!(output.unwrap(), "8\nput failed\n");
    assert_eq!(fs::read(file.path()).unwrap(), [7, 8, 9]);
}
