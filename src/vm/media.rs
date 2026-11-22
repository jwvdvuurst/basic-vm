//! Console and graphics statements, event polling and handler dispatch.

use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::Path;

use super::{Vm, boolean, number};
use crate::console::{KeyProvider, NoKeys, ansi_color_code};
use crate::files::describe;
use crate::graphics::{
    COORDINATE_LIMIT, Canvas, Event, GraphicsBackend, ImageFormat, MAX_RADIUS, PALETTE,
};
use crate::value::Value;
use crate::{Builtin, EventKind};

type Result<T> = std::result::Result<T, String>;

/// Longest `SLEEP`, in milliseconds (one hour).
const MAX_SLEEP_MS: i64 = 3_600_000;
/// Bounds of the unread-key and undispatched-event queues; the oldest
/// entries are dropped beyond them.
const MAX_QUEUED_KEYS: usize = 256;
const MAX_QUEUED_EVENTS: usize = 1024;
const MAX_CURSOR: i64 = 9999;

/// A `DOEVENTS` in progress: the events of this batch not yet delivered.
/// Delivery is re-entrant: each handler call returns to the same
/// instruction, which then delivers the next event.
struct Dispatch {
    pc: usize,
    depth: usize,
    queue: VecDeque<Event>,
}

/// The open graphics screen: one pixel buffer (single-buffered) or two
/// (double-buffered). The backend only ever shows pages that were presented;
/// `visible` names the page that was presented last (or, in single-buffer
/// mode, the only page) and `active` the page drawing statements target.
struct Screen {
    pages: Vec<Canvas>,
    active: usize,
    visible: usize,
}

impl Screen {
    fn is_double(&self) -> bool {
        self.pages.len() > 1
    }
}

pub(super) struct Media {
    backend: Option<Box<dyn GraphicsBackend>>,
    screen: Option<Screen>,
    color: u32,
    keys: Box<dyn KeyProvider>,
    key_queue: VecDeque<String>,
    keys_down: HashSet<String>,
    mouse: (i64, i64, u32),
    closed: bool,
    pending: VecDeque<Event>,
    handlers: [Option<usize>; 3],
    dispatch: Option<Dispatch>,
    /// `COLOR` changed the terminal colours, so they are reset at the end.
    styled: bool,
}

impl Default for Media {
    fn default() -> Self {
        Self {
            backend: None,
            screen: None,
            color: PALETTE[15],
            keys: Box::new(NoKeys),
            key_queue: VecDeque::new(),
            keys_down: HashSet::new(),
            mouse: (0, 0, 0),
            closed: false,
            pending: VecDeque::new(),
            handlers: [None; 3],
            dispatch: None,
            styled: false,
        }
    }
}

impl Media {
    pub(super) fn cancel_dispatch(&mut self) {
        self.dispatch = None;
    }
}

fn who_error(who: &str, message: impl std::fmt::Display) -> String {
    format!("{who}: {message}")
}

/// A coordinate: numbers truncate toward zero and are clamped to
/// `±COORDINATE_LIMIT`.
fn coordinate(value: Value, who: &str, what: &str) -> Result<i64> {
    let n = match value {
        Value::Int(n) => n,
        other => number(other)
            .map_err(|_| who_error(who, format!("{what} must be a number")))?
            .trunc() as i64,
    };
    Ok(n.clamp(-COORDINATE_LIMIT, COORDINATE_LIMIT))
}

/// A whole number within `low..=high` (numbers truncate toward zero).
fn ranged(value: Value, who: &str, what: &str, low: i64, high: i64) -> Result<i64> {
    // Range-check the original (unclamped) value so errors report what was written.
    let n = match value {
        Value::Int(n) => n,
        other => number(other)
            .map_err(|_| who_error(who, format!("{what} must be a number")))?
            .trunc() as i64,
    };
    if !(low..=high).contains(&n) {
        return Err(who_error(
            who,
            format!("{what} must be from {low} to {high}, got {n}"),
        ));
    }
    Ok(n)
}

/// A colour: 0..=15 selects the QBasic palette, 16..=16777215 is packed
/// `0xRRGGBB`. `NOTHING` (an omitted argument) gives `default`.
fn colour(value: Value, who: &str, default: u32) -> Result<u32> {
    if matches!(value, Value::Nothing) {
        return Ok(default);
    }
    let n = coordinate(value, who, "color")?;
    match n {
        0..=15 => Ok(PALETTE[n as usize]),
        16..=0xFF_FFFF => Ok(n as u32),
        _ => Err(who_error(
            who,
            format!(
                "color must be a palette index 0-15 or a packed RGB value up to 16777215, got {n}"
            ),
        )),
    }
}

/// The zero-based page index of buffer `number` (1 or 2). A single-buffered
/// screen only has buffer 1; asking for another is an explicit error.
fn buffer_index(screen: &Screen, number: i64, who: &str) -> Result<usize> {
    let count = screen.pages.len();
    if (1..=count as i64).contains(&number) {
        return Ok(number as usize - 1);
    }
    Err(who_error(
        who,
        if count == 1 {
            format!(
                "buffer {number} does not exist: the screen is single-buffered \
                 (open it with SCREEN width, height, title$, 2)"
            )
        } else {
            format!("buffer must be 1 or 2, got {number}")
        },
    ))
}

fn key_kind(event: &Event) -> Option<EventKind> {
    match event {
        Event::KeyDown(_) => Some(EventKind::Key),
        Event::Mouse { .. } => Some(EventKind::Mouse),
        Event::Close => Some(EventKind::Close),
        Event::KeyUp(_) => None,
    }
}

impl Vm {
    /// Installs the backend that `SCREEN w, h` opens. Without one, `SCREEN`
    /// fails with an explicit error.
    pub fn set_graphics_backend(&mut self, backend: Box<dyn GraphicsBackend>) {
        self.media.backend = Some(backend);
    }

    /// Installs the keyboard behind text-mode `INKEY$` and `SLEEP`.
    pub fn set_key_provider(&mut self, keys: Box<dyn KeyProvider>) {
        self.media.keys = keys;
    }

    /// The graphics buffer drawing currently targets (the only canvas in
    /// single-buffer mode), if `SCREEN` opened one. It stays available after
    /// the program ends.
    pub fn canvas(&self) -> Option<&Canvas> {
        self.media
            .screen
            .as_ref()
            .map(|screen| &screen.pages[screen.active])
    }

    /// The graphics buffer the display shows: the one published by the last
    /// `FLIP` (the same as [`Vm::canvas`] in single-buffer mode).
    pub fn visible_canvas(&self) -> Option<&Canvas> {
        self.media
            .screen
            .as_ref()
            .map(|screen| &screen.pages[screen.visible])
    }

    /// End-of-run housekeeping: terminal colours are reset when `COLOR` was
    /// used, and a pending frame is presented and the backend closed.
    pub(super) fn finish_media(&mut self, output: &mut impl Write) -> Result<()> {
        let mut result = Ok(());
        if self.media.styled {
            self.media.styled = false;
            result = write!(output, "\x1b[0m").map_err(|e| format!("output failed: {e}"));
        }
        if self.media.screen.is_some() {
            if let Err(message) = self.present_if_dirty() {
                result = result.and(Err(message));
            }
            if let Some(backend) = self.media.backend.as_mut() {
                backend.close();
            }
        }
        result
    }

    fn no_screen(who: &str) -> String {
        who_error(
            who,
            "no graphics screen (open one with SCREEN width, height)",
        )
    }

    fn screen_mut(&mut self, who: &str) -> Result<&mut Screen> {
        self.media
            .screen
            .as_mut()
            .ok_or_else(|| Self::no_screen(who))
    }

    /// The buffer drawing statements target.
    fn canvas_mut(&mut self, who: &str) -> Result<&mut Canvas> {
        let screen = self.screen_mut(who)?;
        Ok(&mut screen.pages[screen.active])
    }

    /// Sends page `index` to the backend and marks it clean.
    fn present_page(&mut self, index: usize) -> Result<()> {
        let media = &mut self.media;
        if let (Some(backend), Some(screen)) = (media.backend.as_mut(), media.screen.as_mut()) {
            let page = &mut screen.pages[index];
            backend
                .present(page)
                .map_err(|e| format!("cannot present the frame: {e}"))?;
            page.mark_clean();
        }
        Ok(())
    }

    /// Automatic presenting (`DOEVENTS`, `SLEEP`, end of run): publishes the
    /// *visible* page when it changed. A hidden back buffer is never shown.
    fn present_if_dirty(&mut self) -> Result<()> {
        let visible = self
            .media
            .screen
            .as_ref()
            .filter(|screen| screen.pages[screen.visible].is_dirty())
            .map(|screen| screen.visible);
        match visible {
            Some(index) => self.present_page(index),
            None => Ok(()),
        }
    }

    /// Reads the backend's events into the key queue, key/mouse state, the
    /// close flag and the undispatched queue.
    fn pump_events(&mut self) {
        let media = &mut self.media;
        let (Some(backend), Some(_)) = (media.backend.as_mut(), media.screen.as_ref()) else {
            return;
        };
        for event in backend.poll_events() {
            let event = match event {
                Event::KeyDown(key) => Event::KeyDown(key.to_uppercase()),
                Event::KeyUp(key) => Event::KeyUp(key.to_uppercase()),
                other => other,
            };
            match &event {
                Event::KeyDown(key) => {
                    media.keys_down.insert(key.clone());
                    media.key_queue.push_back(key.clone());
                    if media.key_queue.len() > MAX_QUEUED_KEYS {
                        media.key_queue.pop_front();
                    }
                }
                Event::KeyUp(key) => {
                    media.keys_down.remove(key);
                }
                Event::Mouse { x, y, buttons } => media.mouse = (*x, *y, *buttons),
                Event::Close => media.closed = true,
            }
            if key_kind(&event).is_some() {
                media.pending.push_back(event);
                if media.pending.len() > MAX_QUEUED_EVENTS {
                    media.pending.pop_front();
                }
            }
        }
    }

    fn close_screen(&mut self) -> Result<()> {
        if self.media.screen.is_none() {
            return Ok(());
        }
        let presented = self.present_if_dirty();
        if let Some(backend) = self.media.backend.as_mut() {
            backend.close();
        }
        // The last screen (all its buffers) is released, not kept readable:
        // `SCREEN 0` frees every buffer.
        self.media.screen = None;
        self.media.keys_down.clear();
        self.media.key_queue.clear();
        self.media.pending.clear();
        self.media.mouse = (0, 0, 0);
        self.media.closed = false;
        presented
    }

    fn open_screen(&mut self, width: i64, height: i64, title: &str, buffers: usize) -> Result<()> {
        self.close_screen()?;
        let pages = (0..buffers)
            .map(|_| Canvas::new(width as usize, height as usize))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|message| who_error("SCREEN", message))?;
        let backend = self.media.backend.as_mut().ok_or(
            "SCREEN: no graphics backend is available (the CLI offers --headless-graphics; \
             library users call Vm::set_graphics_backend)",
        )?;
        backend
            .open_buffered(pages[0].width(), pages[0].height(), title, buffers)
            .map_err(|e| format!("SCREEN: cannot open the graphics screen: {e}"))?;
        // Double-buffered: buffer 1 is the front (shown) page, buffer 2 the
        // back page drawing starts in.
        let active = buffers - 1;
        self.media.screen = Some(Screen {
            pages,
            active,
            visible: 0,
        });
        self.media.color = PALETTE[15];
        Ok(())
    }

    /// `SCREENSHOT file$ [, VISIBLE]`: encodes the active (or visible) buffer
    /// by the file extension and writes it, replacing an existing file. The
    /// framebuffer is read, never the display, so it works with every backend.
    fn save_screenshot(&self, path: &str, visible: bool) -> Result<()> {
        let screen = self
            .media
            .screen
            .as_ref()
            .ok_or_else(|| Self::no_screen("SCREENSHOT"))?;
        let target = Path::new(path);
        let format = ImageFormat::from_path(target)
            .ok_or_else(|| format!("SCREENSHOT: \"{path}\" must end in .ppm, .bmp or .png"))?;
        if let Some(file) = self.files.open_as(target) {
            return Err(format!(
                "SCREENSHOT: \"{path}\" is open as file #{file}; CLOSE it first"
            ));
        }
        let page = &screen.pages[if visible {
            screen.visible
        } else {
            screen.active
        }];
        // `fs::write` creates or truncates the file and closes it on return.
        std::fs::write(target, page.encode(format))
            .map_err(|e| format!("SCREENSHOT: cannot write \"{path}\": {}", describe(&e)))
    }

    /// `DOEVENTS`: delivers the pending events to their handlers, in order.
    fn do_events(&mut self) -> Result<()> {
        let pc = self.pc - 1;
        let depth = self.frames.len();
        match &self.media.dispatch {
            Some(d) if d.pc == pc && d.depth == depth => {}
            Some(d) if depth > d.depth => {
                return Err("DOEVENTS: cannot be used inside an event handler".into());
            }
            _ => {
                self.present_if_dirty()?;
                self.pump_events();
                let queue = std::mem::take(&mut self.media.pending);
                self.media.dispatch = Some(Dispatch { pc, depth, queue });
            }
        }
        loop {
            let Some(event) = self
                .media
                .dispatch
                .as_mut()
                .and_then(|dispatch| dispatch.queue.pop_front())
            else {
                self.media.dispatch = None;
                return Ok(());
            };
            let Some(function) =
                key_kind(&event).and_then(|kind| self.media.handlers[kind.index()])
            else {
                continue;
            };
            let arguments = match event {
                Event::KeyDown(key) => {
                    let value = self.allocate(&key)?;
                    self.stack.push(value);
                    1
                }
                Event::Mouse { x, y, buttons } => {
                    self.stack.push(Value::Number(x as f64));
                    self.stack.push(Value::Number(y as f64));
                    self.stack.push(Value::Number(f64::from(buttons)));
                    3
                }
                Event::Close | Event::KeyUp(_) => 0,
            };
            // The handler returns to this instruction, which delivers the
            // next event.
            self.pc -= 1;
            return self.call(function, arguments, true, None);
        }
    }

    pub(super) fn register_handler(&mut self, kind: EventKind, function: Option<usize>) {
        self.media.handlers[kind.index()] = function;
    }

    pub(super) fn media_builtin(
        &mut self,
        builtin: Builtin,
        argument_count: usize,
        output: &mut impl Write,
    ) -> Result<Option<Value>> {
        let args = self.pop_arguments(argument_count)?;
        let who = builtin.name();
        let fail = |e: std::io::Error| format!("output failed: {e}");
        let value = match builtin {
            Builtin::Cls => {
                write!(output, "\x1b[2J\x1b[H").map_err(fail)?;
                return Ok(None);
            }
            Builtin::Color => {
                let foreground = ranged(args[0], who, "foreground", 0, 15)? as u8;
                let mut codes = vec![ansi_color_code(foreground, false)];
                if let Some(background) = args.get(1) {
                    let background = ranged(*background, who, "background", 0, 15)? as u8;
                    codes.push(ansi_color_code(background, true));
                }
                let codes: Vec<String> = codes.iter().map(u32::to_string).collect();
                write!(output, "\x1b[{}m", codes.join(";")).map_err(fail)?;
                self.media.styled = true;
                return Ok(None);
            }
            Builtin::Locate => {
                let row = ranged(args[0], who, "row", 1, MAX_CURSOR)?;
                let column = match args.get(1) {
                    Some(value) => ranged(*value, who, "column", 1, MAX_CURSOR)?,
                    None => 1,
                };
                write!(output, "\x1b[{row};{column}H").map_err(fail)?;
                return Ok(None);
            }
            Builtin::Sleep => {
                let ms = ranged(args[0], who, "milliseconds", 0, MAX_SLEEP_MS)? as u64;
                output.flush().map_err(fail)?;
                self.present_if_dirty()?;
                if self.test.no_sleep {
                    // Tests never wait; TIMER still sees the time pass.
                    self.test.skipped += std::time::Duration::from_millis(ms);
                    if self.media.screen.is_some() {
                        self.pump_events();
                    }
                } else if self.media.screen.is_some() {
                    if let Some(backend) = self.media.backend.as_mut() {
                        backend.sleep(ms);
                    }
                    self.pump_events();
                } else {
                    self.media.keys.sleep(ms);
                }
                return Ok(None);
            }
            Builtin::Inkey | Builtin::GetKey => {
                if self.media.screen.is_some() {
                    self.pump_events();
                    let key = self.media.key_queue.pop_front().unwrap_or_default();
                    self.allocate(&key)?
                } else if builtin == Builtin::Inkey {
                    output.flush().map_err(fail)?;
                    let key = self.media.keys.poll_key().unwrap_or_default();
                    self.allocate(&key)?
                } else {
                    self.allocate("")?
                }
            }
            Builtin::Screen => {
                if argument_count == 1 {
                    if ranged(args[0], who, "mode", 0, i64::MAX)? != 0 {
                        return Err(
                            "SCREEN: use SCREEN 0 or SCREEN width, height [, title$ [, buffers]]"
                                .into(),
                        );
                    }
                    self.close_screen()?;
                } else {
                    let width = ranged(args[0], who, "width", 1, 4096)?;
                    let height = ranged(args[1], who, "height", 1, 4096)?;
                    let title = match args.get(2) {
                        Some(Value::String(reference)) => self.heap.get(*reference)?.to_string(),
                        Some(_) => return Err("SCREEN: the title must be a string".into()),
                        None => "BASIC".to_string(),
                    };
                    let buffers = match args.get(3) {
                        Some(value) => {
                            let count = coordinate(*value, who, "buffers")?;
                            if !(1..=2).contains(&count) {
                                return Err(format!(
                                    "SCREEN: buffers must be 1 (single) or 2 (double buffering), got {count}"
                                ));
                            }
                            count as usize
                        }
                        None => 1,
                    };
                    self.open_screen(width, height, &title, buffers)?;
                }
                return Ok(None);
            }
            Builtin::Gcls => {
                let color = colour(args.first().copied().unwrap_or(Value::Nothing), who, 0)?;
                self.canvas_mut(who)?.clear(color);
                return Ok(None);
            }
            Builtin::Gcolor => {
                let color = colour(args[0], who, PALETTE[15])?;
                self.canvas_mut(who)?;
                self.media.color = color;
                return Ok(None);
            }
            Builtin::Pset => {
                let x = coordinate(args[0], who, "x")?;
                let y = coordinate(args[1], who, "y")?;
                let default = self.media.color;
                let color = colour(args.get(2).copied().unwrap_or(Value::Nothing), who, default)?;
                self.canvas_mut(who)?.set_pixel(x, y, color);
                return Ok(None);
            }
            Builtin::Point => {
                let x = coordinate(args[0], who, "x")?;
                let y = coordinate(args[1], who, "y")?;
                let pixel = self.canvas_mut(who)?.pixel(x, y);
                Value::Int(pixel.map_or(-1, i64::from))
            }
            Builtin::Line | Builtin::LineBox | Builtin::LineFill => {
                let x1 = coordinate(args[0], who, "x1")?;
                let y1 = coordinate(args[1], who, "y1")?;
                let x2 = coordinate(args[2], who, "x2")?;
                let y2 = coordinate(args[3], who, "y2")?;
                let default = self.media.color;
                let color = colour(args.get(4).copied().unwrap_or(Value::Nothing), who, default)?;
                let canvas = self.canvas_mut(who)?;
                match builtin {
                    Builtin::Line => canvas.line(x1, y1, x2, y2, color),
                    Builtin::LineBox => canvas.rect(x1, y1, x2, y2, color),
                    _ => canvas.fill_rect(x1, y1, x2, y2, color),
                }
                return Ok(None);
            }
            Builtin::Circle | Builtin::CircleFill => {
                let x = coordinate(args[0], who, "x")?;
                let y = coordinate(args[1], who, "y")?;
                let radius = ranged(args[2], who, "radius", 0, MAX_RADIUS)?;
                let default = self.media.color;
                let color = colour(args.get(3).copied().unwrap_or(Value::Nothing), who, default)?;
                let canvas = self.canvas_mut(who)?;
                if builtin == Builtin::Circle {
                    canvas.circle(x, y, radius, color);
                } else {
                    canvas.fill_circle(x, y, radius, color);
                }
                return Ok(None);
            }
            Builtin::DrawText => {
                let x = coordinate(args[0], who, "x")?;
                let y = coordinate(args[1], who, "y")?;
                let Value::String(reference) = args[2] else {
                    return Err("TEXT: the text must be a string".into());
                };
                let text = self.heap.get(reference)?.to_string();
                let default = self.media.color;
                let color = colour(args.get(3).copied().unwrap_or(Value::Nothing), who, default)?;
                self.canvas_mut(who)?.text(x, y, &text, color);
                return Ok(None);
            }
            Builtin::Flip | Builtin::FlipClear | Builtin::Present => {
                let screen = self.screen_mut(who)?;
                let clear = builtin == Builtin::FlipClear;
                if clear && !screen.is_double() {
                    return Err(
                        "FLIP CLEAR: needs a double-buffered screen (SCREEN w, h, title$, 2)"
                            .into(),
                    );
                }
                // Publish the active buffer: it becomes the visible one. In
                // double-buffered mode drawing then moves to the other page.
                let published = screen.active;
                self.present_page(published)?;
                let screen = self.screen_mut(who)?;
                screen.visible = published;
                if screen.is_double() {
                    screen.active = 1 - published;
                    if clear {
                        screen.pages[screen.active].clear(0);
                    }
                }
                self.pump_events();
                return Ok(None);
            }
            Builtin::Buffer => {
                let number = coordinate(args[0], who, "buffer")?;
                let screen = self.screen_mut(who)?;
                screen.active = buffer_index(screen, number, who)?;
                return Ok(None);
            }
            Builtin::CopyBuffer => {
                let source = coordinate(args[0], who, "source buffer")?;
                let target = coordinate(args[1], who, "destination buffer")?;
                let screen = self.screen_mut(who)?;
                let (source, target) = (
                    buffer_index(screen, source, who)?,
                    buffer_index(screen, target, who)?,
                );
                if source != target {
                    let (low, high) = screen.pages.split_at_mut(source.max(target));
                    let (from, to) = if source < target {
                        (&low[source], &mut high[0])
                    } else {
                        (&high[0], &mut low[target])
                    };
                    to.copy_from(from);
                }
                return Ok(None);
            }
            Builtin::ActiveBuffer | Builtin::VisibleBuffer | Builtin::Buffers => {
                let screen = self.screen_mut(who)?;
                Value::Int(match builtin {
                    Builtin::ActiveBuffer => screen.active + 1,
                    Builtin::VisibleBuffer => screen.visible + 1,
                    _ => screen.pages.len(),
                } as i64)
            }
            Builtin::Screenshot | Builtin::ScreenshotVisible => {
                // Named SCREENSHOT in messages for both forms.
                let who = "SCREENSHOT";
                self.screen_mut(who)?;
                let Value::String(reference) = args[0] else {
                    return Err("SCREENSHOT: the file name must be a string".into());
                };
                let path = self.heap.get(reference)?.to_string();
                self.save_screenshot(&path, builtin == Builtin::ScreenshotVisible)?;
                return Ok(None);
            }
            Builtin::DoEvents => {
                self.do_events()?;
                return Ok(None);
            }
            Builtin::Rgb => {
                let mut packed = 0i64;
                for (value, name) in args.iter().zip(["red", "green", "blue"]) {
                    packed = packed << 8 | ranged(*value, who, name, 0, 255)?;
                }
                Value::Int(packed)
            }
            Builtin::KeyDown => {
                let Value::String(reference) = args[0] else {
                    return Err("KEYDOWN: the key name must be a string".into());
                };
                let name = self.heap.get(reference)?.to_uppercase();
                self.pump_events();
                boolean(self.media.keys_down.contains(&name))
            }
            Builtin::MouseX | Builtin::MouseY | Builtin::MouseB => {
                self.pump_events();
                let (x, y, buttons) = self.media.mouse;
                Value::Number(match builtin {
                    Builtin::MouseX => x as f64,
                    Builtin::MouseY => y as f64,
                    _ => f64::from(buttons),
                })
            }
            Builtin::Closed => {
                self.pump_events();
                boolean(self.media.closed)
            }
            Builtin::IsGraphics => boolean(self.media.screen.is_some()),
            Builtin::WindowOpen => {
                self.pump_events();
                boolean(self.media.screen.is_some() && !self.media.closed)
            }
            Builtin::ScreenW | Builtin::ScreenH | Builtin::Width | Builtin::Height => {
                let canvas = self.canvas_mut(who)?;
                Value::Number(if matches!(builtin, Builtin::ScreenW | Builtin::Width) {
                    canvas.width() as f64
                } else {
                    canvas.height() as f64
                })
            }
            Builtin::Center | Builtin::CenterX | Builtin::CenterY => {
                let canvas = self.canvas_mut(who)?;
                let (x, y) = ((canvas.width() / 2) as f64, (canvas.height() / 2) as f64);
                match builtin {
                    Builtin::CenterX => Value::Number(x),
                    Builtin::CenterY => Value::Number(y),
                    _ => self.new_list(&[Value::Number(x), Value::Number(y)])?,
                }
            }
            Builtin::Inside => {
                let x = coordinate(args[0], who, "x")?;
                let y = coordinate(args[1], who, "y")?;
                let canvas = self.canvas_mut(who)?;
                let inside = (0..canvas.width() as i64).contains(&x)
                    && (0..canvas.height() as i64).contains(&y);
                boolean(inside)
            }
            other => {
                return Err(format!(
                    "{}: not a console or graphics routine",
                    other.name()
                ));
            }
        };
        Ok(Some(value))
    }
}
