# Agent Dashboard architecture

This document describes the current Rust/Tauri application. Historical implementation notes
are kept under `docs/progress/`; the Python proof of concept is archived separately under
`prototypes/python-dashboard/`.

## Product boundary

Agent Dashboard is a local desktop session manager for terminal-based AI agents. Each card maps
to a live PTY session or an externally reported task and displays a stable status plus a concise
summary. The model is informational only: commands, approvals, merges, and other control actions
remain with the user.

The desktop HTTP server binds only to `127.0.0.1:8787`. Remote task creation and remote terminal
input are intentionally not exposed.

## Repository layers

```text
agent-dashboard/
├─ frontend/                       # single HTML/CSS/JS front-end source
│  └─ index.html
├─ src-tauri/                      # production Rust/Tauri desktop application
│  ├─ src/
│  │  ├─ agent_support.rs          # agent types, state machine, localization
│  │  ├─ api.rs                    # loopback HTTP API and embedded front end
│  │  ├─ llm.rs                    # DeepSeek/local llama.cpp summaries
│  │  ├─ pty.rs                    # portable PTY abstraction
│  │  ├─ session.rs                # terminal lifecycle and stable summary pipeline
│  │  └─ store.rs                  # externally reported task storage
│  ├─ capabilities/ · permissions/ # Tauri ACL
│  └─ tauri.conf.json
├─ integrations/                   # production OpenCode/Claude Code/Codex adapters
├─ prototypes/python-dashboard/    # archived v0 Python proof of concept
├─ docs/
│  ├─ architecture.md
│  └─ progress/                    # chronological development records
├─ assets/                         # repository images and artwork
├─ models/                         # optional local model weights; ignored by Git
└─ runtime/                        # logs and local runtime data; ignored by Git
```

`frontend/index.html` is the only front-end source. Both Tauri `frontendDist` and Rust
`include_str!` point to this file, eliminating the former source/dist synchronization problem.

## Runtime components

1. `lib::run` loads `.env`, creates the shared `SessionManager` and `Store`, then starts axum on a
   dedicated Tokio runtime.
2. Tauri opens `http://127.0.0.1:8787/` only after this process has successfully bound the port.
3. `api.rs` serves the compile-time embedded `frontend/index.html` and the reporting APIs.
4. The front end creates xterm.js terminals; Tauri commands connect them to PTYs managed by
   `session.rs` and `pty.rs`.
5. OpenCode, Claude Code, and Codex adapters may send structured lifecycle events. Screen parsing
   remains a fallback when an adapter is absent or temporarily silent.

## Stable summary pipeline

The front end reads the current xterm screen rather than scrollback, normalizes known dynamic TUI
elements, and classifies only the bottom control area. A semantic key contains agent kind, agent
state, selected language, and normalized screen text.

The Rust layer provides the final stability guarantees:

- one active summary worker per terminal;
- pending-state collapse instead of an unbounded inference queue;
- per-session semantic result cache;
- stale-result rejection when a newer semantic key or structured event arrives;
- structured lifecycle state overrides LLM state;
- language-specific cache keys prevent Chinese and English results from mixing.

DeepSeek is used when `DEEPSEEK_API_KEY` exists. Otherwise the application tries the local model
at `LOCAL_LLM_MODEL_PATH`, defaulting to `models/Qwen3.5-2B-Q4_K_M.gguf`.

## Session names

Card title precedence is:

1. a name manually entered in the dashboard;
2. a native OpenCode/Claude Code/Codex session title;
3. the command name fallback.

Once manually renamed, later OSC title or structured metadata updates are recorded but cannot
overwrite the card title.

## APIs

Generic reporting:

- `GET /api/status`
- `POST /api/report`
- `GET /api/remove?id=...`

Structured agent reporting:

- `POST /api/agent-event`

The structured body accepts the dashboard session ID, agent kind, lifecycle event, native session
ID/title, legacy `summary`, and optional `summary_zh_cn` / `summary_en` fields.

PTY creation, input, resize, detach, close, naming, metadata, language selection, and screen
summarization use local Tauri commands governed by `src-tauri/permissions/app.toml`.

## Local data and secrets

- `.env` contains local credentials and is ignored by Git.
- model weights live under `models/` and are ignored by Git.
- diagnostics default to `runtime/logs/` and are ignored by Git; override with
  `AGENT_DASHBOARD_LOG_DIR`.
- external task state is persisted beside the executable by the Rust store.
- the application does not automatically approve agent actions or expose PTY control over HTTP.
