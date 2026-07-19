# Agent 控制台 — 主设计文档

> 面向多 agent 协作的任务状态控制台。从「状态仪表盘」演进为「带内建终端的会话管理器」。
> 目标：做成可长期演进的开源桌面软件。

本文档为总纲（定位 / 原则 / 架构 / 契约）；阶段性升级计划见 `progress/`。

---

## 1. 项目定位

同时跑多个 coding agent（claude code、codex、自研 CLI 等）时，缺一个「一眼看全局、谁需介入、并能直接上手操作」的统一控制台。本软件提供：

- 一个主页网格，每个卡片代表一个 agent 任务，实时显示「在做什么 / 状态」。
- （v2）点开卡片即进入该任务的真实终端，可直接交互。
- 任意语言的 agent 通过 HTTP 上报状态；本机用户在面板内统一查看与操作。

参考坐标：≈ Tabby / VS Code 集成终端的 Tauri + Rust 轻量版，但面向 agent 管理。

## 2. 设计原则

1. **模型做信息层，人保留控制层。** 模型只负责摘要 / 聚合 / 告警；确认、执行、决策由人完成。系统目前不含任何「自动确认 / 自动合并」逻辑。
2. **终端会话仅本机用户操作。** 远程 agent 只能上报摘要，不能远程开启终端（终端 = 任意命令执行，否则即远程代码执行面）。
3. **数据契约语言无关。** 以 HTTP API 为契约，前端 / 后端 / 上报端各自演进、互不绑定。
4. **单文件可分发、便携、低依赖。** 桌面 exe 自包含前端，状态文件放 exe 旁，拷走即用。
5. **够用为止，量大了再换。** 存储 JSON 起步，超规模再上 SQLite。

## 3. 演进路线

| 版本 | 主题 | 状态 | 要点 |
|---|---|---|---|
| v0 | Python MVP | ✅ 完成 | 验证设想：JSON 状态存储 + HTTP 轮询 + 网格 UI + CLI 上报 + Haiku 摘要包装 |
| v1 | Tauri 桌面化 | ✅ 完成 | Rust 重写核心 + axum + Tauri webview，单 exe ≈9MB，前端零改动复用 |
| v2 | 终端会话化 | 🚧 规划中 | xterm.js + portable-pty，每 task 绑定活终端，点开即交互 |
| 未来 | — | 待定 | 全局态势摘要层、异常检测、桌面通知、自动更新 / 安装包、跨平台、历史查询 |

各阶段详细计划见 `progress/`。

## 4. 当前架构（v1）

```
┌─────────────────────────────────────────────┐
│            agent-dashboard.exe (≈9MB)        │
│  ┌───────────────────────────────────────┐  │
│  │  Tauri webview 窗口                    │  │
│  │  加载 http://127.0.0.1:8787/           │  │
│  │  dist/index.html · 每 5s 轮询          │  │
│  └───────────────┬───────────────────────┘  │
│                  │ HTTP                       │
│  ┌───────────────▼───────────────────────┐  │
│  │  axum HTTP server  127.0.0.1:8787      │  │
│  │   GET /  ·  GET /api/status           │  │
│  │   POST /api/report · GET /api/remove  │  │
│  └───────────────┬───────────────────────┘  │
│  ┌───────────────▼───────────────────────┐  │
│  │  Rust Store (Mutex<HashMap> + JSON)    │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
        ▲  HTTP POST /api/report（任意语言 agent）
   claude code / codex / 自研 CLI(Hermes / Anima) …
```

**启动流程**：`main` → `lib::run` 在独立线程 + tokio runtime 起 axum → `wait_for_port` 等待 8787 就绪 → 启动 Tauri 窗口加载该地址（避免窗口加载到未就绪端口）。

**前端**：`dist/index.html` 在编译时 `include_str!` 嵌入，单 exe 自包含。

## 5. 数据模型

### 5.1 v1 Entry（status_store 等价）

```json
{
  "node": "gejifengai",
  "cli": "claude",
  "summary": "正在重构 Bully 选举模块的心跳超时逻辑",
  "status": "ok",
  "last_seen": 1784259257.0,
  "updated_at": 1784259257.0,
  "history": ["[11:34:17] 定位到超时判断的边界条件问题"],
  "stale": false
}
```

- `task_id`（建议 `{node}-{task}`）为主键。
- `status`：`ok` / `warn` / `err` / `idle`。
- 摘要未变只刷新 `last_seen`，不刷新 `updated_at`、不追加 history（避免前端闪动）。
- `STALE_SECONDS=180`：`ok` 超时未更新自动降级 `warn` 并标 `stale`。
- history 只留最近 50 条。

### 5.2 v2 演进：Session

`task_id` 绑定一个 PTY 会话：进程 + 输出缓冲 + 状态 + 摘要（摘要从 PTY 输出采样由模型生成）。`status_store` 演进为 `session_store`。详见 `progress/v2-terminal-sessions-plan.md`。

## 6. API 契约

| 方法 | 路径 | 作用 |
|---|---|---|
| GET | `/` | 前端页面 |
| GET | `/api/status` | 全部任务状态（自动标 stale） |
| POST | `/api/report` | 上报 / 更新任务（任意语言 agent） |
| GET | `/api/remove?id=` | 删除任务 |

`POST /api/report` body：

```json
{"task_id":"...","node":"...","cli":"...","summary":"...","status":"ok","history_line":"可选"}
```

v2 将新增会话相关接口；PTY I/O 走 Tauri 命令（双向流、低延迟），状态 / 上报仍走 HTTP。

## 7. 技术栈

- **后端**：Rust + Tauri 2 + axum 0.7 + tokio + serde / serde_json + chrono
- **前端**：原生 HTML / CSS / JS（Inter + JetBrains Mono）；v2 加 xterm.js
- **PTY（v2）**：portable-pty（Windows 走 ConPTY）
- **构建**：cargo；前置 MSVC C++ Build Tools + WebView2 运行时

## 8. 模块划分

### v1（当前）

```
src-tauri/
  Cargo.toml · build.rs · tauri.conf.json
  capabilities/default.json
  src/
    main.rs    # 入口（release 无控制台）
    lib.rs     # run()：起 axum 线程 + wait_for_port + Tauri
    store.rs   # 状态存储：内存 HashMap + JSON 持久化 + stale 判定
    api.rs     # axum 路由 + handler（前端 include_str! 嵌入）
dist/
  index.html   # 前端（源文件为根目录 dashboard.html）
```

### v0（Python MVP，保留为可选示例）

```
server.py        # 最小 HTTP 服务
status_store.py  # JSON 状态存储 + 读写协议（schema 源头）
report_cli.py    # 命令行上报工具
cli_wrapper.py   # 包装第三方 CLI + Haiku 摘要
dashboard.html   # 前端（v1 dist/index.html 的源头）
```

### v2（规划）

新增 `session.rs`（会话管理）、`pty.rs`（portable-pty 封装）、前端 `terminal` 视图与 xterm.js。

## 9. 安全边界

- **终端 = 任意命令执行。** 终端会话只能由本机用户在 UI 上本地创建 / 操作。
- 远程 agent 仅能通过 `POST /api/report` 上报摘要，**不能**远程开启终端或下发命令。
- HTTP server 仅监听 `127.0.0.1`，不暴露到网络。
- 未来若需远程触发任务，须显式设计审批流，不直接连终端。

## 10. 分发

- 单 exe（约 9MB），拷到任何装 WebView2 的 Windows 机器双击即用。
- 状态文件 `status.json` 落在 exe 同目录（便携）。
- 未来：NSIS 安装包 + 自动更新 + macOS / Linux 构建。

## 11. 开源化待办

- LICENSE、README（中英）、CONTRIBUTING
- CI（build / lint / release artifact）
- 跨平台构建矩阵
- 原 Python 脚本定位为「可选示例上报工具」，主线以 `src-tauri` 为准

## 12. 相关文档

- `progress/v2-terminal-sessions-plan.md` — 终端会话化升级计划
