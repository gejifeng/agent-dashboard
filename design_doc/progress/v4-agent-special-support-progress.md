# v4 OpenCode / Claude Code / Codex 特殊支持 - 开发进程

> 目标：三类 coding agent 优先使用结构化生命周期事件更新卡片；未安装集成时，使用各自的 TUI 语义适配器兜底，避免 spinner、计时、token、footer 等动态区域导致重复推理和状态抖动。

状态：✅ 第一阶段已完成（2026-07-19）
关联：`v3-ai-summary-renaming-plan.md`、`terminal-compat-fix.md`

## 1. 本阶段结论

统一摘要链路改为：

```text
结构化事件（plugin / hook） ──优先──> 固定状态机 ──> 卡片
                                      ↑
专用 TUI 适配器 ──稳定采样──> 语义屏幕 ──> LLM 只补充任务描述
```

- `ok / idle / warn / err` 不再完全交给 LLM 猜测。OpenCode、Claude Code、Codex 的已知状态由适配器或 hook 确定。
- LLM 只负责从稳定语义文本提炼“当前具体在做什么”；即使模型措辞变化，也不能改写适配器给出的状态码。
- 同一个 `agent + state + semantic text` 形成同一缓存键，回到旧状态时直接复用历史结果。
- 结构化事件到达后的 20 秒内，屏幕采集暂停抢占，防止 hook 刚写入的准确状态被 TUI 文本覆盖。
- 卡片标题与摘要分离：标题优先采用 CLI 原生 session title，摘要继续显示当前活动。
- 标题优先级为 `面板手动命名 > CLI 原生标题 > 启动命令兜底`，CLI 自动改名不能覆盖用户手动命名。

## 2. 已完成内容

### 2.1 Rust agent 状态模型

新增 `src-tauri/src/agent_support.rs`：

- `AgentKind`：`opencode / claude_code / codex / generic`。
- `AgentState`：`working / idle / waiting_approval / retrying / error / unknown`。
- 状态固定映射：

| AgentState | 卡片状态 | 默认描述 |
|---|---|---|
| working | ok | `<Agent> 正在执行任务` |
| idle | idle | `<Agent> 已停止执行，等待输入` |
| waiting_approval | err | `<Agent> 等待用户确认` |
| retrying | warn | `<Agent> 正在重试` |
| error | err | `<Agent> 执行出错，需要检查` |

- 命令名可直接识别三类 agent；从 `pwsh -> WSL -> agent` 启动时，由前端屏幕标记二次识别。
- LLM 输入显式携带 agent 类型和状态提示，worker 在解析模型输出后再次用适配器状态覆盖模型状态，确保状态稳定。

### 2.2 三套 TUI 语义适配器

`dashboard.html` 新增：

- `detectAgentKind`：从启动命令和 TUI 特征识别 OpenCode、Claude Code、Codex。
- `classifyAgentState`：识别执行中、空闲、权限确认、重试、错误。
- `agentSemanticScreen`：按 agent 去除各自 footer、快捷键、context/token 状态栏等非任务信息。
- 专用 agent 连续 2 次采样一致后提交；普通终端仍需连续 3 次。
- 发送给 Rust 的语义键包含 agent 和状态，状态变化即使没有新增正文也会更新一次。

`dist/index.html` 已同步；运行时仍以根目录 `dashboard.html` 为唯一嵌入源。

### 2.3 结构化事件入口

新增本机接口：

```http
POST http://127.0.0.1:8787/api/agent-event
Content-Type: application/json

{
  "dashboard_session_id": "term-...",
  "agent": "opencode | claude_code | codex",
  "event": "PreToolUse | Stop | session.idle | permission.asked | ...",
  "summary": "可选的稳定描述",
  "agent_session_id": "CLI 内部 session/thread ID",
  "session_title": "CLI 原生任务标题"
}
```

- 接口只监听 `127.0.0.1`。
- PTY 自动注入：
  - `AGENT_DASHBOARD_SESSION_ID`
  - `AGENT_DASHBOARD_EVENT_URL`
  - `AGENT_DASHBOARD_HOOK`
- 同时扩展 `WSLENV`，因此从面板的 PowerShell 进入 WSL 后仍可继承会话 ID、事件地址和自动转换后的 hook 脚本路径。
- 上报失败会静默忽略，不能阻塞 agent 或改变 hook 决策。

### 2.4 CLI 原生标题同步

- Session 新增 `external_session_id`、`external_title`、`name_source`。
- OpenCode plugin 从 `session.created/session.updated/session.status` 等事件读取 session ID/title。
- Claude Code/Codex 共用 hook 转换器会读取 `session_id/thread_id` 与 `session_name/thread_name` 等结构化字段。
- xterm.js 同时监听 OSC terminal title；Claude Code 的 `--name`/`/rename` 等终端标题变化可直接同步，不依赖 LLM。
- 提供可选 `claude-code-statusline.json`，直接转发 Claude Code 官方 statusline JSON 中的 `session_name`；已有自定义 statusline 的用户应把上报调用合并进原脚本，而不是覆盖原配置。
- 对 PowerShell、WSL、shell 路径、单纯产品名/版本号做过滤，避免把宿主 shell 标题误认为任务名。
- 用户点击面板重命名后，`name_source` 固定为 `manual`，后续 CLI 标题仅记录到 `external_title`，不再覆盖卡片标题。

### 2.5 官方扩展模板

新增 `integrations/`：

- `agent_dashboard_hook.py`：Claude Code / Codex 共用的 JSON hook 转换器。
- `examples/claude-code-hooks.json`：监听 SessionStart、UserPromptSubmit、PreToolUse、PermissionRequest、PostToolUseFailure、Stop。
- `examples/claude-code-statusline.json`：可选同步 `--name`/`/rename` 的 `session_name`。
- `examples/codex-hooks.json`：监听 SessionStart、UserPromptSubmit、PreToolUse、PermissionRequest、Stop，含 Windows `commandWindows`。
- `examples/opencode-agent-dashboard.js`：监听 `session.status`、`session.idle`、`session.error`、`permission.asked`、tool events。

模板没有自动写入用户的全局配置，避免覆盖现有 hooks。安装方式：

1. OpenCode：将插件复制到项目 `.opencode/plugins/` 或全局 `~/.config/opencode/plugins/`。
2. Claude Code：把示例中的 `hooks` 合并到 `.claude/settings.json` 或用户设置。面板内 WSL 会话可直接使用 `$AGENT_DASHBOARD_HOOK`。
3. Codex：把示例合并到 `.codex/hooks.json` 或用户 `~/.codex/hooks.json`，随后在 `/hooks` 中检查并信任 hook。

官方能力依据：

- OpenCode TUI 自带 HTTP server，并提供 `/session/status`、SSE `/event`；plugin 可监听 `session.status`、`session.idle`、权限和 tool 事件：<https://opencode.ai/docs/server/>、<https://opencode.ai/docs/plugins/>
- Claude Code command/HTTP hooks接收生命周期 JSON，包含 `session_id`、`hook_event_name`，支持 Stop、PermissionRequest、Pre/PostToolUse 等：<https://code.claude.com/docs/en/hooks>
- Codex 支持 `hooks.json` / `[hooks]` 生命周期 hook，以及 `notify` 的 `agent-turn-complete` JSON：<https://learn.chatgpt.com/docs/hooks>、<https://learn.chatgpt.com/docs/config-file/config-advanced#notifications>

## 3. 文件变更

- `src-tauri/src/agent_support.rs`：agent 类型、状态机、事件映射、LLM 上下文。
- `src-tauri/src/session.rs`：状态来源、结构化事件写入、状态权威覆盖、缓存键扩展。
- `src-tauri/src/api.rs`：`/api/agent-event`。
- `src-tauri/src/pty.rs`：注入会话关联环境变量和 WSLENV。
- `src-tauri/src/lib.rs`：HTTP 服务与 Tauri command 共享同一 `SessionManager`。
- `dashboard.html`、`dist/index.html`：三套屏幕适配器与结构化事件优先策略。
- `src-tauri/permissions/app.toml`、`capabilities/default.json`：允许前端同步 agent 原生元数据。
- `integrations/`：hook 转换脚本和三类 agent 配置示例。

## 4. 验证记录

- `cargo check --manifest-path src-tauri/Cargo.toml`：✅
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：✅ 4 passed（command 识别、结构化事件映射、原生标题优先级、元数据清理）。
- `python -B integrations/agent_dashboard_hook.py --agent codex`：✅ 脚本可加载，缺少面板环境变量时按设计静默退出。
- Claude hooks、Claude statusline、Codex hooks 三份 JSON 模板经 PowerShell `ConvertFrom-Json`：✅。
- Claude statusline 模拟输入 `session_name=auth-refactor`：✅ 输出并同步原生标题 `auth-refactor`。
- `dashboard.html` 与 `dist/index.html` SHA-256 均为 `3F5A90E31D6793CDED1CE20050F09DF355AC9589B07AD8F8A5347213ECE262B1`：✅。
- 测试链接阶段有既存 `LNK4098 msvcrt` warning，不影响测试通过。
- 未调用 DeepSeek API，避免产生费用。

## 5. 当前限制与下一阶段

- OpenCode、Claude Code 当前未安装在 Windows PATH，无法在本轮直接完成三套真实 TUI 的端到端 hook 验证；本机 Codex 可用于后续实测。
- hooks 需要用户合并到对应 agent 配置；未安装时专用屏幕适配器已经生效。
- OpenCode 的最终形态应由面板以固定端口启动 TUI，直接订阅 `/event` 和 `/session/status`，届时不需要用户安装 plugin。
- Codex 的最终形态可接入 `app-server` WebSocket；非交互任务可直接使用 `codex exec --json`。
- Claude Code 可进一步用 HTTP hook 直接 POST 面板，但需要处理 dashboard session ID 的绑定；当前 command hook 通过继承环境变量绑定最简单可靠。
- 下一阶段需要采集三类 agent 各 3 组真实屏幕/事件日志，补齐版本差异规则，并为屏幕适配器增加前端自动化测试。

## 6. 2026-07-19 OpenCode 状态误报修复

### 日志结论

- 新版本中每个最新会话只产生一个语义键和一次 `[screen]` 推理，旧版每 10 秒重复 `[buffer]` 推理的问题未再出现。
- `term-1784451887058` 的部署已经完成、健康检查返回 200，但被判定为 `retrying -> warn`。
- 触发误报的是回答正文中的“上游 403 自动换号重试”，而非 OpenCode 当前状态栏。
- 同一屏幕还有“启动日志无 error/panic/fatal”，旧规则也可能把否定诊断误判为错误。
- 日志没有 `[structured:*]`，说明本次测试没有安装/加载 OpenCode plugin，实际由屏幕适配器兜底。

### 已修复

- 生命周期分类只读取屏幕底部 6 条 TUI 控制行，不再扫描整段回答正文。
- `retrying` 只接受独立明确状态行，如 `Retrying in 5s`、`正在重试 5秒`；普通技术描述中的“支持重试”不命中。
- 错误只接受以 `Error/Fatal/Panic/执行失败` 等开头的明确控制行。
- 排除“无/没有/未发现/no/without/zero/0 error|panic|fatal|failure”等否定诊断。
- working 只接受底部的 spinner、`esc to interrupt`、`running tool` 等控制信号。
- OpenCode 底部出现 `Ask anything`、`ctrl+p commands`、`tab agents` 时判定 idle。
- `summarize_screen` 新增 `state_evidence`；`screen_capture.log` 头部记录 agent、state 和实际命中行。
