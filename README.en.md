# Agent Dashboard

[简体中文](README.md) | English

Agent Dashboard is a Tauri 2 desktop application for running and monitoring terminal-based AI agents. It provides persistent xterm.js sessions, stable AI-generated status summaries, native session titles, and dedicated lifecycle integration for OpenCode, Claude Code, and Codex.

## Language support

Use the language selector in the top-right corner to switch between Simplified Chinese and English. The selection is saved locally and controls:

- all dashboard labels, filters, dialogs, timestamps, empty states, and terminal notices;
- fixed lifecycle summaries generated from OpenCode, Claude Code, and Codex events;
- the output language requested from the DeepSeek or local summary model;
- demo data shown when no real task source is available.

Agent-provided task names, terminal output, and summaries sent through the generic reporting API are treated as user data and remain in their original language.

## Run from source

Requirements:

- Rust and Cargo
- the platform prerequisites required by Tauri 2
- optionally, `DEEPSEEK_API_KEY` in `.env` for external summaries

```powershell
cargo run --manifest-path .\src-tauri\Cargo.toml
```

The desktop window loads its embedded dashboard through `http://127.0.0.1:8787/`. The local API only listens on loopback.

## Build and test

```powershell
cargo test --manifest-path .\src-tauri\Cargo.toml --lib
cargo build --manifest-path .\src-tauri\Cargo.toml
```

The debug executable is written to `src-tauri/target/debug/agent-dashboard.exe`.

## Agent integrations

The application injects these variables into terminal sessions:

- `AGENT_DASHBOARD_SESSION_ID`
- `AGENT_DASHBOARD_EVENT_URL`
- `AGENT_DASHBOARD_HOOK`

Integration templates are available under `integrations/examples/`:

- copy `opencode-agent-dashboard.js` into a project `.opencode/plugins/` directory or the global OpenCode plugin directory;
- merge `claude-code-hooks.json` into Claude Code settings;
- merge `codex-hooks.json` into Codex hooks settings;
- optionally merge `claude-code-statusline.json` to forward Claude Code's native session name.

The shared `integrations/agent_dashboard_hook.py` adapter reports bilingual structured lifecycle summaries. Reporting failures are silent and never block or change agent behavior.

## Local reporting API

Generic tools can report their own status to `POST /api/report`:

```json
{
  "task_id": "backend-tests",
  "node": "local",
  "cli": "codex",
  "summary": "Running the backend test suite",
  "status": "ok",
  "history_line": "Started cargo test"
}
```

Valid status codes are `ok`, `warn`, `err`, and `idle`.

Structured agent adapters report to `POST /api/agent-event`. They can send `summary_zh_cn` and `summary_en`; the dashboard selects the active language without another model call.
