#!/usr/bin/env python3
"""
命令行上报工具。给 Hermes / Anima 这类自研 agent 的心跳机制直接调用。

用法示例（在 heartbeat 逻辑里 shell out 一下即可）：
    python report_cli.py \
        --task-id noos-node1-election-fix \
        --node gejifengai \
        --cli claude \
        --summary "正在重构 Bully 选举模块的心跳超时逻辑" \
        --status ok

也可以直接 import status_store.report() 在 Python 里调用，效果一样，
命令行版本是为了方便非 Python 的 agent（比如 shell 脚本心跳）调用。
"""

import argparse
from status_store import report


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--task-id", required=True)
    p.add_argument("--node", required=True)
    p.add_argument("--cli", required=True, help="claude / codex / hermes 等")
    p.add_argument("--summary", required=True, help="一句话，当前在做什么")
    p.add_argument("--status", default="ok", choices=["ok", "warn", "err", "idle"])
    p.add_argument("--history-line", default=None, help="可选，写入历史记录的一行文字")
    args = p.parse_args()

    report(
        task_id=args.task_id,
        node=args.node,
        cli=args.cli,
        summary=args.summary,
        status=args.status,
        history_line=args.history_line,
    )
    print(f"已上报: {args.task_id} -> {args.summary}")


if __name__ == "__main__":
    main()
