#!/usr/bin/env python3
"""Write a local demo session status without invoking CodeBuddy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import time


parser = argparse.ArgumentParser()
parser.add_argument(
    "state", choices=["idle", "working", "waiting", "completed", "error"]
)
args = parser.parse_args()

path = Path.home() / ".codebuddy-light" / "sessions" / "demo.json"
path.parent.mkdir(parents=True, exist_ok=True)
if args.state == "idle":
    path.unlink(missing_ok=True)
else:
    path.write_text(
        json.dumps(
            {
                "state": args.state,
                "message": "Local demo",
                "timestamp": int(time.time()),
            }
        ),
        encoding="utf-8",
    )

