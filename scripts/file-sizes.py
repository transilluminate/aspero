#!/usr/bin/env python3
"""Fail on Rust source files grown too long to take in whole.

    scripts/file-sizes.py

Counts the lines of every crate's `src/**/*.rs` (each `Cargo.toml` under
`crates/`, however deep, so `crates/providers/*` too) up to its first top-level
`#[cfg(test)]` (the whole file if it has none), so a file's unit tests
don't count against it. Test directories (`tests/`), `fuzz/` and generated
files (`@generated` in the first lines) are skipped.

A file over LIMIT lines fails the check unless `scripts/file-sizes.allow`
lists it, one `path  # reason` per line; one over WARN lines is reported
but passes. An allow-listed file that has shrunk below LIMIT is reported
too, so the list doesn't keep stale entries. Exits 1 on any failure.
"""

import pathlib
import sys

LIMIT = 1000
WARN = 800

ROOT = pathlib.Path(__file__).resolve().parent.parent
ALLOW = ROOT / "scripts" / "file-sizes.allow"


def code_lines(path):
    """Lines before the first top-level `#[cfg(test)]`."""
    count = 0
    with path.open(encoding="utf-8") as f:
        for line in f:
            if line.startswith("#[cfg(test)]"):
                break
            count += 1
    return count


def generated(path):
    with path.open(encoding="utf-8") as f:
        head = [next(f, "") for _ in range(5)]
    return any("@generated" in line for line in head)


def sources():
    crates = (ROOT / "crates").rglob("Cargo.toml")
    paths = {p for c in crates for p in (c.parent / "src").glob("**/*.rs")}
    for path in sorted(paths):
        rel = path.relative_to(ROOT)
        if "tests" in rel.parts or "fuzz" in rel.parts or generated(path):
            continue
        yield rel, path


def allow_list():
    allowed = {}
    if not ALLOW.exists():
        return allowed
    for n, raw in enumerate(ALLOW.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        path, sep, reason = line.partition("#")
        if not sep or not reason.strip():
            sys.exit(f"{ALLOW.relative_to(ROOT)}:{n}: needs `path  # reason`")
        allowed[path.strip()] = reason.strip()
    return allowed


def main():
    allowed = allow_list()
    failures, warnings, stale = [], [], []
    seen = list(sources())
    # The provider crates sit a level deeper; a glob that misses them
    # passes silently, so insist on seeing them.
    if not any(rel.parts[:2] == ("crates", "providers") for rel, _ in seen):
        sys.exit("error: no source under crates/providers/ was counted")
    for rel, path in seen:
        n = code_lines(path)
        key = rel.as_posix()
        if n > LIMIT:
            if key not in allowed:
                failures.append((key, n))
        elif key in allowed:
            stale.append((key, n))
        elif n > WARN:
            warnings.append((key, n))
    stale += [(p, None) for p in allowed if not (ROOT / p).is_file()]

    for key, n in warnings:
        print(f"warning: {key}: {n} lines (limit {LIMIT})")
    for key, n in stale:
        what = "missing" if n is None else f"{n} lines, under the limit"
        print(f"error: {key} is allow-listed but {what}: drop it from "
              f"{ALLOW.relative_to(ROOT)}")
    for key, n in failures:
        print(f"error: {key}: {n} lines before its tests (limit {LIMIT})")
    if failures:
        print(f"fix: split by concern, or allow-list with a reason in "
              f"{ALLOW.relative_to(ROOT)} (`path  # reason`)")
    if failures or stale:
        return 1
    print(f"file sizes ok: no source over {LIMIT} lines outside the allow-list")
    return 0


if __name__ == "__main__":
    sys.exit(main())
