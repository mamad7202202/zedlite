//! Modular bottom/right panels. To add a new panel:
//!
//! 1. create `panels/<name>.rs` with a `XxxPanel { .. }` state struct and a
//!    `render(&self, ...) -> gpui::Div` method,
//! 2. add a field + toggle on `Workspace`,
//! 3. append a toolbar button that flips the toggle.
//!
//! Panels never own global state; they read/write through `Arc<AiHub>` and
//! receive async results via the single `UiEvent` pump in `workspace.rs`.

pub mod chat;
pub mod memoryview;
pub mod terminal;

use gpui::{div, prelude::*, px, App, FocusHandle, KeyDownEvent, Stateful};

use crate::theme::{hex, Theme};

/// A tiny single-line text input used by chat & terminal panels.
pub struct MiniInput {
    pub focus: FocusHandle,
    pub text: String,
}

impl MiniInput {
    pub fn new(cx: &mut App) -> Self {
        MiniInput { focus: cx.focus_handle(), text: String::new() }
    }

    /// Handle a key event. Returns true when Enter was pressed (submit).
    pub fn on_key(&mut self, event: &KeyDownEvent) -> bool {
        let ks = &event.keystroke;
        match ks.key.as_str() {
            "enter" => return true,
            "backspace" => {
                self.text.pop();
                return false;
            }
            "escape" => {
                self.text.clear();
                return false;
            }
            _ => {}
        }
        if ks.modifiers.control || ks.modifiers.alt || ks.modifiers.platform {
            // let editor/global shortcuts pass through untouched
            return false;
        }
        if let Some(ch) = ks.key_char.as_ref().and_then(|t| t.chars().next()) {
            if ch == ' ' || !ch.is_control() {
                self.text.push(ch);
            }
        }
        false
    }

    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.text)
    }
}

/// Small pill button used across panels.
pub fn pill(id: &'static str, label: &'static str, active: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py(px(2.0))
        .rounded_sm()
        .text_size(px(11.0))
        .cursor_pointer()
        .text_color(if active {
            hex(Theme::BG)
        } else {
            hex(Theme::TEXT_DIM)
        })
        .bg(if active { hex(Theme::ACCENT) } else { hex(0x2a2f38) })
        .hover(|s| s.bg(hex(0x353b45)))
        .child(label)
}

