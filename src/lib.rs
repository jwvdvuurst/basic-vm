mod bytecode;
mod compiler;
mod console;
mod files;
mod graphics;
mod heap;
mod lexer;
mod stdlib;
mod testing;
mod value;
mod vm;
#[cfg(feature = "window")]
mod window;

pub use bytecode::{
    BinaryOp, Builtin, CallBinding, CollectionKind, EventKind, FieldDefault, Instruction, Op,
    Place, Program, RecordField, RecordType, RefArg, TestCase, VarType,
};
pub use compiler::{compile, compile_file, compile_from};
pub use console::{ConsoleKeys, KeyProvider, NoKeys, ScriptedKeys, enable_terminal_sequences};
pub use graphics::{
    AutoClose, Canvas, Event, GraphicsBackend, HeadlessBackend, ImageFormat, MAX_DIMENSION,
    MAX_RADIUS, PALETTE,
};
pub use heap::{HeapStats, INITIAL_HEAP_BYTES};
pub use testing::{
    MAX_CAPTURED_OUTPUT, TestOptions, TestReport, TestResult, TestStatus, run_tests, run_tests_with,
};
pub use vm::{MAX_CALL_DEPTH, Vm};
#[cfg(feature = "window")]
pub use window::WindowBackend;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub message: String,
    /// The user function that raised a runtime error; `None` for the main
    /// program and for compile errors. Functions of imported modules are
    /// qualified (`ALIAS.F`).
    pub function: Option<String>,
    /// The imported source file the error belongs to; `None` for the main
    /// program.
    pub file: Option<String>,
    /// The error is a failed `ASSERT`, `ASSERTEQ` or `ASSERTNEAR` (as opposed
    /// to any other runtime error). Always false for compile errors.
    pub assertion: bool,
}

impl Error {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
            function: None,
            file: None,
            assertion: false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.file, self.line) {
            (Some(file), line) if line > 0 => write!(f, "{file}:{line}: {}", self.message)?,
            (Some(file), _) => write!(f, "{file}: {}", self.message)?,
            (None, 0) => write!(f, "{}", self.message)?,
            (None, line) => write!(f, "line {line}: {}", self.message)?,
        }
        if let Some(function) = &self.function {
            write!(f, " (in function {function})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}
