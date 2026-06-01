#!/usr/bin/env python3
"""Write a CodeBuddy hook event to a per-session status file."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
from typing import Any


SESSIONS_DIR = Path.home() / ".codebuddy-light" / "sessions"
ASK_TOOL_NAMES = {
    "askuserquestion",
    "askuser",
    "requestuserinput",
    "elicitation",
}
QUESTION_CUE = re.compile(
    r"(请选择|请确认|选择一个|选项|你希望|你想|是否|要不要|哪个|哪种|回复|"
    r"choose|select|confirm|which)",
    re.IGNORECASE,
)
OPTION_LINE = re.compile(r"(?m)^\s*(?:[-*]\s+|\d+[.)、]\s*|[A-Da-d][.)、]\s*)")


def read_event() -> dict[str, Any]:
    try:
        value = json.load(sys.stdin)
        return value if isinstance(value, dict) else {}
    except json.JSONDecodeError:
        return {}


def safe_session_id(event: dict[str, Any]) -> str:
    raw = (
        event.get("session_id")
        or event.get("conversation_id")
        or os.environ.get("CODEBUDDY_SESSION_ID")
        or event.get("transcript_path")
        or os.getcwd()
    )
    value = str(raw)
    readable = re.sub(r"[^a-zA-Z0-9._-]+", "-", value).strip("-")
    if readable and len(readable) <= 96:
        return readable
    return hashlib.sha256(value.encode("utf-8")).hexdigest()[:24]


def write_json_atomic(path: Path, content: dict[str, Any]) -> None:
    SESSIONS_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=SESSIONS_DIR, delete=False
    ) as handle:
        json.dump(content, handle, ensure_ascii=False, separators=(",", ":"))
        temporary_path = Path(handle.name)
    temporary_path.replace(path)


def normalized_tool_name(event: dict[str, Any]) -> str:
    return re.sub(r"[^a-z0-9]+", "", str(event.get("tool_name", "")).lower())


def truthy(value: Any) -> bool:
    return value is True or str(value).lower() in {"1", "true", "yes"}


def collect_text(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        return [text for item in value for text in collect_text(item)]
    if isinstance(value, dict):
        return [
            text
            for key, item in value.items()
            if key in {"content", "message", "text"}
            for text in collect_text(item)
        ]
    return []


def assistant_text(record: Any) -> str:
    if not isinstance(record, dict):
        return ""
    message = record.get("message")
    message_role = message.get("role") if isinstance(message, dict) else None
    if record.get("type") != "assistant" and record.get("role") != "assistant":
        if message_role != "assistant":
            return ""
    return "\n".join(collect_text(message or record.get("content") or record))


def latest_assistant_text(event: dict[str, Any]) -> str:
    transcript_path = Path(str(event.get("transcript_path", "")))
    if not transcript_path.is_file():
        return ""
    try:
        content = transcript_path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return ""

    try:
        records = json.loads(content)
        if not isinstance(records, list):
            records = [records]
    except json.JSONDecodeError:
        records = []
        for line in content.splitlines():
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                continue

    for record in reversed(records):
        text = assistant_text(record)
        if text:
            return text
    return ""


def stop_waits_for_user(event: dict[str, Any]) -> bool:
    text = latest_assistant_text(event).strip()
    if not text or not QUESTION_CUE.search(text):
        return False
    return len(OPTION_LINE.findall(text)) >= 2 or text.endswith(("?", "？"))


def adjust_state(event: dict[str, Any], state: str, message: str) -> tuple[str, str]:
    event_name = str(event.get("hook_event_name", ""))
    if event_name == "PreToolUse":
        tool_input = event.get("tool_input")
        if normalized_tool_name(event) in ASK_TOOL_NAMES:
            return "waiting", "等待选择"
        if isinstance(tool_input, dict) and truthy(tool_input.get("requires_approval")):
            return "waiting", "等待权限确认"
    elif event_name == "PostToolUse":
        tool_response = event.get("tool_response")
        if isinstance(tool_response, dict):
            exit_code = tool_response.get("exitCode", tool_response.get("exit_code"))
            if truthy(tool_response.get("is_error")) or (
                isinstance(exit_code, int) and exit_code != 0
            ):
                return "error", "工具执行失败"
    elif event_name == "Stop" and stop_waits_for_user(event):
        return "waiting", "等待选择或确认"
    return state, message


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--state", required=True)
    parser.add_argument("--message", default="")
    parser.add_argument("--notification-only", action="store_true")
    args = parser.parse_args()

    event = read_event()
    if args.notification_only:
        notification_type = str(event.get("notification_type", ""))
        if notification_type == "permission_prompt":
            args.state = "waiting"
            args.message = "等待权限确认"
        elif notification_type == "elicitation_dialog":
            args.state = "waiting"
            args.message = "等待补充信息"
        elif notification_type == "idle_prompt":
            args.state = "completed"
            args.message = "回复完成"
        else:
            return

    args.state, args.message = adjust_state(event, args.state, args.message)
    path = SESSIONS_DIR / f"{safe_session_id(event)}.json"
    if args.state == "idle":
        path.unlink(missing_ok=True)
        return

    write_json_atomic(
        path,
        {
            "state": args.state,
            "message": args.message,
            "cwd": str(event.get("cwd") or os.getcwd()),
            "timestamp": int(time.time()),
        },
    )


if __name__ == "__main__":
    main()
