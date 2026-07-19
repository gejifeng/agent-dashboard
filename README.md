# Agent Dashboard

简体中文 | [English](README.en.md)

面向 OpenCode、Claude Code、Codex 及普通 CLI 的本地桌面控制台。它把多个真实终端会话集中为卡片，稳定展示当前状态、任务摘要和 Agent 原生会话名称，并允许直接进入 xterm.js 终端继续操作。

## 当前能力

- Tauri 2 + Rust + portable-pty 管理真实交互式终端，会话可 detach 后继续运行。
- OpenCode、Claude Code、Codex 专用屏幕适配和结构化生命周期 Hook。
- AI 摘要采用语义去重、单会话串行、pending 合并、结果缓存和过时结果丢弃，不会因 TUI spinner、计时或 token 数持续重复推理。
- DeepSeek `deepseek-v4-flash` 外部摘要；未配置 Key 时可回退本地 llama.cpp 模型。
- 卡片标题优先使用 Agent 原生任务名，用户手动重命名后不再被自动标题覆盖。
- 简体中文/English 完整切换，界面、固定状态摘要、AI 输出语言和 NSIS 安装器均支持双语。
- 本地 HTTP 上报 API，可接入自研 Agent；HTTP 服务仅监听 `127.0.0.1:8787`。

## 项目结构

```text
frontend/                       唯一前端源码（HTML/CSS/JavaScript）
src-tauri/                      正式 Rust/Tauri 桌面应用
integrations/                   正式 Agent Hook、插件与配置示例
prototypes/python-dashboard/    已归档的 v0 Python 概念验证，不参与正式运行
docs/architecture.md            当前架构与安全边界
docs/progress/                  各阶段开发记录
assets/                         项目图片资源
models/                         可选本地模型，不提交 Git
runtime/                        本地日志与运行数据，不提交 Git
```

前端只有 [frontend/index.html](frontend/index.html) 一份源码。Rust 内嵌页面与 Tauri `frontendDist` 都直接使用它，不再维护容易失配的 `dashboard.html` / `dist/index.html` 双副本。

## 开发运行

已验证环境为 Windows。需要 Rust/Cargo、Tauri 2 的 Windows 构建依赖和 WebView2；前端为原生单页，不需要 Node.js。

```powershell
git clone https://github.com/gejifeng/agent-dashboard.git
cd agent-dashboard
Copy-Item .env.example .env
cargo run --manifest-path .\src-tauri\Cargo.toml
```

应用占用本机 `127.0.0.1:8787`。启动新构建前请退出仍占用该端口的旧实例。

## AI 摘要配置

外部 DeepSeek 摘要：

```dotenv
DEEPSEEK_API_KEY=your_key_here
```

不要提交 `.env`。仓库只包含空的 `.env.example`。

本地回退模型是可选能力；模型文件不随仓库分发：

```dotenv
LOCAL_LLM_MODEL_PATH=models/Qwen3.5-2B-Q4_K_M.gguf
AGENT_DASHBOARD_LOG_DIR=runtime/logs
```

## Agent 专用集成

面板创建 PTY 时注入：

- `AGENT_DASHBOARD_SESSION_ID`
- `AGENT_DASHBOARD_EVENT_URL`
- `AGENT_DASHBOARD_HOOK`

集成文件位于 [integrations](integrations)：

- OpenCode：复制 `examples/opencode-agent-dashboard.js` 到项目 `.opencode/plugins/` 或全局插件目录。
- Claude Code：将 `examples/claude-code-hooks.json` 合并进 Claude 设置；可选合并 statusline 示例同步原生会话名。
- Codex：将 `examples/codex-hooks.json` 合并进 Codex hooks 配置并确认信任。

Claude Code/Codex 共用 `agent_dashboard_hook.py`。Hook 只向 loopback 上报，失败静默退出，不会阻塞或改变 Agent 行为。

## HTTP 上报 API

普通任务可调用 `POST http://127.0.0.1:8787/api/report`：

```json
{
  "task_id": "backend-tests",
  "node": "local",
  "cli": "codex",
  "summary": "正在运行 Rust 测试",
  "status": "ok",
  "history_line": "开始 cargo test"
}
```

状态码为 `ok`、`warn`、`err`、`idle`。专用适配器使用 `/api/agent-event`，可同时提交 `summary_zh_cn` 与 `summary_en`，切换语言时无需再次调用模型。

## 测试与构建

```powershell
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo test --manifest-path .\src-tauri\Cargo.toml --lib
cargo build --manifest-path .\src-tauri\Cargo.toml
```

调试程序输出到 `src-tauri/target/debug/agent-dashboard.exe`。NSIS 打包可使用 Tauri CLI；配置已启用 English 与 SimpChinese 安装器语言。

## 文档与历史原型

- [当前架构](docs/architecture.md)
- [开发进度](docs/progress/)
- [Python v0 原型](prototypes/python-dashboard/README.md)

Python 原型仅保留用于追溯早期 API/存储设计，不是正式后端，也不应与 Rust 应用同时启动在 8787 端口。

## 安全边界

- 终端等价于任意命令执行，只能由本机 UI 创建和操作。
- HTTP API 不提供远程 PTY 输入或远程创建终端能力。
- 模型只生成摘要和状态信息，不自动确认、合并或授权 Agent 操作。
- `.env`、模型、日志、运行数据和构建产物均由 `.gitignore` 排除。
