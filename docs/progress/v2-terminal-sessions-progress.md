# v2 终端会话化 - 开发进程记录

> 记录 v2 终端会话化的实际开发过程、踩坑与优化。原始计划见 `v2-terminal-sessions-plan.md`。

状态：✅ 完成（M1-M4 + 性能优化）

## 完成里程碑

| 里程碑 | 状态 | 说明 |
|---|---|---|
| M1 PTY 后端 | ✅ | portable-pty 0.9 封装（spawn/读写/resize/kill），Windows ConPTY |
| M2 xterm.js 前端 | ✅ | xterm.js 5.5 + addon-fit，全屏终端视图 |
| M3 会话管理 | ✅ | SessionManager + 6 个命令（create/list/detach/input/resize/close） |
| M4 卡片=终端会话 | ✅ | 主页卡片来自 list_sessions，每卡片对应一个终端 |
| 性能优化 | ✅ | Channel + 输入 batch + detach，灵敏度接近原生 |

## 实际实现

### PTY 后端（src/pty.rs）
- portable-pty 0.9：`native_pty_system` / `openpty` / `spawn_command` / `try_clone_reader` / `take_writer` / `resize` / `kill`。
- Windows 走 ConPTY。
- 踩坑：0.9 的 resize 方法名是 `resize`（非 `set_size`），通过查 crate 源码确认。

### 会话管理（src/session.rs）
- `SessionManager: Mutex<HashMap<String, Session>>`。
- Session 字段：id, command, started_at, alive(Arc<AtomicBool>), output(Arc<Mutex<Option<Channel>>>), pty。
- 命令：
  - `create_session`（幂等：已存在则替换 output channel 用于重连，不重复 spawn）
  - `list_sessions`（返回会话列表供卡片渲染）
  - `detach_session`（output=None，reader 停推送）
  - `pty_input` / `pty_resize` / `close_session`
- reader 线程：读 PTY -> raw bytes -> `Channel.send(InvokeResponseBody::Raw)`。

### 前端（dist/index.html）
- xterm.js 终端视图（全屏 overlay）。
- `fetchTasks` 在 Tauri 下用 `invoke('list_sessions')`，卡片=终端会话（不再用 /api/status 上报任务）。
- 卡片点击 `openTerminal(id, command)`。
- 新建终端按钮 + detach/结束/关闭。

### 踩坑：Tauri 2 ACL
- 现象：invoke 报 `Command create_session not allowed by ACL`。
- 误判：以为应用命令需 permission 文件（加了 `permissions/app.toml`）。实际应用命令默认 allowed。
- **根因**：webview 加载 remote URL（`http://127.0.0.1:8787/`），capability 默认 `local:true` 只对 bundled 代码生效，remote webview 无权限。
- 修复：`capabilities/default.json` 加 `"remote":{"urls":["http://127.0.0.1:8787/**"]}`。

### 性能优化（终端灵敏度）
- 问题：终端反应不灵敏。根因：每键一次 invoke + 每输出一个 event（base64），高频 IPC 往返累积。
- 优化：
  1. 输出：`event + base64` -> `Channel<InvokeResponseBody>` raw bytes（专用流式通道，无 base64，无全局 event bus）。
  2. 输入：每键 invoke -> 8ms 窗口 batch 合并。
  3. detach：`detach_session` 命令置 `output=None`，reader 停推送。
  4. buf 8KB -> 16KB。
- 依据：Tauri 文档 "Channels are designed to be fast... used internally for child process output"。
- 效果：灵敏度接近原生终端。

## 当前数据通路
```
输入: xterm.onData -> 8ms batch -> invoke('pty_input', base64) -> decode -> PTY.write
输出: PTY -> reader.read(16KB) -> Channel.send(Raw bytes) -> 前端 onmessage -> term.write(Uint8Array)
退出: reader EOF -> alive=false -> emit('pty_exit') -> 前端提示
```

## 文件
- `src-tauri/src/pty.rs` - PTY 封装
- `src-tauri/src/session.rs` - 会话管理 + 命令
- `src-tauri/src/lib.rs` - 注册命令
- `src-tauri/permissions/app.toml` - 命令权限
- `src-tauri/capabilities/default.json` - remote 授权
- `dist/index.html` - 前端（终端视图 + 卡片）

## 待办（后续）
- reader coalesce（合并短时间小包，超大输出场景，当前不需要）。
- detach 期间输出缓冲恢复（M5，重连不丢中间输出）。
- 跨平台 PTY 验证（macOS/Linux）。
