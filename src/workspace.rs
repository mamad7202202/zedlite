use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    actions, div, prelude::*, px, App, Context, ElementId, Entity, Focusable, InteractiveElement,
    KeyBinding, MouseButton, PathPromptOptions, Render, SharedString, Styled, Window,
};

use crate::ai::{self, AiHub, UiEvent};
use crate::document::Document;
use crate::editor::EditorPane;
use crate::panels::chat::{ChatPanel, AI_PANEL_W};
use crate::panels::memoryview::MemoryPanel;
use crate::panels::terminal::TerminalPanel;
use crate::syntax;
use crate::theme::{hex, SIDEBAR_WIDTH, Theme};

actions!(
    workspace,
    [
        OpenFolder,
        OpenFiles,
        NewFile,
        SaveActive,
        SaveActiveAs,
        CloseActiveTab,
        NextTab,
        PrevTab,
        ToggleAi,
        ToggleTerminal,
        ToggleMemory,
        OpenSettings,
        NewSession,
        StopAi,
    ]
);

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-o", OpenFiles, None),
        KeyBinding::new("cmd-o", OpenFiles, None),
        KeyBinding::new("ctrl-shift-o", OpenFolder, None),
        KeyBinding::new("shift-cmd-o", OpenFolder, None),
        KeyBinding::new("ctrl-n", NewFile, None),
        KeyBinding::new("cmd-n", NewFile, None),
        KeyBinding::new("ctrl-s", SaveActive, None),
        KeyBinding::new("cmd-s", SaveActive, None),
        KeyBinding::new("ctrl-shift-s", SaveActiveAs, None),
        KeyBinding::new("shift-cmd-s", SaveActiveAs, None),
        KeyBinding::new("ctrl-w", CloseActiveTab, None),
        KeyBinding::new("cmd-w", CloseActiveTab, None),
        KeyBinding::new("ctrl-pagedown", NextTab, None),
        KeyBinding::new("ctrl-pageup", PrevTab, None),
        // panel toggles & AI commands
        KeyBinding::new("alt-a", ToggleAi, None),
        KeyBinding::new("alt-t", ToggleTerminal, None),
        KeyBinding::new("alt-m", ToggleMemory, None),
        KeyBinding::new("alt-c", OpenSettings, None),
        KeyBinding::new("alt-n", NewSession, None),
        KeyBinding::new("alt-q", StopAi, None),
    ]);
}

#[derive(Debug, Clone)]
struct TreeNode {
    name: String,
    path: PathBuf,
    is_dir: bool,
    expanded: bool,
    children: Vec<TreeNode>,
}

impl TreeNode {
    fn from_path(path: &Path, name: String, is_dir: bool) -> Self {
        TreeNode {
            name,
            path: path.to_path_buf(),
            is_dir,
            expanded: false,
            children: Vec::new(),
        }
    }

    fn load_children(&mut self) {
        if !self.is_dir || !self.children.is_empty() {
            return;
        }
        let mut entries: Vec<TreeNode> = match fs::read_dir(&self.path) {
            Ok(read_dir) => read_dir
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') || name == "target" {
                        return None;
                    }
                    let is_dir = path.is_dir();
                    Some(TreeNode::from_path(&path, name, is_dir))
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        entries.sort_by(|a, b| match (b.is_dir, a.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        self.children = entries;
    }

    fn toggle_at(&mut self, target: &Path) -> bool {
        if self.is_dir && self.path == target {
            self.expanded = !self.expanded;
            if self.expanded {
                self.load_children();
            }
            return true;
        }
        for child in &mut self.children {
            if child.toggle_at(target) {
                return true;
            }
        }
        false
    }
}

fn tree_node_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub struct Workspace {
    root: Option<TreeNode>,
    panes: Vec<Entity<EditorPane>>,
    active_pane: usize,
    status_message: String,
    focus_handle: gpui::FocusHandle,
    // ------------------------------------------------------------- AI stack
    pub hub: Arc<AiHub>,
    /// Shared channel every async producer (agent, terminal, inline suggest)
    /// streams results into; drained by the pump task in main.rs.
    pub ai_tx: futures::channel::mpsc::UnboundedSender<UiEvent>,
    pub chat: ChatPanel,
    pub term: TerminalPanel,
    pub mem: MemoryPanel,
    pub show_ai: bool,
    pub show_term: bool,
    pub show_mem: bool,
}

impl Workspace {
    pub fn new(
        startup_path: Option<PathBuf>,
        hub: Arc<AiHub>,
        ai_tx: futures::channel::mpsc::UnboundedSender<UiEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut workspace = Workspace {
            root: None,
            panes: Vec::new(),
            active_pane: 0,
            status_message: "Ctrl+O open · Ctrl+Shift+O folder · Alt+A AI · Alt+T terminal"
                .to_string(),
            focus_handle: cx.focus_handle(),
            hub: hub.clone(),
            ai_tx,
            chat: ChatPanel::new(cx),
            term: TerminalPanel::new(cx),
            mem: MemoryPanel::new(),
            show_ai: true,
            show_term: false,
            show_mem: false,
        };

        match startup_path {
            Some(path) if path.is_dir() => {
                workspace.open_folder_at(path.clone(), cx);
                let seed = path.join("src").join("main.rs");
                if seed.is_file() {
                    workspace.open_file_in(&seed, cx);
                }
            }
            Some(path) => workspace.open_file_in(&path, cx),
            None => {
                let pane = make_editor(None, &hub, workspace.ai_tx.clone(), cx);
                workspace.panes.push(pane);
            }
        }
        workspace
    }

    pub fn focus_active_pane(&self, window: &mut Window, cx: &App) {
        if let Some(pane) = self.active_pane_entity() {
            pane.read(cx).focus(window);
        }
    }

    fn active_pane_entity(&self) -> Option<Entity<EditorPane>> {
        self.panes.get(self.active_pane).cloned()
    }

    fn open_folder_at(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let mut node = TreeNode::from_path(&path, tree_node_name(&path), true);
        node.expanded = true;
        node.load_children();
        self.root = Some(node);
        self.hub.set_root(path.clone());
        self.status_message = format!("Folder: {}", path.display());
        cx.notify();
    }

    fn open_file_in(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !path.is_file() {
            self.status_message = format!("Not a file: {}", path.display());
            cx.notify();
            return;
        }
        if let Some(existing) = self.panes.iter().position(|pane| {
            pane.read(cx)
                .doc
                .read(cx)
                .path
                .as_ref()
                .is_some_and(|p| p == path)
        }) {
            self.active_pane = existing;
            cx.notify();
            return;
        }
        let pane = make_editor(Some(path.to_path_buf()), &self.hub, self.ai_tx.clone(), cx);
        self.panes.push(pane);
        self.active_pane = self.panes.len() - 1;
        self.status_message = format!("Opened {}", path.display());
        cx.notify();
    }

    /// Open the live config.toml as an editable tab; saving it re-applies AI
    /// settings (providers, model, proxy routing, memory engine...).
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let path = ai::config::Config::path();
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, ai::config::Config::template());
        }
        self.open_file_in(&path, cx);
    }

    fn handle_tree_click(&mut self, path: &Path, cx: &mut Context<Self>) {
        if path.is_dir() {
            if let Some(root) = &mut self.root {
                root.toggle_at(path);
            }
        } else {
            self.open_file_in(path, cx);
        }
        cx.notify();
    }

    fn new_file(&mut self, cx: &mut Context<Self>) {
        let pane = make_editor(None, cx);
        self.panes.push(pane);
        self.active_pane = self.panes.len() - 1;
        self.status_message = "New untitled buffer".to_string();
        cx.notify();
    }

    fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if self.panes.is_empty() {
            return;
        }
        self.panes.remove(self.active_pane);
        if self.active_pane >= self.panes.len() {
            self.active_pane = self.panes.len().saturating_sub(1);
        }
        self.status_message = "Tab closed".to_string();
        cx.notify();
    }

    fn cycle_tab(&mut self, offset: isize, cx: &mut Context<Self>) {
        if self.panes.is_empty() {
            return;
        }
        let count = self.panes.len() as isize;
        let next = (self.active_pane as isize + offset).rem_euclid(count) as usize;
        self.active_pane = next;
        cx.notify();
    }

    fn save_active(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane_entity() else {
            return;
        };
        let has_path = pane.read(cx).doc.read(cx).path.is_some();
        if has_path {
            let name = pane.read(cx).doc.read(cx).display_name.clone();
            let saved_path = pane.read(cx).doc.read(cx).path.clone();
            let outcome =
                pane.update(cx, |pane, cx| pane.doc.update(cx, |doc, _| doc.save()));
            self.status_message = match &outcome {
                Ok(()) => format!("Saved {}", name),
                Err(err) => format!("Save failed: {}", err),
            };
            // Live-reload AI config when the user saves the config tab.
            if outcome.is_ok()
                && saved_path.as_deref() == Some(ai::config::Config::path().as_path())
            {
                match self.hub.reload_config() {
                    Ok(summary) => self.status_message = summary,
                    Err(e) => self.status_message = format!("config error: {e:#}"),
                }
            }
            cx.notify();
        } else {
            self.save_active_as(cx);
        }
    }

    fn save_active_as(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.active_pane_entity() else {
            return;
        };
        let directory = pane
            .read(cx)
            .doc
            .read(cx)
            .path
            .clone()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| self.root.as_ref().map(|r| r.path.clone()))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let receiver = cx.prompt_for_new_path(&directory, Some("untitled.rs"));
        cx.spawn(async move |this, cx| {
            let picked = receiver
                .await
                .ok()
                .and_then(|result| result.ok())
                .flatten();
            if let Some(path) = picked {
                this.update(cx, |workspace, cx| {
                    if let Some(pane) = workspace.active_pane_entity() {
                        pane.update(cx, |pane, cx| {
                            pane.doc.update(cx, |doc, _| {
                                doc.set_path(path.clone());
                                let _ = doc.save();
                            });
                        });
                        workspace.status_message = format!("Saved {}", path.display());
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn prompt_open(&mut self, directories: bool, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: !directories,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let picked = receiver
                .await
                .ok()
                .and_then(|result| result.ok())
                .flatten();
            if let Some(paths) = picked {
                this.update(cx, |workspace, cx| {
                    for path in paths {
                        if path.is_dir() {
                            workspace.open_folder_at(path, cx);
                        } else {
                            workspace.open_file_in(&path, cx);
                        }
                    }
                })
                .ok();
            }
        })
        .detach();
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let status = self.status_message.clone();
        let active_pane = self.active_pane_entity();
        let show_ai = self.show_ai;
        let show_mem = self.show_mem && !self.show_ai;
        let show_term = self.show_term;

        let hub_for_actions = self.hub.clone();
        let hub_session = self.hub.clone();
        let hub_stop = self.hub.clone();

        div()
            .id("workspace")
            .size_full()
            .flex()
            .flex_col()
            .bg(hex(Theme::BG))
            .text_color(hex(Theme::TEXT))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.prompt_open(true, cx)))
            .on_action(cx.listener(|this, _: &OpenFiles, _, cx| this.prompt_open(false, cx)))
            .on_action(cx.listener(|this, _: &NewFile, _, cx| this.new_file(cx)))
            .on_action(cx.listener(|this, _: &SaveActive, _, cx| this.save_active(cx)))
            .on_action(cx.listener(|this, _: &SaveActiveAs, _, cx| this.save_active_as(cx)))
            .on_action(cx.listener(|this, _: &CloseActiveTab, _, cx| this.close_active_tab(cx)))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.cycle_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| this.cycle_tab(-1, cx)))
            .on_action(cx.listener(|this, _: &ToggleAi, _, cx| {
                this.show_ai = !this.show_ai;
                if this.show_ai {
                    this.show_mem = false;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminal, _, cx| {
                this.show_term = !this.show_term;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleMemory, _, cx| {
                this.show_mem = !this.show_mem;
                if this.show_mem {
                    this.show_ai = false;
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.open_settings(cx)))
            .on_action(cx.listener(move |this, _: &NewSession, _, cx| {
                let _ = hub_session.new_session();
                this.chat.msgs.push(crate::panels::chat::ChatMsg::System(
                    "— new session started —".into(),
                ));
                this.chat.tasks.clear();
                this.chat.pending = None;
                this.chat.streaming.clear();
                this.status_message = "new AI session".into();
                cx.notify();
            }))
            .on_action(move |_: &StopAi, _, _| {
                hub_stop.stop();
            })
            // ---- toolbar ---------------------------------------------------
            .child(self.render_toolbar(hub_for_actions, entity.clone(), window, cx))
            // ---- main area --------------------------------------------------
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(self.render_sidebar(entity.clone(), cx))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(self.render_tab_bar(cx))
                            .child(
                                div().flex_1().overflow_hidden().when_some(
                                    active_pane,
                                    |el, pane| el.child(pane),
                                ),
                            )
                            .children(show_term.then(|| {
                                self.render_terminal_panel(self.hub.clone(), window, cx)
                            })),
                    )
                    .when(show_ai || show_mem, |el| {
                        if show_ai {
                            el.child(self.render_chat_panel(self.hub.clone(), window, cx))
                        } else {
                            el.child(self.render_memory_panel(self.hub.clone(), window, cx))
                        }
                    }),
            )
            .child(self.render_status_bar(status, cx))
    }
}

impl Workspace {
    fn render_toolbar(
        &mut self,
        hub: Arc<AiHub>,
        ws: Entity<Self>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let busy = hub.busy();
        let mut bar = div()
            .id("toolbar")
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(32.0))
            .flex_none()
            .bg(hex(Theme::PANEL))
            .border_b_1()
            .border_color(hex(Theme::PANEL_BORDER));

        let folder_ws = ws.clone();
        let files_ws = ws.clone();
        let new_ws = ws.clone();
        let save_ws = ws.clone();
        let term_ws = ws.clone();
        let mem_ws = ws.clone();
        let cfg_ws = ws.clone();
        let toggle_ws = ws;

        bar = bar
            .child(tool_btn("tb-folder", "Folder", move |ws, cx| {
                ws.prompt_open(true, cx)
            }, folder_ws))
            .child(tool_btn("tb-files", "Files", move |ws, cx| {
                ws.prompt_open(false, cx)
            }, files_ws))
            .child(tool_btn("tb-new", "New", move |ws, cx| ws.new_file(cx), new_ws))
            .child(tool_btn("tb-save", "Save", move |ws, cx| ws.save_active(cx), save_ws))
            .child(div().flex_1())
            .child(if busy {
                div()
                    .text_size(px(11.0))
                    .text_color(hex(Theme::YELLOW))
                    .child("AI busy")
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child({
                let stop_hub = hub.clone();
                pill_tb("tb-stop", "Stop AI", false).on_mouse_down(MouseButton::Left, move |_, _, _| {
                    stop_hub.stop();
                })
            })
            .child({
                let sess_ws = toggle_ws.clone();
                pill_tb("tb-newchat", "+ Chat", false).on_mouse_down(MouseButton::Left, move |_, _, app| {
                    let _ = sess_ws.update(app, |ws, cx| {
                        let _ = ws.hub.new_session();
                        ws.chat.msgs.push(crate::panels::chat::ChatMsg::System(
                            "— new session started —".into(),
                        ));
                        ws.chat.tasks.clear();
                        ws.chat.pending = None;
                        ws.chat.streaming.clear();
                        cx.notify();
                    });
                })
            })
            .child(pill_tb("tb-ai", "Chat", self.show_ai).on_mouse_down(MouseButton::Left, {
                move |_, _, app| {
                    let _ = toggle_ws.update(app, |ws, cx| {
                        ws.show_ai = !ws.show_ai;
                        if ws.show_ai {
                            ws.show_mem = false;
                        }
                        cx.notify();
                    });
                }
            }))
            .child(pill_tb("tb-term", "Terminal", self.show_term).on_mouse_down(MouseButton::Left, {
                move |_, _, app| {
                    let _ = term_ws.update(app, |ws, cx| {
                        ws.show_term = !ws.show_term;
                        cx.notify();
                    });
                }
            }))
            .child(pill_tb("tb-mem", "Memory", self.show_mem).on_mouse_down(MouseButton::Left, {
                move |_, _, app| {
                    let _ = mem_ws.update(app, |ws, cx| {
                        ws.show_mem = !ws.show_mem;
                        if ws.show_mem {
                            ws.show_ai = false;
                        }
                        cx.notify();
                    });
                }
            }))
            .child(pill_tb("tb-cfg", "Settings", false).on_mouse_down(MouseButton::Left, move |_, _, app| {
                let _ = cfg_ws.update(app, |ws, cx| ws.open_settings(cx));
            }));
        bar
    }
}

/// Toolbar button that calls a workspace method on click.
fn tool_btn<F>(id: &'static str, label: &'static str, f: F, ws: Entity<Workspace>) -> gpui::Stateful<gpui::Div>
where
    F: Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
{
    div()
        .id(id)
        .px_2()
        .py(px(3.0))
        .rounded_sm()
        .text_size(px(11.5))
        .text_color(hex(Theme::TEXT_DIM))
        .bg(hex(0x2a2f38))
        .cursor_pointer()
        .hover(|s| s.bg(hex(0x353b45)).text_color(hex(Theme::TEXT)))
        .child(label)
        .on_mouse_down(MouseButton::Left, move |_, _, app| {
            let _ = ws.update(app, |ws, cx| f(ws, cx));
        })
}

fn pill_tb(id: &'static str, label: &'static str, active: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py(px(3.0))
        .rounded_sm()
        .text_size(px(11.0))
        .cursor_pointer()
        .text_color(if active { hex(Theme::BG) } else { hex(Theme::TEXT_DIM) })
        .bg(if active { hex(Theme::ACCENT) } else { hex(0x2a2f38) })
        .hover(|s| s.bg(hex(0x353b45)).text_color(hex(Theme::TEXT)))
        .child(label)
}

/// Cross-thread request flags consumed by the pump in main.rs. Buttons live
/// in closures without async contexts; flags keep the bridge trivial.
pub const AI_PANEL_WIDTH: f32 = AI_PANEL_W;

impl Workspace {
    fn render_sidebar(
        &self,
        workspace: Entity<Workspace>,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let mut sidebar = div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(hex(Theme::PANEL))
            .border_r_1()
            .border_color(hex(Theme::PANEL_BORDER));

        let folder_btn = toolbar_button("btn-folder", "Folder")
            .on_mouse_down(MouseButton::Left, {
                let workspace = workspace.clone();
                move |_, _, cx: &mut App| {
                    workspace.update(cx, |ws, cx| ws.prompt_open(true, cx));
                }
            });
        let files_btn = toolbar_button("btn-files", "Files")
            .on_mouse_down(MouseButton::Left, {
                let workspace = workspace.clone();
                move |_, _, cx: &mut App| {
                    workspace.update(cx, |ws, cx| ws.prompt_open(false, cx));
                }
            });
        let new_btn = toolbar_button("btn-new", "New")
            .on_mouse_down(MouseButton::Left, {
                let workspace = workspace.clone();
                move |_, _, cx: &mut App| {
                    workspace.update(cx, |ws, cx| ws.new_file(cx));
                }
            });

        sidebar = sidebar.child(
            div()
                .flex()
                .gap_2()
                .p_2()
                .border_b_1()
                .border_color(hex(Theme::PANEL_BORDER))
                .child(folder_btn)
                .child(files_btn)
                .child(new_btn),
        );

        sidebar = sidebar.child(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_size(px(10.5))
                .text_color(hex(Theme::TEXT_DIM))
                .child("EXPLORER"),
        );

        if let Some(root) = &self.root {
            let root_clone = root.clone();
            sidebar = sidebar.child(
                div()
                    .id("tree-scroll")
                    .flex_1()
                    .overflow_scroll()
                    .child(render_tree_node(&root_clone, 0, workspace)),
            );
        } else {
            sidebar = sidebar.child(
                div()
                    .p_3()
                    .text_size(px(12.0))
                    .line_height(px(18.0))
                    .text_color(hex(Theme::TEXT_DIM))
                    .child("No folder open.\nUse Folder / Files above\nor press Ctrl+Shift+O."),
            );
        }
        sidebar
    }

    fn render_tab_bar(&self, cx: &Context<Self>) -> gpui::Div {
        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(34.0))
            .flex_none()
            .overflow_hidden()
            .bg(hex(Theme::PANEL))
            .border_b_1()
            .border_color(hex(Theme::PANEL_BORDER));

        let workspace = cx.entity();
        for (index, pane) in self.panes.iter().enumerate() {
            let doc = pane.read(cx).doc.read(cx);
            let title = doc.display_name.clone();
            let dirty = doc.dirty;
            let is_active = index == self.active_pane;

            let tab_workspace = workspace.clone();
            let tab = div()
                .id(ElementId::named_usize("tab", index))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py(px(4.0))
                .rounded_sm()
                .text_size(px(12.0))
                .cursor_pointer()
                .flex_none()
                .bg(if is_active {
                    hex(Theme::TAB_ACTIVE)
                } else {
                    hex(Theme::TAB_INACTIVE)
                })
                .text_color(if is_active {
                    hex(Theme::TEXT)
                } else {
                    hex(Theme::TEXT_DIM)
                })
                .hover(|style| style.text_color(hex(Theme::TEXT)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx: &mut App| {
                    tab_workspace.update(cx, |ws, cx| {
                        ws.active_pane = index;
                        cx.notify();
                    });
                });

            let label = if dirty {
                format!("{title} ●")
            } else {
                title
            };

            bar = bar.child(tab.child(SharedString::from(label)));
        }
        bar
    }

    fn render_status_bar(&self, status: String, cx: &Context<Self>) -> gpui::Div {
        let (cursor_info, language, dirty) = self
            .active_pane_entity()
            .map(|pane| {
                let doc = pane.read(cx).doc.read(cx);
                (
                    format!("Ln {}, Col {}", doc.cursor.row + 1, doc.cursor.col + 1),
                    doc.language_label().to_string(),
                    doc.dirty,
                )
            })
            .unwrap_or_else(|| ("—".to_string(), "Plain Text".to_string(), false));

        let mode = self.hub.current_mode();
        let busy = self.hub.busy();
        let model = self.hub.model_label();
        let proxy = {
            let cfg = self.hub.cfg.lock().unwrap();
            if cfg.settings.proxy_active() {
                format!(
                    "proxy[llm:{} web:{} fetch:{}]",
                    cfg.settings.proxy_llm,
                    cfg.settings.proxy_web_search,
                    cfg.settings.proxy_fetch
                )
            } else {
                String::new()
            }
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .h(px(26.0))
            .flex_none()
            .text_size(px(11.5))
            .bg(hex(Theme::PANEL))
            .border_t_1()
            .border_color(hex(Theme::PANEL_BORDER))
            .text_color(hex(Theme::TEXT_DIM))
            .child(SharedString::from(status))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .children(busy.then(|| SharedString::from("● AI busy")))
                    .child(SharedString::from(format!("{:?}", mode)))
                    .child(SharedString::from(model))
                    .children((!proxy.is_empty()).then(|| SharedString::from(proxy)))
                    .child(if dirty { "● modified" } else { "" })
                    .child(language)
                    .child(cursor_info),
            )
    }
}

fn toolbar_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py(px(2.0))
        .rounded_sm()
        .text_size(px(11.5))
        .text_color(hex(Theme::TEXT_DIM))
        .bg(hex(0x2a2f38))
        .cursor_pointer()
        .hover(|style| style.bg(hex(0x353b45)).text_color(hex(Theme::TEXT)))
        .child(label)
}

fn tree_id(path: &Path) -> ElementId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    ElementId::named_usize("tree-node", hasher.finish() as usize)
}

fn render_tree_node(
    node: &TreeNode,
    depth: usize,
    workspace: Entity<Workspace>,
) -> gpui::AnyElement {
    let icon = if node.is_dir {
        if node.expanded {
            "▾ "
        } else {
            "▸ "
        }
    } else {
        "  "
    };
    let color = if node.is_dir {
        Theme::ACCENT
    } else if syntax::is_code_file(&node.path) {
        Theme::GREEN
    } else {
        Theme::TEXT_DIM
    };

    let row = div()
        .id(tree_id(&node.path))
        .flex()
        .items_center()
        .pl(px((depth * 12 + 4) as f32))
        .pr_2()
        .py(px(1.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .text_color(hex(color))
        .hover(|style| style.bg(hex(0x2c313a)))
        .on_mouse_down(MouseButton::Left, {
            let workspace = workspace.clone();
            let path = node.path.clone();
            move |_, _, cx: &mut App| {
                workspace.update(cx, |ws, cx| ws.handle_tree_click(&path, cx));
            }
        })
        .child(icon)
        .child(SharedString::from(node.name.clone()));

    let children: Vec<gpui::AnyElement> = if node.is_dir && node.expanded {
        node.children
            .iter()
            .map(|child| render_tree_node(child, depth + 1, workspace.clone()))
            .collect()
    } else {
        Vec::new()
    };

    div().flex().flex_col().child(row).children(children).into_any_element()
}

fn make_editor(
    path: Option<PathBuf>,
    hub: &Arc<AiHub>,
    ai_tx: futures::channel::mpsc::UnboundedSender<UiEvent>,
    cx: &mut Context<Workspace>,
) -> Entity<EditorPane> {
    let doc = match path {
        Some(p) => match Document::open(p) {
            Ok(doc) => doc,
            Err(_) => Document::new_empty(),
        },
        None => Document::new_empty(),
    };
    let doc_entity = cx.new(|_| doc);
    cx.new(|cx| EditorPane::new(doc_entity, cx).with_ai(hub.clone(), ai_tx))
}
