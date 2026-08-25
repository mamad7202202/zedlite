# ZedLite

یک ادیتور کد کوچک الهام‌گرفته از [Zed](https://github.com/zed-industries/zed) که کاملاً با
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) — فریم‌ورک UI گپ‌شتاب‌دهی‌شده تیم Zed — نوشته شده.

A tiny Zed-inspired code editor written entirely in Rust with [GPUI](https://gpui.rs), the GPU-accelerated UI framework from the Zed team.

## Features

- 📁 **File explorer** — open a folder (Ctrl+Shift+O), browse and expand directories
- 📄 **Tabs** — multiple open files, click to switch
- ✍️ **Real text editing** — insert, delete, newline with auto-indent, word jumps
- ↩️ **Undo / redo** — snapshot history (Ctrl+Z / Ctrl+Y)
- 🎨 **Syntax highlighting** — lightweight tokenizer for Rust, JS/TS, Python, Go, C/C++, …
- 💾 **Save** — Ctrl+S to save, Save As dialog for untitled buffers
- 📊 **Status bar** — cursor position, line count, language, modified indicator
- 🖱️ Click-to-place caret using real text shaping metrics

## Keyboard shortcuts

| Keys | Action |
| --- | --- |
| `Ctrl+O` / `Cmd+O` | Open file(s) |
| `Ctrl+Shift+O` | Open folder |
| `Ctrl+N` | New buffer |
| `Ctrl+S` | Save |
| `Ctrl+Shift+S` | Save as |
| `Ctrl+W` | Close tab |
| `Ctrl+PageDown` / `Ctrl+PageUp` | Next / previous tab |
| Arrows, Home, End, PageUp/Down | Move caret |
| `Ctrl+←` / `Ctrl+→` | Jump by word |
| `Tab` | Indent |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut line · paste |

## Build

```sh
cargo run --release
```

Requires Rust 1.85+ (the `gpui` crate uses edition 2024).

Linux system packages:

```sh
sudo apt install cmake pkg-config libfontconfig1-dev libxcb1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev
```

You can also pass a path: `cargo run --release -- path/to/folder`

## CI

GitHub Actions builds release binaries for Windows, macOS and Linux on every push and uploads them as artifacts.
