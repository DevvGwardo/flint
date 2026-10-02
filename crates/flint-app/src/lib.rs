//! flint: a native agent/coding workstation on GPUI.
//!
//! The binary (`src/main.rs`) is a thin launcher; everything lives here so the
//! UI integration tests in `tests/` can drive the real views.

pub mod agents;
pub mod app;
pub mod app_actions;
pub mod app_demo;
pub mod app_engine;
pub mod app_input;
pub mod app_store;
pub mod changes_panel;
pub mod composer;
pub mod demo;
pub mod diff;
pub mod engine;
pub mod header;
pub mod layout;
pub mod mention;
pub mod menus;
pub mod palette;
pub mod session;
pub mod settings;
pub mod settings_view;
pub mod sidebar;
pub mod slash;
pub mod store;
pub mod theme;
pub mod transcript;
pub mod turns;
pub mod ui;
pub mod view_model;

pub mod automation;
pub mod synthetic;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::app::Options;

/// Parses command-line flags (everything after the program name).
pub fn parse_options(args: impl IntoIterator<Item = String>) -> Options {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => options.demo = true,
            "--demo-instant" => options.demo_instant = true,
            "--demo-stop" => options.demo_stop = args.next().and_then(|n| n.parse().ok()),
            "--demo-approval" => options.demo_approval = true,
            "--demo-expand" => options.demo_expand = true,
            "--prompt" => options.prompt = args.next(),
            "--workspace" => options.workspace = args.next().map(Into::into),
            "--exit-after-turn" => options.exit_after_turn = true,
            "--demo-long" => options.demo_long = args.next().and_then(|n| n.parse().ok()),
            "--stream-test" => options.stream_test = args.next().and_then(|n| n.parse().ok()),
            "--scroll-test" => options.scroll_test = true,
            "--settings" => options.open_settings = true,
            "--mention" => options.open_mention = true,
            "--slash" => options.open_slash = true,
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

/// Global key bindings.
pub fn bind_keys(cx: &mut App) {
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
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-r", RevealWorkspace, None),
        KeyBinding::new("cmd-shift-t", OpenTerminal, None),
        // While a composer menu is open, these keys drive it instead of the input.
        KeyBinding::new("up", MenuUp, Some("menu > Input")),
        KeyBinding::new("down", MenuDown, Some("menu > Input")),
        KeyBinding::new("enter", MenuAccept, Some("menu > Input")),
        KeyBinding::new("tab", MenuAccept, Some("menu > Input")),
        KeyBinding::new("escape", MenuDismiss, Some("menu > Input")),
        // Ahead of the input's own Shift+Tab (outdent) inside the composer.
        KeyBinding::new("shift-tab", ToggleApproval, Some("FlintApp > Input")),
        KeyBinding::new("shift-tab", ToggleApproval, Some("FlintApp")),
    ]);
}

/// Window options for the main window at the given size.
pub fn window_options(cx: &App, width: f32, height: f32) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
    WindowOptions {
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
    }
}

/// App-wide setup shared by the binary and the UI tests.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    theme::install(cx);
    bind_keys(cx);
    cx.on_action(|_: &app::Quit, cx| cx.quit());
}
