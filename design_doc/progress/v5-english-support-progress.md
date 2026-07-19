# v5 全套英文支持开发进度

日期：2026-07-19

## 1. 目标

为桌面端、浏览器托管页面、AI 摘要和三类 TUI 集成提供完整的简体中文/英文切换能力。语言切换不能破坏既有的语义去重、单会话推理串行、结构化事件优先和 CLI 原生标题机制。

## 2. 已完成

### 2.1 前端界面本地化

- 新增 `zh-CN`、`en` 双语字典和右上角语言选择器。
- 首次启动按浏览器/系统语言选择；后续保存到 `localStorage` 的 `agent-dashboard-locale`。
- 已覆盖：应用标题、刷新状态、状态统计、筛选器、搜索框、总览句、任务数量、卡片状态、相对时间、空状态、详情弹窗、重命名、新建终端、终端退出/失败提示及按钮 tooltip/ARIA label。
- 演示 fallback 数据提供完整英文摘要与活动历史。
- Tauri 配置的初始窗口标题为 `Agent Dashboard`；页面初始化和切换语言时同步更新原生窗口标题。
- NSIS 安装器启用 `English` 与 `SimpChinese`，安装/卸载前显示语言选择器。

### 2.2 AI 摘要语言协议

- Rust 新增 `UiLanguage::{ZhCn, En}`，所有语言输入归一化为 `zh-CN` 或 `en`。
- `summarize_screen` 新增 `language` 参数；语言进入语义缓存 key，相同屏幕的中英文结果分别缓存。
- DeepSeek 与本地 llama.cpp 共用英文结构化 prompt，并显式约束描述输出为简体中文或英文。
- `set_language` Tauri command 会立即废弃旧语言的可见 key；旧语言在途推理完成后不能覆盖新语言卡片。
- 切换时先显示对应语言的确定性状态模板，下一次稳定屏幕采样再生成具体任务摘要。
- detach 后端兜底摘要读取当前全局语言。
- `screen_capture.log` 新增 `language=` 字段，方便诊断跨语言缓存或乱序问题。

### 2.3 OpenCode / Claude Code / Codex 双语事件

- `/api/agent-event` 新增可选 `summary_zh_cn` 与 `summary_en`，并保留旧 `summary` 字段兼容已有 Hook。
- `agent_dashboard_hook.py` 对权限、错误、工具调用、任务开始同时生成中英文摘要。
- OpenCode plugin 示例同时上报中英文固定状态摘要。
- 结构化事件的两个语言版本保存在会话中；切换语言时直接替换，不调用 LLM。
- Agent 原生任务标题、助手正文和通用 `/api/report` 摘要属于用户数据，保持原语言，不做可能失真的自动翻译。

### 2.4 英文开发与使用资料

- 新增根目录 `README.en.md`，说明语言切换、运行、构建、Agent 集成和 HTTP API。
- `README.md` 顶部增加英文文档入口。
- Cargo 包描述、Tauri 权限描述和 capability 描述改为英文。

## 3. 关键文件

- `dashboard.html`：语言字典、选择器、所有运行时 UI 文案和摘要语言提交。
- `dist/index.html`：与唯一前端源文件同步的构建产物。
- `src-tauri/src/agent_support.rs`：语言类型、双语固定摘要、LLM 上下文。
- `src-tauri/src/llm.rs`：双语摘要提示词。
- `src-tauri/src/session.rs`：语言状态、缓存隔离、切换失效、双语结构化摘要。
- `src-tauri/src/api.rs`：双语 Agent 事件字段。
- `integrations/agent_dashboard_hook.py`、`integrations/examples/opencode-agent-dashboard.js`：双语事件上报。
- `README.en.md`：英文使用文档。

## 4. 验证记录

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`：6 项全部通过，包含双语固定摘要和中英文无分隔符 fallback 状态解析。
- Chrome headless 以 `--lang=en-US` 实际执行页面：`html[lang=en]`、英文应用名、英文搜索提示、英文总览和 `+ New terminal` 均生效。
- `python -B integrations/agent_dashboard_hook.py --agent codex`：可加载且无面板环境时静默退出。
- Claude Code hooks、statusline 与 Codex hooks 三份 JSON：PowerShell `ConvertFrom-Json` 全部通过。
- `dashboard.html` 与 `dist/index.html` SHA-256 均为 `5E2D9A05ACDBF649D926CEEB4E81CBEF8E2072C1FA7D78880D4B18049120C3D2`。
- 独立输出目录执行 `cargo build`：通过，生成 27,407,872 字节可执行文件。
- 主 `target/debug/agent-dashboard.exe` 被 PID 16584 的旧实例锁定，主目录构建仅在替换该文件时失败；没有结束该进程，以免中断用户当前 PTY 会话。退出旧实例后重新执行 `cargo build` 即可覆盖。
- 测试/构建仍有既存 `LNK4098 msvcrt` warning，不影响通过。
- 未调用 DeepSeek API，避免测试产生外部费用。

## 5. 兼容性边界

- 已安装的旧版 Hook 若只发送 `summary`，仍可工作，但该自由文本不会自动翻译；重新复制本版本示例即可获得双语结构化摘要。
- 终端原始屏幕、Agent 原生标题和外部上报内容不会被翻译，这是对用户数据和技术文本的有意保真。
- 两种语言共用状态码 `ok/warn/err/idle`，因此语言切换不改变状态机判断。
