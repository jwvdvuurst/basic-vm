//! The test runner behind `basic-vm --test`: runs the `TEST` blocks of a
//! compiled program, each on its own fresh [`Vm`].

use std::fmt::Write as _;
use std::io;
use std::time::{Duration, Instant};

use crate::{Error, HeadlessBackend, Program, Vm};

/// Most bytes of output kept per test; the rest is discarded (and counted in
/// [`TestResult::output_omitted`]).
pub const MAX_CAPTURED_OUTPUT: usize = 64 * 1024;

/// Which tests [`run_tests`] runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestOptions {
    /// Run only tests whose full name contains this text (case-insensitive).
    pub filter: Option<String>,
    /// Also run the tests declared in imported modules.
    pub include_imports: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    /// The test finished without an uncaught error.
    Passed,
    /// An assertion failed, or the test left a file open.
    Failed,
    /// Any other uncaught runtime error (including `END` or `STOP`).
    Errored,
}

/// The outcome of one test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    /// The full name: `name`, or `Module.name` for a module's test.
    pub name: String,
    pub status: TestStatus,
    /// The failure or error message, exactly as raised; empty for a pass.
    /// (The report escapes control characters when it prints it.)
    pub message: String,
    /// The source file of the failing statement; `None` is the main program.
    pub file: Option<String>,
    /// The line of the failing statement (0 for a pass).
    pub line: usize,
    /// The user function the error was raised in, when not the test itself.
    pub function: Option<String>,
    /// What the test printed: at most [`MAX_CAPTURED_OUTPUT`] bytes, from the
    /// start of its output.
    pub output: String,
    /// Bytes of output beyond the cap that were discarded.
    pub output_omitted: usize,
    pub elapsed: Duration,
}

/// All results of one [`run_tests`] call, in source order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestReport {
    pub results: Vec<TestResult>,
}

impl TestReport {
    pub fn total(&self) -> usize {
        self.results.len()
    }

    fn count(&self, status: TestStatus) -> usize {
        self.results
            .iter()
            .filter(|result| result.status == status)
            .count()
    }

    pub fn passed(&self) -> usize {
        self.count(TestStatus::Passed)
    }

    pub fn failed(&self) -> usize {
        self.count(TestStatus::Failed)
    }

    pub fn errors(&self) -> usize {
        self.count(TestStatus::Errored)
    }

    /// True when at least one test ran and every test passed.
    pub fn success(&self) -> bool {
        !self.results.is_empty() && self.passed() == self.total()
    }

    /// The summary line: `N tests: P passed, F failed, E errors`.
    pub fn summary(&self) -> String {
        format!(
            "{} tests: {} passed, {} failed, {} errors",
            self.total(),
            self.passed(),
            self.failed(),
            self.errors()
        )
    }

    /// The report text: for each test its [`TestResult::render`] block, then
    /// the summary line. `main_name` names the main program in locations
    /// (the program path); when empty, locations read `line N`.
    pub fn render(&self, main_name: &str) -> String {
        let mut text = String::new();
        for result in &self.results {
            text.push_str(&result.render(main_name));
        }
        let _ = writeln!(text, "{}", self.summary());
        text
    }
}

impl TestResult {
    /// The single report line of this test. Control characters in the
    /// message are escaped, so the line is always one line.
    pub fn line_text(&self, main_name: &str) -> String {
        let keyword = match self.status {
            TestStatus::Passed => return format!("PASS {}", self.name),
            TestStatus::Failed => "FAIL",
            TestStatus::Errored => "ERROR",
        };
        let file = self.file.as_deref().unwrap_or(main_name);
        let mut place = if file.is_empty() {
            format!("line {}", self.line)
        } else {
            format!("{file}:{}", self.line)
        };
        if let Some(function) = &self.function {
            let _ = write!(place, ", in function {function}");
        }
        let mut message = String::new();
        escape_into(&self.message, &mut message);
        format!("{keyword} {}: {message} ({place})", self.name)
    }

    /// The report lines of this test, each ending in a newline: its
    /// [`line_text`](Self::line_text) and, when it did not pass, its captured
    /// output indented by four spaces (followed by a truncation note when
    /// output was discarded).
    pub fn render(&self, main_name: &str) -> String {
        let mut text = self.line_text(main_name);
        text.push('\n');
        if self.status != TestStatus::Passed {
            for line in self.output.lines() {
                let _ = writeln!(text, "    {line}");
            }
            if self.output_omitted > 0 {
                let _ = writeln!(
                    text,
                    "    ... output truncated ({} bytes omitted)",
                    self.output_omitted
                );
            }
        }
        text
    }
}

/// Appends `ch` to `out`, writing line breaks, tabs and other control
/// characters as escapes (`\n`, `\r`, `\t`, `\u{1b}`); everything else,
/// including ordinary Unicode text, is copied as is.
pub(crate) fn escape_char_into(ch: char, out: &mut String) {
    match ch {
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        // Unicode line and paragraph separators also break lines.
        ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
            let _ = write!(out, "\\u{{{:x}}}", ch as u32);
        }
        ch => out.push(ch),
    }
}

fn escape_into(text: &str, out: &mut String) {
    for ch in text.chars() {
        escape_char_into(ch, out);
    }
}

/// An output sink that keeps the first `limit` bytes and silently discards
/// (but counts) the rest, so a chatty test is never an error.
struct CappedOutput {
    kept: Vec<u8>,
    limit: usize,
    omitted: usize,
}

impl CappedOutput {
    fn new(limit: usize) -> Self {
        Self {
            kept: Vec::new(),
            limit,
            omitted: 0,
        }
    }

    /// The kept text and the number of discarded bytes (a character cut in
    /// half by the cap counts as discarded).
    fn finish(mut self) -> (String, usize) {
        if self.omitted > 0
            && let Err(error) = std::str::from_utf8(&self.kept)
            && error.error_len().is_none()
        {
            self.omitted += self.kept.len() - error.valid_up_to();
            self.kept.truncate(error.valid_up_to());
        }
        (
            String::from_utf8_lossy(&self.kept).into_owned(),
            self.omitted,
        )
    }
}

impl io::Write for CappedOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let room = self.limit - self.kept.len();
        let taken = room.min(buf.len());
        self.kept.extend_from_slice(&buf[..taken]);
        self.omitted += buf.len() - taken;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Runs the selected tests of `program`, each on a fresh VM.
pub fn run_tests(program: &Program, options: &TestOptions) -> TestReport {
    run_tests_with(program, options, |_| {})
}

/// Like [`run_tests`], but calls `on_result` with each result as soon as its
/// test has finished (this is how the CLI prints progress while the run is
/// still going).
pub fn run_tests_with(
    program: &Program,
    options: &TestOptions,
    mut on_result: impl FnMut(&TestResult),
) -> TestReport {
    let filter = options.filter.as_ref().map(|text| text.to_lowercase());
    let mut report = TestReport::default();
    for (index, case) in program.tests().iter().enumerate() {
        if case.file != 0 && !options.include_imports {
            continue;
        }
        let name = case.full_name();
        if let Some(filter) = &filter
            && !name.to_lowercase().contains(filter)
        {
            continue;
        }
        let result = run_one(program, index, name);
        on_result(&result);
        report.results.push(result);
    }
    report
}

fn run_one(program: &Program, index: usize, name: String) -> TestResult {
    let started = Instant::now();
    let case = &program.tests()[index];
    let hidden = program.functions[case.function].qualified.clone();
    let location = |error: &Error| match &error.file {
        Some(file) => (Some(file.clone()), error.line),
        None => (None, error.line),
    };
    let mut output = CappedOutput::new(MAX_CAPTURED_OUTPUT);
    let outcome = match Vm::new(program.clone()) {
        Err(error) => Err(error),
        Ok(mut vm) => {
            // Graphics never open a real window while testing, and SLEEP
            // never waits.
            vm.set_graphics_backend(Box::new(HeadlessBackend::new()));
            vm.skip_sleeps();
            let mut outcome = vm.run_test_case(index, &mut output);
            if outcome.is_ok() {
                let open = vm.open_file_numbers();
                if !open.is_empty() {
                    let numbers = open
                        .iter()
                        .map(|number| format!("#{number}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut error = Error::new(
                        case.line,
                        format!(
                            "resource leak: file {numbers} still open at the end of the test (CLOSE it)"
                        ),
                    );
                    error.assertion = true;
                    error.file = Some(program.file_name(case.file).to_string())
                        .filter(|name| !name.is_empty());
                    outcome = Err(error);
                }
            }
            if let Err(message) = vm.finish_test(&mut output)
                && outcome.is_ok()
            {
                let mut error = Error::new(case.line, message);
                error.file =
                    Some(program.file_name(case.file).to_string()).filter(|name| !name.is_empty());
                outcome = Err(error);
            }
            outcome
        }
    };
    let (output, output_omitted) = output.finish();
    let elapsed = started.elapsed();
    match outcome {
        Ok(()) => TestResult {
            name,
            status: TestStatus::Passed,
            message: String::new(),
            file: None,
            line: 0,
            function: None,
            output,
            output_omitted,
            elapsed,
        },
        Err(error) => {
            let (file, line) = location(&error);
            TestResult {
                name,
                status: if error.assertion {
                    TestStatus::Failed
                } else {
                    TestStatus::Errored
                },
                function: error.function.filter(|function| *function != hidden),
                message: error.message,
                file,
                line,
                output,
                output_omitted,
                elapsed,
            }
        }
    }
}
