"""
状态存储核心模块。

设计原则：
- 单文件 JSON 存储，够用为止，量大了再换 SQLite。
- 每个任务用 task_id 作为主键（建议格式：{node}-{task}，例如 node-a100-1-vllm-deploy）。
- 摘要只有在实质变化时才更新时间戳，避免前端一直"闪"。
- 这是唯一的 schema 定义，agent 上报脚本 / 第三方 CLI wrapper / 前端都读写同一份。
"""

import json
import os
import time
import threading
from pathlib import Path
from typing import Optional, Literal

STORE_PATH = Path(os.environ.get("AGENT_DASHBOARD_STORE", str(Path.home() / ".agent_dashboard" / "status.json")))
STORE_PATH.parent.mkdir(parents=True, exist_ok=True)

_lock = threading.Lock()

Status = Literal["ok", "warn", "err", "idle"]

# 判定"迟缓"的阈值：摘要超过这么久没更新，前端应该把它标黄
STALE_SECONDS = 180


def _read() -> dict:
    if not STORE_PATH.exists():
        return {}
    try:
        with open(STORE_PATH, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, FileNotFoundError):
        return {}


def _write(data: dict):
    tmp = STORE_PATH.with_suffix(".tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    tmp.replace(STORE_PATH)  # 原子写，避免前端读到半截文件


def report(
    task_id: str,
    node: str,
    cli: str,
    summary: str,
    status: Status = "ok",
    history_line: Optional[str] = None,
):
    """
    agent / wrapper 调用这个函数上报状态。

    task_id: 唯一标识，建议 "{node}-{task}"
    node:    机器名，例如 "gejifengai" / "ROG-XUBUNTU"
    cli:     "claude" / "codex" / "hermes" 等
    summary: 一句话，当前在做什么
    status:  ok / warn / err / idle
    history_line: 可选，追加到该任务的历史记录里（用于点开详情时展示）
    """
    with _lock:
        data = _read()
        now = time.time()
        prev = data.get(task_id)

        # 摘要没变化就不刷新 updated_at，只刷新 last_seen（用于判断是否真的还活着）
        summary_changed = prev is None or prev.get("summary") != summary
        entry = prev.copy() if prev else {"history": []}

        entry["node"] = node
        entry["cli"] = cli
        entry["summary"] = summary
        entry["status"] = status
        entry["last_seen"] = now
        if summary_changed:
            entry["updated_at"] = now
            if history_line:
                entry["history"].append(f"[{time.strftime('%H:%M:%S')}] {history_line}")
            else:
                entry["history"].append(f"[{time.strftime('%H:%M:%S')}] {summary}")
            entry["history"] = entry["history"][-50:]  # 只留最近 50 条

        data[task_id] = entry
        _write(data)


def get_all() -> dict:
    """给前端 API 用：读取全部任务状态，并自动把长时间没更新的标记为迟缓。"""
    with _lock:
        data = _read()
        now = time.time()
        for entry in data.values():
            if entry["status"] == "ok" and now - entry["last_seen"] > STALE_SECONDS:
                entry["status"] = "warn"
                entry["stale"] = True
        return data


def remove(task_id: str):
    with _lock:
        data = _read()
        data.pop(task_id, None)
        _write(data)


if __name__ == "__main__":
    # 简单自测
    report("demo-node-task1", node="demo-node", cli="claude", summary="正在调研 API 文档", status="ok")
    print(json.dumps(get_all(), ensure_ascii=False, indent=2))
