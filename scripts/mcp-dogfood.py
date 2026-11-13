#!/usr/bin/env python3
"""Dogfoods `aspero mcp`: a fresh agent session per task, the aspero tools only.

Usage: scripts/mcp-dogfood.py [--model sonnet] [--only id,id] [--out DIR] [--bin PATH]

Each task in bench/mcp-tasks.jsonl runs as `claude -p` in an empty directory,
with only the aspero MCP server (strict config, built-in tools disallowed) and
the user's settings left out. Keys and the contact address reach the server
through the environment, never a file. Reports per task: whether the expected
text appears, tool calls, tool errors, retries (a tool called again right after
it failed), and the agent's own note on friction; transcripts go to --out.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUFFIX = (
    "\n\nWhen you're done, end with a short section headed 'Tool friction:' "
    "listing anything about the aspero tools that was confusing or got in your "
    "way (descriptions, inputs, errors, results), or 'none'."
)
BUILTINS = "Bash Edit Write Read WebFetch WebSearch Glob Grep Task NotebookEdit TodoWrite"


def run(task, args, out):
    with tempfile.TemporaryDirectory() as cwd:
        config = Path(cwd) / "mcp.json"
        config.write_text(json.dumps(
            {"mcpServers": {"aspero": {"command": str(args.bin), "args": ["mcp"]}}}
        ))
        cmd = [
            "claude", "-p", task["prompt"] + SUFFIX,
            "--model", args.model,
            "--strict-mcp-config", "--mcp-config", str(config),
            "--setting-sources", "project",
            "--allowedTools", "mcp__aspero",
            "--disallowedTools", *BUILTINS.split(),
            "--output-format", "stream-json", "--verbose",
            "--no-session-persistence",
        ]
        start = time.time()
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=900)
        elapsed = time.time() - start
    (out / f"{task['id']}.jsonl").write_text(proc.stdout)
    if proc.returncode != 0:
        (out / f"{task['id']}.stderr").write_text(proc.stderr)
    return summarise(task, proc.stdout, elapsed)


def summarise(task, transcript, elapsed):
    calls, errors, retries, answer = [], 0, 0, ""
    pending = {}
    last_failed = None
    for line in transcript.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        message = event.get("message") or {}
        for block in message.get("content") or []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use":
                if not block["name"].startswith("mcp__aspero__"):
                    continue  # the harness's own, e.g. loading tool schemas
                name = block["name"].removeprefix("mcp__aspero__")
                calls.append(name)
                pending[block["id"]] = name
                if last_failed == name:
                    retries += 1
            elif block.get("type") == "tool_result":
                name = pending.get(block.get("tool_use_id"))
                failed = bool(block.get("is_error"))
                errors += failed
                last_failed = name if failed else None
        if event.get("type") == "result":
            answer = event.get("result") or ""
    found = all(e.lower() in answer.lower() for e in task.get("expect", [])) and all(
        t in calls for t in task.get("tools", [])
    )
    friction = answer.split("Tool friction:", 1)[1].strip() if "Tool friction:" in answer else ""
    return {
        "id": task["id"],
        "solved": found,
        "calls": len(calls),
        "tools": calls,
        "errors": errors,
        "retries": retries,
        "seconds": round(elapsed),
        "friction": friction,
    }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--model", default="sonnet")
    p.add_argument("--only", default="")
    p.add_argument("--out", type=Path, default=Path(tempfile.mkdtemp(prefix="mcp-dogfood-")))
    p.add_argument("--bin", type=Path, default=ROOT / "target/release/aspero")
    p.add_argument("--tasks", type=Path, default=ROOT / "bench/mcp-tasks.jsonl")
    args = p.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    only = set(filter(None, args.only.split(",")))
    tasks = [json.loads(l) for l in args.tasks.read_text().splitlines() if l.strip()]
    results = []
    for task in tasks:
        if only and task["id"] not in only:
            continue
        r = run(task, args, args.out)
        results.append(r)
        mark = "PASS" if r["solved"] else "MISS"
        print(f"{mark} {r['id']:<14} calls={r['calls']:<2} errors={r['errors']} "
              f"retries={r['retries']} {r['seconds']}s  {' '.join(r['tools'])}", flush=True)
        if r["friction"] and r["friction"].lower().rstrip(".") != "none":
            print(f"     friction: {r['friction'][:400]}", flush=True)
    (args.out / "summary.json").write_text(json.dumps(results, indent=2))
    solved = sum(r["solved"] for r in results)
    print(f"\n{solved}/{len(results)} solved; "
          f"{sum(r['calls'] for r in results)} calls, {sum(r['errors'] for r in results)} errors, "
          f"{sum(r['retries'] for r in results)} retries; transcripts in {args.out}")


if __name__ == "__main__":
    sys.exit(main())
