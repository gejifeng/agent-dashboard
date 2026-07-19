# v6 项目结构重构进度

日期：2026-07-19

## 目标

将正式 Rust/Tauri 应用、唯一前端源码、正式 Agent 集成、历史 Python 原型、架构文档、资源与本地运行数据明确分层，并重写已经过时的中英文 README。

## 目录迁移

| 旧路径 | 新路径 | 定位 |
|---|---|---|
| `dashboard.html` + `dist/index.html` | `frontend/index.html` | 唯一前端源码，消除双副本漂移 |
| `cli_wrapper.py` 等四个根目录脚本 | `prototypes/python-dashboard/` | v0 概念验证归档 |
| `design_doc/main.md` | `docs/architecture.md` | 当前正式架构，已完全重写 |
| `design_doc/progress/` | `docs/progress/` | 历史开发记录 |
| `logo.png` | `assets/logo.png` | 项目资源 |
| 根目录诊断日志 | `runtime/logs/` | 本地运行数据，不入 Git |

正式的 Claude Code/Codex Hook 与 OpenCode plugin 保留在 `integrations/`，因为它们属于当前产品能力，不是 Python 原型。

## 路径与运行时修复

- `api.rs` 改为内嵌 `frontend/index.html`。
- Tauri `frontendDist` 改为 `../frontend`，不再需要生成/复制 `dist/index.html`。
- 本地模型路径不再硬编码开发机绝对路径；由 `LOCAL_LLM_MODEL_PATH` 配置，默认 `models/Qwen3.5-2B-Q4_K_M.gguf`。
- 摘要诊断日志不再写死项目根目录；由 `AGENT_DASHBOARD_LOG_DIR` 配置，默认 `runtime/logs/`。
- `.env.example` 与 `.gitignore` 同步增加正式的本地模型、日志和 runtime 约定。
- Rust `test_llm` example 更新为当前双语 `llm::summarize` 接口。
- Python prototype server 从新位置读取正式 `frontend/index.html`，原型仍可单独验证。

## 文档

- `README.md` 与 `README.en.md` 按当前实际功能重新编写。
- 新 README 覆盖：稳定摘要机制、三类 Agent 支持、双语、目录树、运行配置、HTTP API、测试构建、安全边界和原型定位。
- `docs/architecture.md` 从早期 v1/v2 规划更新为当前实现架构。

## 验证

- `cargo fmt --check`：通过。
- `cargo check --examples`：通过，包含更新后的 `test_llm`。
- `cargo test --lib`：6 项全部通过。
- 4 个 Python prototype 文件与正式 `agent_dashboard_hook.py`：AST 解析通过；prototype CLI 可正常显示 help。
- Chrome headless 实际执行 `frontend/index.html`：英文语言、标题、搜索框和新建终端文案均生效。
- 独立输出目录完整 `cargo build`：通过，生成 27,417,088 字节调试程序。
- 构建仍有既存 `LNK4098 msvcrt` warning，不影响通过。
