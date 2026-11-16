#!/usr/bin/env python3
"""Dogfoods the codebase's navigability: can a naive agent find things in it?

Usage: scripts/nav-dogfood.py [--model sonnet] [--runs 2] [--jobs 4]
                              [--only id,id] [--rev HEAD] [--out DIR]

Each task in bench/nav-tasks.jsonl is a question a developer or agent would
ask about the code, with the answer (file and function or constant) known.
The tree at --rev is exported (`git archive`, so no target/, no .git, no
local files) and each task runs there as `claude -p`: read-only tools only
(Read, Grep, Glob), no MCP servers, no user settings or CLAUDE.md, nothing
pointing at the docs. Reports per run: correctness (a right path and name;
partial: a right path only), tool calls by tool, distinct files read and
lines read and grep result lines, wasted searches (Grep and Glob calls before
the first focused touch of a right file: reading it, grepping inside it, or a
search whose results name it among at most FOCUSED files), whether DESIGN.md
or README.md was read and how many search result lines came from the docs,
turns, wall time and tokens.
Transcripts and summary.json go to --out (default under target/, ignored).
"""

import argparse
import json
import re
import subprocess
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUFFIX = "\n\nAnswer with the file path(s) and the function or constant name. Be brief."
TOOLS = "Read,Grep,Glob"
DOCS = ("DESIGN.md", "README.md")
DOC_PREFIXES = ("DESIGN.md", "README.md", "CHANGELOG.md", "docs/")
# A search lands on a right file when it names it among at most this many.
FOCUSED = 5
FILE_LINE = re.compile(r"^([\w.][\w./-]*\.\w+)(?::\d|-\d+-|$)")


def export(rev, into):
    """The tree at `rev`, as committed, in `into`/aspero."""
    tree = into / "aspero"
    tree.mkdir()
    archive = subprocess.run(["git", "-C", str(ROOT), "archive", rev],
                             capture_output=True, check=True)
    subprocess.run(["tar", "-x", "-C", str(tree)], input=archive.stdout, check=True)
    return tree


def run(task, n, args, tree, out):
    cmd = [
        "claude", "-p", task["prompt"] + SUFFIX,
        "--model", args.model,
        "--tools", TOOLS,
        "--allowedTools", *TOOLS.split(","),
        "--strict-mcp-config",
        "--setting-sources", "project",
        "--disable-slash-commands",
        "--max-turns", str(args.max_turns),
        "--output-format", "stream-json", "--verbose",
        "--no-session-persistence",
    ]
    start = time.time()
    proc = subprocess.run(cmd, cwd=tree, capture_output=True, text=True, timeout=900)
    elapsed = time.time() - start
    name = f"{task['id']}.{n}"
    (out / f"{name}.jsonl").write_text(proc.stdout)
    if proc.returncode != 0:
        (out / f"{name}.stderr").write_text(proc.stderr)
    return summarise(task, n, proc.stdout, elapsed)


def relative(path, tree):
    path = str(path or "")
    root = str(tree).removeprefix("/private") + "/"  # macOS: /var is /private/var
    return path.removeprefix("/private").removeprefix(root)


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(b.get("text", "") for b in content if isinstance(b, dict))
    return ""


def grade(task, answer):
    """'yes' when every part names one of its files and a name in it,
    'partial' when some part's file is named, else 'no'."""
    def path_in(alt):
        return alt["path"].removeprefix("crates/") in answer

    def name_in(alt):
        return any(re.search(p, answer) for p in alt["names"])

    parts = task["parts"]
    if all(any(path_in(a) and name_in(a) for a in part) for part in parts):
        return "yes"
    if any(any(path_in(a) for a in part) for part in parts):
        return "partial"
    return "no"


def files_in(body, tree):
    """The files a Grep or Glob result names (paths, or `path:line:` and
    `path-line-` lines)."""
    found = set()
    for line in body.splitlines():
        m = FILE_LINE.match(relative(line.strip(), tree))
        if m:
            found.add(m.group(1))
    return found


def summarise(task, n, transcript, elapsed=None):
    right = {a["path"] for part in task["parts"] for a in part}
    tools = Counter()
    pending = {}
    reads, greps = [], []  # in order: (path, lines), (pattern, lines, files)
    searches = 0  # Grep and Glob calls answered so far
    waste = None  # searches before the first focused touch of a right file
    doc_lines = 0  # search result lines from the docs
    answer, result, tree = "", {}, ""
    for line in transcript.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "system" and event.get("subtype") == "init":
            tree = event.get("cwd") or ""
        if event.get("type") == "result":
            result = event
            answer = event.get("result") or ""
            continue
        message = event.get("message") or {}
        for block in message.get("content") or []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use":
                name, args = block["name"], block.get("input") or {}
                tools[name] += 1
                pending[block["id"]] = (name, args)
                path = relative(args.get("file_path") or args.get("path"), tree)
                if waste is None and path in right and name in ("Read", "Grep"):
                    waste = searches
            elif block.get("type") == "tool_result":
                name, args = pending.get(block.get("tool_use_id"), ("", {}))
                body = text_of(block.get("content"))
                lines = body.count("\n") + 1 if body.strip() else 0
                if name == "Read":
                    reads.append((relative(args.get("file_path"), tree), lines))
                elif name in ("Grep", "Glob"):
                    found = files_in(body, tree)
                    doc_lines += sum(1 for l in body.splitlines()
                                     if relative(l, tree).startswith(DOC_PREFIXES))
                    if name == "Grep":
                        greps.append((args.get("pattern", ""), lines, len(found)))
                    if waste is None and found & right and len(found) <= FOCUSED:
                        waste = searches
                    searches += 1
    usage = result.get("usage") or {}
    tokens_in = sum(usage.get(k) or 0 for k in
                    ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"))
    files = sorted({p for p, _ in reads})
    return {
        "id": task["id"],
        "run": n,
        "correct": grade(task, answer),
        "calls": sum(tools.values()),
        "tools": dict(tools),
        "files": len(files),
        "files_read": files,
        "lines_read": sum(l for _, l in reads),
        "greps": greps,
        "grep_lines": sum(l for _, l, _ in greps),
        "wasted": searches if waste is None else waste,
        "landed": waste is not None,
        "docs": [d for d in DOCS if d in files],
        "doc_lines": doc_lines,
        "turns": result.get("num_turns"),
        "seconds": round(elapsed if elapsed is not None
                         else (result.get("duration_ms") or 0) / 1000),
        "tokens_in": tokens_in,
        "tokens_out": usage.get("output_tokens"),
        "cost_usd": result.get("total_cost_usd"),
        "stop": result.get("subtype"),
        "answer": answer,
    }


def doc_mark(r):
    """'R' when a doc was read, 'g' when only searches hit the docs."""
    return "R" if r["docs"] else "g" if r["doc_lines"] else "-"


def mean(xs):
    xs = list(xs)
    return sum(xs) / len(xs) if xs else 0.0


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--model", default="sonnet")
    p.add_argument("--runs", type=int, default=2)
    p.add_argument("--jobs", type=int, default=4)
    p.add_argument("--max-turns", type=int, default=25)
    p.add_argument("--only", default="")
    p.add_argument("--rev", default="HEAD")
    p.add_argument("--out", type=Path,
                   default=ROOT / "target/nav-dogfood" / time.strftime("%Y%m%d-%H%M%S"))
    p.add_argument("--tasks", type=Path, default=ROOT / "bench/nav-tasks.jsonl")
    p.add_argument("--rescore", action="store_true",
                   help="summarise the transcripts already in --out, running nothing")
    args = p.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    only = set(filter(None, args.only.split(",")))
    tasks = [json.loads(l) for l in args.tasks.read_text().splitlines() if l.strip()]
    tasks = [t for t in tasks if not only or t["id"] in only]
    commit = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", args.rev],
                            capture_output=True, text=True, check=True).stdout.strip()
    jobs = [(t, n) for t in tasks for n in range(1, args.runs + 1)]
    if args.rescore:
        old = json.loads((args.out / "summary.json").read_text())
        commit, args.model = old["commit"], old["model"]
        results = [summarise(t, n, (args.out / f"{t['id']}.{n}.jsonl").read_text())
                   for t, n in jobs if (args.out / f"{t['id']}.{n}.jsonl").exists()]
    else:
        with tempfile.TemporaryDirectory(prefix="nav-dogfood-") as tmp:
            tree = export(args.rev, Path(tmp))
            with ThreadPoolExecutor(args.jobs) as pool:
                results = list(pool.map(lambda j: run(j[0], j[1], args, tree, args.out), jobs))
    (args.out / "summary.json").write_text(json.dumps(
        {"commit": commit, "model": args.model, "results": results}, indent=2))

    print(f"{'task':<17}{'run':>3} {'ok':<8}{'calls':>5}{'R':>4}{'G':>4}{'Gl':>4}{'files':>6}"
          f"{'lines':>7}{'glines':>7}{'waste':>6}{'docs':>5}{'turns':>6}{'s':>5}{'ktok':>6}")
    for r in results:
        t = r["tools"]
        print(f"{r['id']:<17}{r['run']:>3} {r['correct']:<8}{r['calls']:>5}{t.get('Read', 0):>4}"
              f"{t.get('Grep', 0):>4}{t.get('Glob', 0):>4}{r['files']:>6}{r['lines_read']:>7}"
              f"{r['grep_lines']:>7}{r['wasted']:>6}{doc_mark(r):>5}{r['turns'] or 0:>6}"
              f"{r['seconds']:>5}{round(r['tokens_in'] / 1000):>6}")
    n = len(results)
    yes = sum(r["correct"] == "yes" for r in results)
    partial = sum(r["correct"] == "partial" for r in results)
    print(f"\n{n} runs at {commit} ({args.model}): {yes} correct, {partial} partial "
          f"({yes / n:.0%} correct); mean {mean(r['calls'] for r in results):.1f} tool calls, "
          f"{mean(r['files'] for r in results):.1f} files, "
          f"{mean(r['lines_read'] for r in results):.0f} lines read, "
          f"{mean(r['wasted'] for r in results):.1f} wasted searches, "
          f"{mean(r['turns'] or 0 for r in results):.1f} turns, "
          f"{mean(r['seconds'] for r in results):.0f}s; "
          f"docs read in {sum(bool(r['docs']) for r in results)}, "
          f"grep-hit in {sum(bool(r['doc_lines']) for r in results)}; "
          f"${sum(r['cost_usd'] or 0 for r in results):.2f}; transcripts in {args.out}")


if __name__ == "__main__":
    sys.exit(main())
