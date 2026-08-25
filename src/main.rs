pub mod document;
pub mod editor;
pub mod syntax;
pub mod theme;
pub mod workspace;

use std::path::PathBuf;

use gpui::{point, prelude::*, px, size, App, Application, Bounds, WindowBounds, WindowOptions};

fn main() {
    let startup_path = std::env::args().nth(1).map(PathBuf::from);

    Application::new().run(|cx: &mut App| {
        editor::register_keybindings(cx);
        workspace::register_keybindings(cx);

        let display_size = cx.displays().first().map(|display| display.bounds().size);
        let window_size = size(px(1100.), px(720.));
        let bounds = match display_size {
            Some(screen) => Bounds {
                origin: point(
                    ((screen.width - window_size.width) / 2.0).max(px(0.)),
                    ((screen.height - window_size.height) / 2.0).max(px(0.)),
                ),
                size: window_size,
            },
            None => Bounds {
                origin: point(px(100.), px(100.)),
                size: window_size,
            },
        };

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("ZedLite — a tiny Zed-inspired editor".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let workspace = cx.new(|cx| workspace::Workspace::new(startup_path, cx));
            workspace.update(cx, |ws, cx| ws.focus_active_pane(window, cx));
            workspace
        })
        .unwrap();

        cx.activate(true);
    });
}
