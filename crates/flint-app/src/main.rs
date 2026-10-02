//! flint: a native agent/coding workstation on GPUI.
//!
//! Usage: `flint [WORKSPACE] [--demo] [--demo-stop N] [--demo-instant]
//!        [--demo-approval] [--prompt TEXT] [--palette] [--changes] [--select-change]`

mod app;
mod changes_panel;
mod composer;
mod demo;
mod diff;
mod engine;
mod palette;
mod session;
mod sidebar;
mod status_bar;
mod theme;
mod title_bar;
mod transcript;
mod ui;
mod view_model;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::Options;

fn parse_options() -> Options {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => options.demo = true,
            "--demo-instant" => options.demo_instant = true,
            "--demo-stop" => options.demo_stop = args.next().and_then(|n| n.parse().ok()),
            "--demo-approval" => options.demo_approval = true,
            "--prompt" => options.prompt = args.next(),
            "--palette" => options.open_palette = true,
            "--changes" => options.open_changes = true,
            "--select-change" => options.select_change = true,
            path if !path.starts_with("--") => options.workspace = Some(path.into()),
            other => eprintln!("flint: ignoring unknown flag {other}"),
        }
    }
    options
}

fn bind_keys(cx: &mut App) {
    use app::*;
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewSession, None),
        KeyBinding::new("cmd-k", TogglePalette, None),
        KeyBinding::new("cmd-shift-p", TogglePalette, None),
        KeyBinding::new("cmd-j", ToggleChanges, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-a", ToggleApproval, None),
        KeyBinding::new("cmd-o", OpenWorkspace, None),
        KeyBinding::new("cmd-.", Interrupt, None),
        KeyBinding::new("escape", Interrupt, Some("FlintApp")),
        KeyBinding::new("cmd-l", FocusComposer, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}

fn main() {
    let options = parse_options();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            bind_keys(cx);
            cx.on_action(|_: &app::Quit, cx| cx.quit());

            let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(560.))),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| FlintApp::new(options, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
