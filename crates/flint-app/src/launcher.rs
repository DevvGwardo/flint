//! Hooter launcher. Usage: `hooter [WORKSPACE] [--workspace DIR] [--demo]
//! [--demo-stop N] [--demo-instant] [--demo-queue] [--demo-approval] [--demo-expand]
//! [--demo-long N] [--prompt TEXT] [--exit-after-turn] [--size WxH]
//! [--palette] [--changes] [--select-change]`. See `tools/blueprint/README.md`
//! for the automation flags and `FLINT_BP_*` hooks.

use crate::app::FlintApp;
use gpui_kit::*;

fn background_window_needs_edge(background: bool, capture: bool, frame_probe: bool) -> bool {
    background && (capture || frame_probe)
}

fn frame_probe_workload(options: &crate::app::Options, frames_enabled: bool) -> bool {
    frames_enabled
        && (options.diff_test.is_some() || options.stream_test.is_some() || options.scroll_test)
}

pub fn run() {
    crate::automation::mark_process_start();
    let options = crate::parse_options(std::env::args().skip(1));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            crate::init(cx);
            let (w, h) = options.window_size.unwrap_or((1440., 900.));
            let mut window_options = crate::window_options(cx, w, h);
            // Automation-only: harness runs must not steal keyboard focus
            // from whatever the user is typing into.
            let background = std::env::var_os("FLINT_BP_NO_ACTIVATE").is_some();
            window_options.focus = !background;
            // A centered background window can be completely covered, which
            // stops macOS display-link frames. Keep explicit captures and
            // frame workloads visible without changing window kind or focus.
            let frame_probe =
                frame_probe_workload(&options, std::env::var_os("FLINT_BP_FRAMES").is_some());
            if background_window_needs_edge(
                background,
                crate::automation::capture_enabled(),
                frame_probe,
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
    use super::{background_window_needs_edge, frame_probe_workload};
    use crate::app::Options;

    #[test]
    fn explicit_background_frame_workloads_stay_visible_without_focus() {
        for options in [
            Options {
                stream_test: Some(2000),
                ..Options::default()
            },
            Options {
                scroll_test: true,
                ..Options::default()
            },
            Options {
                diff_test: Some(10000),
                ..Options::default()
            },
        ] {
            assert!(!frame_probe_workload(&options, false));
            assert!(!background_window_needs_edge(
                false,
                false,
                frame_probe_workload(&options, true),
            ));
            assert!(background_window_needs_edge(
                true,
                false,
                frame_probe_workload(&options, true),
            ));
        }
    }

    #[test]
    fn ordinary_launches_do_not_become_edge_frame_probes() {
        for options in [
            Options::default(),
            Options {
                demo: true,
                ..Options::default()
            },
            Options {
                demo_long: Some(200),
                ..Options::default()
            },
        ] {
            assert!(!frame_probe_workload(&options, true));
        }
    }

    #[test]
    fn display_edge_is_only_for_background_capture_or_frame_probes() {
        for (background, capture, frame_probe, expected) in [
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
                background_window_needs_edge(background, capture, frame_probe),
                expected,
                "background={background}, capture={capture}, frame_probe={frame_probe}",
            );
        }
    }
}
