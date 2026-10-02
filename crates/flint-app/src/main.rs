//! flint launcher. Usage: `flint [WORKSPACE] [--workspace DIR] [--demo]
//! [--demo-stop N] [--demo-instant] [--demo-approval] [--demo-expand]
//! [--demo-long N] [--prompt TEXT] [--exit-after-turn] [--size WxH]
//! [--palette] [--changes] [--select-change]`. See `tools/blueprint/README.md`
//! for the automation flags and `FLINT_BP_*` hooks.

use flint_app::app::FlintApp;
use gpui_kit::*;

fn main() {
    flint_app::automation::mark_process_start();
    let options = flint_app::parse_options(std::env::args().skip(1));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            flint_app::init(cx);
            let (w, h) = options.window_size.unwrap_or((1440., 900.));
            let mut window_options = flint_app::window_options(cx, w, h);
            // Automation-only: harness runs must not steal keyboard focus
            // from whatever the user is typing into.
            let background = std::env::var_os("FLINT_BP_NO_ACTIVATE").is_some();
            window_options.focus = !background;
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| FlintApp::new(options, window, cx))
            })
            .expect("failed to open window");
            if !background {
                cx.activate(true);
            }
        });
}
