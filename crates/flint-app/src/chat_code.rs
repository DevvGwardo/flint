//! Native code-block actions: clipboard or ordinary draft only, never dispatch.

use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use gpui_kit::base::text::CodeBlock;
use gpui_kit::base::{Disableable as _, TextViewStyle};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::*;

use crate::{
    app::FlintApp,
    chat_content::RowKey,
    view_model::{Item, RowIdentity},
};

pub(crate) type CodeHook = dyn Fn(&CodeBlock, &mut Window, &mut App) -> AnyElement + Send + Sync;

/// A painted native block's immutable ownership receipt, not engine authority.
#[derive(Clone, Debug)]
pub struct CodeAction {
    pub(crate) key: RowKey,
    pub(crate) identity: RowIdentity,
    pub(crate) span: Range<usize>,
    pub(crate) source: String,
    pub(crate) code: SharedString,
    pub(crate) language: Option<SharedString>,
    horizontal_offset: Arc<AtomicU32>,
}

impl CodeAction {
    /// Actual native block scroll observed while prepainting its action header.
    pub fn horizontal_offset(&self) -> f32 {
        f32::from_bits(self.horizontal_offset.load(Ordering::Relaxed))
    }
}

pub(crate) fn style(cx: &App) -> TextViewStyle {
    let mut block = StyleRefinement::default().pt(px(68.)).whitespace_nowrap();
    block.overflow.x = Some(Overflow::Scroll);
    TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx)).with_code_block(block)
}

pub(crate) fn hook(
    key: RowKey,
    app: WeakEntity<FlintApp>,
    actions: Arc<Mutex<HashMap<usize, CodeAction>>>,
    origin: Arc<AtomicU32>,
) -> Arc<CodeHook> {
    Arc::new(move |block, _, cx| {
        let language = block.lang();
        let label = language.as_deref().unwrap_or("Plain text");
        let offset = block.span.map_or(0, |span| span.start);
        let previous = actions.lock().expect("code receipts").get(&offset).cloned();
        let receipt = app.upgrade().and_then(|app| {
            let app = app.read(cx);
            previous
                .filter(|action| {
                    app.valid_code_action(action)
                        && action.code == block.code()
                        && action.language == language
                })
                .or_else(|| app.code_action(&key, block))
        });
        {
            let mut actions = actions.lock().expect("code receipts");
            if let Some(action) = &receipt {
                actions.insert(offset, action.clone());
            } else {
                actions.remove(&offset);
            }
        }
        let unavailable = receipt.is_none();
        let child = key.child.is_some();
        let id = |action: &str| key.element_id(&format!("code-{action}-{offset}"));
        let horizontal_offset = receipt
            .as_ref()
            .map(|receipt| receipt.horizontal_offset.clone());
        let copy = receipt.clone();
        let stage = receipt;
        let copy_app = app.clone();
        let stage_app = app.clone();
        let p = crate::theme::palette();
        let header = div()
            .id(id("header"))
            .w(px(220.))
            .max_w_full()
            .min_w_0()
            .text_color(p.text_muted)
            .text_size(px(crate::theme::size::XS))
            .font_family(crate::theme::MONO_FONT)
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id(id("language"))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(label.to_string())
                            .test_support(),
                    )
                    .child(
                        Button::new(id("copy"))
                            .small()
                            .ghost()
                            .label("Copy")
                            .disabled(unavailable)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                gpui_kit::base::GlobalState::suppress_text_selection(cx);
                                cx.stop_propagation();
                            })
                            .on_click(move |_, _, cx| {
                                if let Some(action) = &copy {
                                    copy_app
                                        .update(cx, |app, cx| app.copy_code(action, cx))
                                        .ok();
                                }
                                cx.stop_propagation();
                            }),
                    )
                    .child(
                        Button::new(id("stage"))
                            .small()
                            .ghost()
                            .label("Stage")
                            .accessibility_label(if child {
                                "Stage unavailable: child is read-only"
                            } else {
                                "Stage code in ordinary draft only"
                            })
                            .disabled(child || unavailable)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                gpui_kit::base::GlobalState::suppress_text_selection(cx);
                                cx.stop_propagation();
                            })
                            .on_click(move |_, window, cx| {
                                if let Some(action) = &stage {
                                    stage_app
                                        .update(cx, |app, cx| app.stage_code(action, window, cx))
                                        .ok();
                                }
                                cx.stop_propagation();
                            }),
                    ),
            )
            .child(
                div()
                    .id(id("notice"))
                    .aria_label(if child {
                        "Child read-only: Stage disabled"
                    } else {
                        "Stage adds to draft only"
                    })
                    .font_family(crate::theme::DISPLAY_FONT)
                    .child(if child {
                        "Child read-only · Stage disabled"
                    } else if unavailable {
                        "Source changed or still parsing"
                    } else {
                        "Stage adds to draft only"
                    })
                    .test_support(),
            )
            .test_support();
        // Native code blocks scroll their children, including the action hook.
        // Chat lists scroll vertically only; keep this header at the block's
        // unscrolled horizontal origin while leaving vertical scrolling intact.
        HorizontalHeader {
            inner: header.into_any_element(),
            horizontal_offset,
            origin: origin.clone(),
        }
        .into_any_element()
    })
}

impl FlintApp {
    pub fn code_action(&self, key: &RowKey, block: &CodeBlock) -> Option<CodeAction> {
        let view = self.conversation_view(&key.target())?;
        let row = view.row_index(key.row)?;
        let Item::Assistant { text, .. } = &view.items[row] else {
            return None;
        };
        let span = block.span?;
        let source = text.get(span.start..span.end)?.to_string();
        let code = block.code();
        let language = block.lang();
        // Verify the native async parse corresponds to the current canonical
        // span before minting an action receipt. Incomplete/stale blocks wait.
        let (value, source_language) = fenced_value(&source, text.get(..span.start)?)?;
        if value != code.as_ref() || source_language.as_deref() != language.as_deref() {
            return None;
        }
        Some(CodeAction {
            key: key.clone(),
            identity: view.row_identity(row)?,
            span: span.start..span.end,
            source,
            code,
            language,
            horizontal_offset: Arc::new(AtomicU32::new(0f32.to_bits())),
        })
    }

    fn valid_code_action(&self, action: &CodeAction) -> bool {
        let Some(view) = self.conversation_view(&action.key.target()) else {
            return false;
        };
        let Some(row) = view.row_index(action.key.row) else {
            return false;
        };
        view.row_identity(row) == Some(action.identity)
            && matches!(&view.items[row], Item::Assistant { text, .. }
                if text.get(action.span.clone()) == Some(action.source.as_str()))
    }

    pub fn copy_code(&mut self, action: &CodeAction, cx: &mut Context<Self>) {
        if self.valid_code_action(action) {
            cx.write_to_clipboard(ClipboardItem::new_string(action.code.to_string()));
        }
    }

    pub fn stage_code(&mut self, action: &CodeAction, window: &mut Window, cx: &mut Context<Self>) {
        // Never target a hidden parent's or another pane's composer.
        if action.key.child.is_some()
            || self.session().uid != action.key.parent
            || self.session().selected_subagent.is_some()
            || !self.valid_code_action(action)
        {
            return;
        }
        let snippet = format!(
            "\n\n{}\n\n",
            fenced_snippet(&action.code, action.language.as_deref())
        );
        // insert preserves all existing text (even an active selection), the
        // retained input entity/undo history and attachment/paste lifetimes.
        self.composer
            .update(cx, |input, cx| input.insert(snippet, window, cx));
        cx.notify();
    }
}

fn fenced_snippet(code: &str, language: Option<&str>) -> String {
    let fence = "`".repeat(
        code.split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(3),
    );
    let language = language
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace() && *c != '`')
        .collect::<String>();
    format!("{fence}{language}\n{code}\n{fence}")
}

/// Canonical fenced span, with the blockquote/list continuation prefix removed.
fn fenced_value(source: &str, before: &str) -> Option<(String, Option<String>)> {
    let prefix = before.rsplit('\n').next().unwrap_or("");
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.lines();
    let first = lines.next()?.trim_start();
    let marker = first.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = first.chars().take_while(|c| *c == marker).count();
    if length < 3 {
        return None;
    }
    let language = first[length..]
        .split_whitespace()
        .next()
        .map(str::to_string);
    let mut body: Vec<&str> = lines.collect();
    let closing = body
        .pop()?
        .strip_prefix(prefix)
        .unwrap_or_else(|| normalized.lines().last().unwrap())
        .trim();
    if closing.chars().count() < length || !closing.chars().all(|c| c == marker) {
        return None;
    }
    Some((
        body.into_iter()
            .map(|line| line.strip_prefix(prefix).unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n"),
        language,
    ))
}

struct HorizontalHeader {
    inner: AnyElement,
    horizontal_offset: Option<Arc<AtomicU32>>,
    origin: Arc<AtomicU32>,
}
impl IntoElement for HorizontalHeader {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for HorizontalHeader {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.inner.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let offset = window.element_offset();
        let origin = px(f32::from_bits(self.origin.load(Ordering::Relaxed)));
        if let Some(observed) = &self.horizontal_offset {
            observed.store(f32::from(offset.x - origin).to_bits(), Ordering::Relaxed);
        }
        window.with_element_offset(point(origin - offset.x, px(0.)), |window| {
            self.inner.prepaint(window, cx)
        });
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}

/// Before native text prepaint, mark physical width reflows selection-compatible
/// and capture the owning list/pane's offset (not the whole-window origin).
pub(crate) struct TextFrame {
    inner: AnyElement,
    state: Entity<gpui_kit::component::text::TextViewState>,
    origin: Arc<AtomicU32>,
}
impl TextFrame {
    pub(crate) fn new(
        inner: AnyElement,
        state: Entity<gpui_kit::component::text::TextViewState>,
        origin: Arc<AtomicU32>,
    ) -> Self {
        Self {
            inner,
            state,
            origin,
        }
    }
}
impl IntoElement for TextFrame {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TextFrame {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.inner.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.origin.store(
            f32::from(window.element_offset().x).to_bits(),
            Ordering::Relaxed,
        );
        if self.state.read(cx).bounds().size.width != bounds.size.width {
            self.state
                .update(cx, |state, cx| state.invalidate_inline_layout(cx));
        }
        self.inner.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.paint(window, cx);
    }
}
