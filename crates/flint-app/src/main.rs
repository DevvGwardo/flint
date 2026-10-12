//! Backward-compatible Flint launcher.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    flint_app::run();
}
