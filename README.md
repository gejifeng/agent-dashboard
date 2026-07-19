# Agent 控制台 — 使用说明

[English documentation](README.en.md) | 简体中文

## 文件结构
```
dashboard.html          前端网格 UI（4x4，点击展开详情，5秒轮询 /api/status）
backend/
  status_store.py       核心：JSON 状态存储 + 读写协议
  report_cli.py          命令行上报工具，给自研 agent（Hermes/Anima）心跳调用
  cli_wrapper.py         包装 codex / claude code 等第三方 CLI，自动尾随输出并用 Haiku 生成摘要
  server.py              最小 HTTP 服务，暴露 /api/status，托管前端页面
```

## 快速开始（在你 Windows 主机或某台 Linux 节点上）

1. 启动服务：
   ```bash
   cd backend
   pip install anthropic --break-system-packages   # 仅 cli_wrapper.py 需要
   export ANTHROPIC_API_KEY=你的key                 # 仅 cli_wrapper.py 需要
   python server.py --port 8787
   ```
2. 浏览器打开 `http://<这台机器IP>:8787`（同一 WireGuard 网络内的其他设备也能直接访问）。
3. 一开始状态存储是空的，前端会自动 fallback 显示 mock 数据；等有真实任务上报后会自动切换成真实数据。

## 接入自研 agent（Hermes / Anima）

在心跳逻辑里加一行 shell out，或者直接 `from status_store import report` 在 Python 里调用：

```bash
python report_cli.py \
  --task-id noos-node1-election-fix \
  --node gejifengai \
  --cli claude \
  --summary "正在重构 Bully 选举模块的心跳超时逻辑" \
  --status ok
```

`status` 取值：`ok`(正常) / `warn`(迟缓或需要留意) / `err`(报错需要处理) / `idle`(空闲或等待你确认)。

## 接入第三方 CLI（Codex / Claude Code）

不改动这些工具本身，用 wrapper 包一层：

```bash
python cli_wrapper.py \
  --task-id vllm-deploy \
  --node gejifengai \
  --cli codex \
  -- codex run --your-flags-here
```

- 你的终端还是能看到完整原始输出（透传，没有丢失任何信息）。
- 每 20 秒会把最近输出丢给 Haiku 生成一句话摘要并上报，命中错误关键词会直接标红，不等模型判断。
- 可以按需调整 `cli_wrapper.py` 里的 `INTERVAL_SECONDS` / `ERROR_KEYWORDS` / 摘要 prompt。

## 下一步可以加的（先不做，等 MVP 跑顺了再考虑）

- 全局态势摘要层：把所有任务的 summary 再丢给一次模型调用，生成"3个正常推进，1个需要你介入"这种顶部提示。
- 异常检测：同一摘要长时间不变 → 自动标黄（`status_store.py` 里的 `STALE_SECONDS` 已经做了最简单版本，可以做得更细）。
- 桌面通知：某个任务变成 `err` 时用 Windows Toast 或者你现有的 Telegram/WeChat bot 推送提醒。
- 把 JSON 文件换成 SQLite（任务数超过几十个、或者要留存历史查询时再做）。

## 设计原则（别忘了）

模型只做信息层的工作（摘要、聚合、告警），人保留所有控制层的权力（确认、执行、决策）。
这套系统里目前没有任何"自动确认/自动合并"的逻辑，也建议先别加。
