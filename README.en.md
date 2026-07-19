# Agent Dashboard

[简体中文](README.md) | English

A local desktop control center for OpenCode, Claude Code, Codex, and ordinary CLI tools. It groups real terminal sessions into cards, presents stable status and task summaries, uses native agent session names, and lets the user return directly to an interactive xterm.js terminal.

## Current capabilities

- Tauri 2, Rust, and portable-pty power real interactive terminals that continue running when detached.
- Dedicated screen adapters and structured lifecycle hooks for OpenCode, Claude Code, and Codex.
- Stable AI summaries with semantic deduplication, one worker per session, pending-state collapse, result caching, and stale-result rejection. Dynamic TUI spinners, timers, and token counters do not cause continuous inference.
- External summaries through DeepSeek `deepseek-v4-flash`, with an optional local llama.cpp fallback.
- Card titles prefer native agent task names; a manual dashboard rename cannot be overwritten later.
- Complete Simplified Chinese/English support for UI text, deterministic lifecycle summaries, AI output language, and the NSIS installer.
- A loopback HTTP reporting API for custom agents, bound only to `127.0.0.1:8787`.

## Repository layout

```text
frontend/                       single HTML/CSS/JavaScript front-end source
src-tauri/                      production Rust/Tauri desktop application
integrations/                   production agent hooks, plugins, and examples
prototypes/python-dashboard/    archived v0 Python proof of concept
docs/architecture.md            current architecture and security boundaries
docs/progress/                  chronological development records
assets/                         repository image assets
models/                         optional local model weights; not committed
runtime/                        local logs and runtime data; not committed
```

[frontend/index.html](frontend/index.html) is the only front-end source. Rust embedding and Tauri `frontendDist` both use it directly; the old `dashboard.html` / `dist/index.html` duplicate pair no longer exists.

## Run for development

Windows is the currently verified platform. Install Rust/Cargo, the Tauri 2 Windows prerequisites, and WebView2. The front end is a native single page and does not require Node.js.

```powershell
git clone https://github.com/gejifeng/agent-dashboard.git
cd agent-dashboard
Copy-Item .env.example .env
cargo run --manifest-path .\src-tauri\Cargo.toml
```

The application uses `127.0.0.1:8787`. Exit any older instance that still owns this port before starting a new build.

## AI summary configuration

To use external DeepSeek summaries:

```dotenv
DEEPSEEK_API_KEY=your_key_here
```

Never commit `.env`; the repository only contains an empty `.env.example`.

The optional local fallback model is not distributed with the repository:

```dotenv
LOCAL_LLM_MODEL_PATH=models/Qwen3.5-2B-Q4_K_M.gguf
AGENT_DASHBOARD_LOG_DIR=runtime/logs
```

## Agent integrations

Each PTY receives:

- `AGENT_DASHBOARD_SESSION_ID`
- `AGENT_DASHBOARD_EVENT_URL`
- `AGENT_DASHBOARD_HOOK`

Integration assets live under [integrations](integrations):

- OpenCode: copy `examples/opencode-agent-dashboard.js` to a project `.opencode/plugins/` directory or the global plugin directory.
- Claude Code: merge `examples/claude-code-hooks.json` into Claude settings; optionally merge the statusline example to forward the native session name.
- Codex: merge `examples/codex-hooks.json` into Codex hooks configuration and approve the hook.

Claude Code and Codex share `agent_dashboard_hook.py`. It reports only to loopback and fails silently, so it cannot block or change agent behavior.

## HTTP reporting API

Generic tasks can call `POST http://127.0.0.1:8787/api/report`:

```json
{
  "task_id": "backend-tests",
  "node": "local",
  "cli": "codex",
  "summary": "Running the Rust test suite",
  "status": "ok",
  "history_line": "Started cargo test"
}
```

Status codes are `ok`, `warn`, `err`, and `idle`. Dedicated adapters use `/api/agent-event` and may submit both `summary_zh_cn` and `summary_en`, allowing instant language switches without another model call.

## Test and build

```powershell
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo test --manifest-path .\src-tauri\Cargo.toml --lib
cargo build --manifest-path .\src-tauri\Cargo.toml
```

The debug executable is written to `src-tauri/target/debug/agent-dashboard.exe`. Tauri CLI can produce the NSIS package; its configuration includes both English and SimpChinese installer languages.

## Documentation and archived prototype

- [Current architecture](docs/architecture.md)
- [Development progress](docs/progress/)
- [Python v0 prototype](prototypes/python-dashboard/README.md)

The Python prototype is retained only as a record of the early API and storage design. It is not the production backend and should not run on port 8787 at the same time as the Rust application.

## Security boundaries

- A terminal is arbitrary command execution and can only be created and controlled from the local UI.
- The HTTP API does not expose remote PTY input or remote terminal creation.
- Models provide summaries and status information only; the application never auto-approves, merges, or authorizes agent actions.
- `.env`, model weights, logs, runtime data, and build output are excluded by `.gitignore`.
