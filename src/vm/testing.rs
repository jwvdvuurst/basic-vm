//! Running one `TEST` block on a fresh VM (see `crate::testing`).

use std::io::{self, Write};
use std::time::Duration;

use super::Vm;
use crate::Error;

/// Where an `END` or `STOP` stopped a test.
pub(super) struct Halted {
    pub keyword: &'static str,
    pub line: usize,
    pub file: usize,
    pub function: Option<String>,
}

/// Per-VM test-mode state.
#[derive(Default)]
pub(super) struct TestState {
    /// `SLEEP` does not wait; it advances `TIMER` instead.
    pub no_sleep: bool,
    /// The test's own function has been entered.
    pub running: bool,
    /// Set when `END`/`STOP` ended the test.
    pub halted: Option<Halted>,
    /// Virtual time added to `TIMER` by skipped sleeps.
    pub skipped: Duration,
}

impl Vm {
    /// Makes `SLEEP` return immediately (it advances `TIMER` by the requested
    /// time instead of waiting), whatever key provider or graphics backend is
    /// installed.
    pub(crate) fn skip_sleeps(&mut self) {
        self.test.no_sleep = true;
    }

    /// Runs test number `test` of the program: the imported modules'
    /// top-level initialisation, then (for tests of the main program) its
    /// top-level `CONST`/`DIM` statements, then the test's hidden function.
    /// The main program's own body is never executed. Meant for a freshly
    /// created VM; call [`Vm::finish_test`] afterwards.
    pub(crate) fn run_test_case(
        &mut self,
        test: usize,
        output: &mut impl Write,
    ) -> Result<(), Error> {
        let case = self
            .program
            .tests
            .get(test)
            .cloned()
            .ok_or_else(|| Error::new(0, "invalid test index"))?;
        let mut input = io::empty();
        let main_start = self.program.main_start;
        self.pc = 0;
        self.run_instructions(&mut input, output, main_start)?;
        if case.file == 0 {
            for range in self.program.prelude.clone() {
                self.pc = range.start;
                self.run_instructions(&mut input, output, range.end)?;
            }
        }
        // Return from the test lands on the program's final `Halt`.
        self.pc = self.program.instructions.len().saturating_sub(1);
        self.call(case.function, 0, true, None).map_err(|message| {
            let mut error = Error::new(case.line, message);
            error.file =
                Some(self.program.file_name(case.file).to_string()).filter(|name| !name.is_empty());
            error
        })?;
        self.test.running = true;
        self.run_instructions(&mut input, output, usize::MAX)?;
        // `END`/`STOP` halt the VM quietly; a test must not pass that way.
        match self.test.halted.take() {
            None => Ok(()),
            Some(halted) => {
                let mut error =
                    Error::new(halted.line, format!("test ended by {}", halted.keyword));
                error.function = halted.function;
                error.file = Some(self.program.file_name(halted.file).to_string())
                    .filter(|name| !name.is_empty());
                Err(error)
            }
        }
    }

    /// The numbers of the files still open.
    pub(crate) fn open_file_numbers(&self) -> Vec<usize> {
        self.files.open_numbers()
    }

    /// End-of-test housekeeping: finishes the media layer and closes every
    /// file, reporting the first problem.
    pub(crate) fn finish_test(&mut self, output: &mut impl Write) -> Result<(), String> {
        let media = self.finish_media(output);
        let closed = self.files.close_all();
        media.and(closed)
    }
}
