//! The window: title bar, sidebar (machines → sessions, tailnet), tabs of terminals, status bar,
//! command palette (⌘K / ⌘⇧P) and the New Session dialog. Laid out after Zed.
//!
//! Design rules (from Mobbin references — Devin, Better Stack, Plane, Neon, Replit, Square):
//!   * hierarchy: two-line rows — the name is primary, a small coloured status line is secondary
//!   * grouping: sessions sit under their machine; a machine's header carries its count and load,
//!     and "+ new session" lives inside the group it creates into
//!   * spacing: rows in a group are tight; unrelated sections (overview / machines / tailnet) get clear gaps
//!   * one filled primary action per surface; everything else is quiet until hovered
//!   * optimistic updates: a new session appears at once (row + tab) in a pending state and settles in
//!     place; the browser toggle flips immediately and only reverts if the command fails

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke,
    TextEdit, Ui, pos2, vec2,
};

use crate::gwm::{self, Msg, Peer, Session, State};
use crate::term::Terminal;
use crate::theme;

const COORD: &str = "coordinator";

struct Tab {
    key: String, // session name, or "coordinator"
    term: Option<Terminal>,
    error: Option<String>,
    pending: Option<u64>, // being created: the job that creates it
}

enum JobKind {
    NewSession(String),
    EndSession(String),
    Send,
    Browser,
}

struct Job {
    kind: JobKind,
    title: String,
    lines: Vec<String>,
    done: Option<bool>,
}

struct Toast {
    text: String,
    ok: bool,
    at: Instant,
}

#[derive(Default)]
struct Palette {
    query: String,
    selected: usize,
}

#[derive(Clone)]
struct NewSession {
    machine: String, // a saved machine's name, or a tailnet DNS name
    engine: String,
    account: String,
    folder: String,
    name: String,
    name_edited: bool,
    user: String,
    more: bool,
}

/// A session that's being created: shown right away, settled when gwm finishes.
struct Pending {
    form: NewSession,
    job: u64,
    failed: bool,
}

#[derive(Clone)]
enum Action {
    Open(String),
    Activate(usize),
    Close(usize),
    NewSession(Option<String>),
    Retry(String),
    Palette,
    PasteClipboard(String),
    PasteLast(String),
    Share(Vec<PathBuf>, String),
    Browser(bool),
    ToggleSidebar,
    Font(f32),
    Reconnect(usize),
    EndSession(String),    // ask first
    ConfirmEnd(String),    // stop it on its machine (gwm close --yes)
}

pub struct App {
    state: State,
    peers: Vec<Peer>,
    browser: bool,
    browser_pending: Option<bool>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    poll_wake: Sender<()>,
    tabs: Vec<Tab>,
    active: usize,
    sidebar: bool,
    collapsed: HashSet<String>,
    palette: Option<Palette>,
    dialog: Option<NewSession>,
    confirm_end: Option<String>, // session whose "End session?" dialog is open
    ending: HashSet<String>,     // sessions being ended right now
    pending: Vec<Pending>,
    jobs: HashMap<u64, Job>,
    next_job: u64,
    toasts: Vec<Toast>,
    font_size: f32,
    screenshot: Option<PathBuf>,
    screenshot_done: Arc<AtomicBool>,
    started: Instant,
    actions: Vec<Action>,
}

impl App {
    pub fn new(ctx: &egui::Context, open: Vec<String>, screenshot: Option<PathBuf>, show: Option<String>) -> Self {
        let (tx, rx) = channel();
        let poll_wake = gwm::spawn_poller(tx.clone(), ctx.clone());
        let mut app = Self {
            state: State::default(),
            peers: Vec::new(),
            browser: false,
            browser_pending: None,
            tx,
            rx,
            poll_wake,
            tabs: Vec::new(),
            active: 0,
            sidebar: true,
            collapsed: HashSet::new(),
            palette: None,
            dialog: None,
            confirm_end: None,
            ending: HashSet::new(),
            pending: Vec::new(),
            jobs: HashMap::new(),
            next_job: 1,
            toasts: Vec::new(),
            font_size: 13.0,
            screenshot,
            screenshot_done: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            actions: Vec::new(),
        };
        app.open_tab(ctx, COORD);
        for s in open {
            app.open_tab(ctx, &s);
        }
        match show.as_deref() {
            Some("palette") => app.palette = Some(Palette::default()),
            Some("dialog") => app.open_dialog(None),
            Some(x) if x.starts_with("end:") => app.confirm_end = Some(x[4..].to_owned()),
            _ => {}
        }
        app
    }

    // ---------- tabs ----------
    fn open_tab(&mut self, ctx: &egui::Context, key: &str) {
        if let Some(i) = self.tabs.iter().position(|t| t.key == key) {
            self.active = i;
            return;
        }
        let mut tab = Tab { key: key.to_owned(), term: None, error: None, pending: None };
        self.spawn_term(ctx, &mut tab);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    fn spawn_term(&self, ctx: &egui::Context, tab: &mut Tab) {
        let args: Vec<String> = if tab.key == COORD {
            vec!["attach-coordinator".into()]
        } else {
            vec!["attach".into(), tab.key.clone()]
        };
        match Terminal::spawn(&gwm::gwm_path().to_string_lossy(), &args, ctx) {
            Ok(mut t) => {
                t.font_size = self.font_size;
                tab.term = Some(t);
                tab.error = None;
            }
            Err(e) => tab.error = Some(format!("couldn't start a terminal: {e}")),
        }
    }

    fn active_key(&self) -> Option<String> {
        self.tabs.get(self.active).map(|t| t.key.clone())
    }

    fn host_of(&self, key: &str) -> String {
        if key == COORD {
            return self.state.coordinator_host.clone();
        }
        if let Some(s) = self.state.session(key) {
            return s.host.clone();
        }
        self.pending.iter().find(|p| p.form.name == key).map(|p| short(&p.form.machine).to_owned()).unwrap_or_default()
    }

    fn label_of(&self, key: &str) -> String {
        if key == COORD { "overview".into() } else { key.into() }
    }

    fn icon_of(&self, key: &str) -> &'static str {
        if key == COORD {
            return "✻";
        }
        if let Some(s) = self.state.session(key) {
            return s.icon();
        }
        self.pending.iter().find(|p| p.form.name == key).map(|p| engine_icon(&p.form.engine)).unwrap_or("›")
    }

    // ---------- background jobs ----------
    fn job(&mut self, ctx: &egui::Context, kind: JobKind, title: impl Into<String>, args: Vec<String>) -> u64 {
        let id = self.next_job;
        self.next_job += 1;
        self.jobs.insert(id, Job { kind, title: title.into(), lines: Vec::new(), done: None });
        gwm::run_job(id, args, self.tx.clone(), ctx.clone());
        id
    }

    fn toast(&mut self, text: impl Into<String>, ok: bool) {
        self.toasts.push(Toast { text: text.into(), ok, at: Instant::now() });
    }

    fn refresh_now(&self) {
        let _ = self.poll_wake.send(());
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::State(s) => self.state = s,
                Msg::Tailnet(p) => self.peers = p,
                Msg::Browser(b) => {
                    if self.browser_pending.is_none() {
                        self.browser = b; // while a toggle is in flight, keep showing where it's going
                    }
                }
                Msg::JobLine(id, line) => {
                    if let Some(j) = self.jobs.get_mut(&id) {
                        j.lines.push(line);
                    }
                }
                Msg::JobDone(id, ok, last) => {
                    let Some(j) = self.jobs.get_mut(&id) else { continue };
                    j.done = Some(ok);
                    let title = j.title.clone();
                    match &j.kind {
                        JobKind::NewSession(name) => {
                            let name = name.clone();
                            if ok {
                                self.pending.retain(|p| p.job != id);
                                self.refresh_now();
                                // settle the optimistic tab into a live terminal
                                if let Some(i) = self.tabs.iter().position(|t| t.key == name) {
                                    let mut tab = self.tabs.remove(i);
                                    tab.pending = None;
                                    self.spawn_term(ctx, &mut tab);
                                    self.tabs.insert(i, tab);
                                }
                                self.toast(format!("'{name}' is ready"), true);
                            } else {
                                if let Some(p) = self.pending.iter_mut().find(|p| p.job == id) {
                                    p.failed = true;
                                }
                                self.toast(format!("Couldn't start '{name}': {last}"), false);
                            }
                        }
                        JobKind::EndSession(name) => {
                            let name = name.clone();
                            self.ending.remove(&name);
                            self.refresh_now();
                            if ok {
                                self.toast(format!("Ended '{name}' (its screen is in the archive)"), true);
                            } else {
                                self.toast(format!("Couldn't end '{name}': {last}"), false);
                            }
                        }
                        JobKind::Send => {
                            let msg = if last.is_empty() { title } else { last };
                            self.toast(msg, ok);
                        }
                        JobKind::Browser => {
                            self.browser_pending = None;
                            self.browser = gwm::browser_up(); // reconcile with what actually happened
                            if !ok {
                                self.toast(if last.is_empty() { "The browser command failed".to_owned() } else { last }, false);
                            }
                        }
                    }
                }
            }
        }
        // Drop completed jobs that aren't attached to a pending/failed tab so memory stays bounded.
        let tabs = &self.tabs;
        self.jobs.retain(|id, j| j.done.is_none() || tabs.iter().any(|t| t.pending == Some(*id)));
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(6));
    }

    // ---------- actions ----------
    fn run_actions(&mut self, ctx: &egui::Context) {
        for a in std::mem::take(&mut self.actions) {
            match a {
                Action::Open(k) => self.open_tab(ctx, &k),
                Action::Activate(i) => self.active = i.min(self.tabs.len().saturating_sub(1)),
                Action::Close(i) => {
                    if i < self.tabs.len() {
                        let tab = self.tabs.remove(i); // disconnects; the session keeps running on its machine
                        // closing a failed pending session's tab discards it
                        self.pending.retain(|p| !(p.failed && p.form.name == tab.key));
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        } else if i < self.active {
                            self.active -= 1;
                        }
                    }
                }
                Action::NewSession(machine) => self.open_dialog(machine),
                Action::Retry(name) => {
                    if let Some(i) = self.pending.iter().position(|p| p.form.name == name) {
                        let p = self.pending.remove(i);
                        if let Some(t) = self.tabs.iter().position(|t| t.key == name) {
                            self.tabs.remove(t);
                            self.active = self.active.min(self.tabs.len().saturating_sub(1));
                        }
                        self.dialog = Some(p.form);
                    }
                }
                Action::Palette => self.palette = Some(Palette::default()),
                Action::PasteClipboard(k) => {
                    let host = self.host_of(&k);
                    self.job(ctx, JobKind::Send, format!("Sending the clipboard to {host}…"), vec!["paste".into(), k]);
                }
                Action::PasteLast(k) => {
                    let host = self.host_of(&k);
                    self.job(ctx, JobKind::Send, format!("Sending your newest screenshot to {host}…"), vec!["paste".into(), k, "--last".into()]);
                }
                Action::Share(paths, k) => {
                    let host = self.host_of(&k);
                    let mut args = vec!["share".to_owned()];
                    args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
                    args.push(k);
                    self.job(ctx, JobKind::Send, format!("Copying {} file(s) to {host}…", paths.len()), args);
                }
                Action::Browser(on) => {
                    // optimistic: show the new state now; JobDone reconciles (and reverts on failure)
                    self.browser_pending = Some(on);
                    self.browser = on;
                    let (t, a) = if on { ("Opening the shared browser…", "start") } else { ("Closing the shared browser…", "stop") };
                    self.job(ctx, JobKind::Browser, t, vec!["chrome".into(), a.into()]);
                }
                Action::ToggleSidebar => self.sidebar = !self.sidebar,
                Action::Font(d) => {
                    self.font_size = (self.font_size + d).clamp(9.0, 24.0);
                    for t in self.tabs.iter_mut().filter_map(|t| t.term.as_mut()) {
                        t.font_size = self.font_size;
                    }
                }
                Action::EndSession(name) => {
                    // only saved sessions (not the coordinator, not one still being created)
                    if self.state.session(&name).is_some() && !self.ending.contains(&name) {
                        self.confirm_end = Some(name);
                    }
                }
                Action::ConfirmEnd(name) => {
                    // optimistic: drop its tab now and show it as ending; JobDone refreshes the list
                    if let Some(i) = self.tabs.iter().position(|t| t.key == name) {
                        self.tabs.remove(i);
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        } else if i < self.active {
                            self.active -= 1;
                        }
                    }
                    self.ending.insert(name.clone());
                    let host = self.host_of(&name);
                    self.job(ctx, JobKind::EndSession(name.clone()), format!("Ending '{name}' on {host}…"), vec!["close".into(), name, "--yes".into()]);
                }
                Action::Reconnect(i) => {
                    if let Some(mut tab) = (i < self.tabs.len()).then(|| self.tabs.remove(i)) {
                        tab.term = None;
                        self.spawn_term(ctx, &mut tab);
                        self.tabs.insert(i, tab);
                    }
                }
            }
        }
    }

    fn open_dialog(&mut self, machine: Option<String>) {
        let machine = machine.unwrap_or_else(|| {
            self.active_key().map(|k| self.host_of(&k)).filter(|h| !h.is_empty()).unwrap_or_else(|| self.state.me.clone())
        });
        let mut d = NewSession {
            machine,
            engine: "claude".into(),
            account: "default".into(),
            folder: "~".into(),
            name: String::new(),
            name_edited: false,
            user: std::env::var("USER").unwrap_or_default(),
            more: false,
        };
        d.name = self.suggest_name(&d);
        self.dialog = Some(d);
    }

    fn suggest_name(&self, d: &NewSession) -> String {
        let m: String = short(&d.machine).to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let base = format!("{m}-{}", d.engine);
        let taken = |n: &str| self.state.sessions.iter().any(|s| s.name == n) || self.pending.iter().any(|p| p.form.name == n);
        if !taken(&base) {
            return base;
        }
        (2..).map(|i| format!("{base}-{i}")).find(|n| !taken(n)).unwrap_or(base)
    }

    /// Create a session optimistically: its row and tab appear now; gwm does the work in the background.
    fn create_session(&mut self, ctx: &egui::Context, d: NewSession) {
        let is_peer = self.peers.iter().any(|p| p.dns == d.machine && p.saved_as.is_empty());
        let name = d.name.trim().to_owned();
        let mut args = vec![
            "new".into(), name.clone(), "--on".into(), d.machine.clone(), "--engine".into(), d.engine.trim().into(),
            "--folder".into(), d.folder.trim().into(), "--account".into(),
            if d.engine.trim() == "claude" { d.account.trim().into() } else { "-".into() },
        ];
        if is_peer && !d.user.trim().is_empty() {
            args.extend(["--user".into(), d.user.trim().into()]);
        }
        let title = if is_peer {
            format!("Setting up {} and starting '{name}'…", short(&d.machine))
        } else {
            format!("Starting '{name}' on {}…", d.machine)
        };
        let job = self.job(ctx, JobKind::NewSession(name.clone()), title, args);
        self.pending.push(Pending { form: d, job, failed: false });
        self.tabs.retain(|t| t.key != name);
        self.tabs.push(Tab { key: name, term: None, error: None, pending: Some(job) });
        self.active = self.tabs.len() - 1;
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        ctx.input_mut(|i| {
            if i.consume_key(cmd, Key::K) || i.consume_key(cmd_shift, Key::P) {
                self.actions.push(Action::Palette);
            }
            if i.consume_key(cmd, Key::T) || i.consume_key(cmd, Key::N) {
                self.actions.push(Action::NewSession(None));
            }
            if i.consume_key(cmd_shift, Key::W) {
                if let Some(k) = self.tabs.get(self.active).map(|t| t.key.clone()) {
                    self.actions.push(Action::EndSession(k));
                }
            }
            if i.consume_key(cmd, Key::W) {
                self.actions.push(Action::Close(self.active));
            }
            if i.consume_key(cmd, Key::B) {
                self.actions.push(Action::ToggleSidebar);
            }
            if i.consume_key(cmd, Key::Equals) || i.consume_key(cmd, Key::Plus) {
                self.actions.push(Action::Font(1.0));
            }
            if i.consume_key(cmd, Key::Minus) {
                self.actions.push(Action::Font(-1.0));
            }
            if i.consume_key(cmd_shift, Key::CloseBracket) && !self.tabs.is_empty() {
                self.actions.push(Action::Activate((self.active + 1) % self.tabs.len()));
            }
            if i.consume_key(cmd_shift, Key::OpenBracket) && !self.tabs.is_empty() {
                self.actions.push(Action::Activate((self.active + self.tabs.len() - 1) % self.tabs.len()));
            }
            if let Some(k) = self.tabs.get(self.active).map(|t| t.key.clone()) {
                if i.consume_key(cmd_shift, Key::V) {
                    self.actions.push(Action::PasteLast(k));
                }
            }
            let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
            for (n, k) in nums.iter().enumerate() {
                if i.consume_key(cmd, *k) {
                    self.actions.push(Action::Activate(if n == 8 { usize::MAX } else { n }));
                }
            }
        });
    }

    // ---------- title bar: breadcrumb on the left, one primary action on the right ----------
    fn title_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let bg = ui.interact(rect, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let p = ui.painter();
        p.rect_filled(rect, 0.0, theme::BG);
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::BORDER_SOFT));
        // breadcrumb: machine › session (leaves room for the traffic lights)
        let y = rect.center().y;
        let mut x = rect.left() + 84.0;
        if let Some(k) = self.active_key() {
            let host = self.host_of(&k);
            if !host.is_empty() {
                x = p.text(pos2(x, y), Align2::LEFT_CENTER, &host, FontId::proportional(13.0), theme::MUTED).right() + 8.0;
                x = p.text(pos2(x, y), Align2::LEFT_CENTER, "›", FontId::proportional(13.0), theme::FAINT).right() + 8.0;
            }
            p.text(pos2(x, y), Align2::LEFT_CENTER, self.label_of(&k), FontId::proportional(13.0), theme::TEXT);
        }
        let mut right = rect.right() - 10.0;
        if pill(ui, &mut right, y, "+  New session", Pill::Primary).on_hover_text("⌘T").clicked() {
            self.actions.push(Action::NewSession(None));
        }
        if pill(ui, &mut right, y, "⌘K", Pill::Ghost(theme::MUTED)).on_hover_text("Commands").clicked() {
            self.actions.push(Action::Palette);
        }
        let (text, color, tip) = match (self.browser_pending, self.browser) {
            (Some(true), _) => ("◌  Opening browser…", theme::YELLOW, "Starting a shared Chrome window on this Mac"),
            (Some(false), _) => ("◌  Closing browser…", theme::YELLOW, "Stopping the shared browser"),
            (None, true) => ("●  Browser shared", theme::GREEN, "Claude on your machines can drive the Chrome window on this Mac. Click to stop sharing."),
            (None, false) => ("○  Browser", theme::MUTED, "Share a Chrome window on this Mac with Claude on every machine (gwm chrome)"),
        };
        let resp = pill(ui, &mut right, y, text, Pill::Ghost(color)).on_hover_text(tip);
        if resp.clicked() && self.browser_pending.is_none() {
            self.actions.push(Action::Browser(!self.browser));
        }
    }

    // ---------- sidebar: overview · machines (each a group of its sessions) · tailnet ----------
    fn sidebar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 0.0; // rows inside a group touch; sections get explicit gaps
        let active = self.active_key();

        section(ui, "OVERVIEW", 10.0);
        let coord = if self.state.coordinator_host.is_empty() { "…".to_owned() } else { format!("coordinator on {}", self.state.coordinator_host) };
        let r = session_row(ui, RowLook { icon: "✻", icon_color: theme::ACCENT, name: "overview", status: &coord, status_color: theme::FAINT, active: active.as_deref() == Some(COORD), dim: false, busy: false });
        if r.clicked() {
            self.actions.push(Action::Open(COORD.into()));
        }

        section(ui, "MACHINES", 18.0);
        if !self.state.loaded {
            note(ui, "Checking your machines…", theme::FAINT);
        }
        if let Some(e) = self.state.error.clone() {
            note(ui, &e, theme::RED);
        }
        let hosts = self.state.hosts.clone();
        for (gi, h) in hosts.iter().enumerate() {
            if gi > 0 {
                ui.add_space(10.0); // space between machine groups
            }
            let open = !self.collapsed.contains(&h.name);
            let mut sessions: Vec<Session> = self.state.sessions.iter().filter(|s| s.host == h.name).cloned().collect();
            // working first, then idle (most recent first), then not started
            sessions.sort_by_key(|s| (!s.working(), !s.running, s.idle_secs));
            let pending: Vec<(String, String, bool)> = self.pending.iter().filter(|p| p.form.machine == h.name).map(|p| (p.form.name.clone(), p.form.engine.clone(), p.failed)).collect();
            let count = sessions.len() + pending.len();
            let (dot, dot_color) = match (h.online, h.needs_approval) {
                (_, true) => ("◐", theme::YELLOW),
                (Some(true), _) => ("●", theme::GREEN),
                _ => ("○", theme::FAINT),
            };
            let meta = if h.online == Some(true) {
                [h.cpu.as_str(), h.ram.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
            } else if h.needs_approval {
                "needs approval".into()
            } else {
                "offline".into()
            };
            let tag = (h.name == self.state.me).then_some("this Mac");
            let resp = group_header(ui, open, dot, dot_color, &h.name, count, &meta, tag).on_hover_text(format!(
                "{}\n{}{}",
                h.role,
                h.target,
                if h.used_by.is_empty() { String::new() } else { format!("\nalso used by {}", h.used_by) }
            ));
            if resp.clicked() {
                if open {
                    self.collapsed.insert(h.name.clone());
                } else {
                    self.collapsed.remove(&h.name);
                }
            }
            resp.context_menu(|ui| {
                if ui.button(format!("New session on {}…", h.name)).clicked() {
                    self.actions.push(Action::NewSession(Some(h.name.clone())));
                    ui.close();
                }
            });
            if !open {
                continue;
            }
            for s in &sessions {
                let ending = self.ending.contains(&s.name);
                let (status, status_color) = if ending { ("ending…".to_owned(), theme::FAINT) } else { session_status(s) };
                let r = session_row(ui, RowLook {
                    icon: s.icon(),
                    icon_color: if s.running { theme::ACCENT } else { theme::FAINT },
                    name: &s.name,
                    status: &status,
                    status_color,
                    active: active.as_deref() == Some(s.name.as_str()),
                    dim: !s.running || ending,
                    busy: ending,
                })
                .on_hover_text(format!("{} · account {} · folder {}", s.engine, s.account, s.folder));
                if r.clicked() {
                    self.actions.push(Action::Open(s.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Open").clicked() {
                        self.actions.push(Action::Open(s.name.clone()));
                        ui.close();
                    }
                    if ui.button("Paste clipboard into it").clicked() {
                        self.actions.push(Action::PasteClipboard(s.name.clone()));
                        ui.close();
                    }
                    if ui.button("Send newest screenshot").clicked() {
                        self.actions.push(Action::PasteLast(s.name.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.add_enabled(!ending, egui::Button::new(RichText::new("End session…").color(theme::RED))).clicked() {
                        self.actions.push(Action::EndSession(s.name.clone()));
                        ui.close();
                    }
                });
            }
            for (name, engine, failed) in &pending {
                self.pending_row(ui, name, engine, *failed, active.as_deref() == Some(name.as_str()));
            }
            if add_row(ui, "new session").clicked() {
                self.actions.push(Action::NewSession(Some(h.name.clone())));
            }
        }
        // machines being set up right now (picked from the tailnet): shown as groups already
        let new_machines: Vec<String> = {
            let mut v: Vec<String> = self.pending.iter().map(|p| p.form.machine.clone()).filter(|m| !hosts.iter().any(|h| &h.name == m)).collect();
            v.dedup();
            v
        };
        for m in &new_machines {
            ui.add_space(10.0);
            let failed = self.pending.iter().any(|p| &p.form.machine == m && p.failed);
            let entries: Vec<(String, String, bool)> = self.pending.iter().filter(|p| &p.form.machine == m).map(|p| (p.form.name.clone(), p.form.engine.clone(), p.failed)).collect();
            group_header(ui, true, "◌", if failed { theme::RED } else { theme::YELLOW }, short(m), entries.len(), if failed { "setup failed" } else { "setting up…" }, None);
            for (name, engine, failed) in &entries {
                self.pending_row(ui, name, engine, *failed, active.as_deref() == Some(name.as_str()));
            }
        }

        let new_peers: Vec<Peer> = self.peers.iter().filter(|p| p.saved_as.is_empty() && !new_machines.contains(&p.dns)).cloned().collect();
        if !new_peers.is_empty() {
            section(ui, "ON YOUR TAILNET", 22.0);
            note(ui, "Not set up yet — gwm installs what it needs on first use", theme::FAINT);
            ui.add_space(4.0);
            for p in &new_peers {
                let status = format!("{} · {}", p.os, if p.online { "start a session here" } else { "offline" });
                let r = session_row(ui, RowLook {
                    icon: if p.online { "+" } else { "○" },
                    icon_color: if p.online { theme::ACCENT } else { theme::FAINT },
                    name: p.short(),
                    status: &status,
                    status_color: theme::FAINT,
                    active: false,
                    dim: !p.online,
                    busy: false,
                })
                .on_hover_text(&p.dns);
                if r.clicked() && p.online {
                    self.actions.push(Action::NewSession(Some(p.dns.clone())));
                }
            }
        }
        ui.add_space(16.0);
    }

    fn pending_row(&mut self, ui: &mut Ui, name: &str, engine: &str, failed: bool, active: bool) {
        let r = session_row(ui, RowLook {
            icon: engine_icon(engine),
            icon_color: if failed { theme::RED } else { theme::ACCENT },
            name,
            status: if failed { "couldn't start — click for details" } else { "starting…" },
            status_color: if failed { theme::RED } else { theme::ACCENT },
            active,
            dim: false,
            busy: !failed,
        });
        if r.clicked() {
            self.actions.push(Action::Open(name.to_owned()));
        }
    }

    // ---------- tab bar ----------
    fn tab_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let h = rect.height();
        ui.painter().rect_filled(rect, 0.0, theme::PANEL);
        ui.painter().line_segment([rect.left_bottom() - vec2(0.0, 0.5), rect.right_bottom() - vec2(0.0, 0.5)], Stroke::new(1.0, theme::BORDER_SOFT));
        let font = FontId::proportional(12.5);
        egui::ScrollArea::horizontal()
            .id_salt("gwm-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                ui.horizontal(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let label = self.label_of(&tab.key);
                        let host = self.host_of(&tab.key);
                        let text_w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), theme::TEXT).size().x);
                        let host_w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(host.clone(), FontId::proportional(11.0), theme::FAINT).size().x);
                        let w = (text_w + host_w + 72.0).clamp(110.0, 280.0);
                        let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
                        let p = ui.painter();
                        let active = i == self.active;
                        let bg = if active { theme::EDITOR } else if resp.hovered() { theme::HOVER } else { theme::PANEL };
                        p.rect_filled(r, 0.0, bg);
                        p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, theme::BORDER_SOFT));
                        if !active {
                            p.line_segment([r.left_bottom() - vec2(0.0, 0.5), r.right_bottom() - vec2(0.0, 0.5)], Stroke::new(1.0, theme::BORDER_SOFT));
                        }
                        let failed = self.pending.iter().any(|pd| pd.form.name == tab.key && pd.failed);
                        let icon_r = Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(11.0, 11.0));
                        if tab.pending.is_some() && !failed {
                            egui::Spinner::new().size(11.0).color(theme::ACCENT).paint_at(ui, icon_r);
                        } else {
                            let c = if failed { theme::RED } else if tab.term.as_ref().is_some_and(|t| t.exited) { theme::FAINT } else { theme::ACCENT };
                            p.text(icon_r.center(), Align2::CENTER_CENTER, self.icon_of(&tab.key), theme::mono(12.0), c);
                        }
                        p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, &label, font.clone(), if active { theme::TEXT } else { theme::MUTED });
                        p.text(pos2(r.left() + 34.0 + text_w, r.center().y + 0.5), Align2::LEFT_CENTER, &host, FontId::proportional(11.0), theme::FAINT);
                        let close = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(16.0, 16.0));
                        let close_resp = ui.interact(close, ui.scope_id().with(("close", &tab.key)), Sense::click());
                        if active || resp.hovered() || close_resp.hovered() {
                            if close_resp.hovered() {
                                p.rect_filled(close, 3.0, theme::SELECTED);
                            }
                            p.text(close.center(), Align2::CENTER_CENTER, "×", FontId::proportional(14.0), theme::MUTED);
                        }
                        if close_resp.clicked() || resp.middle_clicked() {
                            self.actions.push(Action::Close(i));
                        } else if resp.clicked() {
                            self.actions.push(Action::Activate(i));
                        }
                        let _ = close_resp.on_hover_text("Close tab (the session keeps running)");
                        let key = tab.key.clone();
                        resp.context_menu(|ui| {
                            if ui.button("Close tab").clicked() {
                                self.actions.push(Action::Close(i));
                                ui.close();
                            }
                            if key != COORD && tab.pending.is_none() && ui.button(RichText::new("End session…").color(theme::RED)).clicked() {
                                self.actions.push(Action::EndSession(key.clone()));
                                ui.close();
                            }
                        });
                    }
                    let (plus, pr) = ui.allocate_exact_size(vec2(34.0, h), Sense::click());
                    let pr = pr.on_hover_text("New session (⌘T)");
                    if pr.hovered() {
                        ui.painter().rect_filled(plus.shrink(5.0), 4.0, theme::HOVER);
                    }
                    ui.painter().text(plus.center(), Align2::CENTER_CENTER, "+", FontId::proportional(16.0), theme::MUTED);
                    if pr.clicked() {
                        self.actions.push(Action::NewSession(None));
                    }
                });
            });
    }

    // ---------- status bar: machines (left) · what's happening (middle) · help (right) ----------
    fn status_bar(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, theme::BG);
        p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme::BORDER_SOFT));
        let mut x = rect.left() + 10.0;
        let y = rect.center().y;
        let small = FontId::proportional(11.5);
        let sidebar_r = Rect::from_center_size(pos2(x + 8.0, y), vec2(20.0, 18.0));
        let sr = ui.interact(sidebar_r, ui.scope_id().with("sb"), Sense::click()).on_hover_text("Toggle sidebar (⌘B)");
        p.text(sidebar_r.center(), Align2::CENTER_CENTER, "▤", FontId::proportional(13.0), if sr.hovered() { theme::TEXT } else { theme::MUTED });
        if sr.clicked() {
            self.actions.push(Action::ToggleSidebar);
        }
        x += 30.0;
        for h in &self.state.hosts {
            let (dot, c) = if h.online == Some(true) { ("●", theme::GREEN) } else if h.needs_approval { ("◐", theme::YELLOW) } else { ("○", theme::FAINT) };
            let r = p.text(pos2(x, y), Align2::LEFT_CENTER, dot, small.clone(), c);
            x = r.right() + 4.0;
            let t = if h.online == Some(true) && !h.cpu.is_empty() { format!("{} {}", h.name, h.cpu) } else { h.name.clone() };
            let r = p.text(pos2(x, y), Align2::LEFT_CENTER, t, small.clone(), theme::MUTED);
            x = r.right() + 14.0;
        }
        x += 10.0;
        let running = self.jobs.values().filter(|j| j.done.is_none()).map(|j| j.title.clone()).next();
        if let Some(t) = running {
            egui::Spinner::new().size(10.0).color(theme::YELLOW).paint_at(ui, Rect::from_center_size(pos2(x + 5.0, y), vec2(10.0, 10.0)));
            p.text(pos2(x + 16.0, y), Align2::LEFT_CENTER, t, small.clone(), theme::YELLOW);
        } else if let Some(t) = self.toasts.last() {
            p.text(pos2(x, y), Align2::LEFT_CENTER, format!("{}  {}", if t.ok { "✓" } else { "✕" }, t.text), small.clone(), if t.ok { theme::GREEN } else { theme::RED });
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        p.text(pos2(rect.right() - 12.0, y), Align2::RIGHT_CENTER, "⌘K commands   ⌘T new   ⇧↵ newline   ⌘V screenshots", small, theme::FAINT);
    }

    // ---------- center ----------
    fn center(&mut self, ui: &mut Ui, term_focus: bool) {
        let dropping = ui.input(|i| !i.raw.hovered_files.is_empty());
        let dropped: Vec<PathBuf> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        let Some(tab) = self.tabs.get(self.active) else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Pick a session on the left, or press ⌘T for a new one").color(theme::FAINT));
            });
            return;
        };
        let key = tab.key.clone();
        if let Some(job) = tab.pending {
            self.pending_view(ui, &key, job);
            return;
        }
        if let Some(e) = &tab.error {
            ui.label(RichText::new(e).color(theme::RED));
            return;
        }
        let Some(term) = self.tabs.get_mut(self.active).and_then(|t| t.term.as_mut()) else { return };
        let (resp, out) = term.ui(ui, term_focus);
        if term_focus {
            ui.memory_mut(|m| {
                m.request_focus(resp.id);
                m.set_focus_lock_filter(
                    resp.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true, ..Default::default() },
                );
            });
        }
        if let Some(s) = out.copy {
            ui.ctx().copy_text(s);
        }
        if out.reconnect {
            self.actions.push(Action::Reconnect(self.active));
        }
        if out.paste_image {
            self.actions.push(Action::PasteClipboard(key.clone()));
        }
        if let Some(text) = out.paste_text {
            // Files copied in Finder arrive as their names; send the files themselves instead.
            let files = arboard::Clipboard::new().ok().and_then(|mut c| c.get().file_list().ok()).unwrap_or_default();
            if files.is_empty() {
                if let Some(t) = self.tabs.get(self.active).and_then(|t| t.term.as_ref()) {
                    t.paste(&text);
                }
            } else {
                self.actions.push(Action::Share(files, key.clone()));
            }
        }
        if !dropped.is_empty() {
            self.actions.push(Action::Share(dropped, key.clone()));
        }
        if dropping {
            let host = self.host_of(&key);
            let r = resp.rect.shrink(18.0);
            let p = ui.painter();
            p.rect_filled(r, 10.0, Color32::from_rgba_unmultiplied(0x28, 0x2c, 0x33, 220));
            p.rect_stroke(r, 10.0, Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, format!("Drop to send to {} on {host}", self.label_of(&key)), FontId::proportional(18.0), theme::TEXT);
        }
    }

    /// The tab of a session that's still being created: progress now, the terminal when it's ready.
    fn pending_view(&mut self, ui: &mut Ui, key: &str, job: u64) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, theme::EDITOR);
        let failed = self.jobs.get(&job).is_some_and(|j| j.done == Some(false));
        let (title, lines) = self.jobs.get(&job).map(|j| (j.title.clone(), j.lines.clone())).unwrap_or_default();
        let width = 560.0_f32.min(rect.width() - 48.0);
        let inner = Rect::from_min_size(pos2(rect.center().x - width / 2.0, rect.top() + rect.height() * 0.22), vec2(width, rect.height() * 0.7));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.horizontal(|ui| {
                if failed {
                    ui.label(RichText::new("✕").color(theme::RED).size(16.0));
                    ui.label(RichText::new(format!("Couldn't start {key}")).size(16.0).color(theme::TEXT));
                } else {
                    ui.add(egui::Spinner::new().size(16.0).color(theme::ACCENT));
                    ui.label(RichText::new(title).size(16.0).color(theme::TEXT));
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(if failed { "Nothing was left half-done on your side. Fix what the log says, then try again." } else { "You can keep working in other tabs — this one turns into the terminal when it's ready." }).color(theme::FAINT).small());
            ui.add_space(12.0);
            Frame::new().fill(theme::PANEL).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if lines.is_empty() {
                    ui.label(RichText::new("waiting for gwm…").font(theme::mono(11.5)).color(theme::FAINT));
                }
                for l in lines.iter().rev().take(14).rev() {
                    ui.label(RichText::new(l).font(theme::mono(11.5)).color(theme::MUTED));
                }
            });
            if failed {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Try again").color(theme::EDITOR)).fill(theme::ACCENT)).clicked() {
                        self.actions.push(Action::Retry(key.to_owned()));
                    }
                    if ui.button("Close tab").clicked() {
                        self.actions.push(Action::Close(self.active));
                    }
                });
            }
        });
    }

    // ---------- command palette ----------
    fn commands(&self) -> Vec<(String, String, Action)> {
        let mut v = Vec::new();
        v.push(("Open overview".into(), format!("coordinator on {}", self.state.coordinator_host), Action::Open(COORD.into())));
        for s in &self.state.sessions {
            v.push((format!("Open {}", s.name), format!("{} on {}{}", s.engine, s.host, if s.running { "" } else { " · not started" }), Action::Open(s.name.clone())));
        }
        v.push(("New session…".into(), "⌘T".into(), Action::NewSession(None)));
        for h in &self.state.hosts {
            v.push((format!("New session on {}…", h.name), String::new(), Action::NewSession(Some(h.name.clone()))));
        }
        for p in self.peers.iter().filter(|p| p.saved_as.is_empty() && p.online) {
            v.push((format!("New session on {}…", p.short()), "tailnet · set up automatically".into(), Action::NewSession(Some(p.dns.clone()))));
        }
        if let Some(k) = self.active_key() {
            v.push(("Paste clipboard into this session".into(), "screenshot, image or copied files · ⌘V".into(), Action::PasteClipboard(k.clone())));
            v.push(("Send newest screenshot to this session".into(), "⌘⇧V".into(), Action::PasteLast(k.clone())));
            v.push(("Close tab".into(), "⌘W · the session keeps running".into(), Action::Close(self.active)));
            if k != COORD {
                v.push((format!("End session {k}…"), "⌘⇧W · stops it on its machine".into(), Action::EndSession(k.clone())));
            }
        }
        if self.browser {
            v.push(("Browser: stop sharing".into(), "gwm chrome stop".into(), Action::Browser(false)));
        } else {
            v.push(("Browser: share Chrome with every machine".into(), "gwm chrome".into(), Action::Browser(true)));
        }
        v.push(("Toggle sidebar".into(), "⌘B".into(), Action::ToggleSidebar));
        v.push(("Bigger text".into(), "⌘=".into(), Action::Font(1.0)));
        v.push(("Smaller text".into(), "⌘-".into(), Action::Font(-1.0)));
        v
    }

    fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(mut pal) = self.palette.take() else { return };
        let all = self.commands();
        let q = pal.query.to_lowercase();
        let matches: Vec<&(String, String, Action)> =
            all.iter().filter(|(t, d, _)| q.split_whitespace().all(|w| t.to_lowercase().contains(w) || d.to_lowercase().contains(w))).collect();
        let mut keep = true;
        let mut run: Option<Action> = None;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                keep = false;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                pal.selected = (pal.selected + 1).min(matches.len().saturating_sub(1));
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                pal.selected = pal.selected.saturating_sub(1);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                run = matches.get(pal.selected).map(|m| m.2.clone());
            }
        });
        let screen = ctx.content_rect();
        let width = 560.0_f32.min(screen.width() - 40.0);
        let area = egui::Area::new(egui::Id::NULL.with("gwm-palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + 70.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::ELEVATED)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(8))
                    .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) })
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        let te = ui.add(
                            TextEdit::singleline(&mut pal.query)
                                .hint_text("Open a session, start one, paste, share the browser…")
                                .frame(egui::Frame::NONE)
                                .desired_width(width)
                                .margin(Margin::symmetric(8, 6))
                                .font(FontId::proportional(14.0)),
                        );
                        te.request_focus();
                        if te.changed() {
                            pal.selected = 0;
                        }
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for (i, (title, detail, action)) in matches.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                                let sel = i == pal.selected;
                                if sel || resp.hovered() {
                                    ui.painter().rect_filled(r, 4.0, if sel { theme::SELECTED } else { theme::HOVER });
                                }
                                ui.painter().text(pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.0), theme::TEXT);
                                ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, detail, FontId::proportional(11.5), theme::FAINT);
                                if sel {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
                                    run = Some(action.clone());
                                }
                            }
                            if matches.is_empty() {
                                ui.label(RichText::new("  No matching commands").color(theme::FAINT));
                            }
                        });
                    });
            });
        if ctx.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer() {
            keep = false;
        }
        if let Some(a) = run {
            self.actions.push(a);
            keep = false;
        }
        if keep {
            self.palette = Some(pal);
        }
    }

    // ---------- end session confirmation ----------
    fn confirm_end_ui(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_end.clone() else { return };
        let host = self.host_of(&name);
        let (mut keep, mut end) = (true, false);
        let modal = egui::Modal::new(egui::Id::NULL.with("gwm-end-session"))
            .frame(Frame::new().fill(theme::ELEVATED).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(10)))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                Frame::new().inner_margin(Margin::same(20)).show(ui, |ui| {
                    ui.label(RichText::new(format!("End '{name}'?")).size(15.0).strong().color(theme::TEXT));
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!(
                        "It stops on {host} and leaves your saved sessions. Its screen is saved to the archive first, so you can bring it back with gwm resume."
                    )).color(theme::MUTED));
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("End session  ↵").color(theme::EDITOR)).fill(theme::RED).min_size(vec2(110.0, 28.0)).corner_radius(6)).clicked() {
                            end = true;
                        }
                        if ui.add(egui::Button::new("Cancel").fill(theme::HOVER).min_size(vec2(72.0, 28.0)).corner_radius(6)).clicked() {
                            keep = false;
                        }
                    });
                });
            });
        if modal.should_close() {
            keep = false; // Esc or a click outside
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            end = true;
        }
        if end {
            self.actions.push(Action::ConfirmEnd(name));
            self.confirm_end = None;
        } else if !keep {
            self.confirm_end = None;
        }
    }

    // ---------- new session dialog ----------
    // Header (title, close) · machine list · engine cards · "More options" · footer (summary, Cancel, Create).
    // Closes at once on Create; the session then appears optimistically in the sidebar and as a tab.
    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.take() else { return };
        if d.machine.is_empty() {
            // opened before the machine list arrived: default to this Mac, else the first machine
            d.machine = if self.state.me.is_empty() { self.state.hosts.first().map(|h| h.name.clone()).unwrap_or_default() } else { self.state.me.clone() };
        }
        let mut keep = true;
        let mut create = false;
        let peer = self.peers.iter().find(|p| p.dns == d.machine && p.saved_as.is_empty()).cloned();
        let name = d.name.trim().to_owned();
        let name_taken = self.state.sessions.iter().any(|s| s.name == name) || self.pending.iter().any(|p| p.form.name == name);
        let name_valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && name != COORD;
        let engine_valid = !d.engine.trim().is_empty() && d.engine.trim().chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        let user_valid = peer.is_none() || !d.user.trim().is_empty();
        let ok = name_valid && !name_taken && engine_valid && user_valid && !d.machine.is_empty();
        let width = 480.0;
        let modal = egui::Modal::new(egui::Id::NULL.with("gwm-new-session"))
            .frame(Frame::new().fill(theme::ELEVATED).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(10)))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                // header
                Frame::new().inner_margin(Margin { left: 20, right: 12, top: 16, bottom: 14 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("New session").size(16.0).color(theme::TEXT));
                            ui.add_space(3.0);
                            ui.label(RichText::new("A terminal that keeps running on its machine — close the tab anytime.").size(11.5).color(theme::FAINT));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if ui.add(egui::Button::new(RichText::new("×").size(16.0).color(theme::MUTED)).fill(Color32::TRANSPARENT).frame(false)).on_hover_text("Close (Esc)").clicked() {
                                keep = false;
                            }
                        });
                    });
                });
                divider(ui);

                // body
                Frame::new().inner_margin(Margin { left: 20, right: 20, top: 16, bottom: 18 }).show(ui, |ui| {
                    // 1. where
                    field_label(ui, "Machine", Some("Where it runs. New tailnet machines are set up for you."));
                    for h in &self.state.hosts {
                        let (dot, dc) = if h.online == Some(true) { ("●", theme::GREEN) } else { ("○", theme::FAINT) };
                        let meta = if h.online == Some(true) {
                            format!("{}{}", if h.name == self.state.me { "this Mac · " } else { "" }, [h.cpu.as_str(), h.ram.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · "))
                        } else {
                            "offline".into()
                        };
                        if choice_row(ui, d.machine == h.name, dot, dc, &h.name, &meta).clicked() {
                            d.machine = h.name.clone();
                        }
                    }
                    let new: Vec<Peer> = self.peers.iter().filter(|p| p.saved_as.is_empty() && p.online).cloned().collect();
                    for p in &new {
                        if choice_row(ui, d.machine == p.dns, "+", theme::ACCENT, p.short(), &format!("{} · new — set up automatically", p.os)).clicked() {
                            d.machine = p.dns.clone();
                        }
                    }
                    if peer.is_some() {
                        ui.add_space(10.0);
                        field_label(ui, "Your username there", Some("Run whoami on that machine if unsure."));
                        ui.add(TextEdit::singleline(&mut d.user).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5)));
                    }

                    // 2. what
                    ui.add_space(18.0);
                    field_label(ui, "Engine", None);
                    let cards = [("claude", "✻", "Claude Code", "Coding agent"), ("opencode", "◇", "opencode", "Local models"), ("shell", "$", "Shell", "Plain terminal")];
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 16.0) / 3.0;
                        for (key, icon, title, desc) in cards {
                            if engine_card(ui, w, d.engine == key, icon, title, desc).clicked() {
                                d.engine = key.into();
                            }
                        }
                    });
                    let custom = !cards.iter().any(|c| c.0 == d.engine);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("or another CLI").size(11.5).color(theme::FAINT));
                        let mut other = if custom { d.engine.clone() } else { String::new() };
                        if ui.add(TextEdit::singleline(&mut other).desired_width(140.0).hint_text("codex, gemini, kimi…").margin(Margin::symmetric(6, 3))).changed() {
                            d.engine = if other.trim().is_empty() { "claude".into() } else { other };
                        }
                    });

                    // 3. more options (collapsed by default)
                    ui.add_space(16.0);
                    let label = format!("{}  More options", if d.more { "▾" } else { "▸" });
                    if ui.add(egui::Label::new(RichText::new(label).size(12.0).color(theme::MUTED)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        d.more = !d.more;
                    }
                    if d.more {
                        ui.add_space(10.0);
                        if d.engine == "claude" {
                            field_label(ui, "Claude account", Some("default, or a label like work (its own login)"));
                            ui.add(TextEdit::singleline(&mut d.account).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5)));
                            ui.add_space(10.0);
                        }
                        ui.columns(2, |cols| {
                            field_label(&mut cols[0], "Start folder", None);
                            cols[0].add(TextEdit::singleline(&mut d.folder).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5)));
                            field_label(&mut cols[1], "Tab name", None);
                            if cols[1].add(TextEdit::singleline(&mut d.name).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 5))).changed() {
                                d.name_edited = true;
                            }
                        });
                    }
                    if name_taken || !name_valid {
                        ui.add_space(8.0);
                        let msg = if name_taken { format!("'{name}' is already a session — pick another tab name under More options") } else { "Tab name: letters, digits, - and _ only".to_owned() };
                        ui.label(RichText::new(msg).size(11.5).color(theme::RED));
                    }
                    if let Some(p) = &peer {
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("{} isn't set up yet. Creating connects over Tailscale SSH and installs tmux and go-with-me there. The session shows up right away while that runs.", p.short())).size(11.5).color(theme::YELLOW));
                    }
                });
                divider(ui);

                // footer: what will happen, then the actions
                Frame::new().inner_margin(Margin { left: 20, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let account = if d.engine == "claude" && d.account.trim() != "default" { format!(" · {}", d.account.trim()) } else { String::new() };
                        ui.label(RichText::new(format!("{name} · {} on {}{account}", d.engine.trim(), short(&d.machine))).size(11.5).color(theme::FAINT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ui.add_enabled(ok, egui::Button::new(RichText::new("Create  ↵").color(theme::EDITOR)).fill(theme::ACCENT).min_size(vec2(92.0, 28.0)).corner_radius(6)).clicked() {
                                create = true;
                            }
                            if ui.add(egui::Button::new("Cancel").fill(theme::HOVER).min_size(vec2(72.0, 28.0)).corner_radius(6)).clicked() {
                                keep = false;
                            }
                        });
                    });
                });
            });
        if !d.name_edited {
            d.name = self.suggest_name(&d);
        }
        if ok && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            create = true;
        }
        if modal.should_close() {
            keep = false;
        }
        if create {
            self.create_session(ctx, d);
            return;
        }
        if keep {
            self.dialog = Some(d);
        }
    }

    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        if self.screenshot_done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let Some(path) = self.screenshot.clone() else { return };
        if self.started.elapsed() < Duration::from_secs(7) {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        self.screenshot = None;
        let done = self.screenshot_done.clone();
        let c = ctx.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            let _ = crate::png::write(&path, img.size[0], img.size[1], &rgba);
            done.store(true, Ordering::Relaxed);
            c.request_repaint();
        })));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_messages(&ctx);
        if self.palette.is_none() && self.dialog.is_none() && self.confirm_end.is_none() {
            self.shortcuts(&ctx);
        }
        for a in &mut self.actions {
            if let Action::Activate(i) = a {
                if *i == usize::MAX {
                    *i = self.tabs.len().saturating_sub(1); // ⌘9 = last tab
                }
            }
        }
        let window_focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let term_focus = self.palette.is_none() && self.dialog.is_none() && self.confirm_end.is_none() && window_focused;
        if self.jobs.values().any(|j| j.done.is_none()) {
            ctx.request_repaint_after(Duration::from_millis(100)); // keep spinners moving
        }

        egui::Panel::top("title").exact_size(38.0).frame(Frame::NONE).show(ui, |ui| self.title_bar(ui));
        egui::Panel::bottom("status").exact_size(26.0).frame(Frame::NONE).show(ui, |ui| self.status_bar(ui));
        if self.sidebar {
            egui::Panel::left("sidebar")
                .default_size(260.0)
                .size_range(190.0..=420.0)
                .resizable(true)
                .frame(Frame::new().fill(theme::PANEL).stroke(Stroke::new(1.0, theme::BORDER_SOFT)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.sidebar(ui));
                });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let full = ui.max_rect();
            let bar = Rect::from_min_size(full.min, vec2(full.width(), 34.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| self.tab_bar(ui));
            let body = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| self.center(ui, term_focus));
        });

        self.palette_ui(&ctx);
        self.dialog_ui(&ctx);
        self.confirm_end_ui(&ctx);
        self.run_actions(&ctx);
        self.maybe_screenshot(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::EDITOR.to_normalized_gamma_f32()
    }
}

// ---------- helpers ----------
fn short(machine: &str) -> &str {
    machine.split('.').next().unwrap_or(machine)
}

fn engine_icon(engine: &str) -> &'static str {
    Session { engine: engine.to_owned(), ..Default::default() }.icon()
}

fn session_status(s: &Session) -> (String, Color32) {
    let with_title = |state: String| if s.title.is_empty() { state } else { format!("{state} · {}", s.title) };
    if s.working() {
        (with_title("working".into()), theme::YELLOW)
    } else if s.running {
        let idle = match s.idle_secs {
            0..=59 => "idle".to_owned(),
            60..=3599 => format!("idle {}m", s.idle_secs / 60),
            _ => format!("idle {}h", s.idle_secs / 3600),
        };
        (with_title(idle), theme::FAINT)
    } else {
        ("not started".into(), theme::FAINT)
    }
}

// ---------- small painted widgets ----------
fn section(ui: &mut Ui, title: &str, gap_above: f32) {
    ui.add_space(gap_above);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(10.5), theme::FAINT);
}

fn note(ui: &mut Ui, text: &str, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let room = ((r.width() - 28.0) / 5.6).max(0.0) as usize;
    ui.painter().text(pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, elide(text, room), FontId::proportional(11.0), color);
}

fn field_label(ui: &mut Ui, text: &str, help: Option<&str>) {
    ui.label(RichText::new(text).size(12.0).color(theme::TEXT));
    if let Some(h) = help {
        ui.add_space(2.0);
        ui.label(RichText::new(h).size(11.0).color(theme::FAINT));
    }
    ui.add_space(6.0);
}

fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::BORDER_SOFT);
}

/// One option in a visible list (like a radio): status dot, name, a quiet second fact on the right.
fn choice_row(ui: &mut Ui, selected: bool, dot: &str, dot_color: Color32, name: &str, meta: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    let fill = if selected { theme::SELECTED } else if resp.hovered() { theme::HOVER } else { Color32::TRANSPARENT };
    p.rect_filled(r.shrink2(vec2(0.0, 1.0)), 6.0, fill);
    if selected {
        p.rect_stroke(r.shrink2(vec2(0.0, 1.0)), 6.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    p.text(pos2(r.left() + 14.0, r.center().y), Align2::CENTER_CENTER, dot, FontId::proportional(10.0), dot_color);
    p.text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(13.0), theme::TEXT);
    p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, meta, FontId::proportional(11.0), theme::FAINT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A choice card: icon, title, one-line description.
fn engine_card(ui: &mut Ui, width: f32, selected: bool, icon: &str, title: &str, desc: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 8.0, if selected { theme::SELECTED } else if resp.hovered() { theme::HOVER } else { theme::PANEL });
    p.rect_stroke(r, 8.0, Stroke::new(1.0, if selected { theme::ACCENT } else { theme::BORDER_SOFT }), egui::StrokeKind::Inside);
    p.text(pos2(r.left() + 12.0, r.top() + 18.0), Align2::LEFT_CENTER, icon, theme::mono(13.0), if selected { theme::ACCENT } else { theme::MUTED });
    p.text(pos2(r.left() + 28.0, r.top() + 18.0), Align2::LEFT_CENTER, title, FontId::proportional(13.0), theme::TEXT);
    let room = ((width - 24.0) / 5.4) as usize;
    p.text(pos2(r.left() + 12.0, r.top() + 42.0), Align2::LEFT_CENTER, elide(desc, room), FontId::proportional(11.0), theme::FAINT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A machine: status dot, name, how many sessions, and its load — the header of its group.
#[allow(clippy::too_many_arguments)]
fn group_header(ui: &mut Ui, open: bool, dot: &str, dot_color: Color32, name: &str, count: usize, meta: &str, tag: Option<&str>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, 0.0, theme::HOVER);
    }
    let y = r.center().y;
    p.text(pos2(r.left() + 14.0, y), Align2::LEFT_CENTER, if open { "▾" } else { "▸" }, FontId::proportional(10.0), theme::FAINT);
    p.text(pos2(r.left() + 28.0, y), Align2::LEFT_CENTER, dot, FontId::proportional(10.0), dot_color);
    let mut x = p.text(pos2(r.left() + 42.0, y), Align2::LEFT_CENTER, name, FontId::proportional(13.5), theme::TEXT).right() + 6.0;
    if count > 0 {
        x = p.text(pos2(x, y + 0.5), Align2::LEFT_CENTER, count.to_string(), FontId::proportional(11.0), theme::FAINT).right() + 6.0;
    }
    if let Some(t) = tag {
        let g = p.layout_no_wrap(t.to_owned(), FontId::proportional(10.0), theme::MUTED);
        let tr = Rect::from_min_size(pos2(x, y - 8.0), vec2(g.size().x + 10.0, 16.0));
        p.rect_filled(tr, 4.0, theme::HOVER);
        p.galley(pos2(tr.left() + 5.0, y - g.size().y / 2.0), g, theme::MUTED);
        x = tr.right() + 6.0;
    }
    let room = ((r.right() - x - 12.0) / 5.6).max(0.0) as usize;
    p.text(pos2(r.right() - 12.0, y + 0.5), Align2::RIGHT_CENTER, elide(meta, room), FontId::proportional(11.0), theme::FAINT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct RowLook<'a> {
    icon: &'a str,
    icon_color: Color32,
    name: &'a str,
    status: &'a str,
    status_color: Color32,
    active: bool,
    dim: bool,
    busy: bool,
}

/// A session: icon + name on the first line, its state (coloured) on the second.
fn session_row(ui: &mut Ui, look: RowLook<'_>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let p = ui.painter();
    if look.active {
        p.rect_filled(r, 0.0, theme::SELECTED);
        p.rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 0.0, theme::ACCENT);
    } else if resp.hovered() {
        p.rect_filled(r, 0.0, theme::HOVER);
    }
    let top = r.top() + 12.0;
    let bottom = r.top() + 27.0;
    let icon_r = Rect::from_center_size(pos2(r.left() + 32.0, top), vec2(11.0, 11.0));
    if look.busy {
        egui::Spinner::new().size(10.0).color(look.icon_color).paint_at(ui, icon_r);
    } else {
        p.text(icon_r.center(), Align2::CENTER_CENTER, look.icon, theme::mono(12.0), look.icon_color);
    }
    let name_color = if look.active { theme::TEXT } else if look.dim { theme::MUTED } else { theme::TEXT };
    p.text(pos2(r.left() + 46.0, top), Align2::LEFT_CENTER, look.name, FontId::proportional(13.0), name_color);
    let room = ((r.width() - 58.0) / 5.6).max(0.0) as usize;
    p.text(pos2(r.left() + 46.0, bottom), Align2::LEFT_CENTER, elide(look.status, room), FontId::proportional(11.0), look.status_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "+ new session" inside a machine's group: quiet until hovered.
fn add_row(ui: &mut Ui, text: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let c = if resp.hovered() { theme::TEXT } else { theme::FAINT };
    if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, theme::HOVER);
    }
    ui.painter().text(pos2(r.left() + 32.0, r.center().y), Align2::CENTER_CENTER, "+", FontId::proportional(13.0), c);
    ui.painter().text(pos2(r.left() + 46.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(12.0), c);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

enum Pill {
    Primary,
    Ghost(Color32),
}

fn pill(ui: &mut Ui, right: &mut f32, y: f32, text: &str, kind: Pill) -> egui::Response {
    let font = FontId::proportional(12.5);
    let w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), theme::TEXT).size().x) + 22.0;
    let r = Rect::from_center_size(pos2(*right - w / 2.0, y), vec2(w, 26.0));
    *right -= w + 6.0;
    let resp = ui.interact(r, ui.scope_id().with(("pill", text)), Sense::click());
    let p = ui.painter();
    match kind {
        Pill::Primary => {
            let fill = if resp.hovered() { theme::ACCENT.gamma_multiply(1.15) } else { theme::ACCENT };
            p.rect_filled(r, 6.0, fill);
            p.text(r.center(), Align2::CENTER_CENTER, text, font, theme::EDITOR);
        }
        Pill::Ghost(color) => {
            if resp.hovered() {
                p.rect_filled(r, 6.0, theme::SELECTED);
            }
            p.text(r.center(), Align2::CENTER_CENTER, text, font, color);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else if max <= 1 {
        String::new()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}
