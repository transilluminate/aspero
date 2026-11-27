#!/usr/bin/env python3
"""Fail on an aspero diagnostics event whose message isn't its name.

    scripts/event-names.py

DESIGN "Diagnostics events": an event is one line, its name and then
`key=value` fields, so a log reader takes the first word as the name.
Every `tracing::{info,warn,error}!` with an `aspero::` target (or none, in
an aspero crate) must have a snake_case name as its message
(`mcp_listening`, `record_undecodable`), never a sentence. Debug events
and `aspero::tape` (a developer tool that the CLI alone reads, naming
requests on purpose) are left out, and so are tests (`tests/`,
`#[cfg(test)]` modules). Every name must also be in DESIGN's
"Diagnostics events" section, as `` `name` ``, so a new event is listed
where an embedder reads the list. Exits 1 on any failure.
"""

import pathlib
import re
import sys

CALL = re.compile(r"tracing::(info|warn|error)!\((.*?)\);", re.S)
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')
NAME = re.compile(r"[a-z][a-z0-9_]*")


def source(path):
    """The file up to its first top-level `#[cfg(test)]`."""
    text = path.read_text()
    at = text.find("\n#[cfg(test)]")
    return text if at < 0 else text[:at]


def listed():
    """The `names` in DESIGN's "Diagnostics events" section."""
    design = pathlib.Path("DESIGN.md").read_text()
    start = design.index("\n## Diagnostics events\n")
    end = design.find("\n## ", start + 1)
    return set(re.findall(r"`([a-z][a-z0-9_]*)`", design[start:end]))


def main():
    bad = []
    names = {}
    for path in sorted(pathlib.Path("crates").rglob("src/**/*.rs")):
        text = source(path)
        for call in CALL.finditer(text):
            body = call.group(2)
            target = re.search(r'target:\s*"([^"]+)"', body)
            if target and (
                not target.group(1).startswith("aspero") or target.group(1) == "aspero::tape"
            ):
                continue
            literals = LITERAL.findall(body)
            # The message is the last literal, after the fields.
            message = literals[-1] if literals else ""
            if target and message == target.group(1):
                message = ""
            # A name passed in (`"{what}"`): its callers are checked below.
            if re.fullmatch(r"\{[a-z_]+\}", message):
                continue
            line = text[: call.start()].count("\n") + 1
            if not NAME.fullmatch(message):
                bad.append(f"{path}:{line}: {call.group(1)} {message!r}")
            else:
                names.setdefault(message, f"{path}:{line}")
    # The names passed to `look_up`/`failed_lookups` as `what`.
    for path in sorted(pathlib.Path("crates").rglob("src/**/*.rs")):
        text = source(path)
        for call in re.finditer(r"\.look_up\([^;]*?\"([^\"]*)\"\s*\)", text, re.S):
            line = text[: call.start()].count("\n") + 1
            if not NAME.fullmatch(call.group(1)):
                bad.append(f"{path}:{line}: look_up {call.group(1)!r}")
            else:
                names.setdefault(call.group(1), f"{path}:{line}")
    for b in bad:
        print(f"error: {b}: an event's message is its snake_case name")
    known = listed()
    unlisted = sorted((where, name) for name, where in names.items() if name not in known)
    for where, name in unlisted:
        print(f"error: {where}: `{name}` isn't in DESIGN's \"Diagnostics events\"")
    return 1 if bad or unlisted else 0


if __name__ == "__main__":
    sys.exit(main())
