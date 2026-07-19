# v3 AI 任务摘要 + 卡片重命名 - 开发计划

> 卡片显示终端任务的 AI 实时摘要（默认本地 qwen 3.5 0.8b 量化模型，可选外部 API）+ 卡片重命名。

状态：🚧 规划/开发中
关联：../main.md, v2-terminal-sessions-progress.md

## 1. 功能

### A. 卡片显示任务进程摘要（AI 总结）
- 终端会话输出定期用 AI 总结成一句话，显示在卡片 summary（取代当前的"终端会话：command"）。
- 默认：本地 qwen 3.5 0.8b 量化模型（软件自带，减少资源占用）。
- 可选：手动指定外部 API（endpoint + key + model）。

### B. 卡片重命名
- 新建终端时可命名（默认用 command）。
- 之后可修改（卡片上编辑）。

## 2. 技术方案

### 本地 LLM 推理（默认）
- 候选引擎（调研中）：
  - **llama-cpp-2**（llama.cpp 的 Rust 绑定，GGUF 通用，成熟，需编译 C++）
  - **candle**（HuggingFace 纯 Rust，无 C++ 依赖，量化支持较弱）
- 模型：qwen3.5-0.8b GGUF 量化（如 Q4_K_M）。
- 分发：模型文件随软件 resources，或首次运行下载到用户目录。
- 推理：Rust 加载模型，PTY 缓冲尾部采样 -> 生成一句话摘要。

### 外部 API（可选）
- OpenAI 兼容协议（`/v1/chat/completions`）。
- 用户配置 endpoint + api_key + model。
- 设置界面存储配置。

### 摘要调度
- 每 N 秒（如 20s）或输出静止一段时间后触发。
- 从会话输出缓冲尾部采样（如最近 2KB）。
- AI 生成一句话摘要 -> 写入 `Session.summary`。
- `list_sessions` 返回 summary，卡片显示。

### 重命名
- Session 加 `name` 字段（默认 = command）。
- `rename_session` 命令。
- 前端：新建终端时输入名字；卡片双击/按钮编辑。

## 3. 里程碑

- ✅ **M1 重命名**（已完成）：Session name + rename_session + 前端 UI（新建命名 + 卡片 ✎ 编辑）。
- 🚧 **M2 本地 LLM 集成**：llama-cpp-2（已选型）+ 编译 + 模型加载 + 单次推理验证。
- ✅ **M3 摘要调度**：Session 加 summary + buffer（尾部 8KB），reader 追加，后台线程每 5s 采样 -> `LlmEngine.summarize` -> summary。
- ✅ **M4 卡片显示**：`list_sessions` 返回 summary，卡片显示 AI 摘要。
- 踩坑：`LlamaBatch` 容量 512 tokens，buf 增长后 tokenize 超限报 `Insufficient Space of 512` -> 改 batch 4096 + n_ctx 4096 + 输入截断尾部 1500 字符。
- 限制：0.8b 模型对"命令完成状态"判断不准（命令已完成回到提示符，仍摘要"正在执行"）。改进 prompt 强调状态判断 + 提示符识别，但 0.8b 能力有限，准确判断需更大模型/外部 API（M4 设置界面待做）。

## 4. 未决问题
- ~~本地 LLM 引擎选型~~ -> 已定 llama-cpp-2（llama.cpp Rust 绑定，GGUF，活跃维护）。
- ~~模型分发策略~~ -> 混合：默认首次下载 + 设置可覆盖路径。检测中国用户后用 hf-mirror.com 镜像下载（HuggingFace 在中国不可访问）。
- 外部 API 协议（OpenAI 兼容 vs 通用）。
- 摘要触发策略（定时 vs 静止 vs 输出量）。
- 多会话并发推理的资源管理（0.8b 小模型，CPU 推理，串行队列）。

## 5. 开发进程

### M1 重命名 ✅（2026-07-17）
- 后端（session.rs）：Session 加 `name` 字段；`create_session` 加 `name: Option<String>`（默认 command）；新增 `rename_session(id, name)`；`list_sessions`/`SessionInfo` 返回 name。
- 命令注册（lib.rs）+ 权限（`permissions/app.toml` 的 `allow-rename-session` + capabilities 授权）。
- 前端：新建终端两步 prompt（命令 + 名字，留空用命令名）；卡片标题显示 name；卡片右上 ✎ 按钮重命名（prompt -> rename_session -> 刷新）；终端标题用 name。
- 卡片点击进终端传 name；detach 重连复用已有 name。
- 验证：编译通过，exe 运行，前端含 renameSession/card-edit。

### M2 本地 LLM 集成（进行中）
- 选型：llama-cpp-2 0.1.151（llama.cpp Rust 绑定）。已 cargo add。
- 编译环境：CMake 4.3.4 ✅；MSVC ✅；**libclang（bindgen 需要）❌ 待装 LLVM**。
- LLVM 安装：winget silent 被 UAC 取消，需手动装（管理员 `winget install LLVM.LLVM` 或下载 LLVM-22.1.8-win64.exe）。装好后设 `LIBCLANG_PATH=C:\Program Files\LLVM\bin` 再 cargo check。
- 模型分发：混合（默认首次下载 + 可覆盖路径）+ 中国用户 hf-mirror.com 镜像。
- ✅ 推理验证通过：加载 `unsloth/Qwen3.5-0.8B-Q4_K_M.gguf`（507MB，hf-mirror 下载）+ `apply_chat_template` + 生成摘要（输入 cargo build 输出 -> "正在编译 agent-dashboard 项目"）。
- 关键发现：Qwen3.5 是 thinking 模型，`/no_think` 在 llama.cpp `apply_chat_template` 下**不生效**；改用 `LlamaSampler::logit_bias` 禁 `<think>` token（bias -100）强制非 thinking，摘要正常。max_new=64 即可。
- 模型路径（测试）：`models/Qwen3.5-0.8B-Q4_K_M.gguf`。后续做首次下载 + 中国用户镜像 + 路径覆盖（M2b）。
- 注：LLVM/libclang 仅编译时依赖，最终用户运行 exe **无需装 LLVM**（绑定+llama.cpp 静态链接进 exe）。
- 待办：M3 摘要调度（Session 加 summary + 输出缓冲 + 定时触发 `LlmEngine.summarize`）-> M4 卡片显示 summary + 外部 API + 设置。
