#!/usr/bin/env python3
"""Inspect, export and merge HTTP tapes (`ASPERO_HTTP_TAPE`).

    scripts/tape.py stats [TAPE...]       entries, statuses and size per host
    scripts/tape.py export TAPE DEST      one self-contained file (no WAL)
    scripts/tape.py import SRC TAPE       add SRC's entries TAPE lacks

With no TAPE, `stats` reads target/bench-tape.sqlite and
target/redteam-tape.sqlite, where they exist. A tape is one SQLite file
(plus `-wal`/`-shm` while in use); `export` writes a clean copy to share or
keep, and a plain copy of the file works too once nothing is using it.
Tapes hold no API keys or email addresses (aspero strips them before
recording), so they can be shared. "Recorded cost" is what OpenAlex charged
for the requests when they were recorded: what each replay saves.
"""

import argparse
import json
import sqlite3
import sys
from collections import defaultdict
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent.parent
DEFAULTS = [ROOT / "target/bench-tape.sqlite", ROOT / "target/redteam-tape.sqlite"]


def open_tape(path, must_exist=True):
    if must_exist and not Path(path).exists():
        sys.exit(f"{path}: no such tape")
    return sqlite3.connect(path)


def cost(headers):
    for name, value in json.loads(headers):
        if name == "x-ratelimit-cost-usd":
            try:
                return float(value)
            except ValueError:
                return 0.0
    return 0.0


def stats(paths):
    for path in paths:
        db = open_tape(path)
        hosts = defaultdict(lambda: {"n": 0, "bytes": 0, "cost": 0.0, "status": defaultdict(int)})
        for url, status, size, headers in db.execute(
            "SELECT url, status, length(response), headers FROM tape"
        ):
            h = hosts[urlsplit(url).netloc]
            h["n"] += 1
            h["bytes"] += size or 0
            h["cost"] += cost(headers)
            h["status"][status] += 1
        first, last = db.execute("SELECT min(recorded_at), max(recorded_at) FROM tape").fetchone()
        db.close()
        size = sum(p.stat().st_size for p in Path(path).parent.glob(Path(path).name + "*"))
        print(f"{path}  ({size / 1e6:.1f} MB on disk)")
        if not hosts:
            print("  empty\n")
            continue
        print(f"  {'host':<28}{'entries':>9}{'MB':>9}{'recorded cost':>15}  statuses")
        for host in sorted(hosts, key=lambda h: -hosts[h]["n"]):
            h = hosts[host]
            statuses = " ".join(f"{s}:{n}" for s, n in sorted(h["status"].items()))
            print(f"  {host:<28}{h['n']:>9}{h['bytes'] / 1e6:>9.2f}{'$' + format(h['cost'], '.4f'):>15}  {statuses}")
        total = sum(h["n"] for h in hosts.values())
        spent = sum(h["cost"] for h in hosts.values())
        span = f"{first} .. {last}" if first is not None else "-"
        print(f"  {'total':<28}{total:>9}{'':>9}{'$' + format(spent, '.4f'):>15}  recorded (unix s) {span}\n")


def export(src, dest):
    if Path(dest).exists():
        sys.exit(f"{dest} exists")
    db = open_tape(src)
    db.execute("VACUUM INTO ?", (str(dest),))
    db.close()
    print(f"exported {src} to {dest}")


def merge(src, dest):
    open_tape(src).close()
    db = open_tape(dest, must_exist=False)
    db.execute("""CREATE TABLE IF NOT EXISTS tape (
        key TEXT PRIMARY KEY, method TEXT NOT NULL, url TEXT NOT NULL, body BLOB,
        status INTEGER NOT NULL, headers TEXT NOT NULL, response BLOB NOT NULL,
        recorded_at INTEGER NOT NULL)""")
    db.execute("ATTACH DATABASE ? AS src", (str(src),))
    before = db.total_changes
    with db:
        db.execute("INSERT OR IGNORE INTO tape SELECT key, method, url, body, status, headers, response, recorded_at FROM src.tape")
    added = db.total_changes - before
    db.execute("DETACH DATABASE src")
    db.close()
    print(f"added {added} entries from {src} to {dest}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("stats", help="entries, statuses and size per host")
    s.add_argument("tapes", nargs="*", type=Path)
    e = sub.add_parser("export", help="write a self-contained copy")
    e.add_argument("tape", type=Path)
    e.add_argument("dest", type=Path)
    i = sub.add_parser("import", help="add another tape's entries")
    i.add_argument("src", type=Path)
    i.add_argument("tape", type=Path)
    args = ap.parse_args()
    if args.cmd == "stats":
        paths = args.tapes or [p for p in DEFAULTS if p.exists()]
        if not paths:
            sys.exit("no tapes: run `just bench` or scripts/redteam.py first, or name one")
        stats(paths)
    elif args.cmd == "export":
        export(args.tape, args.dest)
    else:
        merge(args.src, args.tape)


if __name__ == "__main__":
    main()
