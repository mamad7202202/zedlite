//! Memory panel: visibility + control over the hybrid memory system.

use gpui::{div, prelude::*, px, MouseButton};
use std::sync::Arc;

use crate::ai::AiHub;
use crate::panels::pill;
use crate::theme::{hex, MONO_FONT, Theme};
use crate::workspace::Workspace;

pub struct MemoryPanel;

impl MemoryPanel {
    pub fn new() -> Self {
        MemoryPanel
    }
}

impl Workspace {
    pub fn render_memory_panel(
        &mut self,
        hub: Arc<AiHub>,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let engine_graph = hub.cfg.lock().unwrap().settings.graph_memory();
        let facts_total = hub.memory.lock().unwrap().all().len();
        let session_id = hub.current_session_id();
        let session_facts = hub
            .memory
            .lock()
            .unwrap()
            .all()
            .iter()
            .filter(|f| f.session.is_some())
            .count();
        let (global_sections, session_sections) = {
            let g = hub.graph.lock().unwrap();
            (
                g.global.len(),
                session_id
                    .as_deref()
                    .and_then(|sid| g.sessions.get(sid))
                    .map(|v| v.len())
                    .unwrap_or(0),
            )
        };

        let graph_preview = hub.graph.lock().unwrap().read_text(session_id.as_deref());
        let mem_path = crate::ai::memory::procedural_path();
        let mem_path_label = mem_path.display().to_string();

        let open_ws = cx.entity();
        let clear_ws = cx.entity();
        let clear_hub = hub.clone();

        div()
            .id("memory-panel")
            .w(px(AI_PANEL_W))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(hex(Theme::PANEL))
            .border_l_1()
            .border_color(hex(Theme::PANEL_BORDER))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(px(30.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(hex(Theme::PANEL_BORDER))
                    .text_size(px(11.0))
                    .text_color(hex(Theme::TEXT_DIM))
                    .child("MEMORY"),
            )
            .child(
                div()
                    .id("mem-scroll")
                    .flex_1()
                    .overflow_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(11.5))
                    // stats grid
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(vec![
                                stat("facts", &facts_total.to_string()),
                                stat("session facts", &session_facts.to_string()),
                                stat("graph sections", &(global_sections + session_sections).to_string()),
                                stat("engine", if engine_graph { "graph" } else { "hybrid" }),
                            ]),
                    )
                    // actions
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(pill("open-mem", "Open MEMORY.md", false).on_mouse_down(
                                MouseButton::Left,
                                move |_, _, app| {
                                    let path = crate::ai::memory::ensure_procedural_file()
                                        .ok()
                                        .unwrap_or_else(mem_path_clone_of);
                                    let _ = open_ws.update(app, |ws, cx| {
                                        ws.open_file_in(&path, cx);
                                    });
                                },
                            ))
                            .child(pill("clear-session", "Clear session memory", false).on_mouse_down(
                                MouseButton::Left,
                                move |_, _, app| {
                                    let sid = clear_hub.current_session_id();
                                    if let Some(sid) = sid {
                                        let _ = clear_hub
                                            .memory
                                            .lock()
                                            .unwrap()
                                            .clear_session(&sid);
                                        let _ = clear_hub.memory.lock().unwrap().save();
                                        clear_hub.graph.lock().unwrap().drop_session(&sid);
                                    }
                                    let _ = clear_ws.update(app, |_, cx| cx.notify());
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_family(MONO_FONT)
                            .text_color(hex(Theme::TEXT_DIM))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(mem_path_label)),
                    )
                    .child(div().border_t_1().border_color(hex(Theme::PANEL_BORDER)))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(hex(Theme::TEXT_DIM))
                            .child("LIVE GRAPH PREVIEW"),
                    )
                    .child(
                        div()
                            .p_2()
                            .rounded_sm()
                            .bg(hex(0x1a1d23))
                            .font_family(MONO_FONT)
                            .text_size(px(10.5))
                            .line_height(px(15.0))
                            .text_color(hex(0xaab2bf))
                            .child(SharedString::from(graph_preview)),
                     ),
             )
    }
}

fn stat(label: &str, value: &str) -> gpui::Div {
    div()
        .flex_1()
        .p_2()
        .rounded_sm()
        .bg(hex(0x1f232b))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(hex(Theme::TEXT))
                .child(SharedString::from(value)),
        )
        .child(
            div()
                .text_size(px(9.5))
                .text_color(hex(Theme::TEXT_DIM))
                .child(SharedString::from(label)),
        )
}

fn mem_path_clone_of() -> std::path::PathBuf {
    crate::ai::memory::procedural_path()
}

pub const AI_PANEL_W: f32 = 380.0;
