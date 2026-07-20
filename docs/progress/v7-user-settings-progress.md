# v7 用户设置面板开发进度

日期：2026-07-20

## 目标

在用户优化后的冷紫黑 UI 中加入一致的设置入口，让语言、API 厂商、Base URL、模型和 API Key 不再依赖手工编辑 `.env`。

## 实现

- 顶部操作区新增齿轮按钮，复用现有半透明面板、圆角、阴影和动效。
- 新增双语设置弹窗：语言、厂商、模型、Base URL、API Key、Key 状态与清除操作。
- 支持 DeepSeek、OpenAI、OpenRouter、SiliconFlow、自定义 OpenAI-compatible 服务。
- 预置厂商的 Base URL 在前后端同时锁定；自定义 URL 只允许 HTTP(S) host，禁止嵌入认证、query 或 fragment。
- 切换预置厂商时自动填写对应官方 URL 和建议模型，用户仍可修改模型。

## Key 与持久化

- 新增 `settings.rs`，设置保存在当前用户配置目录；Windows 默认 `%APPDATA%\Agent Dashboard\settings.json`。
- 前端读取时只获得 `hasApiKey`、来源和末四位提示，完整 Key 不从 Rust 返回 JavaScript。
- 空 Key 表示保留已保存值；提供显式清除按钮。
- 支持 `DEEPSEEK_API_KEY`、`OPENAI_API_KEY`、`OPENROUTER_API_KEY`、`SILICONFLOW_API_KEY` 环境变量回退。
- 自定义本地服务允许不填写 Key；非自定义厂商无 Key 时继续使用本地 llama.cpp 回退。

## 摘要请求

- LLM 摘要从设置读取 provider、Base URL、model 和 Key，不再固定 DeepSeek endpoint。
- endpoint 自动补全 `/chat/completions`，也接受用户填写完整 endpoint。
- DeepSeek V4 使用 `thinking: {type: "disabled"}`；SiliconFlow 使用 `enable_thinking: false`。
- OpenAI 使用 `max_completion_tokens`，其他 OpenAI-compatible 服务保留 `max_tokens`。
- 设置保存后增加配置 revision、清空会话摘要缓存并使旧配置的在途结果失效。

## 验证

- Rust 新增 provider URL、custom URL、Key 脱敏、非思考模型限制及响应检测测试；当前 13 项单元测试全部通过。
- Chrome Headless 页面执行验证通过：英文界面、设置按钮、设置标题、Key 占位文字及 Custom 厂商翻译均正确。
- 设置弹窗视觉检查通过；仅用于检查的预览入口已移除，没有进入正式代码。
- Tauri 设置命令权限已生成到 schema，并由默认 capability 启用。
- 独立 target 目录完整构建通过，生成的可执行文件大小为 27,725,824 字节。
- 构建仅保留既有的 `LNK4098: defaultlib 'msvcrt' conflicts` 链接器警告，不影响本次功能。
- 验证过程未向任何真实 API 厂商发起请求。

## 非思考模式限制

- AI 摘要仅支持非思考模式；设置保存时拒绝明确的 reasoning-only 模型标识。
- OpenAI 和 OpenRouter 的默认模型改为 `gpt-4.1-mini` / `openai/gpt-4.1-mini`。
- DeepSeek V4 继续发送 `thinking: {type: "disabled"}`，SiliconFlow 继续发送 `enable_thinking: false`。
- 请求前会再次校验模型；响应包含 `reasoning_content`、reasoning token 或 `<think>` 标记时直接报错，不再截取思考后的正文。
