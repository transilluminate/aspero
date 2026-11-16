#!/usr/bin/env python3
"""Turn a wrong-verdict issue into a red-team case.

    scripts/issue-to-case.py <number> [--expect plausible --expect mismatch]
                             [--dry-run] [--no-input] [--force]
                             [--repo transilluminate/aspero] [--json-file saved.json]

Reads the issue with `gh issue view <number> --json body,title,labels,url`
(or a saved copy of that JSON, `--json-file`), parses the wrong-verdict form
(`.github/ISSUE_TEMPLATE/wrong-verdict.yml`; GitHub writes each field as
`### <label>` and its value), and appends one case to
`bench/redteam/reports.jsonl` in the corpus's format:

    {"n", "claim", "category": "reported", "real", "expect", "why",
     "ids", "issue", "reported_verdict", "version"}

`expect` is the set of verdicts the policy (bench/redteam/README.md) allows
for this citation: `--expect` flags, else asked for on a terminal, else
"not verified" (plausible, mismatch, not_found). A case that expects
`verified` is a real citation (`real: true`), and the DOIs, PMIDs and arXiv
IDs given as the right work become its `ids`. Run the cases with
`scripts/redteam.py bench/redteam/reports.jsonl`.

The issue's text is untrusted data: it is parsed and stored, never run,
evaluated or passed to a shell. A JSON claim is read with `json.loads`.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FORM = ROOT / ".github/ISSUE_TEMPLATE/wrong-verdict.yml"
REPORTS = ROOT / "bench/redteam/reports.jsonl"
VERDICTS = ["verified", "plausible", "mismatch", "not_found", "unverifiable"]
NOT_VERIFIED = ["plausible", "mismatch", "not_found"]
NO_RESPONSE = "_No response_"
MAX_CLAIM = 5000


def form_labels(path=FORM):
    """The form's field labels by id, by a line scan of its YAML."""
    labels, current = {}, None
    for line in path.read_text(encoding="utf-8").splitlines():
        t = line.strip()
        if t.startswith("id:"):
            current = t[3:].strip()
        elif t.startswith("label:") and current and current not in labels:
            labels[current] = t[6:].strip().strip("\"'")
    return labels


def sections(body):
    """The issue body's `### <label>` sections, label -> text."""
    out, label, lines = {}, None, []
    for line in body.replace("\r\n", "\n").split("\n"):
        m = re.match(r"^### (.+?)\s*$", line)
        if m:
            if label is not None:
                out[label] = "\n".join(lines).strip()
            label, lines = m.group(1), []
        elif label is not None:
            lines.append(line)
    if label is not None:
        out[label] = "\n".join(lines).strip()
    return out


def unfence(text):
    """A rendered textarea's value without its code fence."""
    m = re.match(r"^```[\w-]*\n(.*?)\n?```$", text, re.S)
    return m.group(1).strip() if m else text


def fields(body, labels):
    """The form's values by field id; empty for "_No response_"."""
    by_label = sections(body)
    out = {}
    for field, label in labels.items():
        value = by_label.get(label, "")
        out[field] = "" if value == NO_RESPONSE else unfence(value)
    return out


def claim_of(citation):
    """The claim as the corpus holds it: a JSON object as given, else the
    text on one line."""
    text = citation.strip()
    if text.startswith("{"):
        try:
            obj = json.loads(text)
        except json.JSONDecodeError:
            obj = None
        if isinstance(obj, dict):
            return obj
    return " ".join(text.split())


def ids_in(text):
    """DOIs, PMIDs and arXiv IDs named in `text`, as the corpus writes them."""
    ids = []
    for doi in re.findall(r"\b(10\.\d{4,9}/[^\s\"<>]+)", text):
        ids.append("doi:" + doi.rstrip(".,;)").lower())
    for pmid in re.findall(r"\bPMID:?\s*(\d{1,9})\b", text, re.I):
        ids.append("pmid:" + pmid)
    for pmid in re.findall(r"pubmed\.ncbi\.nlm\.nih\.gov/(\d{1,9})", text):
        ids.append("pmid:" + pmid)
    for arxiv in re.findall(r"\barXiv:\s*(\d{4}\.\d{4,5})", text, re.I):
        ids.append("arxiv:" + arxiv)
    return list(dict.fromkeys(ids))


def ask_expect():
    prompt = (
        "Verdicts the policy allows (comma-separated, from "
        f"{', '.join(VERDICTS)}; Enter for not verified): "
    )
    answer = input(prompt).strip()
    if not answer:
        return NOT_VERIFIED
    return [v.strip() for v in answer.split(",") if v.strip()]


def first_line(text, limit=200):
    line = " ".join(text.split())
    return line if len(line) <= limit else line[: limit - 1] + "…"


def build_case(issue, number, expect, n):
    values = fields(issue.get("body") or "", form_labels())
    if not values.get("citation"):
        sys.exit(f"#{number} has no citation: is it a wrong-verdict issue?")
    if "[x]" not in values.get("privacy", "").lower():
        sys.exit(f"#{number}: the privacy box isn't ticked; not storing it")
    if len(values["citation"]) > MAX_CLAIM:
        sys.exit(f"#{number}: the citation is over {MAX_CLAIM} characters")
    real = "verified" in expect
    url = issue.get("url") or f"https://github.com/transilluminate/aspero/issues/{number}"
    reported = values.get("verdict", "")
    why = f"reported {url}: {first_line(issue.get('title') or '', 120)}"
    if reported:
        why += f"; gave {reported}"
    if values.get("expected"):
        why += f"; expected: {first_line(values['expected'])}"
    case = {
        "n": n,
        "claim": claim_of(values["citation"]),
        "category": "reported",
        "real": real,
        "expect": expect,
        "why": why,
    }
    ids = ids_in(values.get("expected", ""))
    if real and ids:
        case["ids"] = ids
    case["issue"] = number
    if reported:
        case["reported_verdict"] = reported
    if values.get("version"):
        case["version"] = values["version"]
    return case


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("number", type=int, help="the issue's number")
    ap.add_argument("--expect", action="append", choices=VERDICTS,
                    help="a verdict the policy allows (repeatable)")
    ap.add_argument("--dry-run", action="store_true", help="print the case, write nothing")
    ap.add_argument("--no-input", action="store_true", help="never ask; default to not verified")
    ap.add_argument("--force", action="store_true", help="add it even if the issue has a case")
    ap.add_argument("--repo", default="transilluminate/aspero")
    ap.add_argument("--json-file", type=Path,
                    help="a saved `gh issue view --json body,title,labels,url` result")
    ap.add_argument("--out", type=Path, default=REPORTS)
    args = ap.parse_args()

    if args.json_file:
        issue = json.loads(args.json_file.read_text(encoding="utf-8"))
    else:
        proc = subprocess.run(
            ["gh", "issue", "view", str(args.number), "-R", args.repo,
             "--json", "body,title,labels,url"],
            capture_output=True, text=True, check=False,
        )
        if proc.returncode != 0:
            sys.exit(f"gh issue view failed: {proc.stderr.strip()}")
        issue = json.loads(proc.stdout)
    labels = {label.get("name") for label in issue.get("labels") or []}
    if "verdict" not in labels:
        print(f"note: #{args.number} isn't labelled `verdict`", file=sys.stderr)

    existing = []
    if args.out.exists():
        existing = [json.loads(l) for l in args.out.read_text(encoding="utf-8").splitlines() if l.strip()]
    if not args.force and any(c.get("issue") == args.number for c in existing):
        sys.exit(f"#{args.number} already has a case in {args.out} (--force to add another)")

    expect = args.expect
    if not expect:
        expect = NOT_VERIFIED if args.no_input or not sys.stdin.isatty() else ask_expect()
    unknown = [v for v in expect if v not in VERDICTS]
    if unknown:
        sys.exit(f"not verdicts: {', '.join(unknown)}")
    expect = list(dict.fromkeys(expect))

    n = max((c.get("n", 0) for c in existing if isinstance(c.get("n"), int)), default=0) + 1
    case = build_case(issue, args.number, expect, n)
    line = json.dumps(case, ensure_ascii=False)
    if args.dry_run:
        print(line)
        return
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("a", encoding="utf-8") as f:
        f.write(line + "\n")
    print(f"added case {n} (#{args.number}) to {args.out}")


if __name__ == "__main__":
    main()
