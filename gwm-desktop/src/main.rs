//! gwm-desktop — a native macOS window for go-with-me.
//!
//!   gwm-desktop                      open the app (the coordinator tab first)
//!   gwm-desktop <session>…           also open these sessions as tabs
//!   gwm-desktop --screenshot <png>   (for testing) save a picture of the window after a few seconds and quit

mod app;
mod gwm;
mod png;
mod term;
mod theme;

fn main() -> eframe::Result {
    // Don't hand this process's terminal/Claude Code markers to the sessions it starts.
    for (k, _) in std::env::vars() {
        if k.starts_with("ZELLIJ") || k == "TMUX" || k == "CLAUDECODE" || k.starts_with("CLAUDE_CODE_") || k == "CLAUDE_PID" {
            // SAFETY: single-threaded here, before any thread or PTY is started.
            unsafe { std::env::remove_var(k) };
        }
    }
    let mut screenshot = None;
    let mut show = None;
    let mut open = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(std::path::PathBuf::from),
            "--show" => show = args.next(), // testing: open the palette, the new-session dialog, or end:<session>
            "-h" | "--help" => {
                println!("gwm-desktop [session…] — every machine and session from go-with-me (gwm) in one window");
                return Ok(());
            }
            _ => open.push(a),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("gwm-desktop")
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([760.0, 420.0])
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false),
        ..Default::default()
    };
    eframe::run_native(
        "gwm-desktop",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx, open, screenshot, show)))
        }),
    )
}
