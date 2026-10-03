//! Unified diff rendering: gutter markers, tinted add/remove rows, hunk headers.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{
    TextSelectionContentKey, TextSelectionCoverage, TextSelectionHandle, TextSelectionRegistration,
    TextSelectionRun, TextSelectionSnapshot,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub body: SharedString,
    pub marker: &'static str,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

pub struct ParsedDiff {
    pub lines: Vec<DiffLine>,
    widest: usize,
    text: String,
    offsets: Vec<usize>,
}

impl ParsedDiff {
    pub fn new(unified: &str) -> Self {
        let mut lines = Vec::new();
        let (mut old, mut new) = (0usize, 0usize);
        let (mut old_left, mut new_left) = (0usize, 0usize);
        for line in unified.lines() {
            if let Some((a, b)) = hunk_ranges(line) {
                (old, old_left) = a;
                (new, new_left) = b;
                lines.push(DiffLine {
                    body: line.to_string().into(),
                    marker: "",
                    old: None,
                    new: None,
                });
                continue;
            }
            let in_hunk = old_left > 0 || new_left > 0;
            if !in_hunk
                && (line.starts_with("--- ")
                    || line.starts_with("+++ ")
                    || line.starts_with("diff --git ")
                    || line.starts_with("index "))
            {
                continue;
            }
            let (marker, body, a, b) = match line.as_bytes().first() {
                Some(b'+') => {
                    let number = in_hunk.then_some(new);
                    new = new.saturating_add(1);
                    new_left = new_left.saturating_sub(1);
                    ("+", &line[1..], None, number)
                }
                Some(b'-') => {
                    let number = in_hunk.then_some(old);
                    old = old.saturating_add(1);
                    old_left = old_left.saturating_sub(1);
                    ("−", &line[1..], number, None)
                }
                Some(b' ') => {
                    let numbers = (in_hunk.then_some(old), in_hunk.then_some(new));
                    old = old.saturating_add(1);
                    new = new.saturating_add(1);
                    old_left = old_left.saturating_sub(1);
                    new_left = new_left.saturating_sub(1);
                    (" ", &line[1..], numbers.0, numbers.1)
                }
                _ => ("", line, None, None),
            };
            lines.push(DiffLine {
                body: body.to_string().into(),
                marker,
                old: a,
                new: b,
            });
        }
        let widest = lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| unicode_width::UnicodeWidthStr::width(line.body.as_ref()))
            .map_or(0, |(ix, _)| ix);
        let mut text = String::new();
        let mut offsets = Vec::with_capacity(lines.len());
        for line in &lines {
            if !offsets.is_empty() {
                text.push('\n');
            }
            offsets.push(text.len());
            text.push_str(&line.body);
        }
        Self {
            lines,
            widest,
            text,
            offsets,
        }
    }
}

fn hunk_ranges(line: &str) -> Option<((usize, usize), (usize, usize))> {
    let mut fields = line.strip_prefix("@@ ")?.split_whitespace();
    let parse = |range: &str, prefix| {
        let range = range.strip_prefix(prefix)?;
        let (start, count) = range.split_once(',').unwrap_or((range, "1"));
        Some((start.parse().ok()?, count.parse().ok()?))
    };
    Some((parse(fields.next()?, '-')?, parse(fields.next()?, '+')?))
}

pub(crate) struct CachedDiff {
    uid: u64,
    path: String,
    revision: u64,
    parsed: Rc<ParsedDiff>,
}

pub fn render_virtual(
    app: &crate::app::FlintApp,
    file: &crate::view_model::ChangedFile,
) -> impl IntoElement {
    let uid = app.session().uid;
    let revision = app.session().view.changes_revision;
    let mut cache = app.change_diff_cache.borrow_mut();
    if !cache
        .as_ref()
        .is_some_and(|entry| entry.uid == uid && entry.path == file.path)
    {
        app.change_diff_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(Point::default());
        *cache = None;
    }
    if cache
        .as_ref()
        .is_none_or(|entry| entry.revision != revision)
    {
        let unified = file
            .combined
            .as_deref()
            .map(std::borrow::Cow::Borrowed)
            .unwrap_or_else(|| std::borrow::Cow::Owned(file.diffs.join("\n")));
        *cache = Some(CachedDiff {
            uid,
            path: file.path.clone(),
            revision,
            parsed: Rc::new(ParsedDiff::new(&unified)),
        });
    }
    let parsed = cache
        .as_ref()
        .expect("diff cache initialized")
        .parsed
        .clone();
    let document_id = format!("diff-lines-{uid}-{revision}-{}", file.path);
    div()
        .id("change-diff")
        .size_full()
        .min_h_0()
        .font_family(MONO_FONT)
        .font_features(FontFeatures::disable_ligatures())
        .text_size(px(size::SM))
        .line_height(px(18.))
        .child(DiffDocument {
            id: document_id.into(),
            parsed,
            scroll: app.change_diff_scroll.clone(),
            child: None,
        })
        .test_support()
}

/// Renders up to `max_lines` lines of a unified diff; returns the element and
/// how many lines were left out. Long lines are clipped (see [`render_wide`]).
pub fn render(unified: &str, max_lines: usize) -> (Div, usize) {
    render_lines(unified, max_lines, false)
}

/// The whole diff with long lines kept intact, for a horizontally scrolling
/// container.
pub fn render_wide(unified: &str) -> Div {
    render_lines(unified, usize::MAX, true).0
}

fn render_lines(unified: &str, max_lines: usize, wide: bool) -> (Div, usize) {
    let parsed = ParsedDiff::new(unified);
    let hidden = parsed.lines.len().saturating_sub(max_lines);
    let rows = parsed
        .lines
        .iter()
        .take(max_lines)
        .enumerate()
        .map(|(ix, line)| render_row(line, ix, wide, false, None));
    let element = div()
        .when(!wide, |col| col.w_full())
        .when(wide, |col| col.min_w_full())
        .py(px(4.))
        .font_family(MONO_FONT)
        .font_features(FontFeatures::disable_ligatures())
        .text_size(px(size::SM))
        .line_height(px(18.))
        .flex()
        .flex_col()
        .children(rows);
    (element, hidden)
}

fn render_row(
    line: &DiffLine,
    ix: usize,
    wide: bool,
    numbered: bool,
    document: Option<Rc<DiffSelection>>,
) -> AnyElement {
    let p = palette();
    let (fg, bg) = match line.marker {
        "+" => (p.diff_add_fg, Some(p.diff_add_bg)),
        "−" => (p.diff_del_fg, Some(p.diff_del_bg)),
        "" if line.body.starts_with("@@") => (p.diff_hunk_fg, None),
        _ => (p.text_muted, None),
    };
    div()
        .id(("diff-row", ix))
        .flex()
        .when(!wide, |row| row.w_full())
        .when(wide, |row| row.min_w_full().flex_shrink_0())
        .h(px(18.))
        .flex_shrink_0()
        .when_some(bg, |row, bg| row.bg(bg))
        .when(numbered, |row| {
            row.children([line.old, line.new].map(|number| {
                div()
                    .w(px(54.))
                    .pr(px(8.))
                    .flex_shrink_0()
                    .text_right()
                    .text_color(p.text_subtle)
                    .child(number.map_or_else(String::new, |n| n.to_string()))
            }))
        })
        .child(
            div()
                .w(px(22.))
                .flex_shrink_0()
                .text_color(fg)
                .opacity(0.8)
                .flex()
                .justify_center()
                .child(line.marker),
        )
        .child(
            div()
                .when(!wide, |cell| cell.flex_1().min_w_0().overflow_hidden())
                .when(wide, |cell| cell.flex_shrink_0())
                .pr(px(16.))
                .text_color(fg)
                .whitespace_nowrap()
                .child(match document {
                    Some(document) => DiffText {
                        ix,
                        text: StyledText::new(line.body.clone()),
                        document,
                    }
                    .into_any_element(),
                    None => {
                        gpui_kit::base::SelectableText::new(("diff-text", ix), line.body.clone())
                            .document_order(ix as u64)
                            .into_any_element()
                    }
                }),
        )
        .test_support()
        .into_any_element()
}

#[derive(Default)]
struct DiffGeometry {
    bounds: Bounds<Pixels>,
    offset: Point<Pixels>,
    rows: Vec<(usize, TextLayout, Bounds<Pixels>)>,
}

struct DiffSelection {
    handle: TextSelectionHandle,
    parsed: Rc<ParsedDiff>,
    geometry: Rc<RefCell<DiffGeometry>>,
}

impl DiffSelection {
    fn new(parsed: Rc<ParsedDiff>, cx: &mut App) -> Rc<Self> {
        let handle = TextSelectionHandle::new("", cx);
        let geometry = Rc::new(RefCell::new(DiffGeometry::default()));
        let hit_geometry = geometry.clone();
        let content = parsed.clone();
        handle.resolve_content_key_with(
            move |point, _| {
                let geometry = hit_geometry.borrow();
                let row = (f32::from(point.y) / 18.).floor().max(0.) as usize;
                if row >= content.lines.len() {
                    return Some(TextSelectionContentKey::new(content.text.len() as u64));
                }
                let (_, layout, _) = geometry.rows.iter().find(|(ix, _, _)| *ix == row)?;
                let point = point + geometry.bounds.origin + geometry.offset;
                let line = layout.line_layout_for_index(0)?;
                let index = line
                    .closest_index_for_position(point - layout.bounds().origin, px(18.))
                    .unwrap_or_else(|index| index);
                Some(TextSelectionContentKey::new(
                    (content.offsets[row] + index.min(content.lines[row].body.len())) as u64,
                ))
            },
            cx,
        );
        let document = Rc::new(Self {
            handle,
            parsed,
            geometry,
        });
        let weak = Rc::downgrade(&document);
        document.handle.copy_with(
            move |cx| {
                let Some(document) = weak.upgrade() else {
                    return String::new();
                };
                selection_range(
                    document.handle.snapshot(cx),
                    document.handle.entity_id(),
                    document.parsed.text.len(),
                )
                .map_or_else(String::new, |range| document.parsed.text[range].to_string())
            },
            cx,
        );
        document
    }
}

fn selection_range(
    snapshot: Option<TextSelectionSnapshot>,
    owner: EntityId,
    length: usize,
) -> Option<Range<usize>> {
    let snapshot = snapshot?;
    let owned_key = |endpoint: gpui_kit::base::TextSelectionEndpoint| {
        (endpoint.entity_id() == Some(owner))
            .then(|| endpoint.content_key().map(|key| key.value() as usize))
            .flatten()
    };
    let anchor = owned_key(snapshot.anchor());
    let cursor = owned_key(snapshot.cursor());
    let range = match snapshot.coverage() {
        TextSelectionCoverage::Bounded => {
            let (a, b) = (anchor?, cursor?);
            a.min(b)..a.max(b)
        }
        TextSelectionCoverage::FromStart => 0..anchor.or(cursor)?,
        TextSelectionCoverage::ToEnd => anchor.or(cursor)?..length,
        TextSelectionCoverage::Full => 0..length,
    };
    Some(range.start.min(length)..range.end.min(length))
}

// The viewport owns selection; recycled rows only contribute visible glyph
// geometry. Copy uses stable byte offsets into the complete diff document.
struct DiffDocument {
    id: ElementId,
    parsed: Rc<ParsedDiff>,
    scroll: UniformListScrollHandle,
    child: Option<AnyElement>,
}

impl IntoElement for DiffDocument {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for DiffDocument {
    type RequestLayoutState = Rc<DiffSelection>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let document =
            window.with_element_state(id.unwrap(), |retained: Option<Rc<DiffSelection>>, _| {
                let document =
                    retained.unwrap_or_else(|| DiffSelection::new(self.parsed.clone(), cx));
                (document.clone(), document)
            });
        document.geometry.borrow_mut().rows.clear();
        let rows_document = document.clone();
        let parsed = self.parsed.clone();
        self.child = Some(
            uniform_list("diff-list", parsed.lines.len(), move |range, _, _| {
                range
                    .map(|ix| {
                        render_row(
                            &parsed.lines[ix],
                            ix,
                            true,
                            true,
                            Some(rows_document.clone()),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .size_full()
            .with_width_from_item(Some(self.parsed.widest))
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .track_scroll(&self.scroll)
            .into_any_element(),
        );
        (
            self.child.as_mut().unwrap().request_layout(window, cx),
            document,
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        document: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.child.as_mut().unwrap().prepaint(window, cx);
        let offset = self.scroll.0.borrow().base_handle.offset();
        let mut geometry = document.geometry.borrow_mut();
        geometry.bounds = bounds;
        geometry.offset = offset;
        let text_bounds = geometry.rows.iter().map(|(_, _, bounds)| *bounds).collect();
        let runs = geometry
            .rows
            .iter()
            .map(|(ix, layout, bounds)| {
                TextSelectionRun::new(
                    document.parsed.lines[*ix].body.clone(),
                    layout.clone(),
                    *bounds,
                )
                .with_document_order(*ix as u64)
            })
            .collect::<Vec<_>>();
        drop(geometry);
        document.handle.register(
            TextSelectionRegistration::new(hitbox, bounds)
                .with_scroll_offset(offset)
                .with_text_bounds(text_bounds)
                .with_rendered_element(&document.handle, window, cx),
            window,
            cx,
        );
        document.handle.update_runs(&runs, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.as_mut().unwrap().paint(window, cx);
    }
}

struct DiffText {
    ix: usize,
    text: StyledText,
    document: Rc<DiffSelection>,
}

impl IntoElement for DiffText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for DiffText {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(("diff-text", self.ix).into())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
        self.document.geometry.borrow_mut().rows.push((
            self.ix,
            self.text.layout().clone(),
            bounds,
        ));
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let line = &self.document.parsed.lines[self.ix];
        let offset = self.document.parsed.offsets[self.ix];
        if let Some(range) = selection_range(
            self.document.handle.snapshot(cx),
            self.document.handle.entity_id(),
            self.document.parsed.text.len(),
        ) {
            let start = range.start.max(offset);
            let end = range.end.min(offset + line.body.len());
            if start < end {
                let layout = self.text.layout();
                if let (Some(start), Some(end)) = (
                    layout.position_for_index(start - offset),
                    layout.position_for_index(end - offset),
                ) {
                    window.paint_quad(fill(
                        Bounds::from_corners(start, point(end.x, end.y + px(18.))),
                        gpui_kit::base::Theme::global(cx).tokens.colors.selection,
                    ));
                }
            }
        }
        self.text
            .paint(id, inspector, bounds, state, prepaint, window, cx);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn preserves_header_like_content_and_tracks_both_line_numbers() {
        let parsed = super::ParsedDiff::new(
            "--- a/x\n+++ b/x\n@@ -10,3 +20,3 @@\n context\n--- removed\n+++ inserted\n-end\n+next\n",
        );
        assert_eq!(parsed.lines.len(), 6);
        assert_eq!(
            (parsed.lines[1].old, parsed.lines[1].new),
            (Some(10), Some(20))
        );
        assert_eq!(parsed.lines[2].body.as_ref(), "-- removed");
        assert_eq!(parsed.lines[3].body.as_ref(), "++ inserted");
        assert_eq!((parsed.lines[4].old, parsed.lines[4].new), (Some(12), None));
        assert_eq!((parsed.lines[5].old, parsed.lines[5].new), (None, Some(22)));
    }

    #[test]
    fn omitted_counts_empty_ranges_and_no_newline_markers_are_supported() {
        let parsed = super::ParsedDiff::new(
            "@@ -0,0 +1 @@\n+new\n\\ No newline at end of file\n@@ -100 +200,0 @@\n-old\n",
        );
        assert_eq!((parsed.lines[1].old, parsed.lines[1].new), (None, Some(1)));
        assert_eq!((parsed.lines[2].old, parsed.lines[2].new), (None, None));
        assert_eq!(
            (parsed.lines[4].old, parsed.lines[4].new),
            (Some(100), None)
        );
    }

    #[test]
    fn widest_row_includes_long_unicode_lines_and_tabs() {
        let parsed =
            super::ParsedDiff::new("@@ -0,0 +1,3 @@\n+abc\n+\txyz\n+界界界界界界界界界界界界\n");
        assert_eq!(parsed.widest, 3);
        assert_eq!(parsed.lines[2].body.as_ref(), "\txyz");
    }
}
