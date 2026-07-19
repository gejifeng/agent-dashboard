#!/usr/bin/env python3
"""
第三方 CLI（Codex / Claude Code 等你不方便改造 prompt 的工具）的包装脚本。

思路：
1. 用 subprocess 启动目标 CLI，把它的输出实时透传给你的终端（该看的还是看得到）。
2. 同时把输出滚动缓存最近 N 行。
3. 每隔 INTERVAL 秒，把缓存丢给一个小模型（默认用 Claude Haiku），
   让它生成一句话摘要 + 判断状态（ok/warn/err/idle），写入状态存储。
4. 检测到进程退出 / 长时间无输出 时自动标记 idle。

用法：
    python prototypes/python-dashboard/cli_wrapper.py \
        --task-id vllm-deploy \
        --node gejifengai \
        --cli codex \
        -- codex --some-flag ...

注意：
- 需要设置环境变量 ANTHROPIC_API_KEY。
- "--" 之后的内容会原样作为子进程命令执行。
- 这是模板，具体的摘要 prompt 和触发规则可以按你的实际输出格式调整。
"""

import argparse
import os
import subprocess
import sys
import threading
import time
import collections

from status_store import report

INTERVAL_SECONDS = 20          # 多久生成一次摘要
BUFFER_LINES = 40              # 摘要时参考的最近行数
IDLE_TIMEOUT = 300             # 超过这么久没有新输出，判定为 idle（等待人工/挂起）

ERROR_KEYWORDS = ["Error", "Traceback", "错误", "failed", "FAILED", "Exception"]


def summarize_with_haiku(lines: list[str]) -> tuple[str, str]:
    """
    调用 Claude Haiku 生成一句话摘要 + 状态判断。
    返回 (summary, status)。失败时降级为规则判断，不阻塞主流程。
    """
    text = "\n".join(lines[-BUFFER_LINES:])

    # 规则先行：命中明显错误关键词，直接标 err，不必等模型
    if any(kw in text for kw in ERROR_KEYWORDS):
        rule_status = "err"
    else:
        rule_status = None

    try:
        import anthropic  # pip install anthropic
        client = anthropic.Anthropic()
        resp = client.messages.create(
            model="claude-haiku-4-5-20251001",
            max_tokens=60,
            messages=[{
                "role": "user",
                "content": (
                    "以下是一个命令行 agent 最近的输出片段。用不超过20个字的中文总结它当前在做什么"
                    "（例如：正在调研API文档 / 正在写测试 / 等待用户确认 / 报错重试中）。"
                    "只输出这一句话，不要任何前后缀：\n\n" + text
                ),
            }],
        )
        summary = resp.content[0].text.strip()
        status = rule_status or "ok"
        return summary, status
    except Exception as e:
        # 模型调用失败，降级：至少把规则判断的结果报上去
        fallback = "输出解析失败，展示原始最后一行: " + (lines[-1][:40] if lines else "无输出")
        return fallback, rule_status or "warn"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--task-id", required=True)
    p.add_argument("--node", required=True)
    p.add_argument("--cli", required=True)
    p.add_argument("cmd", nargs=argparse.REMAINDER)
    args = p.parse_args()

    cmd = args.cmd
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        print("请在 -- 之后提供要执行的命令，例如: python prototypes/python-dashboard/cli_wrapper.py --task-id x --node y --cli codex -- codex run ...")
        sys.exit(1)

    buf = collections.deque(maxlen=200)
    last_output_time = time.time()

    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)

    def reader():
        nonlocal last_output_time
        for line in proc.stdout:
            print(line, end="")  # 原样透传，你在终端里还是能看到完整输出
            buf.append(line.rstrip("\n"))
            last_output_time = time.time()

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    report(args.task_id, args.node, args.cli, "刚启动，等待输出", status="idle")

    while proc.poll() is None:
        time.sleep(INTERVAL_SECONDS)
        if not buf:
            continue
        if time.time() - last_output_time > IDLE_TIMEOUT:
            report(args.task_id, args.node, args.cli, "长时间无输出，可能已挂起或在等待输入", status="warn")
            continue
        summary, status = summarize_with_haiku(list(buf))
        report(args.task_id, args.node, args.cli, summary, status=status)

    exit_code = proc.wait()
    final_status = "err" if exit_code != 0 else "idle"
    final_summary = f"进程已结束（退出码 {exit_code}）"
    report(args.task_id, args.node, args.cli, final_summary, status=final_status, history_line=final_summary)


if __name__ == "__main__":
    main()
