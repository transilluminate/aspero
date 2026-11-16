#!/usr/bin/env python3
"""Fail on repository paths and DESIGN sections cited but not there.

    scripts/doc-paths.py

Agents find their way by grep and DESIGN.md, so a path or section the
docs cite that a move left behind sends them looking for nothing. This
reads DESIGN.md, README.md, docs/**/*.md, bench/**/*.md, the `## Unreleased`
section of CHANGELOG.md (older entries, and the archived ones in
docs/changelog/, describe the tree as it was), and the `//`, `///` and `//!`
comments of crates/**/*.rs, and checks:

- Paths: `crates/core/src/work.rs`, `scripts/redteam.py`, `bench/...`,
  `docs/...`, a Markdown link's target, and partial ones (`matching/mod.rs`,
  `` `work.rs` ``) must exist. A path under one of the top-level directories
  (ROOTS) must exist from the repository root; any other must exist from
  the citing file's directory, its crate, the root, or as the tail of a
  tracked path. Paths under target/, URLs, DOIs, patterns (`*`, `{a,b}`,
  `<name>`), files a script makes when first needed (MADE) and `file:line`
  citations (pinned to the release they were written at) are skipped.
- Sections: `DESIGN "Storage"`, `DESIGN.md's "Storage"`, `DESIGN, "Storage"`,
  `DESIGN ("Storage")`, `DESIGN.md's Storage section`, `(DESIGN, Principles)`
  and `DESIGN.md#storage` must name a `##` or `###` heading of DESIGN.md (or
  the start of one).

Prints each miss as `file:line: ...` and exits 1 if there is any.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ROOTS = ("crates", "scripts", "bench", "docs", "fuzz", ".github")
EXTENSIONS = "rs|py|sh|md|jsonl|json|toml|yml|yaml|swift"
# A bare file name in backticks is checked only with these (`Work.json` is
# a field, `aspero_ffi.swift` a generated file).
BARE_EXTENSIONS = "rs|py|sh|md|jsonl|toml"
# Files a script makes the first time it needs them.
MADE = {"bench/redteam/reports.jsonl"}
# Released changelog entries, archived as written: not checked.
ARCHIVE = "docs/changelog/"

# A path-like token: word characters, dots, dashes and slashes, plus the
# pattern characters that make it a pattern rather than a path.
TOKEN = re.compile(r"(?<![\w./:@$~-])[\w.{}*<>,-]*(?:/[\w.{}*<>,-]*)*")
LINK = re.compile(r"\]\(([^)\s]+)\)")
BACKTICK = re.compile(r"`([^`\s]+)`")
HAS_EXT = re.compile(rf"\.(?:{EXTENSIONS})$")
HAS_BARE_EXT = re.compile(rf"\.(?:{BARE_EXTENSIONS})$")

QUOTED = re.compile(
    r"DESIGN(?:\.md)?(?:'s|,)?\s+\(?[\"“]([^\"”]+)[\"”]"
)
UNQUOTED = [
    re.compile(r"DESIGN(?:\.md)?'s\s+([A-Z][\w ,()-]*?)\s+section\b"),
    re.compile(r"\(DESIGN(?:\.md)?,\s+([A-Z][\w -]*?)\)"),
]
ANCHOR = re.compile(r"DESIGN\.md#([\w-]+)")


def tracked():
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split("\n")
    files = {f for f in out if f and (ROOT / f).exists()}
    dirs = {str(p) for f in files for p in pathlib.PurePosixPath(f).parents}
    return files, dirs - {"."}


FILES, DIRS = tracked()


def headings():
    found = []
    for line in (ROOT / "DESIGN.md").read_text(encoding="utf-8").splitlines():
        m = re.match(r"#{2,3}\s+(.*)", line)
        if m:
            found.append(m.group(1).strip())
    return found


HEADINGS = headings()


def slug(heading):
    s = re.sub(r"[^\w\s-]", "", heading.lower())
    return re.sub(r"\s", "-", s)


def section_exists(name):
    name = " ".join(name.split()).lower()
    return any(h.lower().startswith(name) for h in HEADINGS)


def exists(rel):
    rel = rel.rstrip("/")
    return rel in FILES or rel in DIRS


def resolves(token, source):
    """Whether `token`, cited in `source` (repository-relative), exists."""
    path = token.rstrip("/")
    if path.split("/", 1)[0] in ROOTS:
        return exists(path)
    here = pathlib.PurePosixPath(source).parent
    bases = [here, pathlib.PurePosixPath(".")]
    parts = pathlib.PurePosixPath(source).parts
    if parts[0] == "crates" and len(parts) > 2:
        crate = pathlib.PurePosixPath(*parts[:2])
        bases += [crate, crate / "src"]
    for base in bases:
        joined = pathlib.PurePosixPath(*(base / path).parts)
        # Resolve `..` lexically.
        norm = []
        for part in joined.parts:
            if part == "..":
                if not norm:
                    break
                norm.pop()
            elif part != ".":
                norm.append(part)
        else:
            if norm and exists("/".join(norm)):
                return True
    tail = "/" + path
    return any(f.endswith(tail) for f in FILES) or any(
        d.endswith(tail) for d in DIRS
    )


def skip(token):
    return (
        not token
        or token in MADE
        or any(c in token for c in "{}*<>,")
        or "..." in token
        or token.startswith(("target/", "/", "~"))
        or re.match(r"10\.\d{4,}/", token)
        or re.search(r"\w\.(?:com|org|net|io|ai|gov|edu)(?:/|$)", token)
    )


def candidates(line, markdown):
    """Path-like tokens on one line of prose or comment."""
    seen = []
    if markdown:
        for m in LINK.finditer(line):
            target = m.group(1).split("#", 1)[0]
            if target and not re.match(r"[a-z]+:", target):
                seen.append(target)
    for m in BACKTICK.finditer(line):
        tok = m.group(1).rstrip(".,;:")
        if HAS_BARE_EXT.search(tok) and "/" not in tok and not tok.startswith("."):
            seen.append(tok)
    for m in TOKEN.finditer(line):
        tok = m.group(0).rstrip(".,;:")
        if "/" not in tok:
            continue
        # A URL (`https://...`) or a `//` comment marker isn't a path.
        if line[m.end():m.end() + 2] == "//" or "://" in line[max(0, m.start() - 3):m.start() + 3]:
            continue
        top = tok.split("/", 1)[0]
        if top in ROOTS or HAS_EXT.search(tok):
            seen.append(tok)
    return [t for t in seen if not skip(t)]


def check_sections(text, lines_at, source, problems):
    def line_of(pos):
        return lines_at(text.count("\n", 0, pos))

    for pattern in [QUOTED, *UNQUOTED]:
        for m in pattern.finditer(text):
            if not section_exists(m.group(1)):
                name = " ".join(m.group(1).split())
                problems.append(f"{source}:{line_of(m.start())}: DESIGN section "
                                f"\"{name}\" is not a heading of DESIGN.md")
    for m in ANCHOR.finditer(text):
        if m.group(1) not in {slug(h) for h in HEADINGS}:
            problems.append(f"{source}:{line_of(m.start())}: DESIGN.md#{m.group(1)} "
                            f"is not a heading's anchor")


def check(source, numbered, markdown, problems):
    """`numbered`: (line number, text) pairs to read."""
    for n, line in numbered:
        for tok in candidates(line, markdown):
            if not resolves(tok, source):
                problems.append(f"{source}:{n}: {tok} does not exist")
    text = "\n".join(line for _, line in numbered)
    check_sections(text, lambda i: numbered[i][0], source, problems)


def markdown_files():
    names = ["DESIGN.md", "README.md"]
    names += sorted(
        f for f in FILES
        if re.match(r"(docs|bench)/.*\.md$", f) and not f.startswith(ARCHIVE)
    )
    for name in names:
        lines = (ROOT / name).read_text(encoding="utf-8").splitlines()
        yield name, list(enumerate(lines, 1))
    lines = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8").splitlines()
    unreleased, inside = [], False
    for n, line in enumerate(lines, 1):
        if line.startswith("## "):
            inside = line.strip() == "## Unreleased"
            continue
        if inside:
            unreleased.append((n, line))
    yield "CHANGELOG.md", unreleased


def comment(line):
    """The `//` comment on a line of Rust, if any (not one in a string)."""
    stripped = line.lstrip()
    if stripped.startswith("//"):
        return stripped.lstrip("/!").strip()
    in_str = False
    for i, c in enumerate(line):
        if c == '"' and (i == 0 or line[i - 1] != "\\"):
            in_str = not in_str
        elif not in_str and line.startswith("//", i):
            return line[i:].lstrip("/!").strip()
    return ""


def rust_files():
    for name in sorted(f for f in FILES if f.startswith("crates/") and f.endswith(".rs")):
        lines = (ROOT / name).read_text(encoding="utf-8").splitlines()
        yield name, [(n, comment(line)) for n, line in enumerate(lines, 1)]


def main():
    problems = []
    for name, numbered in markdown_files():
        check(name, numbered, True, problems)
    for name, numbered in rust_files():
        check(name, numbered, False, problems)
    for p in problems:
        print(p)
    if problems:
        print(f"{len(problems)} stale reference(s): point them at the path or "
              "heading as it is now")
        return 1
    print("doc paths ok: every cited path and DESIGN section exists")
    return 0


if __name__ == "__main__":
    sys.exit(main())
