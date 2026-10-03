//! Docking the four workspace panels without replacing their live contents.
//! Hidden panels keep their place in the saved split tree.

use std::path::Path;

use gpui_kit::component::resizable::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::app::FlintApp;
use crate::theme::{palette, size};
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Sidebar,
    Chat,
    Changes,
    Terminal,
}

impl Panel {
    pub const ALL: [Self; 4] = [Self::Sidebar, Self::Chat, Self::Changes, Self::Terminal];

    pub fn id(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::Chat => "chat",
            Self::Changes => "changes",
            Self::Terminal => "terminal",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sidebar => "Sessions",
            Self::Chat => "Chat",
            Self::Changes => "Changes",
            Self::Terminal => "Terminal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    fn id(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Panel {
        panel: Panel,
    },
    Split {
        axis: SplitAxis,
        sizes: [Option<f32>; 2],
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    fn panel_width(&self, panel: Panel, width: f32, app: &FlintApp) -> Option<f32> {
        match self {
            Self::Panel { panel: found } => (*found == panel).then_some(width),
            Self::Split {
                axis,
                sizes,
                first,
                second,
            } => {
                let first_visible = Panel::ALL
                    .iter()
                    .any(|p| visible(app, *p) && first.contains(*p));
                let second_visible = Panel::ALL
                    .iter()
                    .any(|p| visible(app, *p) && second.contains(*p));
                if !first_visible {
                    return second.panel_width(panel, width, app);
                }
                if !second_visible {
                    return first.panel_width(panel, width, app);
                }
                let a = if *axis == SplitAxis::Horizontal {
                    match sizes {
                        [Some(a), Some(b)] => width * a / (a + b),
                        [Some(a), None] => *a,
                        [None, Some(b)] => width - b,
                        [None, None] => width / 2.,
                    }
                    .clamp(100., (width - 100.).max(100.))
                } else {
                    width
                };
                first.panel_width(panel, a, app).or_else(|| {
                    second.panel_width(
                        panel,
                        if *axis == SplitAxis::Horizontal {
                            width - a
                        } else {
                            width
                        },
                        app,
                    )
                })
            }
        }
    }

    fn contains(&self, panel: Panel) -> bool {
        match self {
            Self::Panel { panel: found } => *found == panel,
            Self::Split { first, second, .. } => first.contains(panel) || second.contains(panel),
        }
    }
    fn panel(panel: Panel) -> Self {
        Self::Panel { panel }
    }

    fn split(axis: SplitAxis, first: Self, second: Self, sizes: [Option<f32>; 2]) -> Self {
        Self::Split {
            axis,
            sizes,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// Structural identity, independent of measured sizes.
    fn key(&self) -> String {
        match self {
            Self::Panel { panel } => panel.id().into(),
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                format!("{axis:?}({},{})", first.key(), second.key())
            }
        }
    }

    fn panels(&self, out: &mut Vec<Panel>) {
        match self {
            Self::Panel { panel } => out.push(*panel),
            Self::Split { first, second, .. } => {
                first.panels(out);
                second.panels(out);
            }
        }
    }

    fn without(self, panel: Panel) -> Option<Self> {
        match self {
            Self::Panel { panel: found } => (panel != found).then_some(self),
            Self::Split {
                axis,
                sizes,
                first,
                second,
            } => match (first.without(panel), second.without(panel)) {
                (Some(first), Some(second)) => Some(Self::split(axis, first, second, sizes)),
                (first, second) => first.or(second),
            },
        }
    }

    fn insert(&mut self, panel: Panel, target: Panel, edge: Edge) {
        match self {
            Self::Panel { panel: found } if *found == target => {
                let old = Self::panel(target);
                let new = Self::panel(panel);
                let axis = match edge {
                    Edge::Left | Edge::Right => SplitAxis::Horizontal,
                    Edge::Top | Edge::Bottom => SplitAxis::Vertical,
                };
                let (first, second) = match edge {
                    Edge::Left | Edge::Top => (new, old),
                    Edge::Right | Edge::Bottom => (old, new),
                };
                *self = Self::split(axis, first, second, [None, None]);
            }
            Self::Split { first, second, .. } => {
                first.insert(panel, target, edge);
                second.insert(panel, target, edge);
            }
            _ => {}
        }
    }

    fn set_sizes(&mut self, key: &str, measured: [f32; 2]) -> bool {
        if self.key() == key
            && let Self::Split { sizes, .. } = self
        {
            *sizes = measured.map(Some);
            return true;
        }
        match self {
            Self::Split { first, second, .. } => {
                first.set_sizes(key, measured) || second.set_sizes(key, measured)
            }
            _ => false,
        }
    }

    fn valid_sizes(&self) -> bool {
        match self {
            Self::Panel { .. } => true,
            Self::Split {
                sizes,
                first,
                second,
                ..
            } => {
                sizes
                    .iter()
                    .flatten()
                    .all(|size| size.is_finite() && (100. ..=16_000.).contains(size))
                    && first.valid_sizes()
                    && second.valid_sizes()
            }
        }
    }

    /// A split must leave room for every visible leaf, not just its container.
    fn minimum(&self, axis: SplitAxis, app: &FlintApp) -> f32 {
        match self {
            Self::Panel { panel } => {
                if visible(app, *panel) {
                    100.
                } else {
                    0.
                }
            }
            Self::Split {
                axis: split,
                first,
                second,
                ..
            } => {
                let a = first.minimum(axis, app);
                let b = second.minimum(axis, app);
                if *split == axis { a + b } else { a.max(b) }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub root: Node,
}

impl Default for Layout {
    fn default() -> Self {
        Self::initial(crate::term_panel::DEFAULT_HEIGHT)
    }
}

impl Layout {
    fn initial(terminal_height: f32) -> Self {
        use Panel::*;
        use SplitAxis::*;
        let height = if terminal_height.is_finite() {
            terminal_height.clamp(crate::term_panel::MIN_HEIGHT, 16_000.)
        } else {
            crate::term_panel::DEFAULT_HEIGHT
        };
        Self {
            root: Node::split(
                Horizontal,
                Node::panel(Sidebar),
                Node::split(
                    Horizontal,
                    Node::split(
                        Vertical,
                        Node::panel(Chat),
                        Node::panel(Terminal),
                        [None, Some(height)],
                    ),
                    Node::panel(Changes),
                    [None, Some(460.)],
                ),
                [
                    Some(crate::layout::SIDEBAR_WIDTH + crate::layout::SIDEBAR_INSET * 2.),
                    None,
                ],
            ),
        }
    }

    pub fn panels(&self) -> Vec<Panel> {
        let mut panels = Vec::new();
        self.root.panels(&mut panels);
        panels
    }

    pub fn move_panel(&mut self, panel: Panel, target: Panel, edge: Edge) -> bool {
        if panel == target || !self.valid() {
            return false;
        }
        let Some(mut root) = self.root.clone().without(panel) else {
            return false;
        };
        root.insert(panel, target, edge);
        self.root = root;
        true
    }

    fn valid(&self) -> bool {
        let panels = self.panels();
        panels.len() == Panel::ALL.len()
            && Panel::ALL.iter().all(|p| panels.contains(p))
            && self.root.valid_sizes()
    }

    pub fn load(home: &Path, terminal_height: f32) -> Self {
        let path = home.join("layout.json");
        if std::fs::metadata(&path).map_or(true, |meta| meta.len() > 16_384) {
            return Self::initial(terminal_height);
        }
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(Self::valid)
            .unwrap_or_else(|| Self::initial(terminal_height))
    }

    fn save(&self, home: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(home)?;
        let temporary = home.join(format!("layout-{}.tmp", std::process::id()));
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, home.join("layout.json"))
    }
}

impl FlintApp {
    pub(crate) fn chat_width(&self, window_width: f32) -> f32 {
        self.dock_layout
            .root
            .panel_width(Panel::Chat, window_width, self)
            .unwrap_or(window_width)
    }
    fn save_dock_layout(&self) {
        if !self.options.ephemeral() {
            self.dock_layout.save(&self.home).ok();
        }
    }

    fn dock_panel(&mut self, panel: Panel, target: Panel, edge: Edge, cx: &mut Context<Self>) {
        if self.dock_layout.move_panel(panel, target, edge) {
            self.dock_revision += 1;
            self.save_dock_layout();
            cx.notify();
        }
    }

    pub fn reset_panel_layout(&mut self, cx: &mut Context<Self>) {
        self.dock_layout = Layout::default();
        self.dock_revision += 1;
        self.save_dock_layout();
        cx.notify();
    }
}

#[derive(Clone)]
struct PanelDrag {
    panel: Panel,
    owner: EntityId,
}

struct Preview {
    panel: Panel,
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        div()
            .px(px(16.))
            .py(px(10.))
            .rounded(px(10.))
            .border_1()
            .border_color(p.accent)
            .bg(p.surface)
            .shadow_lg()
            .child(ui::label(
                format!("Move {}", self.panel.label()),
                size::SM,
                p.text,
            ))
    }
}

/// A dedicated grip keeps selecting text and dragging the window unchanged.
pub fn handle(panel: Panel, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let owner = cx.entity().entity_id();
    let app = cx.weak_entity();
    div()
        .id(SharedString::from(format!("dock-handle-{}", panel.id())))
        .size(px(26.))
        .flex_shrink_0()
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(3.))
        .cursor(CursorStyle::OpenHand)
        .hover(|s| s.bg(p.raised))
        .tooltip(move |window, cx| {
            Tooltip::new(format!("Drag to dock {}", panel.label())).build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_drag(PanelDrag { panel, owner }, move |drag, _, _, cx| {
            cx.stop_propagation();
            let panel = drag.panel;
            let finish = app.clone();
            app.update(cx, |app, cx| {
                app.docking = Some(panel);
                app.drag_armed = false;
                cx.notify();
            })
            .ok();
            cx.new(|cx| {
                cx.on_release(move |_, cx| {
                    finish
                        .update(cx, |app, cx| {
                            app.docking = None;
                            cx.notify();
                        })
                        .ok();
                })
                .detach();
                Preview { panel }
            })
        })
        .children((0..2).map(|_| {
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .children((0..3).map(|_| div().size(px(2.)).rounded_full().bg(p.text_subtle)))
        }))
        .test_support()
        .into_any_element()
}

fn targets(target: Panel, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let owner = cx.entity().entity_id();
    div()
        .absolute()
        .inset_0()
        .children(Edge::ALL.map(|edge| {
            let zone = div()
                .id(SharedString::from(format!(
                    "dock-target-{}-{}",
                    target.id(),
                    edge.id()
                )))
                .absolute()
                .flex()
                .items_center()
                .justify_center()
                .bg(p.accent.opacity(0.12))
                .border_1()
                .border_color(p.accent.opacity(0.4))
                .drag_over::<PanelDrag>(move |s, _, _, _| s.bg(p.accent.opacity(0.4)))
                .can_drop(move |drag, _, _| {
                    drag.downcast_ref::<PanelDrag>()
                        .is_some_and(|drag| drag.owner == owner && drag.panel != target)
                })
                .on_drop(cx.listener(move |app, drag: &PanelDrag, _, cx| {
                    if drag.owner == owner && drag.panel != target {
                        app.dock_panel(drag.panel, target, edge, cx);
                    }
                    cx.stop_propagation();
                }))
                .child(ui::label(edge.id(), size::XS, p.text))
                .test_support();
            match edge {
                Edge::Top => zone.top_0().left_0().right_0().h(px(38.)),
                Edge::Bottom => zone.bottom_0().left_0().right_0().h(px(38.)),
                Edge::Left => zone.left_0().top(px(38.)).bottom(px(38.)).w(px(38.)),
                Edge::Right => zone.right_0().top(px(38.)).bottom(px(38.)).w(px(38.)),
            }
        }))
        .into_any_element()
}

fn visible(app: &FlintApp, panel: Panel) -> bool {
    match panel {
        Panel::Chat => true,
        Panel::Sidebar => app.sidebar_visible,
        Panel::Changes => app.changes_open,
        Panel::Terminal => app.terminal.open,
    }
}

fn render_node(
    node: &Node,
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> Option<AnyElement> {
    match node {
        Node::Panel { panel } => {
            if !visible(app, *panel) {
                return None;
            }
            let content = match panel {
                Panel::Sidebar => div()
                    .size_full()
                    .p(px(crate::layout::SIDEBAR_INSET))
                    .bg(palette().window_tint)
                    .child(crate::sidebar::render(app, cx))
                    .into_any_element(),
                Panel::Chat => div()
                    .id("main-column")
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(crate::header::render(app, window, cx))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(crate::transcript::render_main(app, window, cx))
                            // The room composer popovers must stay inside.
                            .child(crate::popover::probe(&app.popover_room, |room| &room.area)),
                    )
                    .into_any_element(),
                Panel::Changes => crate::changes_panel::render(app, cx).into_any_element(),
                Panel::Terminal => crate::term_panel::render(app, cx)?,
            };
            Some(
                div()
                    .id(SharedString::from(format!("dock-panel-{}", panel.id())))
                    .relative()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(palette().bg)
                    .child(content)
                    .when(
                        app.docking.is_some_and(|moving| moving != *panel) && cx.has_active_drag(),
                        |div| div.child(deferred(targets(*panel, cx)).with_priority(3)),
                    )
                    .test_support()
                    .into_any_element(),
            )
        }
        Node::Split {
            axis,
            sizes,
            first,
            second,
        } => {
            let a = render_node(first, app, window, cx);
            let b = render_node(second, app, window, cx);
            let (a, b) = match (a, b) {
                (Some(a), Some(b)) => (a, b),
                (a, b) => return a.or(b),
            };
            let key = node.key();
            let mask = Panel::ALL.map(|panel| if visible(app, panel) { '1' } else { '0' });
            let id = SharedString::from(format!(
                "dock-{}-{}-{}",
                app.dock_revision,
                mask.iter().collect::<String>(),
                key
            ));
            let group = match axis {
                SplitAxis::Horizontal => h_resizable(id),
                SplitAxis::Vertical => v_resizable(id),
            };
            let sized = |content: AnyElement, size: Option<f32>, minimum: f32| {
                resizable_panel()
                    .size_range(px(minimum)..Pixels::MAX)
                    .when_some(size, |panel, size| panel.size(px(size)).flex_none())
                    .child(content)
            };
            let weak = cx.weak_entity();
            Some(
                group
                    .child(sized(a, sizes[0], first.minimum(*axis, app)))
                    .child(sized(b, sizes[1], second.minimum(*axis, app)))
                    .on_resize(move |state, _, cx| {
                        let sizes = state.read(cx).sizes();
                        if let [first, second] = sizes.as_slice() {
                            let measured = [f32::from(*first), f32::from(*second)];
                            weak.update(cx, |app, _| {
                                if app.dock_layout.root.set_sizes(&key, measured) {
                                    app.save_dock_layout();
                                }
                            })
                            .ok();
                        }
                    })
                    .into_any_element(),
            )
        }
    }
}

pub fn render(app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> AnyElement {
    render_node(&app.dock_layout.root, app, window, cx).expect("chat stays visible")
}

#[cfg(test)]
#[path = "docking_tests.rs"]
mod tests;
