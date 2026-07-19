#!/usr/bin/env python3
"""Convert Claude Code / Codex lifecycle JSON into Agent Dashboard events.

The helper only reports to 127.0.0.1 and fails silently so it never blocks the agent.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request


def read_event() -> dict:
    json_arg = next((arg for arg in sys.argv[1:] if arg.lstrip().startswith("{")), None)
    if json_arg:
        try:
            return json.loads(json_arg)
        except json.JSONDecodeError:
            return {}
    try:
        if not sys.stdin.isatty():
            return json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        pass
    return {}


def event_name(data: dict, explicit: str | None) -> str:
    return explicit or str(
        data.get("hook_event_name")
        or data.get("hookEventName")
        or data.get("type")
        or "unknown"
    )


def event_summaries(data: dict, event: str) -> tuple[str | None, str | None, str | None]:
    assistant = data.get("last_assistant_message") or data.get("last-assistant-message")
    if assistant:
        # Assistant content is user data in an unknown language; preserve it as a legacy fallback.
        return str(assistant).strip(), None, None
    tool = data.get("tool_name") or data.get("tool-name")
    tool_input = data.get("tool_input") or {}
    if "permission" in event.lower() or "approval" in event.lower():
        detail = tool_input.get("description") if isinstance(tool_input, dict) else None
        target = detail or tool
        return None, f"等待确认：{target or 'agent 操作'}", f"Waiting for approval: {target or 'agent action'}"
    if "failure" in event.lower() or "error" in event.lower():
        error = data.get("error") or data.get("reason")
        return (
            None,
            f"执行出错：{error}" if error else "agent 执行出错，需要检查",
            f"Execution error: {error}" if error else "The agent encountered an error and needs attention",
        )
    if tool and ("tool" in event.lower()):
        return None, f"正在调用 {tool}", f"Calling {tool}"
    if "prompt" in event.lower():
        return None, "已收到新任务，正在处理", "New task received; processing it now"
    return None, None, None


def external_session_id(data: dict) -> str | None:
    value = (
        data.get("session_id")
        or data.get("session-id")
        or data.get("thread_id")
        or data.get("thread-id")
    )
    return str(value).strip() if value else None


def session_title(data: dict) -> str | None:
    value = (
        data.get("session_name")
        or data.get("session-name")
        or data.get("thread_name")
        or data.get("thread-name")
        or data.get("session_title")
    )
    return str(value).strip() if value else None


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--agent", default=os.getenv("AGENT_DASHBOARD_AGENT", "generic"))
    parser.add_argument("--event")
    parser.add_argument("--statusline", action="store_true")
    args, _ = parser.parse_known_args(
        [arg for arg in sys.argv[1:] if not arg.lstrip().startswith("{")]
    )
    data = read_event()
    session_id = os.getenv("AGENT_DASHBOARD_SESSION_ID")
    url = os.getenv("AGENT_DASHBOARD_EVENT_URL")
    if not session_id or not url:
        if args.statusline:
            print(session_title(data) or "")
        return 0
    event = event_name(data, args.event)
    summary, summary_zh_cn, summary_en = event_summaries(data, event)
    body = {
        "dashboard_session_id": session_id,
        "agent": args.agent,
        "event": event,
        "summary": summary,
        "summary_zh_cn": summary_zh_cn,
        "summary_en": summary_en,
        "agent_session_id": external_session_id(data),
        "session_title": session_title(data),
    }
    request = urllib.request.Request(
        url,
        data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=0.8):
            pass
    except Exception:
        pass
    if args.statusline:
        print(session_title(data) or "")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
