//! Paints a terminal [`Snapshot`] on the cell grid: background runs, the
//! selection, text (forced to the cell width so columns line up; wide
//! characters take two cells), the cursor, IME text being composed and the
//! visual bell.

use flint_term::CursorShape;
use flint_term::Palette;
use flint_term::Rgb8;
use flint_term::Run;
use flint_term::Snapshot;
use gpui_kit::*;
use unicode_width::UnicodeWidthChar;

use crate::theme::MONO_FONT;

pub const FONT_SIZE: f32 = 13.;
/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.38;

/// Cell geometry for the current font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub cell: Size<Pixels>,
    pub origin: Point<Pixels>,
}

impl Metrics {
    /// Cell size for the mono font (origin filled in at paint time).
    pub fn measure(window: &Window) -> Self {
        let text = window.text_system();
        let font_id = text.resolve_font(&font(MONO_FONT));
        let size = px(FONT_SIZE);
        let width = text
            .advance(font_id, size, 'm')
            .map_or(px(FONT_SIZE * 0.6), |advance| advance.width);
        Self {
            cell: Size {
                width,
                height: px((FONT_SIZE * LINE_HEIGHT).round()),
            },
            origin: Point::default(),
        }
    }

    /// Columns and rows that fit `bounds`.
    pub fn grid(&self, bounds: Bounds<Pixels>) -> (u16, u16) {
        let cols = (bounds.size.width / self.cell.width).floor().max(2.) as u16;
        let rows = (bounds.size.height / self.cell.height).floor().max(1.) as u16;
        (cols, rows)
    }

    /// The cell under `position`, and whether it is the cell's right half.
    pub fn cell_at(
        &self,
        position: Point<Pixels>,
        snapshot_cols: usize,
        rows: usize,
    ) -> (usize, usize, bool) {
        let x = (position.x - self.origin.x) / self.cell.width;
        let y = (position.y - self.origin.y) / self.cell.height;
        let col = (x.max(0.) as usize).min(snapshot_cols.saturating_sub(1));
        let row = (y.max(0.) as usize).min(rows.saturating_sub(1));
        (row, col, x.fract() >= 0.5)
    }

    pub fn cell_bounds(&self, row: usize, col: usize, cells: usize) -> Bounds<Pixels> {
        Bounds {
            origin: point(
                self.origin.x + self.cell.width * col as f32,
                self.origin.y + self.cell.height * row as f32,
            ),
            size: Size {
                width: self.cell.width * cells as f32,
                height: self.cell.height,
            },
        }
    }
}

pub fn color(c: Rgb8) -> Hsla {
    rgb((u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)).into()
}

fn run_font(run: &Run) -> Font {
    let mut f = font(MONO_FONT);
    if run.bold {
        f.weight = FontWeight::BOLD;
    }
    if run.italic {
        f.style = FontStyle::Italic;
    }
    f
}

fn paint_text(
    text: &str,
    run: &Run,
    fg: Hsla,
    (origin, cells): (Point<Pixels>, usize),
    metrics: &Metrics,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let style = TextRun {
        len: text.len(),
        font: run_font(run),
        color: fg,
        background_color: None,
        underline: run.underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(fg),
            wavy: false,
        }),
        strikethrough: run.strikeout.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: Some(fg),
        }),
    };
    let width = metrics.cell.width * (cells.max(1) as f32 / text.chars().count().max(1) as f32);
    let line = window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(FONT_SIZE),
        &[style],
        Some(width),
    );
    line.paint(
        origin,
        metrics.cell.height,
        TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
}

/// Paints a run, placing wide characters on their two cells.
fn paint_run(
    run: &Run,
    row: usize,
    fg: Rgb8,
    metrics: &Metrics,
    window: &mut Window,
    cx: &mut App,
) {
    let fg = color(fg);
    if run.width == run.text.chars().count() {
        let origin = metrics.cell_bounds(row, run.col, 1).origin;
        paint_text(&run.text, run, fg, (origin, run.width), metrics, window, cx);
        return;
    }
    let mut col = run.col;
    let mut narrow = String::new();
    let mut narrow_start = col;
    for ch in run.text.chars() {
        let width = ch.width().unwrap_or(0);
        if width == 0 {
            narrow.push(ch);
            continue;
        }
        if width == 2 {
            let origin = metrics.cell_bounds(row, narrow_start, 1).origin;
            paint_text(
                &narrow,
                run,
                fg,
                (origin, narrow.chars().count()),
                metrics,
                window,
                cx,
            );
            narrow.clear();
            let origin = metrics.cell_bounds(row, col, 1).origin;
            paint_text(&ch.to_string(), run, fg, (origin, 2), metrics, window, cx);
            col += 2;
            narrow_start = col;
        } else {
            if narrow.is_empty() {
                narrow_start = col;
            }
            narrow.push(ch);
            col += 1;
        }
    }
    let origin = metrics.cell_bounds(row, narrow_start, 1).origin;
    paint_text(
        &narrow,
        run,
        fg,
        (origin, narrow.chars().count()),
        metrics,
        window,
        cx,
    );
}

/// The character at a column, if a run covers it.
fn char_at(snapshot: &Snapshot, row: usize, col: usize) -> Option<(char, &Run)> {
    let run = snapshot
        .lines
        .get(row)?
        .iter()
        .find(|r| col >= r.col && col < r.col + r.width)?;
    let mut at = run.col;
    for ch in run.text.chars() {
        let width = ch.width().unwrap_or(0);
        if width == 0 {
            continue;
        }
        if col < at + width {
            return Some((ch, run));
        }
        at += width;
    }
    None
}

/// Everything the painter needs besides the snapshot.
pub struct PaintState {
    pub focused: bool,
    pub marked: Option<String>,
    pub bell: bool,
    pub palette: Palette,
}

pub fn paint(
    bounds: Bounds<Pixels>,
    snapshot: &Snapshot,
    metrics: &Metrics,
    state: &PaintState,
    window: &mut Window,
    cx: &mut App,
) {
    let palette = &state.palette;
    window.paint_quad(fill(bounds, color(palette.background)));
    for (row, runs) in snapshot.lines.iter().enumerate() {
        for run in runs {
            if let Some(bg) = run.bg {
                window.paint_quad(fill(
                    metrics.cell_bounds(row, run.col, run.width),
                    color(bg),
                ));
            }
        }
    }
    let selection = color(palette.selection);
    for span in &snapshot.selection {
        let cells = span.end.saturating_sub(span.start) + 1;
        window.paint_quad(fill(
            metrics.cell_bounds(span.row, span.start, cells),
            selection,
        ));
    }
    for (row, runs) in snapshot.lines.iter().enumerate() {
        for run in runs {
            paint_run(run, row, run.fg, metrics, window, cx);
        }
    }

    let cursor = snapshot.cursor;
    let cursor_color = color(palette.cursor);
    let cell = metrics.cell_bounds(cursor.row, cursor.col, if cursor.wide { 2 } else { 1 });
    match (state.marked.as_deref(), cursor.shape) {
        (Some(marked), _) if !marked.is_empty() => {
            // IME composition: shown at the cursor, underlined.
            let width: usize = marked.chars().map(|c| c.width().unwrap_or(0)).sum();
            let run = Run {
                col: cursor.col,
                width,
                text: marked.to_string(),
                fg: palette.foreground,
                bg: None,
                bold: false,
                italic: false,
                underline: true,
                strikeout: false,
            };
            let area = metrics.cell_bounds(cursor.row, cursor.col, width.max(1));
            window.paint_quad(fill(
                area,
                color(palette.background.mix(palette.foreground, 0.12)),
            ));
            paint_run(&run, cursor.row, palette.foreground, metrics, window, cx);
        }
        (_, CursorShape::Hidden) => {}
        (_, CursorShape::Block) if state.focused => {
            window.paint_quad(fill(cell, cursor_color));
            if let Some((ch, run)) = char_at(snapshot, cursor.row, cursor.col) {
                paint_text(
                    &ch.to_string(),
                    run,
                    color(palette.background),
                    (cell.origin, if cursor.wide { 2 } else { 1 }),
                    metrics,
                    window,
                    cx,
                );
            }
        }
        (_, CursorShape::Block) => {
            let t = px(1.);
            let Bounds { origin, size } = cell;
            for edge in [
                Bounds {
                    origin,
                    size: Size {
                        width: size.width,
                        height: t,
                    },
                },
                Bounds {
                    origin: point(origin.x, origin.y + size.height - t),
                    size: Size {
                        width: size.width,
                        height: t,
                    },
                },
                Bounds {
                    origin,
                    size: Size {
                        width: t,
                        height: size.height,
                    },
                },
                Bounds {
                    origin: point(origin.x + size.width - t, origin.y),
                    size: Size {
                        width: t,
                        height: size.height,
                    },
                },
            ] {
                window.paint_quad(fill(edge, cursor_color));
            }
        }
        (_, CursorShape::Beam) => {
            window.paint_quad(fill(
                Bounds {
                    origin: cell.origin,
                    size: Size {
                        width: px(2.),
                        height: cell.size.height,
                    },
                },
                cursor_color,
            ));
        }
        (_, CursorShape::Underline) => {
            let origin = point(cell.origin.x, cell.origin.y + cell.size.height - px(2.));
            window.paint_quad(fill(
                Bounds {
                    origin,
                    size: Size {
                        width: cell.size.width,
                        height: px(2.),
                    },
                },
                cursor_color,
            ));
        }
    }

    if state.bell {
        let t = px(2.);
        let Bounds { origin, size } = bounds;
        let flash = color(palette.cursor).opacity(0.7);
        window.paint_quad(fill(
            Bounds {
                origin,
                size: Size {
                    width: size.width,
                    height: t,
                },
            },
            flash,
        ));
        window.paint_quad(fill(
            Bounds {
                origin: point(origin.x, origin.y + size.height - t),
                size: Size {
                    width: size.width,
                    height: t,
                },
            },
            flash,
        ));
    }
}
