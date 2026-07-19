# v2 终端会话化升级计划

> 把控制台从「状态仪表盘」升级为「多终端会话管理器」：每个任务卡片绑定一个活终端，点开即进入 xterm.js 交互终端。

状态：🚧 规划中
关联：`../main.md`

---

## 1. 背景与目标

- **现状（v1）**：卡片显示 agent 上报的摘要 + 状态；查看历史靠详情 modal；无法直接操作 agent 所在终端。
- **目标（v2）**：每个 task 绑定一个 PTY 会话；主页仍是卡片网格（摘要由模型从 PTY 采样生成）；点开卡片进入该会话的 xterm.js 终端，可看输出、可输入、可 resize。
- **不做的**：不在主页做「终端缩略图网格」（小尺寸字符不可读、渲染成本高）。

## 2. 技术选型（结论）

终端模拟器分两层：**PTY 后端**（托管进程、收发字节流）与**渲染前端**（解析 VT/ANSI 转义 → 字符网格）。

| 方案 | 结论 | 理由 |
|---|---|---|
| 内建（自研渲染） | ❌ | VT 协议复杂（xterm 数百条转义），自研 = 长期花屏 / TUI 异常，开源不该造此轮 |
| 系统自带终端 | ❌ | 独立窗口无法嵌入 webview、无法集中管理；ConPTY 仅是 PTY 后端 API，非渲染器 |
| 开源组件 | ✅ | 行业标准、MIT、与现有前端同栈 |

**选定**：

- 渲染前端：**xterm.js**（VS Code / Hyper / Tabby 同款，MIT，插件生态：fit / search / webgl 加速）
- PTY 后端：**portable-pty**（wezterm 出品，Rust，跨平台；Windows 走 ConPTY）

参考：Tabby（Electron + xterm.js + node-pty）、VS Code 集成终端。本项目即其 Tauri + Rust 轻量版。

## 3. 四个架构点方案

### 3.1 UI 形态

- **主页**：保持状态卡片网格。卡片摘要由模型从对应 PTY 最近输出采样生成（沿用 v0 的 Haiku 摘要思路，但输入源从「上报文本」变为「PTY 缓冲」）。
- **点开卡片**：进入该会话的 xterm.js 终端视图（全屏或大窗），可交互。
- 理由：兼顾「一眼看全局」与「深入操作」，避免终端缩略图不可读。

### 3.2 会话生命周期

- **创建**：本机用户在 UI「新建会话」，输入命令（或选预设 agent：claude / codex / …）→ portable-pty spawn。
- **进入 / 离开**：点卡片进入；关闭终端视图 = detach（会话保留后台，像 tmux），非销毁。
- **销毁**：显式「结束会话」或进程退出 → 清理 PTY、归档历史、可选保留最终摘要。
- **重启**：对已退出会话一键重启同命令。
- **进程退出检测**：portable-pty 的 child waiter → 标记 idle/err + 退出码写入 history。
- **resize**：前端窗口尺寸变化 → 通知后端 → PTY set_size → SIGWINCH。

### 3.3 安全边界

- 终端会话**只能由本机用户在 UI 本地创建 / 操作**。
- 远程 agent 仅 `POST /api/report` 上报摘要，**不能**远程开终端 / 下发命令。
- HTTP server 仅 `127.0.0.1`。
- 未来远程触发任务须显式审批流，不直连终端。

### 3.4 数据模型

- `status_store` → `session_store`：`task_id` → Session{ pty, 缓冲, 元数据, 状态, 摘要 }。
- Session 元数据：node、cli、命令、启动时间、退出码、输出缓冲（滚动回溯上限，如 10000 行）。
- 摘要：后台定时（如每 20s）从缓冲尾部采样 → 模型一句话 → 写回 summary（沿用 v0 INTERVAL / 规则 + 模型降级）。
- 持久化：会话元数据 + history 落 JSON；PTY 输出缓冲默认内存（可选落盘做回溯）。

## 4. 数据通路

```
portable-pty spawn 进程
   │  stdout/stderr 字节流
   ▼
Rust 读取线程 → Tauri event("pty:<id>", data) ──► xterm.js write(data)
                                                      │ 键盘 / 粘贴
                                                      ▼
                                              Tauri invoke(pty_input, id, data)
                                                      │
Rust 写入 PTY stdin ◄─────────────────────────────────┘
resize: xterm.js onResize → invoke(pty_resize,id,cols,rows) → PTY set_size
```

> PTY I/O 走 Tauri 命令 / event（双向流，低延迟），不走 HTTP；状态 / 上报仍走 axum HTTP。

## 5. 实施里程碑

- **M1 PTY 后端打通**：`portable-pty` 封装（spawn / 读 / 写 / resize / 退出检测）；单会话自测。
- **M2 xterm.js 前端接入**：渲染输出、键盘输入、resize 同步；单会话端到端可交互（跑 vi / htop 验证转义完整）。
- **M3 会话管理 UI**：新建 / 列表 / 进入 / detach / 结束 / 重启；`session_store`。
- **M4 与卡片网格整合**：主页卡片点击 → 进入终端；摘要从 PTY 采样生成（接回模型摘要）。
- **M5（可选）持久化与恢复**：会话元数据落盘；启动时恢复非存活会话的历史 / 最终状态。

## 6. 风险与对策

| 风险 | 对策 |
|---|---|
| 高频输出（编译日志）前端卡 | event 批量合并 + xterm.js webgl 渲染 + 写入节流；超阈值切换「摘要模式」暂停逐字节推送 |
| Windows ConPTY 不可用 / 版本旧 | 启动检测；降级提示；最低 Win10 1809 |
| 多会话输出交错 / 内存膨胀 | 每会话缓冲上限 + LRU；非活跃会话降频推送 |
| 终端与摘要状态不一致 | 摘要为投影，以 PTY 进程真实退出码为最终状态权威 |
| 远程开终端的诱惑 | 架构层禁止：会话创建仅本机 invoke，不暴露 HTTP |

## 7. 验收标准

- 新建会话能跑任意 CLI，输出完整不花屏，TUI 程序（vi / less / htop / claude code）正常。
- 键盘输入、粘贴、resize 实时生效。
- detach 后会话后台存活，重新进入输出连续不丢。
- 进程退出自动标记状态 + 退出码，可重启。
- 多会话并发各自独立、主页卡片摘要随 PTY 更新。
- 远程 HTTP 无法开启终端（安全验证）。

## 8. 未决问题

- 摘要采样的模型调用：复用 v0 的 `ANTHROPIC_API_KEY` + Haiku，还是改为可配置 provider / 本地模型？
- 会话输出是否落盘做长期回溯（影响隐私与体积）？
- 跨平台（macOS / Linux）的 PTY 行为差异验证时机。
- 是否引入会话分组 / 标签（多节点场景）。
