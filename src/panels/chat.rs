//! Chat panel: talk to the agent, approve gated tools, watch the task board.
//!
//! Panel state lives in [`ChatPanel`] (owned by Workspace); all rendering and
//! interaction handlers are `impl Workspace` methods so they can reach both
//! the panel state and the shared [`AiHub`].

use gpui::{div, prelude::*, px, MouseButton, SharedString};
use std::sync::Arc;

use crate::ai::{AiHub, Mode, UiEvent};
use crate::panels::pill;
use crate::theme::{hex, MONO_FONT, Theme};
use crate::workspace::Workspace;

#[derive(Debug, Clone)]
pub enum ChatMsg {
    User(String),
    Assistant(String),
    Tool { name: String, detail: String },
    System(String),
}

pub struct PendingApproval {
    pub id: u64,
    pub tool: String,
    pub detail: String,
}

pub struct ChatPanel {
    pub msgs: Vec<ChatMsg>,
    pub streaming: String,
    pub pending: Option<PendingApproval>,
    pub tasks: Vec<(String, bool)>,
    pub usage: Option<(u64, u64, u64)>,
    pub input: crate::panels::MiniInput,
}

impl ChatPanel {
    pub fn new(cx: &mut gpui::App) -> Self {
        ChatPanel {
            msgs: vec![ChatMsg::System(
                "AI panel ready.\n\nAdd a provider in Settings (toolbar), pick default_model, \
                 then ask anything.\nModes: Chat = plain talk · Plan = read-only · Agent = \
                 full toolkit with approvals."
                    .to_string(),
            )],
            streaming: String::new(),
            pending: None,
            tasks: Vec::new(),
            usage: None,
            input: crate::panels::MiniInput::new(cx),
        }
    }

    fn push(&mut self, msg: ChatMsg) {
        self.msgs.push(msg);
        if self.msgs.len() > 400 {
            self.msgs.remove(0);
        }
    }
}

impl Workspace {
    /// Entry point used by the UiEvent pump in main.rs / workspace.rs.
    pub fn handle_ai_event(&mut self, ev: UiEvent, cx: &mut gpui::Context<Self>) {
        match ev {
            UiEvent::Delta(d) => self.chat.streaming.push_str(&d),
            UiEvent::ToolStart { name, detail } => {
                if !self.chat.streaming.trim().is_empty() {
                    let done = std::mem::take(&mut self.chat.streaming);
                    self.chat.push(ChatMsg::Assistant(done));
                }
                self.chat.push(ChatMsg::Tool { name, detail });
            }
            UiEvent::ToolEnd { .. } => {}
            UiEvent::ApprovalRequest { id, tool, detail } => {
                self.chat.pending = Some(PendingApproval { id, tool, detail });
            }
            UiEvent::Usage { prompt, completion, total } => {
                self.chat.usage = Some((prompt, completion, total));
            }
            UiEvent::Tasks(v) => {
                self.chat.tasks = v
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| {
                        (
                            t.get("text").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                            t.get("status").and_then(|x| x.as_str()) == Some("done"),
                        )
                    })
                    .collect();
            }
            UiEvent::Compacted => self.chat.push(ChatMsg::System("history compacted".into())),
            UiEvent::Stopped => {
                let partial = std::mem::take(&mut self.chat.streaming);
                if !partial.trim().is_empty() {
                    self.chat.push(ChatMsg::Assistant(partial));
                }
                self.chat.push(ChatMsg::System("■ stopped".into()));
            }
            UiEvent::Error(e) => {
                self.chat.streaming.clear();
                self.chat.push(ChatMsg::System(format!("⚠ {e}")));
            }
            UiEvent::Done(final_text) => {
                let streamed = std::mem::take(&mut self.chat.streaming);
                let body = if final_text.trim().is_empty() && !streamed.trim().is_empty() {
                    streamed
                } else if !final_text.trim().is_empty() {
                    final_text
                } else {
                    String::new()
                };
                if !body.trim().is_empty() {
                    self.chat.push(ChatMsg::Assistant(body));
                }
            }
            UiEvent::TerminalOut(line) => self.term.append_line(line),
            UiEvent::TerminalExit(code) => {
                self.term.running = false;
                self.term.append_line(match code {
                    Some(0) => "[exit 0]".to_string(),
                    Some(c) => format!("[exit {c}]"),
                    None => "[terminated]".to_string(),
                });
            }
            UiEvent::AiGhost { text } => {
                if let Some(pane) = self.active_pane_entity() {
                    pane.update(cx, |pane, _| pane.set_ai_ghost(text));
                }
            }
        }
        cx.notify();
    }

    pub fn submit_chat(&mut self, text: String) {
        if !self.chat.streaming.is_empty() || self.chat.pending.is_some() {
            // still busy; drop politely
            return;
        }
        self.chat.push(ChatMsg::User(text));
    }

    pub fn render_chat_panel(
        &mut self,
        hub: Arc<AiHub>,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let mode = hub.current_mode();
        let busy = hub.busy();

        let mut col = div()
            .id("ai-panel")
            .w(px(AI_PANEL_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(hex(Theme::PANEL))
            .border_l_1()
            .border_color(hex(Theme::PANEL_BORDER));

        // ---- header -------------------------------------------------------
        let ws_handle = cx.entity();
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .h(px(30.0))
                .flex_none()
                .border_b_1()
                .border_color(hex(Theme::PANEL_BORDER))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(hex(if busy { Theme::YELLOW } else { Theme::TEXT_DIM }))
                        .child(if busy { "● thinking" } else { "○ idle" }),
                )
                .child(div().flex_1())
                .child(mode_pill(ws_handle.clone(), hub.clone(), Mode::Chat, "mode-chat", "Chat", mode == Mode::Chat))
                .child(mode_pill(ws_handle.clone(), hub.clone(), Mode::Plan, "mode-plan", "Plan", mode == Mode::Plan))
                .child(mode_pill(ws_handle.clone(), hub.clone(), Mode::Agent, "mode-agent", "Agent", mode == Mode::Agent)),
        );

        // ---- transcript -----------------------------------------------------
        let msgs = self.chat.msgs.clone();
        let streaming = self.chat.streaming.clone();
        col = col.child(
            div()
                .id("chat-scroll")
                .flex_1()
                .overflow_scroll()
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .children(msgs.into_iter().enumerate().map(|(i, m)| bubble(i, m)))
                .children((!streaming.is_empty()).then(|| {
                    div()
                        .max_w_full()
                        .rounded_sm()
                        .p_2()
                        .text_size(px(12.0))
                        .line_height(px(17.0))
                        .bg(hex(0x24344a))
                        .child(SharedString::from(streaming + " ▊"))
                })),
        );

        // ---- task board ------------------------------------------------------
        if !self.chat.tasks.is_empty() {
            let tasks = self.chat.tasks.clone();
            col = col.child(
                div()
                    .flex_none()
                    .h(px(110.0))
                    .overflow_hidden()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(hex(Theme::PANEL_BORDER))
                    .child(div().flex().flex_col().children(tasks.into_iter().map(
                        |(text, done)| {
                            div()
                                .text_size(px(11.0))
                                .text_color(hex(if done { Theme::GREEN } else { Theme::TEXT_DIM }))
                                .child(SharedString::from(format!(
                                    "{} {}",
                                    if done { "☑" } else { "☐" },
                                    text
                                )))
                        },
                    ))),
            );
        }

        // ---- approval card ----------------------------------------------------
        if let Some(p) = self.chat.pending.clone() {
            let allow_hub = hub.clone();
            let deny_hub = hub.clone();
            let allow_ws = cx.entity();
            let deny_ws = cx.entity();
            col = col.child(
                div()
                    .flex_none()
                    .m_2()
                    .p_2()
                    .rounded_sm()
                    .bg(hex(0x3a3325))
                    .border_1()
                    .border_color(hex(Theme::YELLOW))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(hex(Theme::YELLOW))
                                    .child(SharedString::from(format!("⚠ {} wants to run", p.tool))),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_family(MONO_FONT)
                                    .text_color(hex(Theme::TEXT_DIM))
                                    .child(SharedString::from(
                                        p.detail.chars().take(160).collect::<String>(),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(pill("approve", "Allow", true).on_mouse_down(
                                        MouseButton::Left,
                                        move |_, _, app| {
                                            allow_hub.respond(p.id, true);
                                            let _ = allow_ws.update(app, |ws, cx| {
                                                ws.chat.pending = None;
                                                cx.notify();
                                            });
                                        },
                                    ))
                                    .child(pill("deny", "Deny", false).on_mouse_down(
                                        MouseButton::Left,
                                        move |_, _, app| {
                                            deny_hub.respond(p.id, false);
                                            let _ = deny_ws.update(app, |ws, cx| {
                                                ws.chat.pending = None;
                                                cx.notify();
                                            });
                                        },
                                    )),
                            ),
                    ),
            );
        }

        // ---- usage footer ------------------------------------------------------
        if let Some((pr, co, to)) = self.chat.usage {
            col = col.child(
                div()
                    .flex_none()
                    .px_2()
                    .pb_1()
                    .text_size(px(10.5))
                    .text_color(hex(Theme::TEXT_DIM))
                    .child(SharedString::from(format!("tokens ↑{pr} ↓{co} Σ{to}"))),
            );
        }

        // ---- input row ----------------------------------------------------------
        let input_focus = self.chat.input.focus.clone();
        let send_hub = hub.clone();
        let send_ws = cx.entity();
        let key_hub = hub.clone();

        col.child(
            div()
                .id("chat-input-row")
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .border_t_1()
                .border_color(hex(Theme::PANEL_BORDER))
                .track_focus(&input_focus)
                .on_mouse_down(MouseButton::Left, move |_, window: &mut gpui::Window, _| {
                    window.focus(&focus_clone_of(&input_focus));
                })
                .on_key_down(move |ws: &mut Workspace, ev: &gpui::KeyDownEvent, _, cx| {
                    let enter = ws.chat.input.on_key(ev);
                    if enter {
                        let text = ws.chat.input.take();
                        if !text.trim().is_empty() {
                            ws.submit_chat(text.clone());
                            let tx = ws.ai_tx.clone();
                            key_hub.send_prompt(&text, tx);
                        }
                    }
                    cx.notify();
                })
                .child(
                    div()
                        .flex_1()
                        .h(px(26.0))
                        .px_2()
                        .py(px(5.0))
                        .rounded_sm()
                        .bg(hex(Theme::EDITOR_BG))
                        .border_1()
                        .border_color(hex(Theme::PANEL_BORDER))
                        .text_size(px(12.0))
                        .child(SharedString::from(if self.chat.input.text.is_empty() {
                            "Ask anything… Enter sends".to_string()
                        } else {
                            format!("{}▊", self.chat.input.text)
                        })),
                )
                .child(pill("send-btn", "Send", true).on_mouse_down(
                    MouseButton::Left,
                    move |_, _, app| {
                        let _ = send_ws.update(app, |ws, _| {
                            let text = ws.chat.input.take();
                            if !text.trim().is_empty() {
                                ws.submit_chat(text.clone());
                                let tx = ws.ai_tx.clone();
                                send_hub.send_prompt(&text, tx);
                            }
                        });
                    },
                )),
        )
    }
}

fn focus_clone_of(h: &gpui::FocusHandle) -> gpui::FocusHandle {
    h.clone()
}

fn mode_pill(
    ws: gpui::Entity<Workspace>,
    hub: Arc<AiHub>,
    mode: Mode,
    id: &'static str,
    label: &'static str,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    pill(id, label, active).on_mouse_down(MouseButton::Left, move |_, _, app| {
        hub.set_mode(mode);
        let _ = ws.update(app, |_, cx| cx.notify());
    })
}

fn bubble(ix: usize, msg: &ChatMsg) -> gpui::AnyElement {
    match msg {
        ChatMsg::User(t) => align_end(ix, hex(0x2a4162), hex(Theme::TEXT), t),
        ChatMsg::Assistant(t) => align_start(ix, hex(0x262b34), hex(Theme::TEXT), t),
        ChatMsg::System(t) => align_start(ix, hex(0x23272e), hex(Theme::TEXT_DIM), t),
        ChatMsg::Tool { name, detail } => div()
            .max_w_full()
            .rounded_sm()
            .p_2()
            .bg(hex(0x30363f))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_family(MONO_FONT)
                    .text_color(hex(Theme::ACCENT))
                    .child(SharedString::from(format!("⚒ {name}"))),
            )
            .child(
                div()
                    .text_size(px(10.5))
                    .font_family(MONO_FONT)
                    .text_color(hex(Theme::TEXT_DIM))
                    .child(SharedString::from(detail.chars().take(140).collect::<String>())),
            )
            .into_any_element(),
    }
}

fn align_start(_ix: usize, bg: gpui::Hsla, fg: gpui::Hsla, text: &str) -> gpui::AnyElement {
    div()
        .max_w_full()
        .rounded_sm()
        .p_2()
        .bg(bg)
        .text_size(px(12.0))
        .line_height(px(17.0))
        .text_color(fg)
        .child(SharedString::from(text.replace("**", "")))
        .into_any_element()
}

fn align_end(_ix: usize, bg: gpui::Hsla, fg: gpui::Hsla, text: &str) -> gpui::AnyElement {
    div()
        .w_full()
        .rounded_sm()
        .p_2()
        .bg(bg)
        .text_size(px(12.0))
        .line_height(px(17.0))
        .text_color(fg)
        .child(SharedString::from(text))
        .into_any_element()
}

pub const AI_PANEL_W: f32 = 380.0;
