//! Retained text belongs to UI entities, never to the canonical event projection.
//! Only demanded rows create entities; batch synchronization touches retained rows
//! and uses fold revisions/byte cursors, not full-text prefix comparisons.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::AtomicU32};

use gpui_kit::base::TextView;
use gpui_kit::component::button::Button;
use gpui_kit::component::text::TextViewState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::subagent_ui::Conversation;
use crate::view_model::{Change, Item, RowIdentity, SessionView};

pub const MAX_UNPINNED_ENTRIES: usize = 128;
pub const MAX_UNPINNED_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RICH_SOURCE_BYTES: usize = 512 * 1024;
pub const FALLBACK_SOURCE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RowKey {
    pub parent: u64,
    pub child: Option<String>,
    pub row: u64,
}

impl RowKey {
    pub fn new(target: &Conversation, identity: RowIdentity) -> Self {
        Self {
            parent: target.parent,
            child: target.child.clone(),
            row: identity.id,
        }
    }

    pub fn target(&self) -> Conversation {
        Conversation {
            parent: self.parent,
            child: self.child.clone(),
        }
    }

    pub fn element_id(&self, prefix: &str) -> ElementId {
        let conversation = match &self.child {
            Some(child) => format!("child-{child}"),
            None => "root".into(),
        };
        SharedString::from(format!(
            "{prefix}-{}-{conversation}-{}",
            self.parent, self.row
        ))
        .into()
    }
}

struct Entry {
    state: Entity<TextViewState>,
    code_hook: Arc<crate::chat_code::CodeHook>,
    code_actions: Arc<Mutex<HashMap<usize, crate::chat_code::CodeAction>>>,
    code_origin: Arc<AtomicU32>,
    identity: RowIdentity,
    ingested: usize,
    source_bytes: usize,
    fallback: bool,
    streaming: bool,
    demanded_frame: u64,
    touched: u64,
    append_updates: usize,
    replace_updates: usize,
    _subscription: Subscription,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub unpinned_entries: usize,
    pub unpinned_source_bytes: usize,
    pub pinned_entries: usize,
    pub pinned_source_bytes: usize,
    pub streaming_pins: usize,
    pub selection_pins: usize,
    pub recent_render_pins: usize,
}

#[derive(Default)]
pub struct ChatContentCache {
    entries: HashMap<RowKey, Entry>,
    frame: u64,
    clock: u64,
}

impl Entry {
    fn pinned(&self, frame: u64, cx: &App) -> bool {
        self.streaming
            || self.demanded_frame.saturating_add(1) >= frame
            || self.state.read(cx).is_selecting()
            || self.state.read(cx).selected_source_range().is_some()
    }

    fn sync(&mut self, identity: RowIdentity, text: &str, streaming: bool, cx: &mut App) {
        self.streaming = streaming;
        if self.identity == identity {
            return;
        }
        self.code_actions.lock().expect("code receipts").clear();
        let fallback = text.len() > MAX_RICH_SOURCE_BYTES;
        if fallback {
            // The displayed prefix is fixed during append-only oversize streams.
            // Replace it only on a trusted replacement or the threshold crossing.
            if !self.fallback || identity.replacement != self.identity.replacement {
                let source = literal_preview(text);
                self.state
                    .update(cx, |state, cx| state.set_text(&source, cx));
                self.source_bytes = source.len();
                self.replace_updates += 1;
            }
        } else if !self.fallback
            && identity.replacement == self.identity.replacement
            && text.len() >= self.ingested
            && text.is_char_boundary(self.ingested)
        {
            let delta = &text[self.ingested..];
            if !delta.is_empty() {
                self.state.update(cx, |state, cx| state.push_str(delta, cx));
                self.append_updates += 1;
            }
            self.source_bytes = text.len();
        } else {
            self.state.update(cx, |state, cx| state.set_text(text, cx));
            self.source_bytes = text.len();
            self.replace_updates += 1;
        }
        self.fallback = fallback;
        self.ingested = text.len();
        self.identity = identity;
    }
}

impl ChatContentCache {
    pub fn begin_frame(&mut self, cx: &App) {
        self.frame += 1;
        self.prune(cx);
    }

    /// Successful owner disposal releases every root/child entity and observer,
    /// including pin exceptions. Undo rehydrates from the canonical projection.
    pub(crate) fn retire_owner(&mut self, parent: u64) {
        self.entries.retain(|key, _| key.parent != parent);
    }

    pub fn state(&self, key: &RowKey) -> Option<Entity<TextViewState>> {
        self.entries.get(key).map(|entry| entry.state.clone())
    }

    /// Inspect a receipt minted by the actual native code action hook.
    pub fn code_action(&self, key: &RowKey, start: usize) -> Option<crate::chat_code::CodeAction> {
        self.entries
            .get(key)?
            .code_actions
            .lock()
            .ok()?
            .get(&start)
            .cloned()
    }

    /// Diagnostic counters for proving append/replacement integration in views.
    pub fn update_counts(&self, key: &RowKey) -> Option<(usize, usize)> {
        self.entries
            .get(key)
            .map(|entry| (entry.append_updates, entry.replace_updates))
    }

    pub fn stats(&self, cx: &App) -> CacheStats {
        let mut stats = CacheStats::default();
        for entry in self.entries.values() {
            stats.entries += 1;
            if entry.pinned(self.frame, cx) {
                stats.pinned_entries += 1;
                stats.pinned_source_bytes += entry.source_bytes;
            } else {
                stats.unpinned_entries += 1;
                stats.unpinned_source_bytes += entry.source_bytes;
            }
            stats.streaming_pins += usize::from(entry.streaming);
            stats.selection_pins += usize::from(
                entry.state.read(cx).is_selecting()
                    || entry.state.read(cx).selected_source_range().is_some(),
            );
            stats.recent_render_pins +=
                usize::from(entry.demanded_frame.saturating_add(1) >= self.frame);
        }
        stats
    }

    pub fn prune(&mut self, cx: &App) {
        loop {
            let stats = self.stats(cx);
            if stats.unpinned_entries <= MAX_UNPINNED_ENTRIES
                && stats.unpinned_source_bytes <= MAX_UNPINNED_SOURCE_BYTES
            {
                break;
            }
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| !entry.pinned(self.frame, cx))
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break };
            self.entries.remove(&oldest);
        }
    }

    pub fn sync_change(
        &mut self,
        target: &Conversation,
        view: &SessionView,
        change: &Change,
        cx: &mut App,
    ) {
        let mut rows = change.updated.clone();
        rows.extend(change.appended.clone());
        rows.sort_unstable();
        rows.dedup();
        for ix in rows {
            let Some(identity) = view.row_identity(ix) else {
                continue;
            };
            let Some(Item::Assistant { text, streaming }) = view.items.get(ix) else {
                continue;
            };
            if let Some(entry) = self.entries.get_mut(&RowKey::new(target, identity)) {
                entry.sync(identity, text, *streaming, cx);
            }
        }
        for (id, child_change) in &change.children {
            if let Some(child) = view.subagent(id) {
                let target = Conversation {
                    parent: target.parent,
                    child: Some(id.clone()),
                };
                self.sync_change(&target, &child.view, child_change, cx);
            }
        }
        self.prune(cx);
    }

    fn demand(
        &mut self,
        key: RowKey,
        identity: RowIdentity,
        text: &str,
        streaming: bool,
        cx: &mut Context<FlintApp>,
    ) -> (
        Entity<TextViewState>,
        bool,
        Arc<crate::chat_code::CodeHook>,
        Arc<AtomicU32>,
    ) {
        self.clock += 1;
        let entry = self.entries.entry(key.clone()).or_insert_with(|| {
            let fallback = text.len() > MAX_RICH_SOURCE_BYTES;
            let preview = fallback.then(|| literal_preview(text));
            let source = preview.as_deref().unwrap_or(text);
            let state = cx.new(|cx| TextViewState::markdown(source, cx));
            let code_actions = Arc::new(Mutex::new(HashMap::new()));
            let code_origin = Arc::new(AtomicU32::new(0f32.to_bits()));
            let code_hook = crate::chat_code::hook(
                key.clone(),
                cx.weak_entity(),
                code_actions.clone(),
                code_origin.clone(),
            );
            let subscription = cx.observe(&state, move |app, _, cx| {
                app.remeasure_chat_row(&key, cx);
            });
            Entry {
                state,
                code_hook,
                code_actions,
                code_origin,
                identity,
                ingested: text.len(),
                source_bytes: source.len(),
                fallback,
                streaming,
                demanded_frame: self.frame,
                touched: self.clock,
                append_updates: 0,
                replace_updates: 0,
                _subscription: subscription,
            }
        });
        entry.sync(identity, text, streaming, cx);
        entry.demanded_frame = self.frame;
        entry.touched = self.clock;
        let result = (
            entry.state.clone(),
            entry.fallback,
            entry.code_hook.clone(),
            entry.code_origin.clone(),
        );
        self.prune(cx);
        result
    }
}

fn literal_preview(text: &str) -> String {
    let mut end = text.len().min(FALLBACK_SOURCE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &text[..end];
    let fence = "`".repeat(
        prefix
            .split(|ch| ch != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(3),
    );
    format!("{fence}\n{prefix}\n{fence}")
}

pub(crate) fn render(
    app: &FlintApp,
    target: &Conversation,
    ix: usize,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let Some(view) = app.conversation_view(target) else {
        return div().into_any_element();
    };
    let Some(identity) = view.row_identity(ix) else {
        return div().into_any_element();
    };
    let Some(Item::Assistant { text, streaming }) = view.items.get(ix) else {
        return div().into_any_element();
    };
    let key = RowKey::new(target, identity);
    let (state, fallback, code_hook, code_origin) =
        app.chat_content
            .borrow_mut()
            .demand(key.clone(), identity, text, *streaming, cx);
    let session = &app.sessions[app
        .session_index(target.parent)
        .expect("conversation owner")];
    let workspace = target
        .child
        .as_ref()
        .and_then(|id| session.view.subagent(id))
        .and_then(|child| child.workspace.as_ref())
        .unwrap_or(&session.workspace)
        .clone();
    let weak = cx.weak_entity();
    let text_view = crate::reply_resources::chat_policy()
        .apply(TextView::new(&state))
        .selectable(true)
        .on_link_click(move |link, _, _, cx| {
            weak.update(cx, |app, cx| app.open_link(link, &workspace, cx))
                .ok();
            cx.stop_propagation();
        });
    let text_view = if fallback {
        text_view
    } else {
        // The inner callback is retained per entry. This installed Base builder
        // wraps it in Arc but does not revise extensions or replace parser text.
        text_view
            .style(crate::chat_code::style(cx))
            .code_block_actions(move |block, window, cx| code_hook(block, window, cx))
    };
    let content = if fallback {
        // A bounded source buffer must also have a bounded viewport. Otherwise
        // a tall source prefix scrolls the warning/copy control out of reach.
        div()
            .h(px(120.))
            .w_full()
            .min_w_0()
            .child(text_view.scrollable(true))
            .into_any_element()
    } else {
        text_view.into_any_element()
    };
    let copy_key = key.clone();
    div()
        .id(key.element_id("chat-content"))
        .min_w_0()
        .w_full()
        .when(fallback, |col| {
            col.child(
                div().text_size(px(crate::theme::size::SM)).text_color(crate::theme::palette().warning)
                    .child("Rich rendering paused above 512 KiB. Showing the first 16 KiB as literal source; full text remains in this conversation.")
            )
        })
        .child(crate::chat_code::TextFrame::new(content, state, code_origin))
        .when(fallback, |col| {
            col.child(
                Button::new(key.element_id("chat-source-copy"))
                    .outline().label("Copy full source")
                    .on_click(cx.listener(move |app, _, _, cx| app.copy_chat_source(&copy_key, cx))),
            )
        })
        .test_support()
        .into_any_element()
}

impl FlintApp {
    fn remeasure_chat_row(&mut self, key: &RowKey, cx: &mut Context<Self>) {
        let Some(session_ix) = self.session_index(key.parent) else {
            return;
        };
        let Some(view) = self.conversation_view(&key.target()) else {
            return;
        };
        let Some(row) = view.row_index(key.row) else {
            return;
        };
        let session = &self.sessions[session_ix];
        let list = match &key.child {
            Some(child) => session.subagent_lists.get(child),
            None => Some(&session.list),
        };
        if let Some(list) = list {
            list.remeasure_items(row..row + 1);
        }
        cx.notify();
    }

    fn copy_chat_source(&mut self, key: &RowKey, cx: &mut Context<Self>) {
        let Some(view) = self.conversation_view(&key.target()) else {
            return;
        };
        let Some(row) = view.row_index(key.row) else {
            return;
        };
        if let Some(Item::Assistant { text, .. }) = view.items.get(row) {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        }
    }
}
