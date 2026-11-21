//! The table of open files. Handles are plain host resources owned by the
//! table (never by the garbage-collected heap); they are flushed and closed
//! by `CLOSE`, when a run ends, or when the table is dropped.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// File numbers run from 1 to this value, so at most 255 files are open.
pub(crate) const MAX_FILES: usize = 255;
/// Largest byte count one `GET` call may return.
pub(crate) const MAX_READ_BYTES: usize = 65_536;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Input,
    Output,
    Append,
    Binary,
}

impl Mode {
    pub(crate) fn from_code(code: f64) -> Option<Self> {
        match code as i64 {
            0 => Some(Self::Input),
            1 => Some(Self::Output),
            2 => Some(Self::Append),
            3 => Some(Self::Binary),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Input => "INPUT",
            Self::Output => "OUTPUT",
            Self::Append => "APPEND",
            Self::Binary => "BINARY",
        }
    }
}

enum Handle {
    Reader(BufReader<File>),
    Writer(BufWriter<File>),
    Binary(File),
}

struct OpenFile {
    canonical: PathBuf,
    handle: Handle,
}

#[derive(Default)]
pub(crate) struct FileTable {
    files: BTreeMap<usize, OpenFile>,
}

pub(crate) fn describe(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "file not found".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => error.to_string(),
    }
}

impl FileTable {
    pub(crate) fn is_open(&self, number: usize) -> bool {
        self.files.contains_key(&number)
    }

    pub(crate) fn free_number(&self) -> Option<usize> {
        (1..=MAX_FILES).find(|number| !self.files.contains_key(number))
    }

    pub(crate) fn open(&mut self, number: usize, path: &str, mode: Mode) -> Result<()> {
        if self.is_open(number) {
            return Err(format!("file #{number} is already open"));
        }
        // Canonicalizing may fail (not-yet-existing files, devices such as NUL);
        // that is not fatal, the raw path is used for identity instead.
        let identity = |path: &str| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
        if let Some(existing) = self.open_as(Path::new(path)) {
            return Err(format!(
                "\"{path}\" is already open as file #{existing}; CLOSE it first"
            ));
        }
        let fail = |error: io::Error| {
            format!(
                "cannot open \"{path}\" for {}: {}",
                mode.name(),
                describe(&error)
            )
        };
        let handle = match mode {
            Mode::Input => Handle::Reader(BufReader::new(File::open(path).map_err(fail)?)),
            Mode::Output => Handle::Writer(BufWriter::new(File::create(path).map_err(fail)?)),
            Mode::Append => Handle::Writer(BufWriter::new(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)
                    .map_err(fail)?,
            )),
            Mode::Binary => {
                let read_write = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path);
                match read_write {
                    Ok(file) => Handle::Binary(file),
                    // Read-only files still allow GET/SEEK; PUT then reports the denial.
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                        Handle::Binary(File::open(path).map_err(|_| fail(error))?)
                    }
                    Err(error) => return Err(fail(error)),
                }
            }
        };
        let canonical = identity(path);
        self.files.insert(number, OpenFile { canonical, handle });
        Ok(())
    }

    /// Flushes and closes one file. The handle is released even if the flush fails.
    pub(crate) fn close(&mut self, number: usize) -> Result<()> {
        let mut file = self
            .files
            .remove(&number)
            .ok_or_else(|| format!("file #{number} is not open"))?;
        file.flush()
            .map_err(|e| format!("cannot write file #{number}: {}", describe(&e)))
    }

    /// Closes every file; reports the first flush failure after closing all.
    pub(crate) fn close_all(&mut self) -> Result<()> {
        let mut first = None;
        for (number, mut file) in std::mem::take(&mut self.files) {
            if let Err(error) = file.flush() {
                first.get_or_insert(format!("cannot write file #{number}: {}", describe(&error)));
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// The number a path is currently open under, if any.
    pub(crate) fn open_as(&self, path: &Path) -> Option<usize> {
        let canonical = std::fs::canonicalize(path).ok()?;
        self.files
            .iter()
            .find(|(_, file)| file.canonical == canonical)
            .map(|(number, _)| *number)
    }

    fn get(&mut self, number: usize) -> Result<&mut OpenFile> {
        self.files
            .get_mut(&number)
            .ok_or_else(|| format!("file #{number} is not open"))
    }

    pub(crate) fn write_text(&mut self, number: usize, text: &str) -> Result<()> {
        self.write_bytes(number, text.as_bytes(), "PRINT")
    }

    pub(crate) fn write_bytes(&mut self, number: usize, bytes: &[u8], who: &str) -> Result<()> {
        let file = self.get(number)?;
        let result = match &mut file.handle {
            Handle::Writer(writer) => writer.write_all(bytes),
            Handle::Binary(file) => file.write_all(bytes),
            Handle::Reader(_) => {
                return Err(format!(
                    "file #{number} is open for INPUT; {who} needs OUTPUT, APPEND or BINARY"
                ));
            }
        };
        result.map_err(|e| format!("cannot write file #{number}: {}", describe(&e)))
    }

    /// Reads one line without its `\n` / `\r\n` terminator.
    pub(crate) fn read_line(&mut self, number: usize) -> Result<String> {
        let file = self.get(number)?;
        let Handle::Reader(reader) = &mut file.handle else {
            return Err(format!("file #{number} is not open for INPUT"));
        };
        let mut bytes = Vec::new();
        let read = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|e| format!("cannot read file #{number}: {}", describe(&e)))?;
        if read == 0 {
            return Err(format!("reading past the end of file #{number}"));
        }
        if bytes.ends_with(b"\n") {
            bytes.pop();
            if bytes.ends_with(b"\r") {
                bytes.pop();
            }
        }
        String::from_utf8(bytes).map_err(|_| format!("file #{number} contains invalid UTF-8"))
    }

    pub(crate) fn at_end(&mut self, number: usize) -> Result<bool> {
        let file = self.get(number)?;
        let read_error = |e: io::Error| format!("cannot read file #{number}: {}", describe(&e));
        match &mut file.handle {
            Handle::Reader(reader) => Ok(reader.fill_buf().map_err(read_error)?.is_empty()),
            Handle::Binary(file) => {
                let position = file.stream_position().map_err(read_error)?;
                Ok(position >= file.metadata().map_err(read_error)?.len())
            }
            Handle::Writer(_) => Err(format!(
                "file #{number} is not open for INPUT or BINARY; EOF is undefined"
            )),
        }
    }

    pub(crate) fn length(&mut self, number: usize) -> Result<u64> {
        let file = self.get(number)?;
        let error = |e: io::Error| format!("cannot query file #{number}: {}", describe(&e));
        match &mut file.handle {
            Handle::Reader(reader) => Ok(reader.get_ref().metadata().map_err(error)?.len()),
            Handle::Writer(writer) => {
                writer.flush().map_err(error)?;
                Ok(writer.get_ref().metadata().map_err(error)?.len())
            }
            Handle::Binary(file) => Ok(file.metadata().map_err(error)?.len()),
        }
    }

    fn binary(&mut self, number: usize) -> Result<&mut File> {
        match &mut self.get(number)?.handle {
            Handle::Binary(file) => Ok(file),
            _ => Err(format!("file #{number} is not open for BINARY")),
        }
    }

    pub(crate) fn position(&mut self, number: usize) -> Result<u64> {
        self.binary(number)?
            .stream_position()
            .map_err(|e| format!("cannot query file #{number}: {}", describe(&e)))
    }

    pub(crate) fn seek(&mut self, number: usize, position: u64) -> Result<()> {
        if position > i64::MAX as u64 {
            return Err(format!(
                "cannot seek file #{number}: position {position} is too large"
            ));
        }
        self.binary(number)?
            .seek(SeekFrom::Start(position))
            .map(|_| ())
            .map_err(|e| match e.kind() {
                io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => format!(
                    "cannot seek file #{number}: position {position} is not valid for this file"
                ),
                _ => format!("cannot seek file #{number}: {}", describe(&e)),
            })
    }

    /// Reads up to `count` bytes; fewer (possibly none) at the end of the file.
    pub(crate) fn read_bytes(&mut self, number: usize, count: usize) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.binary(number)?
            .take(count as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read file #{number}: {}", describe(&e)))?;
        Ok(bytes)
    }

    pub(crate) fn put_bytes(&mut self, number: usize, bytes: &[u8]) -> Result<()> {
        self.binary(number)?;
        self.write_bytes(number, bytes, "PUT")
    }
}

impl OpenFile {
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.handle {
            Handle::Writer(writer) => writer.flush(),
            Handle::Binary(file) => file.flush(),
            Handle::Reader(_) => Ok(()),
        }
    }
}
