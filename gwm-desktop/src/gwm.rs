//! Talking to the `gwm` command-line tool: it already knows every machine, session and account,
//! so the app reads its machine-readable output and runs its commands rather than re-implementing them.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// Where `gwm` lives. Apps opened from Finder get a minimal PATH, so look in the usual places.
pub fn gwm_path() -> PathBuf {
    if let Ok(p) = std::env::var("GWM") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    for p in [
        format!("{home}/.local/bin/gwm"),
        format!("{home}/go-with-me/go-with-me"),
        "/opt/homebrew/bin/gwm".into(),
        "/usr/local/bin/gwm".into(),
    ] {
        if std::path::Path::new(&p).exists() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("gwm")
}

pub fn command(args: &[&str]) -> Command {
    let mut c = Command::new(gwm_path());
    c.args(args).stdin(Stdio::null());
    c.env("GWM_NO_TAB", "1"); // don't open tabs in the Zellij hub; this app shows them itself
    c
}

#[derive(Clone, Debug, Default)]
pub struct Host {
    pub name: String,
    pub target: String,
    pub role: String,
    pub online: Option<bool>, // None = not checked yet
    pub needs_approval: bool,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub models: Vec<String>,
    pub used_by: String,
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub name: String,
    pub host: String,
    pub engine: String,
    pub account: String,
    pub folder: String,
    pub running: bool,
    pub idle_secs: u64,
    pub attached: bool,
    pub title: String,
}

impl Session {
    pub fn working(&self) -> bool {
        self.running && self.idle_secs < 8
    }
    pub fn icon(&self) -> &'static str {
        match self.engine.as_str() {
            "claude" => "✻",
            "opencode" => "◇",
            "shell" => "$",
            "codex" => "◎",
            "gemini" => "✦",
            _ => "›",
        }
    }
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)] // name/ip: kept for matching and future use
pub struct Peer {
    pub dns: String,
    pub name: String,
    pub os: String,
    pub online: bool,
    pub ip: String,
    pub saved_as: String,
}

impl Peer {
    pub fn short(&self) -> &str {
        self.dns.split('.').next().unwrap_or(&self.dns)
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub me: String,
    pub coordinator_host: String,
    pub hosts: Vec<Host>,
    pub sessions: Vec<Session>,
    pub loaded: bool,
    pub error: Option<String>,
}

impl State {
    pub fn session(&self, name: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.name == name)
    }
}

/// Parse `gwm _data` (the same feed the Zellij sidebar reads).
pub fn parse_data(text: &str) -> State {
    let mut st = State { loaded: true, ..Default::default() };
    let mut used: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("#SELF") => st.me = f.get(1).unwrap_or(&"").to_string(),
            Some("#COORD") => st.coordinator_host = f.get(1).unwrap_or(&"").to_string(),
            Some("#HOST") if f.len() >= 3 => st.hosts.push(Host {
                name: f[1].into(),
                target: f[2].into(),
                role: f.get(3).unwrap_or(&"").to_string(),
                ..Default::default()
            }),
            Some("#SESS") if f.len() >= 6 => st.sessions.push(Session {
                name: f[1].into(),
                host: f[2].into(),
                engine: f[3].into(),
                account: f[4].into(),
                folder: f[5].into(),
                ..Default::default()
            }),
            Some("#USED") if f.len() >= 2 => {
                used.insert(f[1].into(), f.get(2).unwrap_or(&"").to_string());
            }
            _ => {}
        }
    }
    // probe lines: host|H|cpu|ram|models|vms|gpu · host|S|name|idle|attached|title · host|A|link · host|DOWN
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let p: Vec<&str> = line.split('|').collect();
        if p.len() < 2 {
            continue;
        }
        let host = p[0];
        match p[1] {
            "H" => {
                if let Some(h) = st.hosts.iter_mut().find(|h| h.name == host) {
                    h.online = Some(true);
                    h.cpu = p.get(2).unwrap_or(&"").to_string();
                    h.ram = p.get(3).unwrap_or(&"").to_string();
                    h.gpu = p.get(6).unwrap_or(&"").to_string();
                }
            }
            "M" => {
                if let Some(h) = st.hosts.iter_mut().find(|h| h.name == host) {
                    h.models.push(p.get(2).unwrap_or(&"").trim_end_matches(":latest").to_string());
                }
            }
            "S" if p.len() >= 5 => {
                if let Some(s) = st.sessions.iter_mut().find(|s| s.name == p[2] && s.host == host) {
                    s.running = true;
                    s.idle_secs = p[3].parse().unwrap_or(0);
                    s.attached = p[4].parse::<u32>().unwrap_or(0) > 0;
                    s.title = p[5..].join("|").trim_start_matches(['✳', '✻', ' ']).to_string();
                }
            }
            "A" => {
                if let Some(h) = st.hosts.iter_mut().find(|h| h.name == host) {
                    h.online = Some(false);
                    h.needs_approval = true;
                }
            }
            "DOWN" => {
                if let Some(h) = st.hosts.iter_mut().find(|h| h.name == host) {
                    h.online = Some(false);
                }
            }
            _ => {}
        }
    }
    for h in &mut st.hosts {
        if h.online.is_none() {
            h.online = Some(false);
        }
        h.used_by = used.remove(&h.name).unwrap_or_default();
    }
    st
}

pub fn parse_tailnet(text: &str) -> Vec<Peer> {
    let mut v: Vec<Peer> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5).then(|| Peer {
                dns: f[0].into(),
                name: f[1].into(),
                os: f[2].into(),
                online: f[3] == "online",
                ip: f[4].into(),
                saved_as: f.get(5).unwrap_or(&"").to_string(),
            })
        })
        .collect();
    v.sort_by(|a, b| b.online.cmp(&a.online).then(a.dns.cmp(&b.dns)));
    v
}

/// Messages from background work to the UI.
pub enum Msg {
    State(State),
    Tailnet(Vec<Peer>),
    Browser(bool),
    /// A long-running command printed a line (for the New Session dialog's log).
    JobLine(u64, String),
    /// A command finished: job id, success, last output line.
    JobDone(u64, bool, String),
}

/// Poll `gwm _data`, `gwm _tailnet` and the shared browser forever, in the background.
pub fn spawn_poller(tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut n: u64 = 0;
        loop {
            match command(&["_data"]).stderr(Stdio::null()).output() {
                Ok(o) => {
                    let mut st = parse_data(&String::from_utf8_lossy(&o.stdout));
                    if st.hosts.is_empty() {
                        st.error = Some("gwm found no machines — run `gwm init` in a terminal first".into());
                    }
                    let _ = tx.send(Msg::State(st));
                }
                Err(e) => {
                    let st = State {
                        loaded: true,
                        error: Some(format!("can't run gwm ({}): {e}", gwm_path().display())),
                        ..Default::default()
                    };
                    let _ = tx.send(Msg::State(st));
                }
            }
            if n % 6 == 0 {
                if let Ok(o) = command(&["_tailnet"]).stderr(Stdio::null()).output() {
                    let _ = tx.send(Msg::Tailnet(parse_tailnet(&String::from_utf8_lossy(&o.stdout))));
                }
            }
            let _ = tx.send(Msg::Browser(browser_up()));
            ctx.request_repaint();
            n += 1;
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

/// Is the gwm shared Chrome (gwm chrome) open on this Mac?
pub fn browser_up() -> bool {
    std::net::TcpStream::connect_timeout(&"127.0.0.1:9339".parse().unwrap(), Duration::from_millis(150)).is_ok()
}

/// Run a gwm command in the background, streaming its output lines to the UI.
pub fn run_job(id: u64, args: Vec<String>, tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let child = command(&refs).env("NO_COLOR", "1").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Msg::JobDone(id, false, format!("couldn't run gwm: {e}")));
                ctx.request_repaint();
                return;
            }
        };
        let err = child.stderr.take().map(|e| {
            let tx = tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let mut last = String::new();
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    let line = strip_ansi(&line);
                    if !line.trim().is_empty() {
                        last = line.clone();
                        let _ = tx.send(Msg::JobLine(id, line));
                        ctx.request_repaint();
                    }
                }
                last
            })
        });
        let mut last = String::new();
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let line = strip_ansi(&line);
                if !line.trim().is_empty() {
                    last = line.clone();
                    let _ = tx.send(Msg::JobLine(id, line));
                    ctx.request_repaint();
                }
            }
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let err_last = err.and_then(|h| h.join().ok()).unwrap_or_default();
        let summary = if ok || err_last.is_empty() { last } else { err_last };
        let _ = tx.send(Msg::JobDone(id, ok, summary));
        ctx.request_repaint();
    });
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gwm_data() {
        let text = "#SELF\tmacbook\n#COORD\tstudio\n#HOST\tstudio\tme@studio.ts.net\tHome Mac\n#HOST\tmacbook\tme@mb.ts.net\tLaptop\n\
#SESS\twork\tstudio\tclaude\twork\t~\n#SESS\tshell\tmacbook\tshell\t-\t~\n#USED\tstudio\tmacbook: 1 tab\n\
studio|H|23%|31/64G|qwen|-|40%\nstudio|M|qwen:latest|17.2\nstudio|S|work|3|1|✳ Fixing tests\nmacbook|DOWN\n";
        let st = parse_data(text);
        assert_eq!(st.me, "macbook");
        assert_eq!(st.coordinator_host, "studio");
        assert_eq!(st.hosts.len(), 2);
        assert_eq!(st.hosts[0].online, Some(true));
        assert_eq!(st.hosts[0].cpu, "23%");
        assert_eq!(st.hosts[0].models, vec!["qwen"]);
        assert_eq!(st.hosts[0].used_by, "macbook: 1 tab");
        assert_eq!(st.hosts[1].online, Some(false));
        let w = st.session("work").unwrap();
        assert!(w.running && w.attached && w.working());
        assert_eq!(w.title, "Fixing tests");
        assert!(!st.session("shell").unwrap().running);
    }

    #[test]
    fn parses_tailnet() {
        let p = parse_tailnet("box.tail.ts.net\tbox\tlinux\toffline\t100.1.1.1\t\nmac.tail.ts.net\tmac\tmacOS\tonline\t100.1.1.2\tstudio\n");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].short(), "mac"); // online first
        assert_eq!(p[0].saved_as, "studio");
        assert!(!p[1].online);
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[32m✓\u{1b}[0m done"), "✓ done");
    }
}
