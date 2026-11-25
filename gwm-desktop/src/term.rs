//! A terminal view: alacritty's terminal engine (parser, grid, PTY) drawn with egui.
//! Each tab runs `gwm attach <session>`, so the session itself keeps living in tmux on its machine.

use std::collections::HashMap;
use std::sync::{Arc, mpsc};

use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg as LoopMsg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use egui::{Align2, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::theme;

#[derive(Clone)]
struct Proxy {
    ctx: egui::Context,
    tx: mpsc::Sender<TermEvent>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: TermEvent) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

struct Size {
    cols: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// What the app should do after a frame of terminal input.
#[derive(Default)]
pub struct Output {
    pub copy: Option<String>,
    pub paste_text: Option<String>,
    pub paste_image: bool,
    pub reconnect: bool,
}

pub struct Terminal {
    term: Arc<FairMutex<Term<Proxy>>>,
    notifier: Notifier,
    events: mpsc::Receiver<TermEvent>,
    cols: usize,
    lines: usize,
    pub font_size: f32,
    pub title: Option<String>,
    pub exited: bool,
    scroll_acc: f32,
    mouse_down: bool,
}

impl Terminal {
    pub fn spawn(program: &str, args: &[String], ctx: &egui::Context) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let proxy = Proxy { ctx: ctx.clone(), tx };
        let (cols, lines) = (100, 30);
        let term = Term::new(Config::default(), &Size { cols, lines }, proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let mut env = HashMap::new();
        env.insert("TERM".to_owned(), "xterm-256color".to_owned());
        env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
        env.insert("TERM_PROGRAM".to_owned(), "gwm-desktop".to_owned());
        if std::env::var("LANG").is_err() {
            env.insert("LANG".to_owned(), "en_US.UTF-8".to_owned());
        }
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.to_owned(), args.to_vec())),
            working_directory: std::env::var("HOME").ok().map(Into::into),
            drain_on_exit: false,
            env,
        };
        let size = WindowSize { num_lines: lines as u16, num_cols: cols as u16, cell_width: 8, cell_height: 16 };
        let pty = tty::new(&options, size, 0)?;
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();
        Ok(Self {
            term,
            notifier,
            events: rx,
            cols,
            lines,
            font_size: 13.0,
            title: None,
            exited: false,
            scroll_acc: 0.0,
            mouse_down: false,
        })
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        let bytes: Vec<u8> = bytes.into();
        if !bytes.is_empty() {
            self.notifier.notify(bytes);
        }
    }

    /// Type text the way a paste does (bracketed, so Claude Code treats it as one paste).
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.write(format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")));
        } else {
            self.write(text);
        }
    }

    fn snap_to_bottom(&self) {
        let mut t = self.term.lock();
        t.selection = None;
        if t.grid().display_offset() != 0 {
            t.scroll_display(Scroll::Bottom);
        }
    }

    fn drain_events(&mut self, out: &mut Output) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                TermEvent::Title(t) => self.title = Some(t),
                TermEvent::ResetTitle => self.title = None,
                TermEvent::PtyWrite(s) => self.write(s),
                TermEvent::ClipboardStore(_, s) => out.copy = Some(s),
                TermEvent::ClipboardLoad(_, f) => {
                    let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
                    self.write(f(&text));
                }
                TermEvent::ColorRequest(i, f) => {
                    let c = self.palette_color(i);
                    self.write(f(Rgb { r: c.r(), g: c.g(), b: c.b() }));
                }
                TermEvent::TextAreaSizeRequest(f) => self.write(f(self.window_size(vec2(8.0, 16.0)))),
                TermEvent::ChildExit(_) | TermEvent::Exit => self.exited = true,
                _ => {}
            }
        }
    }

    fn window_size(&self, cell: Vec2) -> WindowSize {
        WindowSize {
            num_lines: self.lines as u16,
            num_cols: self.cols as u16,
            cell_width: cell.x as u16,
            cell_height: cell.y as u16,
        }
    }

    fn palette_color(&self, i: usize) -> Color32 {
        match i {
            0..=255 => theme::ansi256(i as u8),
            256 => theme::TEXT,
            257 => theme::EDITOR,
            _ => theme::ACCENT,
        }
    }

    /// Draw the terminal into the rest of `ui` and, when `focused`, feed it the keyboard and mouse.
    pub fn ui(&mut self, ui: &mut egui::Ui, focused: bool) -> (egui::Response, Output) {
        let mut out = Output::default();
        self.drain_events(&mut out);

        let font = theme::mono(self.font_size);
        let (cw, ch) = ui.ctx().fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font).ceil()));
        let cell = vec2(cw, ch);
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let pad = vec2(10.0, 6.0);
        let origin = rect.min + pad;
        let cols = (((rect.width() - 2.0 * pad.x) / cw).floor() as usize).max(2);
        let lines = (((rect.height() - 2.0 * pad.y) / ch).floor() as usize).max(1);
        if (cols, lines) != (self.cols, self.lines) {
            self.cols = cols;
            self.lines = lines;
            self.term.lock().resize(Size { cols, lines });
            self.notifier.on_resize(self.window_size(cell));
        }

        if focused {
            self.handle_input(ui, &resp, origin, cell, &mut out);
        }
        self.paint(ui, rect, origin, cell, &font, focused);
        (resp, out)
    }

    fn cell_at(&self, pos: Pos2, origin: Pos2, cell: Vec2, offset: usize) -> (Point, Side) {
        let x = ((pos.x - origin.x) / cell.x).max(0.0);
        let y = ((pos.y - origin.y) / cell.y).max(0.0);
        let col = (x.floor() as usize).min(self.cols.saturating_sub(1));
        let row = (y.floor() as usize).min(self.lines.saturating_sub(1));
        let side = if x.fract() > 0.5 { Side::Right } else { Side::Left };
        (Point::new(Line(row as i32 - offset as i32), Column(col)), side)
    }

    fn handle_input(&mut self, ui: &egui::Ui, resp: &egui::Response, origin: Pos2, cell: Vec2, out: &mut Output) {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        let mode = *self.term.lock().mode();
        let app_cursor = mode.contains(TermMode::APP_CURSOR);
        let mut skip_text = false;
        for ev in events {
            match ev {
                Event::Key { key, pressed: true, modifiers, .. } => {
                    skip_text = false;
                    if self.exited {
                        if key == Key::Enter {
                            out.reconnect = true;
                        }
                        continue;
                    }
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        self.snap_to_bottom();
                        self.write(bytes);
                        // The same key press may also arrive as text (e.g. ⌥P → "π"); we've sent it already.
                        skip_text = modifiers.alt || modifiers.ctrl;
                    }
                }
                Event::Text(t) => {
                    if skip_text || mods.mac_cmd || mods.ctrl || self.exited {
                        skip_text = false;
                        continue;
                    }
                    self.snap_to_bottom();
                    self.write(t);
                }
                Event::Ime(egui::ImeEvent::Commit(t)) => {
                    self.snap_to_bottom();
                    self.write(t);
                }
                Event::Paste(t) => out.paste_text = Some(t),
                Event::PasteImage(_) => out.paste_image = true,
                Event::Copy => {
                    if let Some(s) = self.term.lock().selection_to_string() {
                        out.copy = Some(s);
                    }
                }
                Event::MouseWheel { unit, delta, .. } => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Point => delta.y / cell.y,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * self.lines as f32,
                    };
                    if resp.hovered() {
                        self.scroll(lines, mode, resp.hover_pos(), origin, cell);
                    }
                }
                _ => {}
            }
        }

        // Mouse: apps that asked for the mouse (mouse mode) get it, unless Shift is held; otherwise select text.
        let mouse_mode = mode.intersects(TermMode::MOUSE_MODE) && !mods.shift;
        let offset = self.term.lock().grid().display_offset();
        if mouse_mode {
            let sgr = mode.contains(TermMode::SGR_MOUSE);
            if let Some(pos) = resp.interact_pointer_pos() {
                let (p, _) = self.cell_at(pos, origin, cell, 0);
                let (c, r) = (p.column.0 + 1, p.line.0 as usize + 1);
                if resp.drag_started() || (resp.is_pointer_button_down_on() && !self.mouse_down) {
                    self.mouse_down = true;
                    self.write(mouse_report(sgr, 0, c, r, true));
                } else if resp.dragged() && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                    self.write(mouse_report(sgr, 32, c, r, true));
                }
            }
            if self.mouse_down && !ui.input(|i| i.pointer.primary_down()) {
                self.mouse_down = false;
                if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                    let (p, _) = self.cell_at(pos, origin, cell, 0);
                    self.write(mouse_report(sgr, 0, p.column.0 + 1, p.line.0 as usize + 1, false));
                }
            }
        } else if let Some(pos) = resp.interact_pointer_pos() {
            let (p, side) = self.cell_at(pos, origin, cell, offset);
            let mut t = self.term.lock();
            if resp.double_clicked() {
                t.selection = Some(Selection::new(SelectionType::Semantic, p, side));
            } else if resp.drag_started() {
                t.selection = Some(Selection::new(SelectionType::Simple, p, side));
            } else if resp.dragged() {
                if let Some(s) = t.selection.as_mut() {
                    s.update(p, side);
                }
            } else if resp.clicked() {
                t.selection = None;
            }
        }
    }

    fn scroll(&mut self, lines: f32, mode: TermMode, hover: Option<Pos2>, origin: Pos2, cell: Vec2) {
        self.scroll_acc += lines;
        let n = self.scroll_acc.trunc() as i32;
        if n == 0 {
            return;
        }
        self.scroll_acc -= n as f32;
        if mode.intersects(TermMode::MOUSE_MODE) {
            let (p, _) = self.cell_at(hover.unwrap_or(origin), origin, cell, 0);
            let button = if n > 0 { 64 } else { 65 };
            for _ in 0..n.abs() {
                self.write(mouse_report(mode.contains(TermMode::SGR_MOUSE), button, p.column.0 + 1, p.line.0 as usize + 1, true));
            }
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let key: &[u8] = match (n > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.write(key.repeat(n.unsigned_abs() as usize));
        } else {
            self.term.lock().scroll_display(Scroll::Delta(n));
        }
    }

    fn paint(&self, ui: &egui::Ui, rect: Rect, origin: Pos2, cell: Vec2, font: &egui::FontId, focused: bool) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::EDITOR);
        let t = self.term.lock();
        let content = t.renderable_content();
        let offset = content.display_offset as i32;
        let colors = content.colors;
        let selection = content.selection;
        let cursor = content.cursor;

        struct Run {
            x: f32,
            y: f32,
            text: String,
            fg: Color32,
            underline: bool,
            strike: bool,
            next_col: usize,
            row: i32,
        }
        let flush = |run: &mut Option<Run>| {
            if let Some(r) = run.take() {
                let trimmed = r.text.trim_end();
                if !trimmed.is_empty() {
                    painter.text(pos2(r.x, r.y), Align2::LEFT_TOP, trimmed, font.clone(), r.fg);
                }
                let w = r.text.chars().count() as f32 * cell.x;
                if r.underline {
                    painter.line_segment([pos2(r.x, r.y + cell.y - 1.5), pos2(r.x + w, r.y + cell.y - 1.5)], Stroke::new(1.0, r.fg));
                }
                if r.strike {
                    painter.line_segment([pos2(r.x, r.y + cell.y / 2.0), pos2(r.x + w, r.y + cell.y / 2.0)], Stroke::new(1.0, r.fg));
                }
            }
        };

        let mut run: Option<Run> = None;
        for ind in content.display_iter {
            let c = ind.cell;
            if c.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let row = ind.point.line.0 + offset;
            let col = ind.point.column.0;
            let mut fg = resolve(c.fg, colors, true);
            let mut bg = resolve(c.bg, colors, false);
            if c.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if c.flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.66);
            }
            if c.flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            if selection.is_some_and(|s| s.contains(ind.point)) {
                bg = theme::SELECTION;
            }
            let width = if c.flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
            let x = origin.x + col as f32 * cell.x;
            let y = origin.y + row as f32 * cell.y;
            if bg != theme::EDITOR {
                painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cell.x * width + 0.5, cell.y + 0.5)), 0.0, bg);
            }
            let underline = c.flags.intersects(Flags::ALL_UNDERLINES);
            let strike = c.flags.contains(Flags::STRIKEOUT);
            let ch = c.c;
            if ch.is_ascii() && width == 1.0 && c.zerowidth().is_none() {
                let ch = if ch == '\t' || ch == '\0' { ' ' } else { ch };
                let continues = run.as_ref().is_some_and(|r| {
                    r.row == row && r.next_col == col && r.fg == fg && r.underline == underline && r.strike == strike
                });
                if !continues {
                    flush(&mut run);
                    run = Some(Run { x, y, text: String::new(), fg, underline, strike, next_col: col, row });
                }
                if let Some(r) = run.as_mut() {
                    r.text.push(ch);
                    r.next_col += 1;
                }
            } else {
                flush(&mut run);
                let mut s = String::from(ch);
                if let Some(zw) = c.zerowidth() {
                    s.extend(zw.iter());
                }
                painter.text(pos2(x + cell.x * width / 2.0, y + cell.y / 2.0), Align2::CENTER_CENTER, s, font.clone(), fg);
                if underline {
                    painter.line_segment([pos2(x, y + cell.y - 1.5), pos2(x + cell.x * width, y + cell.y - 1.5)], Stroke::new(1.0, fg));
                }
            }
        }
        flush(&mut run);

        // cursor
        let row = cursor.point.line.0 + offset;
        if cursor.shape != CursorShape::Hidden && row >= 0 && (row as usize) < self.lines && !self.exited {
            let x = origin.x + cursor.point.column.0 as f32 * cell.x;
            let y = origin.y + row as f32 * cell.y;
            let r = Rect::from_min_size(pos2(x, y), cell);
            match (cursor.shape, focused) {
                (CursorShape::Beam, _) => {
                    painter.rect_filled(Rect::from_min_size(r.min, vec2(2.0, cell.y)), 0.0, theme::ACCENT);
                }
                (CursorShape::Underline, _) => {
                    painter.rect_filled(Rect::from_min_size(pos2(x, y + cell.y - 2.0), vec2(cell.x, 2.0)), 0.0, theme::ACCENT);
                }
                (_, true) => {
                    painter.rect_filled(r, 1.0, theme::ACCENT);
                    let under = t.grid()[cursor.point].c;
                    if under != ' ' && under != '\0' {
                        painter.text(r.center(), Align2::CENTER_CENTER, under, font.clone(), theme::EDITOR);
                    }
                }
                (_, false) => {
                    painter.rect_stroke(r, 1.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
                }
            }
        }
        drop(t);

        if self.exited {
            let msg = "Disconnected — the session keeps running on its machine. Press Enter to reconnect.";
            let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 30.0), vec2(rect.width().min(620.0), 28.0));
            painter.rect_filled(r, 6.0, theme::ELEVATED);
            painter.rect_stroke(r, 6.0, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
            painter.text(r.center(), Align2::CENTER_CENTER, msg, egui::FontId::proportional(12.5), theme::MUTED);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(LoopMsg::Shutdown);
    }
}

fn resolve(color: Color, colors: &Colors, fg: bool) -> Color32 {
    let from_rgb = |c: Rgb| Color32::from_rgb(c.r, c.g, c.b);
    match color {
        Color::Spec(rgb) => from_rgb(rgb),
        Color::Indexed(i) => colors[i as usize].map(from_rgb).unwrap_or_else(|| theme::ansi256(i)),
        Color::Named(n) => {
            if let Some(c) = colors[n as usize] {
                return from_rgb(c);
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground => theme::TEXT,
                NamedColor::Background => theme::EDITOR,
                NamedColor::Cursor => theme::ACCENT,
                NamedColor::DimForeground => theme::MUTED,
                n if (n as usize) < 16 => theme::ANSI[n as usize],
                n => {
                    // Dim variants (DimBlack…DimWhite) follow Cursor in the enum
                    let i = (n as usize).saturating_sub(NamedColor::DimBlack as usize).min(7);
                    if fg { theme::ANSI[i].gamma_multiply(0.7) } else { theme::ANSI[i] }
                }
            }
        }
    }
}

fn mouse_report(sgr: bool, button: u8, col: usize, row: usize, pressed: bool) -> Vec<u8> {
    if sgr {
        format!("\x1b[<{button};{col};{row}{}", if pressed { 'M' } else { 'm' }).into_bytes()
    } else {
        let b = if pressed { button } else { 3 };
        vec![0x1b, b'[', b'M', 32 + b, (32 + col.min(222)) as u8, (32 + row.min(222)) as u8]
    }
}

/// The bytes a key press sends (xterm conventions). Plain printable keys come through as text events instead.
fn key_bytes(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let esc = |s: &str| Some(s.as_bytes().to_vec());
    let arrow = |c: char| -> Option<Vec<u8>> {
        let modcode = 1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
        if modcode > 1 {
            Some(format!("\x1b[1;{modcode}{c}").into_bytes())
        } else if app_cursor {
            Some(format!("\x1bO{c}").into_bytes())
        } else {
            Some(format!("\x1b[{c}").into_bytes())
        }
    };
    if m.mac_cmd {
        // ⌘ is the app's; a few line-editing combos behave like other Mac terminals
        return match key {
            Key::Backspace => esc("\x15"),
            Key::ArrowLeft => esc("\x01"),
            Key::ArrowRight => esc("\x05"),
            _ => None,
        };
    }
    match key {
        Key::Enter if m.shift || m.alt => esc("\x1b\r"), // newline in Claude Code's prompt
        Key::Enter => esc("\r"),
        Key::Tab if m.shift => esc("\x1b[Z"),
        Key::Tab => esc("\t"),
        Key::Backspace if m.alt => esc("\x1b\x7f"),
        Key::Backspace if m.ctrl => esc("\x08"),
        Key::Backspace => esc("\x7f"),
        Key::Escape => esc("\x1b"),
        Key::ArrowLeft if m.alt && !m.shift && !m.ctrl => esc("\x1bb"),
        Key::ArrowRight if m.alt && !m.shift && !m.ctrl => esc("\x1bf"),
        Key::ArrowUp => arrow('A'),
        Key::ArrowDown => arrow('B'),
        Key::ArrowRight => arrow('C'),
        Key::ArrowLeft => arrow('D'),
        Key::Home => esc(if app_cursor { "\x1bOH" } else { "\x1b[H" }),
        Key::End => esc(if app_cursor { "\x1bOF" } else { "\x1b[F" }),
        Key::PageUp => esc("\x1b[5~"),
        Key::PageDown => esc("\x1b[6~"),
        Key::Delete => esc("\x1b[3~"),
        Key::Insert => esc("\x1b[2~"),
        Key::F1 => esc("\x1bOP"),
        Key::F2 => esc("\x1bOQ"),
        Key::F3 => esc("\x1bOR"),
        Key::F4 => esc("\x1bOS"),
        Key::F5 => esc("\x1b[15~"),
        Key::F6 => esc("\x1b[17~"),
        Key::F7 => esc("\x1b[18~"),
        Key::F8 => esc("\x1b[19~"),
        Key::F9 => esc("\x1b[20~"),
        Key::F10 => esc("\x1b[21~"),
        Key::F11 => esc("\x1b[23~"),
        Key::F12 => esc("\x1b[24~"),
        _ => {
            let c = key_char(key, m.shift)?;
            if m.ctrl {
                let b = match c.to_ascii_lowercase() {
                    l @ 'a'..='z' => l as u8 & 0x1f,
                    ' ' | '2' | '@' => 0,
                    '[' | '3' => 0x1b,
                    '\\' | '4' => 0x1c,
                    ']' | '5' => 0x1d,
                    '6' | '^' => 0x1e,
                    '/' | '-' | '_' | '7' => 0x1f,
                    _ => return None,
                };
                Some(if m.alt { vec![0x1b, b] } else { vec![b] })
            } else if m.alt {
                // ⌥ as Meta (like "Use Option as Meta key"): Claude Code's ⌥P, ⌥T… work
                let mut v = vec![0x1b];
                v.extend(c.to_string().as_bytes());
                Some(v)
            } else {
                None // plain printable keys arrive as Event::Text
            }
        }
    }
}

fn key_char(key: Key, shift: bool) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Some(if shift { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() });
        }
        if c.is_ascii_digit() {
            return Some(c);
        }
    }
    Some(match key {
        Key::Space => ' ',
        Key::Minus => '-',
        Key::Period => '.',
        Key::Comma => ',',
        Key::Slash => '/',
        Key::Backslash => '\\',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::Semicolon => ';',
        Key::Quote => '\'',
        Key::Backtick => '`',
        Key::Equals => '=',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(alt: bool, ctrl: bool, shift: bool) -> Modifiers {
        Modifiers { alt, ctrl, shift, mac_cmd: false, command: false }
    }

    #[test]
    fn keys() {
        assert_eq!(key_bytes(Key::Enter, m(false, false, false), false).unwrap(), b"\r");
        assert_eq!(key_bytes(Key::Enter, m(false, false, true), false).unwrap(), b"\x1b\r");
        assert_eq!(key_bytes(Key::Tab, m(false, false, true), false).unwrap(), b"\x1b[Z");
        assert_eq!(key_bytes(Key::C, m(false, true, false), false).unwrap(), vec![3]);
        assert_eq!(key_bytes(Key::P, m(true, false, false), false).unwrap(), b"\x1bp");
        assert_eq!(key_bytes(Key::ArrowUp, m(false, false, false), true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(Key::ArrowUp, m(false, false, true), false).unwrap(), b"\x1b[1;2A");
        assert!(key_bytes(Key::A, m(false, false, false), false).is_none());
    }

    #[test]
    fn mouse() {
        assert_eq!(mouse_report(true, 64, 3, 4, true), b"\x1b[<64;3;4M");
        assert_eq!(mouse_report(true, 0, 1, 1, false), b"\x1b[<0;1;1m");
    }
}
