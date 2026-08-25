# ZedLite

ادیتور کد هوشمند الهام‌گرفته از Zed — رابط کاربری با GPUI و موتور هوش مصنوعی پورت‌شده از هسته‌ی dragon-agent.

A Zed-inspired AI code editor. UI in pure Rust with GPUI; the AI engine is the
dragon-agent core (BYOK providers, agent loop, hybrid memory) ported in-tree.

## AI capabilities (from dragon-agent core)

- BYOK providers - any OpenAI-compatible endpoint (OpenAI, OpenRouter, Groq,
  DeepSeek, Ollama, LM Studio...) plus native Anthropic. Streaming SSE.
- Agent loop - chat / plan / agent modes with tool calling:
  read_file, write_file, edit_file, delete_file, list_files, grep,
  run_shell, fetch_url, web_search, task_write and memory tools.
- Approval gate - gated tools show Allow/Deny cards unless auto-approved
  via patterns like "write_file" or "run_shell:cargo".
- Hybrid memory - semantic fact shards (cosine recall, importance, recency),
  procedural MEMORY.md injected into every prompt, episodic JSONL session
  logs, automatic history compaction, and the Memory-Graph engine (typed
  bullets, confidence decay, auto-forget).
- Thinking effort - off/low/medium/high passed to capable models.

## Editor features

- File explorer tree, tabs, syntax highlighting, undo/redo, word jumps,
  auto-indent, line copy/cut/paste, click-to-place caret via text shaping.
- Inline suggestions: instant local word-completion ghost + on-demand AI
  completion (Alt+\, Tab accepts, Esc dismisses).
- Integrated terminal panel streaming output; Stop kills the process.
- Chat / Memory panels docked right; toggled from the toolbar or
  Alt+A / Alt+M / Alt+T; settings live-edit at Alt+C.

## Proxy support

Route traffic user -> proxy -> service, per service:

    [settings]
    proxy_url = "http://127.0.0.1:8080"   # "" disables proxying entirely
    proxy_llm = false                     # model API calls
    proxy_web_search = true               # web_search tool
    proxy_fetch = false                   # fetch_url tool

Open Settings in the toolbar (or Alt+C), edit, press Ctrl+S in that tab -
the config hot-reloads and the agent rebuilds.

## Getting started

1. Toolbar > Settings > fill [[providers]] (name, base_url, api_key, models)
   and set default_model.
2. Open a folder (Ctrl+Shift+O) - tools now resolve paths against it.
3. Ask in the chat panel. Switch Chat/Plan/Agent modes in its header.

Config & data locations: config-dir/zedlite/config.toml and
data-dir/zedlite/{memory,sessions,tasks}.

## Keyboard map

| Keys | Action |
| --- | --- |
| Ctrl+O / Ctrl+Shift+O / Ctrl+N | open files / folder / new buffer |
| Ctrl+S / Ctrl+Shift+S | save / save as |
| Ctrl+W, Ctrl+PgDn/PgUp | close tab, cycle tabs |
| Alt+A Alt+T Alt+M Alt+C | toggle Chat / Terminal / Memory / Settings |
| Alt+N / Alt+Q | new AI session / stop generation |
| Tab / Esc | accept / dismiss inline suggestion |
| Alt+\ | request AI inline completion |
| arrows, Home/End, PgUp/PgDn, Ctrl+arrows | caret motion |

## Adding a panel

Panels are modular by design: add src/panels/<name>.rs with a state struct +
an impl Workspace render_x_panel method, register a field/toggle on Workspace
and one toolbar pill. Async results flow through the single UiEvent channel
in src/ai/mod.rs.

## Build

    cargo run --release

Requires Rust 1.85+ (gpui uses edition 2024). Linux packages:

    sudo apt install cmake pkg-config libfontconfig1-dev libxcb1-dev \
      libxkbcommon-dev libxkbcommon-x11-dev

Pass a path to open it at launch: cargo run --release -- path/to/project

Core credits: AI engine ported from mamad7202202/dragon-agent (MIT).
