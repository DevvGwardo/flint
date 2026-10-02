//! flint: a native agent/coding workstation on GPUI.
//!
//! Usage: `flint [WORKSPACE] [--demo] [--demo-stop N] [--demo-instant]
//!        [--demo-approval] [--demo-expand] [--prompt TEXT] [--size WxH] [--palette] [--changes] [--select-change]`

mod app;
mod app_actions;
mod changes_panel;
mod composer;
mod demo;
mod diff;
mod engine;
mod header;
mod layout;
mod palette;
mod session;
mod sidebar;
mod theme;
mod transcript;
mod turns;
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
            "--demo-expand" => options.demo_expand = true,
            "--prompt" => options.prompt = args.next(),
            "--palette" => options.open_palette = true,
            "--size" => {
                options.window_size = args.next().and_then(|s| {
                    let (w, h) = s.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                });
            }
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

            let (w, h) = options.window_size.unwrap_or((1440., 900.));
            let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(560.))),
                // Vibrancy behind the floating sidebar.
                window_background: WindowBackgroundAppearance::Blurred,
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    // Inside the floating sidebar's top row.
                    traffic_light_position: Some(point(px(20.), px(21.))),
                }),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| FlintApp::new(options, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
