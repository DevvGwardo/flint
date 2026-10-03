//! Composer menus: the `@` file picker (also opened by "+"), the `/` command
//! menu, and the keys that drive them and the pinned approval card.

use flint_agent::ApprovalDecision;
use gpui_kit::*;
use std::path::Path;
use std::path::PathBuf;

use crate::app::FlintApp;
use crate::mention;
use crate::session_options::MenuTarget;
use crate::slash;
use crate::slash::SlashCommand;

pub struct MentionMenu {
    pub query: String,
    pub results: Vec<String>,
    pub selected: usize,
    /// Opened by typing `@` (the query is replaced on pick) rather than "+".
    pub inline: bool,
}

pub struct SlashMenu {
    pub selected: usize,
}

impl FlintApp {
    pub(crate) fn composer_text(&self, cx: &App) -> String {
        self.composer.read(cx).value().to_string()
    }

    /// Opens or updates the `@` picker and `/` menu as the user types.
    pub(crate) fn composer_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer_text(cx);
        match slash::active_query(&text) {
            Some(query) => {
                let count = slash::matches(query).len();
                let menu = self.slash.get_or_insert(SlashMenu { selected: 0 });
                menu.selected = menu.selected.min(count.saturating_sub(1));
            }
            None => self.slash = None,
        }
        match mention::active_query(&text) {
            Some(query) => {
                let query = query.to_string();
                self.update_mention(query, true);
            }
            None if self.mention.as_ref().is_some_and(|m| m.inline) => self.mention = None,
            None => {}
        }
        cx.notify();
    }

    fn update_mention(&mut self, query: String, inline: bool) {
        let workspace = self.session().workspace.clone();
        let files = self
            .file_index
            .entry(workspace.clone())
            .or_insert_with(|| mention::index_files(&workspace));
        let results: Vec<String> = mention::search(files, &query)
            .into_iter()
            .cloned()
            .collect();
        let selected = self
            .mention
            .as_ref()
            .filter(|m| m.query == query)
            .map_or(0, |m| m.selected.min(results.len().saturating_sub(1)));
        self.mention = Some(MentionMenu {
            query,
            results,
            selected,
            inline,
        });
    }

    /// The "+" button: the same picker, without typing `@`.
    pub fn open_mention_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_mention(String::new(), false);
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// Attaches a file and puts an `@path` mention in the message.
    pub fn pick_mention(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.mention.take() else {
            return;
        };
        let mut text = self.composer_text(cx);
        if menu.inline {
            let at = text.rfind('@').unwrap_or(text.len());
            text.truncate(at);
        } else if !text.is_empty() && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        text.push_str(&format!("@{path} "));
        self.composer.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.focus(window, cx);
        });
        if !self.attachments.contains(&path) {
            self.attachments.push(path);
        }
        cx.notify();
    }

    pub fn remove_attachment(&mut self, path: &str, cx: &mut Context<Self>) {
        self.attachments.retain(|p| p != path);
        cx.notify();
    }

    /// Attach up to four images from any folder. The bytes are checked again
    /// when sending, so a changed or missing file cannot silently disappear.
    pub fn open_image_picker(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach images (PNG, JPEG, GIF, WebP)".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update(cx, |app, cx| {
                for path in paths {
                    app.add_image_attachment(path);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn add_image_attachment(&mut self, path: PathBuf) {
        if self.image_attachments.contains(&path) {
            return;
        }
        if self.image_attachments.len() + self.pending_image_pastes
            >= crate::image_attach::MAX_IMAGES
        {
            self.store_error = Some("Attach up to four images per message.".into());
        } else if let Err(error) = crate::image_attach::load(&path) {
            self.store_error = Some(error);
        } else {
            self.store_error = None;
            self.image_attachments.push(path);
        }
    }

    pub fn remove_image_attachment(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.image_attachments.retain(|p| p != path);
        self.pasted_image_files.retain(|file| file.path() != path);
        cx.notify();
    }

    /// Capture image paste before the textarea's text-only Paste action.
    /// Returning false leaves text insertion and its undo/selection behavior intact.
    pub(crate) fn paste_composer_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(clipboard) = cx.read_from_clipboard() else {
            return false;
        };
        let mut handled = false;
        for entry in clipboard.into_entries() {
            match entry {
                ClipboardEntry::Image(image) => {
                    handled = true;
                    if self.image_attachments.len() + self.pending_image_pastes
                        >= crate::image_attach::MAX_IMAGES
                    {
                        self.store_error = Some("Attach up to four images per message.".into());
                        continue;
                    }
                    self.pending_image_pastes += 1;
                    let uid = self.session().uid;
                    let workspace = self.session().workspace.clone();
                    let prepare = cx
                        .background_executor()
                        .spawn(async move { crate::image_attach::clipboard_file(image) });
                    cx.spawn(async move |this, cx| {
                        let result = prepare.await;
                        this.update(cx, |app, cx| {
                            app.pending_image_pastes -= 1;
                            // A delayed paste must not attach to a different session or workspace.
                            if app.session().uid == uid && app.session().workspace == workspace {
                                match result {
                                    Ok(file) => {
                                        let path = file.path().to_path_buf();
                                        app.add_image_attachment(path.clone());
                                        if app.image_attachments.contains(&path) {
                                            app.pasted_image_files.push(file);
                                        }
                                    }
                                    Err(error) => app.store_error = Some(error),
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
                ClipboardEntry::ExternalPaths(paths)
                    if !paths.paths().is_empty()
                        && paths.paths().iter().all(|path| {
                            path.extension().is_some_and(|ext| {
                                matches!(
                                    ext.to_string_lossy().to_ascii_lowercase().as_str(),
                                    "png" | "jpg" | "jpeg" | "gif" | "webp"
                                )
                            })
                        }) =>
                {
                    handled = true;
                    for path in paths.paths() {
                        self.add_image_attachment(path.clone());
                    }
                }
                _ => {}
            }
        }
        if handled {
            cx.notify();
        }
        handled
    }

    pub fn run_slash(
        &mut self,
        command: SlashCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.slash = None;
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        match command {
            SlashCommand::New => self.new_session(window, cx),
            SlashCommand::Clear => self.clear_session(window, cx),
            // For an ACP agent these open its own option menus.
            SlashCommand::Model => match self.session_slots().model {
                Some(model) => self.open_option_menu(MenuTarget::Option(model.id), cx),
                None => self.open_settings(window, cx),
            },
            SlashCommand::Mode => match self.session_slots().mode {
                Some(mode) => self.open_option_menu(MenuTarget::Option(mode.id), cx),
                None => self.toggle_approval(cx),
            },
            SlashCommand::Agent => {
                // Leave "/agent " for the user to finish with a name.
                self.composer.update(cx, |state, cx| {
                    state.set_value("/agent ", window, cx);
                    state.focus(window, cx);
                });
            }
            SlashCommand::Effort => match self.session_slots().reasoning {
                Some(reasoning) => self.open_option_menu(MenuTarget::Option(reasoning.id), cx),
                None => self.cycle_effort(cx),
            },
            SlashCommand::Approval => self.toggle_approval(cx),
            SlashCommand::Review => self.review(None, cx),
            SlashCommand::Help => self.help_open = true,
        }
        cx.notify();
    }

    pub(crate) fn run_selected_slash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer_text(cx);
        let selected = self.slash.as_ref().map_or(0, |m| m.selected);
        let command = slash::active_query(&text)
            .and_then(|query| slash::matches(query).get(selected).map(|(c, _, _)| *c));
        match command {
            Some(command) => self.run_slash(command, window, cx),
            None => {
                self.slash = None;
                cx.notify();
            }
        }
    }

    /// A menu key delivered as an action (see the `menu > Input` bindings).
    pub(crate) fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = Keystroke {
            key: key.to_string(),
            ..Keystroke::default()
        };
        self.handle_menu_key(&keystroke, window, cx);
    }

    /// Keys for open menus and the pinned approval. Returns whether the key
    /// was consumed (so the composer doesn't also handle it).
    pub(crate) fn handle_menu_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let plain = !key.modifiers.platform && !key.modifiers.control && !key.modifiers.alt;
        if self.project_menu.is_some() && (plain || key.key == "escape") {
            return self.project_menu_key(key.key.as_str(), window, cx);
        }
        if self.option_menu.is_some() && (plain || key.key == "escape") {
            return self.option_menu_key(key.key.as_str(), cx);
        }
        if self.agent_menu && key.key == "escape" {
            self.agent_menu = false;
            cx.notify();
            return true;
        }
        if let Some(menu) = &mut self.mention {
            let len = menu.results.len();
            match key.key.as_str() {
                "up" if plain => menu.selected = menu.selected.saturating_sub(1),
                "down" if plain => menu.selected = (menu.selected + 1).min(len.saturating_sub(1)),
                "enter" | "tab" if plain => match menu.results.get(menu.selected).cloned() {
                    Some(path) => self.pick_mention(path, window, cx),
                    None => self.mention = None,
                },
                "escape" => self.mention = None,
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if let Some(menu) = &mut self.slash {
            let text = self.composer.read(cx).value().to_string();
            let len = slash::active_query(&text).map_or(0, |q| slash::matches(q).len());
            match key.key.as_str() {
                "up" if plain => menu.selected = menu.selected.saturating_sub(1),
                "down" if plain => menu.selected = (menu.selected + 1).min(len.saturating_sub(1)),
                "enter" | "tab" if plain && !key.modifiers.shift => {
                    self.run_selected_slash(window, cx)
                }
                "escape" => self.slash = None,
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if self.session().view.pending_approval().is_some() {
            let empty = self.composer.read(cx).value().is_empty();
            let decision = match (
                key.key.as_str(),
                key.modifiers.platform,
                key.modifiers.shift,
            ) {
                ("enter", true, true) => Some(ApprovalDecision::ApproveAlways),
                ("enter", true, false) => Some(ApprovalDecision::Approve),
                ("y", false, false) if empty && plain => Some(ApprovalDecision::Approve),
                ("a", false, false) if empty && plain => Some(ApprovalDecision::ApproveAlways),
                ("n", false, false) if empty && plain => Some(ApprovalDecision::Deny),
                _ => None,
            };
            if let Some(decision) = decision {
                self.answer_pending(decision, cx);
                return true;
            }
        }
        false
    }
}
