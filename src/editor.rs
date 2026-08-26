use gpui::{
    actions, div, prelude::*, px, uniform_list, AnyElement, App, ClipboardItem, Context,
    ElementId, Entity, FocusHandle, Focusable, InteractiveElement, KeyBinding, KeyDownEvent,
    MouseButton, MouseDownEvent, Render, ScrollStrategy, SharedString, TextAlign, TextRun,
    UniformListScrollHandle, Window,
};
use std::sync::Arc;

use crate::ai::AiHub;
use crate::completion::{collect_candidates, word_prefix_at};
use crate::document::Document;
use crate::syntax::{self, TokenKind};
use crate::theme::{
    hex, EDITOR_FONT_SIZE, EDITOR_ORIGIN_X, GUTTER_WIDTH, LINE_HEIGHT, MONO_FONT, Theme,
};

actions!(
    editor,
    [
        Backspace,
        DeleteForward,
        Newline,
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        PageUp,
        PageDown,
        WordLeft,
        WordRight,
        Undo,
        Redo,
        CopyLine,
        CutLine,
        Paste,
        AcceptGhost,
        DismissGhost,
        AiSuggest
    ]
);

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", DeleteForward, None),
        KeyBinding::new("enter", Newline, None),
        KeyBinding::new("left", Left, None),
        KeyBinding::new("right", Right, None),
        KeyBinding::new("up", Up, None),
        KeyBinding::new("down", Down, None),
        KeyBinding::new("home", Home, None),
        KeyBinding::new("end", End, None),
        KeyBinding::new("pageup", PageUp, None),
        KeyBinding::new("pagedown", PageDown, None),
        KeyBinding::new("ctrl-left", WordLeft, None),
        KeyBinding::new("ctrl-right", WordRight, None),
        KeyBinding::new("cmd-left", WordLeft, None),
        KeyBinding::new("cmd-right", WordRight, None),
        KeyBinding::new("tab", AcceptGhost, None),
        KeyBinding::new("escape", DismissGhost, None),
        KeyBinding::new("alt-\\", AiSuggest, None),
        KeyBinding::new("ctrl-z", Undo, None),
        KeyBinding::new("ctrl-y", Redo, None),
        KeyBinding::new("cmd-z", Undo, None),
        KeyBinding::new("shift-cmd-z", Redo, None),
        KeyBinding::new("ctrl-cmd-left", WordLeft, None),
        KeyBinding::new("alt-left", WordLeft, None),
        KeyBinding::new("alt-right", WordRight, None),
    ]);
}

pub struct EditorPane {
    pub doc: Entity<Document>,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    /// AI hub for inline suggestions (None in tests / headless use).
    pub hub: Option<Arc<AiHub>>,
    /// Channel the AI suggestion reply should arrive on.
    pub ai_tx: Option<futures::channel::mpsc::UnboundedSender<crate::ai::UiEvent>>,
    /// Local word-completion ghost (computed each frame).
    ghost_local: Option<String>,
    /// AI-provided ghost text; takes precedence until dismissed/typed over.
    ghost_ai: Option<String>,
}

impl EditorPane {
    pub fn new(doc: Entity<Document>, cx: &mut Context<Self>) -> Self {
        EditorPane {
            doc,
            focus_handle: cx.focus_handle(),
            scroll_handle: UniformListScrollHandle::default(),
            hub: None,
            ai_tx: None,
            ghost_local: None,
            ghost_ai: None,
        }
    }

    pub fn with_ai(
        mut self,
        hub: Arc<AiHub>,
        tx: futures::channel::mpsc::UnboundedSender<crate::ai::UiEvent>,
    ) -> Self {
        self.hub = Some(hub);
        self.ai_tx = Some(tx);
        self
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    /// Called by the workspace pump when an AI inline suggestion arrives.
    pub fn set_ai_ghost(&mut self, text: String) {
        if !text.is_empty() {
            self.ghost_ai = Some(text);
        }
    }

    fn clear_ghosts(&mut self) {
        self.ghost_local = None;
        self.ghost_ai = None;
    }

    /// Central mutation wrapper: every edit clears stale suggestions.
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Document)) {
        self.doc.update(cx, |doc, _| f(doc));
        self.clear_ghosts();
    }

    fn sync_scroll(&mut self, cx: &Context<Self>) {
        let row = self.doc.read(cx).cursor.row;
        self.scroll_handle.scroll_to_item(row, ScrollStrategy::Center);
    }

    /// Pop the ghost that Tab should accept (AI first), clearing both kinds.
    fn take_active_ghost(&mut self) -> Option<String> {
        let g = self.ghost_ai.take().or_else(|| self.ghost_local.take());
        g
    }

    /// Fire an async AI inline-completion request through the hub.
    fn request_ai_ghost(&mut self, cx: &Context<Self>) {
        let (Some(hub), Some(tx)) = (self.hub.as_ref().cloned(), self.ai_tx.clone()) else {
            return;
        };
        let doc = self.doc.read(cx);
        let cursor = doc.cursor;
        let line = doc.line_text(cursor.row);
        let (prefix, start_col) = word_prefix_at(&line, cursor.col);
        let before_start: usize = line
            .char_indices()
            .nth(start_col)
            .map(|(b, _)| b)
            .unwrap_or(0);
        let context_prefix: String = {
            let mut ctx = String::new();
            let from_row = cursor.row.saturating_sub(20);
            for r in from_row..cursor.row {
                ctx.push_str(&doc.line_text(r));
                ctx.push('\n');
            }
            ctx.push_str(&line[..before_start]);
            ctx
        };
        let suffix: String = line[before_start + prefix.len()..].to_string();
        hub.request_inline_suggestion(&context_prefix, &suffix, tx);
    }
}

impl Focusable for EditorPane {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let cursor = self.doc.read(cx).cursor;
        let line_count = self.doc.read(cx).line_count();
        let list_id = ElementId::NamedInteger(
            "editor-lines".into(),
            self.doc.entity_id().as_u64(),
        );

        // ---- inline suggestions ------------------------------------------
        // AI ghosts take precedence; otherwise compute a local word ghost.
        if self.ghost_ai.is_none() && focused {
            let doc = self.doc.read(cx);
            let line = doc.line_text(cursor.row);
            let (prefix, _) = word_prefix_at(&line, cursor.col);
            let window_lines: Vec<String> = {
                let half = 200usize;
                let s = cursor.row.saturating_sub(half);
                let e = (cursor.row + half).min(doc.line_count());
                (s..e).map(|r| doc.line_text(r)).collect()
            };
            self.ghost_local =
                collect_candidates(&window_lines, cursor.row, &prefix)
                    .first()
                    .map(|word| word[prefix.chars().count()..].to_string());
        } else if !focused {
            self.clear_ghosts();
        }
        let ghost_for_render: Option<String> =
            self.ghost_ai.clone().or_else(|| self.ghost_local.clone());

        let doc_for_list = self.doc.clone();
        let focus_for_click = self.focus_handle.clone();

        div()
            .id("editor-pane")
            .size_full()
            .flex()
            .flex_col()
            .bg(hex(Theme::EDITOR_BG))
            .key_context("Editor")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                this.edit(cx, |doc| doc.backspace());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DeleteForward, _, cx| {
                this.edit(cx, |doc| doc.delete_forward());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Newline, _, cx| {
                this.edit(cx, |doc| {
                    doc.split_line();
                    let prev_row = doc.cursor.row.saturating_sub(1);
                    let indent: String = doc
                        .line_text(prev_row)
                        .chars()
                        .take_while(|c: &char| c.is_whitespace())
                        .collect();
                    if !indent.is_empty() {
                        doc.insert_str_no_snapshot(&indent);
                    }
                });
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Left, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_left());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Right, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_right());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Up, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_up());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Down, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_down());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Home, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_home());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &End, _, cx| {
                this.doc.update(cx, |doc, _| doc.move_end());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| {
                this.doc.update(cx, |doc, _| doc.page_up(30));
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| {
                this.doc.update(cx, |doc, _| doc.page_down(30));
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| {
                this.doc.update(cx, |doc, _| doc.word_left());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| {
                this.doc.update(cx, |doc, _| doc.word_right());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AcceptGhost, _, cx| {
                // Tab accepts the active ghost suggestion; otherwise indents.
                if let Some(ghost) = this.take_active_ghost() {
                    this.edit(cx, |doc| doc.insert_str(&ghost));
                } else {
                    this.edit(cx, |doc| doc.insert_str("    "));
                }
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DismissGhost, _, cx| {
                this.clear_ghosts();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &AiSuggest, _, cx| {
                this.request_ai_ghost(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.edit(cx, |doc| doc.undo());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.edit(cx, |doc| doc.redo());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CopyLine, _, cx| {
                let text = this.doc.read(cx).line_text(this.doc.read(cx).cursor.row);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }))
            .on_action(cx.listener(|this, _: &CutLine, _, cx| {
                let text = this.doc.read(cx).line_text(this.doc.read(cx).cursor.row);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                this.edit(cx, |doc| doc.delete_line());
                this.sync_scroll(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Paste, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    this.edit(cx, |doc| doc.insert_str(&text));
                    this.sync_scroll(cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        uniform_list(list_id, line_count, move |visible_range, _window, cx| {
                            render_lines(
                                &doc_for_list,
                                visible_range,
                                cursor,
                                focused,
                                focus_for_click.clone(),
                                ghost_for_render.clone(),
                                cx,
                            )
                        })
                        .track_scroll(self.scroll_handle.clone())
                        .flex_1()
                        .h_full()
                        .font_family(MONO_FONT)
                        .text_size(px(EDITOR_FONT_SIZE))
                        .line_height(px(LINE_HEIGHT)),
                    ),
            )
    }
}

impl EditorPane {
    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;

        if is_named_key(&keystroke.key) {
            return;
        }
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }

        if let Some(ch) = keystroke.key_char.as_ref().and_then(|text| text.chars().next()) {
            if ch.is_control() {
                return;
            }
            self.edit(cx, |doc| doc.insert_char(ch));
            self.scroll_handle
                .scroll_to_item(self.doc.read(cx).cursor.row, ScrollStrategy::Center);
            cx.stop_propagation();
            cx.notify();
        }
    }
}

fn is_named_key(key: &str) -> bool {
    matches!(
        key,
        "enter" | "backspace" | "delete" | "tab" | "escape" | "left" | "right" | "up"
            | "down" | "home" | "end" | "pageup" | "pagedown" | "shift" | "control" | "alt"
            | "super" | "platform" | "fn" | "capslock" | "numlock" | "scrolllock" | "dead"
            | "altgr" | "menu" | "printscreen" | "pause" | "insert"
            | "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11"
            | "f12" | "f13" | "f14" | "f15" | "f16" | "f17" | "f18" | "f19" | "f20"
    )
}

enum Piece {
    Text(String, TokenKind),
    Caret,
    Ghost(String),
}

fn render_lines(
    doc: &Entity<Document>,
    visible_range: std::ops::Range<usize>,
    cursor: crate::document::Cursor,
    focused: bool,
    focus_handle: FocusHandle,
    ghost: Option<String>,
    cx: &App,
) -> Vec<AnyElement> {
    visible_range
        .map(|row| {
            let document = doc.read(cx);
            let text = document.line_text(row);
            let tokens = syntax::tokenize(&text);

            // Build styled pieces from tokens.
            let mut pieces: Vec<Piece> = Vec::new();
            {
                let mut plain_from = 0usize;
                for token in &tokens {
                    if token.start > plain_from {
                        pieces.push(Piece::Text(
                            text[plain_from..token.start].to_string(),
                            TokenKind::Plain,
                        ));
                    }
                    pieces.push(Piece::Text(text[token.start..token.end].to_string(), token.kind));
                    plain_from = token.end;
                }
                if plain_from < text.len() {
                    pieces.push(Piece::Text(text[plain_from..].to_string(), TokenKind::Plain));
                }
            }

            // Insert a caret marker at the cursor column on the active row.
            let caret_in_row = row == cursor.row && focused;
            if caret_in_row {
                let byte_col = char_col_to_byte(&text, cursor.col);
                let mut with_caret: Vec<Piece> = Vec::new();
                let mut offset = 0usize;
                let mut placed = false;
                for piece in std::mem::take(&mut pieces) {
                    match piece {
                        Piece::Text(seg, kind) => {
                            let seg_end = offset + seg.len();
                            if !placed && byte_col >= offset && byte_col <= seg_end {
                                let split_at = byte_col - offset;
                                let (before, after) = seg.split_at(split_at);
                                if !before.is_empty() {
                                    with_caret.push(Piece::Text(before.to_string(), kind));
                                }
                                with_caret.push(Piece::Caret);
                                if !after.is_empty() {
                                    with_caret.push(Piece::Text(after.to_string(), kind));
                                }
                                placed = true;
                            } else {
                                with_caret.push(Piece::Text(seg, kind));
                            }
                            offset = seg_end;
                        }
                        other => with_caret.push(other),
                    }
                }
                if !placed {
                    with_caret.push(Piece::Caret);
                }
                // inline ghost suggestion right after the caret
                if let Some(g) = ghost.clone() {
                    if !g.is_empty() {
                        with_caret.push(Piece::Ghost(g));
                    }
                }
                pieces = with_caret;
            }

            let mut row_el = div()
                .id(ElementId::named_usize("line", row))
                .flex()
                .flex_row()
                .flex_none()
                .h(px(LINE_HEIGHT))
                .overflow_hidden()
                .bg(if row == cursor.row {
                    hex(Theme::CURRENT_LINE)
                } else {
                    gpui::transparent_black()
                })
                .on_mouse_down(MouseButton::Left, {
                    let doc = doc.clone();
                    let focus_handle = focus_handle.clone();
                    let line_text = text.clone();
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let column = column_at_x(&line_text, event.position.x, window);
                        doc.update(cx, |doc, cx| {
                            doc.set_cursor(row, column);
                            cx.notify();
                        });
                        window.focus(&focus_handle);
                    }
                });

            row_el = row_el.child(
                div()
                    .w(px(GUTTER_WIDTH))
                    .flex_none()
                    .text_align(TextAlign::Right)
                    .pr_2()
                    .text_size(px(12.0))
                    .line_height(px(LINE_HEIGHT))
                    .text_color(if caret_in_row {
                        hex(Theme::TEXT_DIM)
                    } else {
                        hex(0x4b5263)
                    })
                    .child((row + 1).to_string()),
            );

            for piece in pieces {
                match piece {
                    Piece::Text(segment, kind) => {
                        if segment.is_empty() {
                            continue;
                        }
                        row_el = row_el.child(
                            div()
                                .whitespace_nowrap()
                                .text_color(kind.rgb())
                                .child(SharedString::from(segment)),
                        );
                    }
                    Piece::Caret => {
                        row_el = row_el.child(
                            div()
                                .w(px(2.0))
                                .h(px(LINE_HEIGHT))
                                .flex_none()
                                .bg(hex(Theme::CURSOR)),
                        );
                    }
                    Piece::Ghost(g) => {
                        row_el = row_el.child(
                            div()
                                .whitespace_nowrap()
                                .text_color(hex(0x5d6573))
                                .child(SharedString::from(g)),
                        );
                    }
                }
            }

            row_el.into_any_element()
        })
        .collect()
}

fn char_col_to_byte(text: &str, char_col: usize) -> usize {
    text.char_indices()
        .nth(char_col)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

fn column_at_x(line_text: &str, x_abs: gpui::Pixels, window: &Window) -> usize {
    let run_font = window.text_style().font();
    let run = TextRun {
        len: line_text.len(),
        font: run_font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(
        SharedString::from(line_text.to_string()),
        px(EDITOR_FONT_SIZE),
        &[run],
        None,
    );
    let local_x = (x_abs - px(EDITOR_ORIGIN_X + GUTTER_WIDTH)).max(px(0.0));
    shaped.closest_index_for_x(local_x)
}
