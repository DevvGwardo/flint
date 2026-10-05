//! Command palette (⌘K): a centered overlay over the gpui-component `Command`.

use gpui_kit::assets::IconName;
use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::Icon;
use gpui_kit::component::command::Command;
use gpui_kit::component::command::CommandGroup;
use gpui_kit::component::command::CommandItem;
use gpui_kit::component::command::CommandState;
use gpui_kit::*;

use crate::app::ArrangeSessionGrid;
use crate::app::CycleSessionGrouping;
use crate::app::DeleteSession;
use crate::app::FlintApp;
use crate::app::FocusComposer;
use crate::app::Interrupt;
use crate::app::NewClaudeSession;
use crate::app::NewCodexSession;
use crate::app::NewDroidSession;
use crate::app::NewSession;
use crate::app::OpenSettings;
use crate::app::OpenTerminal;
use crate::app::OpenWorkspace;
use crate::app::OpenWorktrees;
use crate::app::RenameSession;
use crate::app::ResetPanelLayout;
use crate::app::ResetSessionPanes;
use crate::app::ToggleApproval;
use crate::app::ToggleChanges;
use crate::app::ToggleSidebar;
use crate::app::ToggleTerminal;
use crate::app::UndoLastTurn;
use crate::theme::palette;

fn item(label: &str, icon: IconName, action: Box<dyn Action>) -> CommandItem {
    CommandItem::new()
        .label(label.to_string())
        .icon(Icon::new(icon))
        .action(action)
}

/// Room the palette's search field takes above its list.
const INPUT_HEIGHT: f32 = 52.;
/// Space kept under the palette.
const BOTTOM_MARGIN: f32 = 24.;

pub fn render(
    state: &Entity<CommandState>,
    focus: &FocusHandle,
    window_height: f32,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    // 120 px down and 380 px of list on a tall window; on a short one both
    // shrink so the list ends above the window's bottom edge.
    let top = (window_height * 0.15).min(120.);
    let list_height = (window_height - top - INPUT_HEIGHT - BOTTOM_MARGIN).clamp(120., 380.);
    let this = cx.entity().downgrade();
    let on_confirm = this.clone();
    let command = Command::new(state)
        .placeholder("Type a command…")
        .max_h(px(list_height))
        .group(
            CommandGroup::new()
                .label("Session")
                .item(item(
                    "New session",
                    IconName::SquarePen,
                    Box::new(NewSession),
                ))
                .item(item(
                    "New Claude Code session",
                    IconName::Bot,
                    Box::new(NewClaudeSession),
                ))
                .item(item(
                    "New Codex session",
                    IconName::Bot,
                    Box::new(NewCodexSession),
                ))
                .item(item(
                    "New Droid session",
                    IconName::Bot,
                    Box::new(NewDroidSession),
                ))
                .item(item(
                    "Stop the running turn",
                    IconName::CircleStop,
                    Box::new(Interrupt),
                ))
                .item(item(
                    "Undo the agent's file changes from its last turn",
                    IconName::ArchiveRestore,
                    Box::new(UndoLastTurn),
                ))
                .item(item(
                    "Rename session…",
                    IconName::Pencil,
                    Box::new(RenameSession),
                ))
                .item(item(
                    "Archive session (Undo this run; restore from Sessions after restart)",
                    IconName::Archive,
                    Box::new(DeleteSession),
                ))
                .item(item(
                    "Focus composer",
                    IconName::TextCursorInput,
                    Box::new(FocusComposer),
                )),
        )
        .group(
            CommandGroup::new()
                .label("Workspace")
                .item(item(
                    "Open workspace…",
                    IconName::FolderOpen,
                    Box::new(OpenWorkspace),
                ))
                .item(item(
                    "Git worktrees…",
                    IconName::Folder,
                    Box::new(OpenWorktrees),
                ))
                .item(item(
                    "Toggle changes panel",
                    IconName::PanelRight,
                    Box::new(ToggleChanges),
                ))
                .item(item(
                    "Toggle sidebar",
                    IconName::PanelLeft,
                    Box::new(ToggleSidebar),
                ))
                .item(item(
                    "Group sessions by project, status or agent",
                    IconName::Folder,
                    Box::new(CycleSessionGrouping),
                ))
                .item(item(
                    "Toggle approval mode",
                    IconName::ShieldCheck,
                    Box::new(ToggleApproval),
                ))
                .item(item(
                    "Toggle terminal panel",
                    IconName::SquareTerminal,
                    Box::new(ToggleTerminal),
                ))
                .item(item(
                    "Open in Terminal.app",
                    IconName::SquareTerminal,
                    Box::new(OpenTerminal),
                ))
                .item(item("Settings", IconName::Settings, Box::new(OpenSettings)))
                .item(item(
                    "Reset panel layout",
                    IconName::PanelLeft,
                    Box::new(ResetPanelLayout),
                ))
                .item(item(
                    "Arrange session panes in a grid",
                    IconName::Grid2x2,
                    Box::new(ArrangeSessionGrid),
                ))
                .item(item(
                    "Show only the focused session",
                    IconName::Maximize2,
                    Box::new(ResetSessionPanes),
                )),
        )
        .on_confirm(move |_, window, cx| {
            on_confirm
                .update(cx, |app, cx| app.close_palette(window, cx))
                .ok();
        })
        .on_cancel(move |window, cx| {
            this.update(cx, |app, cx| app.close_palette(window, cx))
                .ok();
        });

    deferred(
        div()
            .absolute()
            .inset_0()
            .bg(hsla(0., 0., 0., 0.45))
            .flex()
            .items_start()
            .justify_center()
            .pt(px(top))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
            )
            .child(
                div()
                    .w(px(560.))
                    .h_auto()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(hsla(240. / 360., 0.08, 0.085, 1.))
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(command)
                    .focus_trap("palette-focus-trap", focus)
                    .test_support(),
            ),
    )
    .with_priority(10)
}
