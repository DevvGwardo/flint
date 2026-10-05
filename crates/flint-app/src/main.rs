//! flint launcher. Usage: `flint [WORKSPACE] [--workspace DIR] [--demo]
//! [--demo-stop N] [--demo-instant] [--demo-queue] [--demo-approval] [--demo-expand]
//! [--demo-long N] [--prompt TEXT] [--exit-after-turn] [--size WxH]
//! [--palette] [--changes] [--select-change]`. See `tools/blueprint/README.md`
//! for the automation flags and `FLINT_BP_*` hooks.

use flint_app::app::FlintApp;
use gpui_kit::*;

fn background_window_needs_edge(background: bool, capture: bool, diff_frames: bool) -> bool {
    background && (capture || diff_frames)
}

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
            // A centered background window can be completely covered, which
            // stops macOS display-link frames. Keep captures and the diff
            // frame probe visible without changing window kind or focus.
            let diff_frames =
                options.diff_test.is_some() && std::env::var_os("FLINT_BP_FRAMES").is_some();
            if background_window_needs_edge(
                background,
                flint_app::automation::capture_enabled(),
                diff_frames,
            ) && let Some(display) = cx.primary_display()
            {
                window_options.window_bounds = Some(WindowBounds::Windowed(Bounds {
                    origin: display.bounds().origin,
                    size: size(px(w), px(h)),
                }));
            }
            gpui_kit::open_window(window_options, cx, move |window, cx| {
                cx.new(|cx| FlintApp::new(options, window, cx))
            })
            .expect("failed to open window");
            if !background {
                cx.activate(true);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::background_window_needs_edge;

    #[test]
    fn display_edge_is_only_for_background_capture_or_diff_frame_probes() {
        for (background, capture, diff_frames, expected) in [
            (false, false, false, false),
            (false, false, true, false),
            (false, true, false, false),
            (false, true, true, false),
            (true, false, false, false),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, true),
        ] {
            assert_eq!(
                background_window_needs_edge(background, capture, diff_frames),
                expected,
                "background={background}, capture={capture}, diff_frames={diff_frames}",
            );
        }
    }
}
