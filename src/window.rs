//! A desktop window [`GraphicsBackend`] built on the pure-Rust `minifb`
//! crate (Win32, X11/Wayland and Cocoa; no C toolchain or system SDK needed).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use minifb::{Key, MouseButton, MouseMode, Scale, Window, WindowOptions};

use crate::graphics::{Canvas, Event, GraphicsBackend};

/// The minimum time between two window updates; keeps the event loop from
/// spinning a core while still being far below one 60 Hz frame.
const UPDATE_INTERVAL: Duration = Duration::from_millis(4);
/// Longest single wait while sleeping, so the window keeps responding.
const SLEEP_SLICE_MS: u64 = 8;

/// The documented upper-case name of a key, or `None` for keys BASIC does
/// not expose (modifiers, lock keys, numpad variants without a name, ...).
pub fn key_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Key0 | Key::NumPad0 => "0",
        Key::Key1 | Key::NumPad1 => "1",
        Key::Key2 | Key::NumPad2 => "2",
        Key::Key3 | Key::NumPad3 => "3",
        Key::Key4 | Key::NumPad4 => "4",
        Key::Key5 | Key::NumPad5 => "5",
        Key::Key6 | Key::NumPad6 => "6",
        Key::Key7 | Key::NumPad7 => "7",
        Key::Key8 | Key::NumPad8 => "8",
        Key::Key9 | Key::NumPad9 => "9",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Up => "UP",
        Key::Down => "DOWN",
        Key::Left => "LEFT",
        Key::Right => "RIGHT",
        Key::Space => "SPACE",
        Key::Enter | Key::NumPadEnter => "ENTER",
        Key::Escape => "ESC",
        Key::Tab => "TAB",
        Key::Backspace => "BACKSPACE",
        Key::Home => "HOME",
        Key::End => "END",
        Key::PageUp => "PGUP",
        Key::PageDown => "PGDN",
        Key::Insert => "INS",
        Key::Delete => "DEL",
        Key::Apostrophe => "'",
        Key::Backquote => "`",
        Key::Backslash => "\\",
        Key::Comma => ",",
        Key::Equal => "=",
        Key::LeftBracket => "[",
        Key::Minus | Key::NumPadMinus => "-",
        Key::Period | Key::NumPadDot => ".",
        Key::RightBracket => "]",
        Key::Semicolon => ";",
        Key::Slash | Key::NumPadSlash => "/",
        Key::NumPadAsterisk => "*",
        _ => return None,
    })
}

/// Key events for the change from `previous` to `current`: releases first
/// (in name order), then presses (in name order).
pub fn diff_keys(
    previous: &BTreeSet<&'static str>,
    current: &BTreeSet<&'static str>,
) -> Vec<Event> {
    let released = previous
        .difference(current)
        .map(|k| Event::KeyUp((*k).to_string()));
    let pressed = current
        .difference(previous)
        .map(|k| Event::KeyDown((*k).to_string()));
    released.chain(pressed).collect()
}

/// Packs the button state into the documented mask: 1 left, 2 right, 4 middle.
pub fn button_mask(left: bool, right: bool, middle: bool) -> u32 {
    u32::from(left) | u32::from(right) << 1 | u32::from(middle) << 2
}

/// Converts a window-relative pointer position to canvas pixels, clamped to
/// the canvas (non-finite values land on 0).
pub fn clamp_mouse(x: f32, y: f32, width: usize, height: usize) -> (i64, i64) {
    let clamp = |value: f32, size: usize| {
        let value = if value.is_finite() {
            value.floor() as i64
        } else {
            0
        };
        value.clamp(0, size as i64 - 1)
    };
    (clamp(x, width), clamp(y, height))
}

/// A mouse event when `current` differs from `previous`.
pub fn mouse_change(previous: (i64, i64, u32), current: (i64, i64, u32)) -> Option<Event> {
    (previous != current).then_some(Event::Mouse {
        x: current.0,
        y: current.1,
        buttons: current.2,
    })
}

/// A real window; the canvas is shown 1:1 in a fixed-size window.
#[derive(Default)]
pub struct WindowBackend {
    window: Option<Window>,
    size: (usize, usize),
    keys: BTreeSet<&'static str>,
    mouse: (i64, i64, u32),
    close_reported: bool,
}

impl WindowBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl GraphicsBackend for WindowBackend {
    fn open(&mut self, width: usize, height: usize, title: &str) -> Result<(), String> {
        self.close();
        let options = WindowOptions {
            resize: false,
            scale: Scale::X1,
            ..WindowOptions::default()
        };
        let mut window = Window::new(title, width, height, options)
            .map_err(|e| format!("cannot create the window ({e}); is a display available?"))?;
        window.set_target_fps(1000 / UPDATE_INTERVAL.as_millis() as usize);
        self.window = Some(window);
        self.size = (width, height);
        self.keys.clear();
        self.mouse = (0, 0, 0);
        self.close_reported = false;
        Ok(())
    }

    fn close(&mut self) {
        // Dropping the window destroys it.
        self.window = None;
    }

    fn present(&mut self, canvas: &Canvas) -> Result<(), String> {
        let Some(window) = self.window.as_mut().filter(|w| w.is_open()) else {
            return Ok(());
        };
        window
            .update_with_buffer(canvas.pixels(), canvas.width(), canvas.height())
            .map_err(|e| e.to_string())
    }

    fn poll_events(&mut self) -> Vec<Event> {
        let Some(window) = self.window.as_mut() else {
            return Vec::new();
        };
        window.update();
        let mut events = Vec::new();
        if !window.is_open() {
            if !self.close_reported {
                self.close_reported = true;
                events.push(Event::Close);
            }
            return events;
        }
        let current: BTreeSet<&'static str> =
            window.get_keys().into_iter().filter_map(key_name).collect();
        events.extend(diff_keys(&self.keys, &current));
        self.keys = current;
        let (x, y) = window
            .get_mouse_pos(MouseMode::Clamp)
            .map_or((self.mouse.0, self.mouse.1), |(x, y)| {
                clamp_mouse(x, y, self.size.0, self.size.1)
            });
        let buttons = button_mask(
            window.get_mouse_down(MouseButton::Left),
            window.get_mouse_down(MouseButton::Right),
            window.get_mouse_down(MouseButton::Middle),
        );
        let state = (x, y, buttons);
        events.extend(mouse_change(self.mouse, state));
        self.mouse = state;
        events
    }

    fn sleep(&mut self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            if let Some(window) = self.window.as_mut() {
                // Keeps the window responsive (and throttled by the rate limit).
                window.update();
            }
            let slice = remaining.min(Duration::from_millis(SLEEP_SLICE_MS));
            std::thread::sleep(slice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&'static str]) -> BTreeSet<&'static str> {
        names.iter().copied().collect()
    }

    #[test]
    fn key_names_follow_the_documented_vocabulary() {
        assert_eq!(key_name(Key::A), Some("A"));
        assert_eq!(key_name(Key::Key7), Some("7"));
        assert_eq!(key_name(Key::NumPad3), Some("3"));
        assert_eq!(key_name(Key::Escape), Some("ESC"));
        assert_eq!(key_name(Key::Enter), Some("ENTER"));
        assert_eq!(key_name(Key::Space), Some("SPACE"));
        assert_eq!(key_name(Key::Up), Some("UP"));
        assert_eq!(key_name(Key::PageDown), Some("PGDN"));
        assert_eq!(key_name(Key::Insert), Some("INS"));
        assert_eq!(key_name(Key::F12), Some("F12"));
        assert_eq!(key_name(Key::LeftShift), None);
        assert_eq!(key_name(Key::Unknown), None);
    }

    #[test]
    fn diff_reports_releases_then_presses() {
        let before = set(&["A", "LEFT"]);
        let after = set(&["A", "SPACE"]);
        assert_eq!(
            diff_keys(&before, &after),
            vec![Event::KeyUp("LEFT".into()), Event::KeyDown("SPACE".into())]
        );
        assert!(diff_keys(&after, &after).is_empty());
    }

    #[test]
    fn mouse_helpers_clamp_mask_and_detect_changes() {
        assert_eq!(button_mask(true, false, true), 5);
        assert_eq!(button_mask(false, true, false), 2);
        assert_eq!(clamp_mouse(-5.0, 99.7, 10, 20), (0, 19));
        assert_eq!(clamp_mouse(3.9, f32::NAN, 10, 20), (3, 0));
        assert_eq!(mouse_change((1, 2, 0), (1, 2, 0)), None);
        assert_eq!(
            mouse_change((1, 2, 0), (1, 2, 1)),
            Some(Event::Mouse {
                x: 1,
                y: 2,
                buttons: 1
            })
        );
    }
}
