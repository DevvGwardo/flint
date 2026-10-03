//! flint's terminal emulator: a PTY running the user's shell, alacritty's
//! VT parser and grid, key encoding and the theme palette. No UI toolkit:
//! the app paints [`Snapshot`]s and sends key bytes.

mod keys;
mod palette;
mod snapshot;
mod term;

pub use keys::Mods;
pub use keys::encode as encode_key;
pub use keys::paste as paste_bytes;
pub use palette::Palette;
pub use palette::Rgb8;
pub use snapshot::Cursor;
pub use snapshot::CursorShape;
pub use snapshot::Run;
pub use snapshot::SelectionSpan;
pub use snapshot::Snapshot;
pub use term::SCROLLBACK_LINES;
pub use term::Size;
pub use term::SpawnConfig;
pub use term::TermEvent;
pub use term::Terminal;
