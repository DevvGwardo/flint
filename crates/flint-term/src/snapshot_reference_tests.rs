//! Frozen snapshot builder for exact compatibility checks.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::Color;
use alacritty_terminal::vte::ansi::CursorShape as AnsiCursorShape;
use alacritty_terminal::vte::ansi::NamedColor;

use crate::palette::Palette;
use crate::snapshot::{Cursor, CursorShape, Run, SelectionSpan, Snapshot};

pub(super) fn build<T: alacritty_terminal::event::EventListener>(
    term: &Term<T>,
    palette: &Palette,
) -> Snapshot {
    let content = term.renderable_content();
    let cols = term.columns();
    let rows = term.screen_lines();
    let offset = content.display_offset as i32;
    let colors = content.colors;
    let mut lines: Vec<Vec<Run>> = vec![Vec::new(); rows];
    for indexed in content.display_iter {
        let row = (indexed.point.line.0 + offset) as usize;
        if row >= rows {
            continue;
        }
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let bold = cell.flags.contains(Flags::BOLD);
        let fg_color = match cell.fg {
            Color::Named(name) if bold && (name as usize) < 8 => Color::Indexed(name as u8 + 8),
            other => other,
        };
        let mut fg = palette.resolve(fg_color, colors);
        let mut bg = match cell.bg {
            Color::Named(NamedColor::Background) => None,
            other => Some(palette.resolve(other, colors)),
        };
        if cell.flags.contains(Flags::DIM) {
            fg = fg.mix(palette.background, 0.35);
        }
        if cell.flags.contains(Flags::INVERSE) {
            let back = bg.unwrap_or(palette.background);
            bg = Some(fg);
            fg = back;
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = bg.unwrap_or(palette.background);
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let mut text = String::new();
        text.push(cell.c);
        if let Some(zero_width) = cell.zerowidth() {
            text.extend(zero_width);
        }
        let style = (
            fg,
            bg,
            bold,
            cell.flags.contains(Flags::ITALIC),
            cell.flags.intersects(Flags::ALL_UNDERLINES),
            cell.flags.contains(Flags::STRIKEOUT),
        );
        let col = indexed.point.column.0;
        let line = &mut lines[row];
        match line.last_mut() {
            Some(run)
                if run.col + run.width == col
                    && (
                        run.fg,
                        run.bg,
                        run.bold,
                        run.italic,
                        run.underline,
                        run.strikeout,
                    ) == style =>
            {
                run.text.push_str(&text);
                run.width += width;
            }
            _ => line.push(Run {
                col,
                width,
                text,
                fg: style.0,
                bg: style.1,
                bold: style.2,
                italic: style.3,
                underline: style.4,
                strikeout: style.5,
            }),
        }
    }
    for line in &mut lines {
        if let Some(last) = line.last_mut()
            && last.bg.is_none()
            && !last.underline
        {
            let trimmed = last.text.trim_end_matches(' ').len();
            let removed = last.text.len() - trimmed;
            last.text.truncate(trimmed);
            last.width -= removed;
        }
        line.retain(|run| run.bg.is_some() || run.underline || run.text.chars().any(|c| c != ' '));
    }
    let cursor_point: Point = content.cursor.point;
    let cursor_row = cursor_point.line.0 + offset;
    let shape = match content.cursor.shape {
        _ if !content.mode.contains(TermMode::SHOW_CURSOR) => CursorShape::Hidden,
        _ if cursor_row < 0 || cursor_row as usize >= rows => CursorShape::Hidden,
        AnsiCursorShape::Block | AnsiCursorShape::HollowBlock => CursorShape::Block,
        AnsiCursorShape::Underline => CursorShape::Underline,
        AnsiCursorShape::Beam => CursorShape::Beam,
        AnsiCursorShape::Hidden => CursorShape::Hidden,
    };
    let wide = term.grid()[cursor_point.line][cursor_point.column]
        .flags
        .contains(Flags::WIDE_CHAR);
    let cursor = Cursor {
        row: cursor_row.max(0) as usize,
        col: cursor_point.column.0,
        shape,
        wide,
    };
    let mut selection = Vec::new();
    if let Some(range) = content.selection {
        for line in range.start.line.0..=range.end.line.0 {
            let row = line + offset;
            if row < 0 || row as usize >= rows {
                continue;
            }
            let start = if line == range.start.line.0 || range.is_block {
                range.start.column.0
            } else {
                0
            };
            let end = if line == range.end.line.0 || range.is_block {
                range.end.column.0
            } else {
                cols - 1
            };
            selection.push(SelectionSpan {
                row: row as usize,
                start,
                end,
            });
        }
    }
    Snapshot {
        cols,
        rows,
        lines,
        cursor,
        selection,
        display_offset: content.display_offset,
        history: term.history_size(),
        app_cursor: content.mode.contains(TermMode::APP_CURSOR),
        bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
    }
}
