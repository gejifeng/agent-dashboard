# 终端兼容性修复 - opencode/codex 等 TUI agent 无输出

## 问题
- 新建终端运行 opencode/codex 等 CLI agent，发指令后**无输出**，但后台 agent 在运行。
- 普通命令（pwsh/dir）正常。

## 根因
- Tauri 内置 axum 过去嵌入的是 `dist/index.html`，而开发时修改的是根目录 `dashboard.html`。两份文件没有构建步骤自动同步，因此多次前端修复可能根本没有进入实际运行的桌面应用。
- 启动逻辑过去只检查 8787 “能否连接”。若旧 `agent-dashboard.exe` 仍占用端口，新应用的 axum bind 失败后仍会打开窗口，并加载旧进程中嵌入的 xterm 5.5 页面。
- 现代 TUI agent（opencode/codex，基于 bubbletea/ratatui 等）依赖 `TERM` 环境变量判断终端能力。
- PTY spawn 时未设 `TERM`（Windows GUI 进程无 TERM，PTY 默认空/dumb），TUI 程序检测到"终端不支持"-> 不渲染 TUI 输出。
- xterm.js 5.5 **尚未支持** DEC mode 2026 synchronized output；该能力在 xterm.js 6.0 才加入。opencode 持续发送整屏 synchronized frame 时，5.5 不能按 frame 提交，高频大帧会使前端解析/渲染队列积压，表现为停止刷新。
- xterm 的 `onData` 在 PTY spawn 后才注册，后端又在 session 入表前启动 reader，存在启动期 CPR/DA/颜色查询应答丢失的竞态。

## 修复
- axum 改为直接 `include_str!("../../dashboard.html")`，将根目录 `dashboard.html` 作为前端唯一源文件，避免运行时继续吃到过期 `dist/index.html`；HTML 响应添加 `Cache-Control: no-store`。
- axum 通过 channel 明确回报本进程 bind 成功后才启动 Tauri 窗口；8787 被旧实例占用时直接报错，不再偷偷加载旧前端。
- `pty.rs` spawn 时设 `TERM=xterm-256color` + `COLORTERM=truecolor`。
- 前端 `openTerminal`：`fit()` 同步在 `create_session` 前调用，PTY 初始大小 = xterm 实际尺寸（非默认 80x24）；创建后立即再 `fit()` 触发 `pty_resize` 同步。
- **升级 xterm.js 6.0** 并保留原始 `\x1b[?2026h/l`，由 xterm 原生按 synchronized frame 提交；删除容易被 Channel 分片边界破坏的手写过滤。
- 在 spawn 前注册 `onData`/`onBinary`，并在后端 session 入表后才启动 reader，保证终端查询应答能回写 PTY。开启 `CSI 14t/16t/18t` 尺寸查询响应。
- detach 改为只隐藏会话容器：每个会话保留自己的 xterm 实例、normal/alternate buffer、DOM 和 Tauri Channel，隐藏期间仍持续接收 PTY 输出。再次进入直接显示原实例并 `refresh`，只有“结束会话”才 dispose，因此历史和 TUI alternate screen 都不会丢失。

## 诊断方法
- reader 线程把 PTY 输出 hex 写到 `pty_debug.log`，肉眼解析转义序列。
- 发现 opencode 启动查询（CPR `\x1b[6n`、XTVERSION `\x1b[>0q`、窗口像素 `\x1b[14t`、颜色 OSC 4）+ 进 alternate（`\x1b[?1049h`）+ 每帧 sync output（`\x1b[?2026h/l`）。

## 教训
- xterm.js 适合渲染这类 TUI，但前端版本必须支持程序实际使用的终端协议；WebGL 只改变 renderer，不会补齐 DEC mode 2026 等协议语义。
- TUI 程序依赖 TERM/COLORTERM 判断能力，必须正确设置，否则降级或不输出。
- 后续若遇其他 TUI 兼容问题，检查：TERM、COLORTERM、鼠标转义、bracketed paste、alternate screen。
