pub mod ai;
pub mod completion;
pub mod document;
pub mod editor;
pub mod panels;
pub mod syntax;
pub mod theme;
pub mod workspace;

use std::path::PathBuf;

use futures::StreamExt;
use gpui::{point, prelude::*, px, size, App, Application, Bounds, WindowBounds, WindowOptions};

fn main() {
    let startup_path = std::env::args().nth(1).map(PathBuf::from);

    Application::new().run(|cx: &mut App| {
        editor::register_keybindings(cx);
        workspace::register_keybindings(cx);

        // ---- AI hub + event channel ------------------------------------
        let hub = match ai::AiHub::new() {
            Ok(h) => h,
            Err(e) => {
                eprintln!("AI init failed: {e:#}");
                cx.quit();
                return;
            }
        };
        let (ai_tx, mut ai_rx) = futures::channel::mpsc::unbounded::<ai::UiEvent>();

        let display_size = cx.displays().first().map(|display| display.bounds().size);
        let window_size = size(px(1280.), px(800.));
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
                title: Some("ZedLite — Zed-inspired AI code editor".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let ws_entity = cx.new(|cx| workspace::Workspace::new(startup_path, hub.clone(), ai_tx.clone(), cx));
        if let Some(root_dir) = std::env::current_dir().ok() {
            hub.set_root(root_dir);
        }
        let workspace_entity = {
            let ws_clone = ws_entity.clone();
            cx.open_window(options, move |window, cx| {
                ws_clone.update(cx, |ws, cx| ws.focus_active_pane(window, cx));
                ws_clone
            })
            .unwrap()
        };

        // ---- pump: tokio side -> GPUI side ------------------------------
        let weak_ws = ws_entity.downgrade();
        cx.spawn(async move |cx| {
            while let Some(ev) = ai_rx.next().await {
                if weak_ws
                    .update(cx, |ws, cx| ws.handle_ai_event(ev, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        cx.activate(true);
    });
}
