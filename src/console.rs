//! Text-console support: ANSI sequences, the [`KeyProvider`] abstraction
//! behind `INKEY$`/`SLEEP`, and dependency-free terminal helpers.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

/// Supplies pending key presses (`INKEY$` in text mode) and implements
/// `SLEEP` for the text console, so tests can script both.
pub trait KeyProvider {
    /// The next pending key without blocking, or `None`. Printable keys are
    /// the typed character; others use the names documented for
    /// [`crate::Event`] (`ENTER`, `ESC`, `UP`, ...).
    fn poll_key(&mut self) -> Option<String>;
    /// Pauses for `ms` milliseconds.
    fn sleep(&mut self, ms: u64);
}

/// No keyboard: `INKEY$` always returns `""`; `SLEEP` really sleeps. The
/// default of a new VM.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoKeys;

impl KeyProvider for NoKeys {
    fn poll_key(&mut self) -> Option<String> {
        None
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

#[derive(Debug, Default)]
struct ScriptedState {
    keys: VecDeque<String>,
    slept: Vec<u64>,
}

/// A scripted keyboard for tests: keys are queued up front, `SLEEP` is
/// recorded instead of performed. Clones share state.
#[derive(Debug, Clone, Default)]
pub struct ScriptedKeys {
    state: Rc<RefCell<ScriptedState>>,
}

impl ScriptedKeys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, key: &str) {
        self.state.borrow_mut().keys.push_back(key.to_string());
    }

    /// The durations of every `SLEEP`, in order.
    pub fn sleeps(&self) -> Vec<u64> {
        self.state.borrow().slept.clone()
    }
}

impl KeyProvider for ScriptedKeys {
    fn poll_key(&mut self) -> Option<String> {
        self.state.borrow_mut().keys.pop_front()
    }

    fn sleep(&mut self, ms: u64) {
        self.state.borrow_mut().slept.push(ms);
    }
}

/// The real console keyboard. On Windows it uses the C runtime's
/// `_kbhit`/`_getch` (only when stdin is a terminal); elsewhere there is no
/// dependency-free way to read keys without blocking, so `INKEY$` returns
/// `""`. `SLEEP` really sleeps.
#[derive(Debug, Clone, Copy)]
pub struct ConsoleKeys {
    interactive: bool,
}

impl Default for ConsoleKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleKeys {
    pub fn new() -> Self {
        use std::io::IsTerminal;
        Self {
            interactive: std::io::stdin().is_terminal(),
        }
    }
}

impl KeyProvider for ConsoleKeys {
    fn poll_key(&mut self) -> Option<String> {
        if !self.interactive {
            return None;
        }
        poll_console_key()
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;

    unsafe extern "C" {
        pub safe fn _kbhit() -> i32;
        pub safe fn _getch() -> i32;
    }

    unsafe extern "system" {
        pub safe fn GetStdHandle(which: u32) -> *mut c_void;
        pub unsafe fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        pub unsafe fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
    }
}

#[cfg(windows)]
fn poll_console_key() -> Option<String> {
    while sys::_kbhit() != 0 {
        let code = sys::_getch();
        let key = match code {
            0 | 0xE0 => special_key(sys::_getch()),
            13 => "ENTER".to_string(),
            27 => "ESC".to_string(),
            9 => "TAB".to_string(),
            8 => "BACKSPACE".to_string(),
            32 => "SPACE".to_string(),
            33..=255 => char::from(code as u8).to_string(),
            _ => String::new(),
        };
        if !key.is_empty() {
            return Some(key);
        }
    }
    None
}

#[cfg(not(windows))]
fn poll_console_key() -> Option<String> {
    None
}

/// Names the second code of a `_getch` extended-key pair.
#[cfg(windows)]
fn special_key(code: i32) -> String {
    match code {
        59..=68 => return format!("F{}", code - 58),
        72 => "UP",
        80 => "DOWN",
        75 => "LEFT",
        77 => "RIGHT",
        71 => "HOME",
        79 => "END",
        73 => "PGUP",
        81 => "PGDN",
        82 => "INS",
        83 => "DEL",
        _ => "",
    }
    .to_string()
}

/// Enables ANSI escape-sequence processing on the Windows console (a no-op
/// elsewhere, where terminals interpret them already). Returns whether
/// sequences will be interpreted. The CLI calls this when stdout is a
/// terminal.
pub fn enable_terminal_sequences() -> bool {
    #[cfg(windows)]
    {
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
        let handle = sys::GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0u32;
        // SAFETY: `handle` comes from GetStdHandle and `mode` is a valid pointer.
        unsafe {
            if sys::GetConsoleMode(handle, &mut mode) == 0 {
                return false;
            }
            sys::SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// ANSI SGR code for QBasic colour `color` (0..=15) as foreground
/// (`background == false`) or background.
pub(crate) fn ansi_color_code(color: u8, background: bool) -> u32 {
    // QBasic orders colours blue/green/red bits; ANSI orders red/green/blue.
    const ANSI_INDEX: [u32; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    let base = match (color >= 8, background) {
        (false, false) => 30,
        (true, false) => 90,
        (false, true) => 40,
        (true, true) => 100,
    };
    base + ANSI_INDEX[usize::from(color & 7)]
}
