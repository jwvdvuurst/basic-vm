mod bytecode;
mod compiler;
mod console;
mod files;
mod graphics;
mod heap;
mod lexer;
mod stdlib;
mod value;
mod vm;
#[cfg(feature = "window")]
mod window;

pub use bytecode::{
    BinaryOp, Builtin, CallBinding, CollectionKind, EventKind, FieldDefault, Instruction, Op,
    Place, Program, RecordField, RecordType, RefArg, VarType,
};
pub use compiler::{compile, compile_file, compile_from};
pub use console::{ConsoleKeys, KeyProvider, NoKeys, ScriptedKeys, enable_terminal_sequences};
pub use graphics::{
    AutoClose, Canvas, Event, GraphicsBackend, HeadlessBackend, MAX_DIMENSION, MAX_RADIUS, PALETTE,
};
pub use heap::{HeapStats, INITIAL_HEAP_BYTES};
pub use vm::Vm;
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
}

impl Error {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
            function: None,
            file: None,
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
