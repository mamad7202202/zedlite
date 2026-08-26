//! Terminal panel: run shell commands in the workspace root and watch their
//! output stream live. Stop kills the running process.

use gpui::{div, prelude::*, px, MouseButton};
use std::collections::VecDeque;
use std::sync::Arc;

use crate::ai::AiHub;
use crate::panels::pill;
use crate::theme::{hex, MONO_FONT, Theme};
use crate::workspace::Workspace;

pub struct TerminalPanel {
    pub lines: VecDeque<String>,
    pub input: crate::panels::MiniInput,
    pub running: bool,
}

const MAX_LINES: usize = 800;

impl TerminalPanel {
    pub fn new(cx: &mut gpui::App) -> Self {
        TerminalPanel {
            lines: VecDeque::new(),
            input: crate::panels::MiniInput::new(cx),
            running: false,
        }
    }

    pub fn append_line(&mut self, line: String) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

impl Workspace {
    pub fn run_terminal(&mut self, command: String) {
        let cmd = command.trim().to_string();
        if cmd.is_empty() || self.term.running {
            return;
        }
        self.term.running = true;
        self.term.append_line(format!("❯ {cmd}"));
        let tx = self.ai_tx.clone();
        self.hub.run_terminal_command(&cmd, tx);
    }

    pub fn render_terminal_panel(
        &mut self,
        hub: Arc<AiHub>,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let cwd = hub.root.lock().unwrap().display().to_string();
        let running = self.term.running;
        let ws_handle = cx.entity();

        let mut col = div()
            .id("terminal-panel")
            .h(px(TERMINAL_H))
            .flex_none()
            .flex()
            .flex_col()
            .bg(hex(0x14161a))
            .border_t_1()
            .border_color(hex(Theme::PANEL_BORDER));

        // header
        let stop_hub = hub.clone();
        let clear_ws = ws_handle.clone();
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(26.0))
                .flex_none()
                .bg(hex(Theme::PANEL))
                .border_b_1()
                .border_color(hex(Theme::PANEL_BORDER))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(hex(Theme::TEXT_DIM))
                        .child("TERMINAL"),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(10.5))
                        .font_family(MONO_FONT)
                        .text_color(hex(Theme::ACCENT))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(SharedString::from(cwd)),
                )
                .children(running.then(|| {
                    div()
                        .text_size(px(11.0))
                        .text_color(hex(Theme::YELLOW))
                        .child("running…")
                }))
                .child(if running {
                    pill("term-stop", "Stop", false).on_mouse_down(MouseButton::Left, move |_, _, _| {
                        stop_hub.kill_terminal_sync();
                    })
                } else {
                    pill("term-clear", "Clear", false).on_mouse_down(MouseButton::Left, move |_, _, app| {
                        let _ = clear_ws.update(app, |ws, _| {
                            ws.term.lines.clear();
                        });
                    })
                }),
        );

        // output
        let lines: Vec<String> = self.term.lines.iter().cloned().collect();
        col = col.child(
            div()
                .id("term-scroll")
                .flex_1()
                .overflow_scroll()
                .px_2()
                .py_1()
                .font_family(MONO_FONT)
                .text_size(px(11.5))
                .line_height(px(16.0))
                .text_color(hex(0xb9c0cb))
                .child(
                    div().flex().flex_col().children(lines.into_iter().map(|l| {
                        div().whitespace_pre().child(SharedString::from(l))
                    })),
                ),
        );

        // prompt / input
        let input_focus = self.term.input.focus.clone();
        let run_ws = ws_handle;

        col.child(
            div()
                .id("term-input-row")
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py(px(4.0))
                .bg(hex(Theme::PANEL))
                .track_focus(&input_focus)
                .on_mouse_down(MouseButton::Left, move |_: &gpui::MouseDownEvent, window: &mut gpui::Window, _: &mut gpui::App| {
                    window.focus(&input_focus.clone());
                })
                .on_key_down(move |ws: &mut Workspace, ev: &gpui::KeyDownEvent, _, cx| {
                    let was_running = ws.term.running;
                    let enter = (!was_running) && ws.term.input.on_key(ev);
                    if enter {
                        let text = ws.term.input.take();
                        if !text.trim().is_empty() {
                            ws.run_terminal(text);
                        }
                    }
                    cx.notify();
                })
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_family(MONO_FONT)
                        .text_color(hex(Theme::GREEN))
                        .child("❯"),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(22.0))
                        .text_size(px(12.0))
                        .font_family(MONO_FONT)
                        .child(SharedString::from(if self.term.input.text.is_empty() {
                            "type a command…".to_string()
                        } else {
                            format!("{}▊", self.term.input.text)
                        })),
                )
                .child(pill("term-run", "Run", true).on_mouse_down(MouseButton::Left, move |_, _, app| {
                    let _ = run_ws.update(app, |ws, _| {
                        let text = ws.term.input.take();
                        if !text.trim().is_empty() {
                            ws.run_terminal(text);
                        }
                    });
                })),
        )
    }
}

pub const TERMINAL_H: f32 = 230.0;
