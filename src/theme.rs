use gpui::rgb;

pub struct Theme;

impl Theme {
    pub const BG: u32 = 0x1e2127;
    pub const PANEL: u32 = 0x23272e;
    pub const PANEL_BORDER: u32 = 0x2f343d;
    pub const EDITOR_BG: u32 = 0x1a1c21;
    pub const CURRENT_LINE: u32 = 0x21242b;
    pub const TEXT: u32 = 0xd7dae0;
    pub const TEXT_DIM: u32 = 0x7f848e;
    pub const ACCENT: u32 = 0x61afef;
    pub const GREEN: u32 = 0x98c379;
    pub const RED: u32 = 0xe06c75;
    pub const YELLOW: u32 = 0xe5c07b;
    pub const CURSOR: u32 = 0x61afef;
    pub const TAB_ACTIVE: u32 = 0x1a1c21;
    pub const TAB_INACTIVE: u32 = 0x23272e;
}

pub fn hex(value: u32) -> gpui::Hsla {
    rgb(value).into()
}

pub const EDITOR_FONT_SIZE: f32 = 14.0;
pub const LINE_HEIGHT: f32 = 20.0;

/// Fixed layout metrics used for mapping mouse clicks to text columns.
pub const SIDEBAR_WIDTH: f32 = 240.0;
pub const EDITOR_ORIGIN_X: f32 = SIDEBAR_WIDTH + 1.0;
pub const GUTTER_WIDTH: f32 = 44.0;

pub const MONO_FONT: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else if cfg!(windows) {
    "Consolas"
} else {
    "DejaVu Sans Mono"
};
