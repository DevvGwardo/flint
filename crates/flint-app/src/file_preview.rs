//! Local links open in the right-hand dock, never through the OS file handler.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::text::TextView;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::{palette, size};
use crate::ui;

const MAX_BYTES: u64 = 512 * 1024;

pub struct FilePreview {
    pub path: PathBuf,
    pub content: PreviewContent,
    pub scroll: ScrollHandle,
}

pub enum PreviewContent {
    Loading,
    Text(SharedString),
    Unavailable(String),
}

#[derive(Debug, PartialEq, Eq)]
enum LinkTarget {
    File(PathBuf),
    Web(String),
    Ignore,
}

fn link_target(link: &str, base: &Path) -> LinkTarget {
    let link = link.trim();
    if link.is_empty() || link.starts_with('#') || link.starts_with("//") {
        return LinkTarget::Ignore;
    }
    if let Ok(url) = reqwest::Url::parse(link) {
        match url.scheme() {
            "http" | "https" | "mailto" => return LinkTarget::Web(link.to_string()),
            "file" => {
                return url
                    .to_file_path()
                    .map(LinkTarget::File)
                    .unwrap_or(LinkTarget::Ignore);
            }
            _ if !link.contains("://")
                && link.rsplit_once(':').is_some_and(|(_, suffix)| {
                    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                }) => {}
            _ => return LinkTarget::Ignore,
        }
    }
    let path = link.split(['#', '?']).next().unwrap_or(link);
    // Agent references commonly append :line or :line:column to a path.
    let mut path = path;
    for _ in 0..2 {
        if let Some((prefix, suffix)) = path.rsplit_once(':')
            && !suffix.is_empty()
            && suffix.bytes().all(|byte| byte.is_ascii_digit())
        {
            path = prefix;
        }
    }
    let base = if base.is_absolute() {
        base.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(base)
    };
    reqwest::Url::from_directory_path(base)
        .ok()
        .and_then(|url| url.join(path).ok())
        .and_then(|url| url.to_file_path().ok())
        .map(LinkTarget::File)
        .unwrap_or(LinkTarget::Ignore)
}

/// The base belongs to the rendered session (or document), not the active pane.
pub fn markdown(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    base: PathBuf,
    cx: &mut Context<FlintApp>,
) -> TextView {
    let app = cx.weak_entity();
    TextView::markdown(id, text)
        .selectable(true)
        .on_link_click(move |link, _, _, cx| {
            app.update(cx, |app, cx| app.open_link(link, &base, cx))
                .ok();
            cx.stop_propagation();
        })
}

fn read_file(path: &Path) -> Result<SharedString, String> {
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("Cannot read this file: {error}"))?;
    if !metadata.is_file() {
        return Err("Select a regular file to preview.".into());
    }
    let file =
        std::fs::File::open(path).map_err(|error| format!("Cannot read this file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot read this file: {error}"))?;
    if !metadata.is_file() {
        return Err("Select a regular file to preview.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("This file is too large to preview (limit: 512 KB).".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read this file: {error}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("This file is too large to preview (limit: 512 KB).".into());
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| "Preview is available for UTF-8 text and Markdown files.".to_string())?;
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err("Preview is available for text and Markdown files, not binary files.".into());
    }
    let markdown = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"));
    if markdown {
        Ok(text.into())
    } else {
        // A longer fence than any in the source preserves arbitrary text literally.
        let fence = "`".repeat(
            text.split(|ch| ch != '`')
                .map(str::len)
                .max()
                .unwrap_or(0)
                .saturating_add(1)
                .max(3),
        );
        Ok(format!("{fence}\n{text}\n{fence}").into())
    }
}

impl FlintApp {
    pub(crate) fn open_link(&mut self, link: &str, base: &Path, cx: &mut Context<Self>) {
        match link_target(link, base) {
            LinkTarget::File(path) => self.open_file_preview(path, cx),
            LinkTarget::Web(url) => cx.open_url(&url),
            LinkTarget::Ignore => {}
        }
    }

    pub fn open_file_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_preview_revision += 1;
        let revision = self.file_preview_revision;
        self.file_preview = Some(FilePreview {
            path: path.clone(),
            content: PreviewContent::Loading,
            scroll: ScrollHandle::new(),
        });
        self.changes_open = true;
        let read = cx
            .background_executor()
            .spawn(async move { read_file(&path) });
        self.file_preview_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            this.update(cx, |app, cx| {
                if app.file_preview_revision == revision
                    && let Some(preview) = &mut app.file_preview
                {
                    preview.content = match result {
                        Ok(text) => PreviewContent::Text(text),
                        Err(message) => PreviewContent::Unavailable(message),
                    };
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn clear_file_preview(&mut self) {
        self.file_preview = None;
        self.file_preview_task = None;
    }
}

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> AnyElement {
    let preview = app.file_preview.as_ref().expect("preview is open");
    let p = palette();
    let path = preview.path.to_string_lossy().into_owned();
    let name = preview
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let header = div()
        .h(px(crate::header::HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .border_b_1()
        .border_color(p.border)
        .child(crate::docking::handle(crate::docking::Panel::Changes, cx))
        .child(ui::icon(
            crate::transcript::file_icon(&path),
            15.,
            p.text_muted,
        ))
        .child(
            ui::label(name, size::BASE, p.text)
                .flex_1()
                .min_w_0()
                .truncate(),
        )
        .child(
            Button::new("file-preview-changes")
                .ghost()
                .label("Changes")
                .tooltip("Back to changes")
                .on_click(cx.listener(|app, _, _, cx| {
                    app.clear_file_preview();
                    cx.notify();
                })),
        )
        .child(
            Button::new("file-preview-close")
                .ghost()
                .icon(IconName::X)
                .tooltip("Close file preview")
                .on_click(cx.listener(|app, _, _, cx| {
                    app.clear_file_preview();
                    app.changes_open = false;
                    cx.notify();
                })),
        );
    let body = match &preview.content {
        PreviewContent::Loading => {
            ui::label("Loading file…", size::BASE, p.text_muted).into_any_element()
        }
        PreviewContent::Unavailable(message) => div()
            .id("file-preview-error")
            .child(ui::label(message.clone(), size::BASE, p.text_muted))
            .test_support()
            .into_any_element(),
        PreviewContent::Text(text) => div()
            .id("file-preview-content")
            .text_size(px(size::BASE))
            .line_height(px(24.))
            .child(markdown(
                SharedString::from(format!("file-preview-text-{}", app.file_preview_revision)),
                text.clone(),
                preview
                    .path
                    .parent()
                    .unwrap_or(&app.session().workspace)
                    .to_path_buf(),
                cx,
            ))
            .test_support()
            .into_any_element(),
    };
    div()
        .id("file-preview")
        .size_full()
        .flex()
        .flex_col()
        .bg(p.chrome)
        .child(header)
        .child(
            div()
                .px(px(18.))
                .py(px(10.))
                .child(ui::mono(path, size::XS, p.text_subtle).truncate()),
        )
        .child(
            div()
                .id("file-preview-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&preview.scroll)
                .p(px(18.))
                .child(body),
        )
        .test_support()
        .into_any_element()
}

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod tests;
