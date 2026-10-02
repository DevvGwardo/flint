//! Command palette (⌘K): a centered overlay over the gpui-component `Command`.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::command::Command;
use gpui_kit::component::command::CommandGroup;
use gpui_kit::component::command::CommandItem;
use gpui_kit::component::command::CommandState;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::FocusComposer;
use crate::app::Interrupt;
use crate::app::NewSession;
use crate::app::OpenWorkspace;
use crate::app::ToggleApproval;
use crate::app::ToggleChanges;
use crate::app::ToggleSidebar;
use crate::theme::palette;

fn item(label: &str, icon: IconName, action: Box<dyn Action>) -> CommandItem {
    CommandItem::new()
        .label(label.to_string())
        .icon(Icon::new(icon))
        .action(action)
}

pub fn render(state: &Entity<CommandState>, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let this = cx.entity().downgrade();
    let on_confirm = this.clone();
    let command = Command::new(state)
        .placeholder("Type a command…")
        .max_h(px(380.))
        .group(
            CommandGroup::new()
                .label("Session")
                .item(item(
                    "New session",
                    IconName::SquarePen,
                    Box::new(NewSession),
                ))
                .item(item(
                    "Stop the running turn",
                    IconName::CircleStop,
                    Box::new(Interrupt),
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
                    "Toggle approval mode",
                    IconName::ShieldCheck,
                    Box::new(ToggleApproval),
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
            .pt(px(120.))
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
                    .child(command),
            ),
    )
    .with_priority(10)
}
