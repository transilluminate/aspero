#!/usr/bin/env python3
"""Red-team `aspero verify`: run bench/redteam/corpus.jsonl and score it.

    scripts/redteam.py [corpus.jsonl] [--bin target/release/aspero]
                       [--results target/redteam/results.json] [--rescore]
                       [--category c:wrong-journal] [--failures]
                       [--tape target/redteam-tape.sqlite | --no-tape]
                       [--replay-only]

Each corpus line is a bench case (`claim`, `expect`, `why`, optional `ids`,
`retracted`) plus `category` and `real`. The claims go to the release CLI in
one batch (`aspero verify - --format json`), through an HTTP tape
(`ASPERO_HTTP_TAPE`, `target/redteam-tape.sqlite` unless `--tape`), so each
distinct request is paid for once, ever, and re-runs replay it, with
today's OpenAlex spend capped (`ASPERO_OPENALEX_CAP_USD`, 0.75 unless set)
and, once the allowance is used up, prepaid credit spent up to $5 a day
(`ASPERO_OPENALEX_CREDIT_CAP_USD`, 5 unless set; aspero's own default,
$0.25, would stop a run part-way).
With the tape each run starts from an empty answer cache, so it asks what a
first run asks and its results depend only on the tape and the code;
`--replay-only` runs offline (a request not on the tape fails, unsent).
`--no-tape` runs live through the dedicated cache `target/redteam-cache`
instead (re-runs within its lifetimes free). `OPENALEX_API_KEY` and
`ASPERO_MAILTO` come from the environment, as for the bench.

Prints a confusion table (category x verdict) and the headline numbers:
false VERIFIED among fabricated or wrong citations (the critical metric),
retracted works verified without a retraction flag, and real-citation
recall (verified or plausible); then each acceptance criterion with PASS
or FAIL (`CRITERIA`). `--failures` lists every false VERIFIED and every
real citation missed. `--rescore` re-scores saved results without running
anything. Exits 1 if any fabricated or wrong citation verified.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VERDICTS = ["verified", "plausible", "mismatch", "not_found", "unverifiable"]
# Wholly invented works and identifiers: never verified, whatever else.
INVENTED = ("a:", "e:", "f:", "i:")
# Words naming invented works and identifiers in corpora that name their
# own categories (holdouts, wild corpora).
INVENTED_WORDS = ("invent", "fabricat", "fake", "nonexist", "hallucin", "swap")


def invented_case(category):
    return category.startswith(INVENTED) or any(w in category for w in INVENTED_WORDS)


def claim_line(claim):
    if isinstance(claim, str):
        # One claim per line: text with line breaks (copied from a PDF)
        # goes as a JSON claim, its breaks kept.
        return json.dumps({"text": claim}, ensure_ascii=False) if "\n" in claim else claim
    return json.dumps(claim, ensure_ascii=False)


def run(cases, binary, cache_dir, tape, replay_only):
    env = dict(os.environ)
    env.setdefault("ASPERO_OPENALEX_CAP_USD", "0.75")
    env.setdefault("ASPERO_OPENALEX_CREDIT_CAP_USD", "5")
    env.pop("ASPERO_HTTP_TAPE_MODE", None)
    stdin = "\n".join(claim_line(c["claim"]) for c in cases) + "\n"
    with tempfile.TemporaryDirectory(prefix="aspero-redteam-") as fresh:
        if tape:
            env["ASPERO_HTTP_TAPE"] = str(tape)
            if replay_only:
                env["ASPERO_HTTP_TAPE_MODE"] = "replay-only"
            # A fresh answer cache: the tape sits below it.
            env["ASPERO_CACHE_DIR"] = fresh
        else:
            env["ASPERO_HTTP_TAPE"] = "off"
            env["ASPERO_CACHE_DIR"] = str(cache_dir)
        proc = subprocess.run(
            [str(binary), "verify", "-", "--format", "json"],
            input=stdin, capture_output=True, text=True, env=env, check=False,
        )
    if not proc.stdout.strip():
        sys.exit(f"aspero failed (exit {proc.returncode}): {proc.stderr[-2000:]}")
    doc = json.loads(proc.stdout)
    results = doc["results"]
    if len(results) != len(cases):
        sys.exit(f"{len(results)} results for {len(cases)} claims")
    for case, r in zip(cases, results):
        if r.get("input") != claim_line(case["claim"]):
            sys.exit(f"results out of order at case {case.get('n')}")
    return results, doc.get("usage", {})


def retraction_flagged(result):
    work = (result.get("best_match") or {}).get("work") or {}
    return any(u.get("kind") == "retraction" for u in work.get("updates", []))


def id_form(work_id):
    """One spelling of an identifier: an ISBN-10 as its ISBN-13, a DOI
    without a version suffix (PsyArXiv's `osf.io/f8hqa_v1`)."""
    i = work_id.lower().replace("-", "") if work_id.lower().startswith("isbn:") else work_id.lower()
    if i.startswith("isbn:") and len(i) == 15:
        core = "978" + i[5:14]
        check = (10 - sum(int(d) * (1 if k % 2 == 0 else 3) for k, d in enumerate(core)) % 10) % 10
        return f"isbn:{core}{check}"
    if i.startswith("doi:"):
        return re.sub(r"_v\d+$", "", i)
    return i


def matched_ids(result):
    work = (result.get("best_match") or {}).get("work") or {}
    return {id_form(i) for i in work.get("ids", [])}


def judge(case, result):
    verdict = result.get("verdict", "error")
    flagged = retraction_flagged(result)
    real = case.get("real", False)
    retracted_case = case["category"].startswith("g:") or "retract" in case["category"]
    row = {
        "n": case.get("n"),
        "category": case["category"],
        "verdict": verdict,
        "real": real,
        "retracted": retracted_case,
        "ok": verdict in case["expect"],
        # Verified where the policy allows it (a year one off) isn't false.
        "false_verified": not real
        and not retracted_case
        and verdict == "verified"
        and "verified" not in case["expect"],
        "unflagged_retraction": retracted_case
        and verdict in ("verified", "plausible") and not flagged,
        "real_miss": real and verdict not in ("verified", "plausible"),
        "wrong_work": False,
        "flagged": flagged,
        # A plausible verdict naming the fields to check.
        "names_field": bool((result.get("reason") or {}).get("differs")),
    }
    if case.get("ids") and verdict in ("verified", "plausible") and real:
        row["wrong_work"] = not ({id_form(i) for i in case["ids"]} & matched_ids(result))
    return row


def pct(a, b):
    return f"{a}/{b} ({100 * a / b:.1f}%)" if b else "0/0"


def criteria(wrong, real, retracted):
    """Prints each acceptance criterion with PASS or FAIL."""
    def share(picked, pool):
        return len(picked) / len(pool) if pool else 0.0

    invented = [r for r in wrong if invented_case(r["category"])]
    plausible = [r for r in wrong if r["verdict"] == "plausible"]
    checks = [
        ("wrong -> verified", [r for r in wrong if r["false_verified"]], wrong, "<=", 0.02),
        ("invented, fake ID, DOI swap -> verified",
         [r for r in invented if r["false_verified"]], invented, "<=", 0.0),
        ("real -> mismatch or not_found",
         [r for r in real if r["verdict"] in ("mismatch", "not_found")], real, "<=", 0.03),
        ("real -> verified", [r for r in real if r["verdict"] == "verified"], real, ">=", 0.85),
        ("real -> verified or plausible", [r for r in real if not r["real_miss"]], real, ">=", 0.95),
        ("wrong -> plausible names a differing field",
         [r for r in plausible if r["names_field"]], plausible, ">=", 0.80),
        ("retracted flagged", [r for r in retracted if r["flagged"]], retracted, ">=", 1.0),
    ]
    print("\nAcceptance criteria:")
    for name, picked, pool, op, bound in checks:
        got = share(picked, pool)
        ok = got <= bound if op == "<=" else got >= bound
        print(f"  {'PASS' if ok else 'FAIL'}  {name:<44} {pct(len(picked), len(pool)):>16}  {op} {bound:.0%}")


def report(cases, results, usage, show_failures):
    rows = [judge(c, r) for c, r in zip(cases, results)]
    table = defaultdict(Counter)
    for row in rows:
        table[row["category"]][row["verdict"]] += 1
        table[row["category"]]["ok"] += row["ok"]

    cats = sorted(table, key=lambda c: (not c.startswith("real"), c))
    width = max(len(c) for c in cats) + 2
    head = f"{'category':<{width}}" + "".join(f"{v[:10]:>11}" for v in VERDICTS) + f"{'n':>6}{'as expected':>13}"
    print(head)
    print("-" * len(head))
    for c in cats:
        n = sum(table[c][v] for v in VERDICTS) + table[c]["error"]
        print(f"{c:<{width}}" + "".join(f"{table[c][v] or '':>11}" for v in VERDICTS) + f"{n:>6}{table[c]['ok']:>13}")
    print()

    # The case's `real` flag decides, as false VERIFIED does: categories
    # are only names.
    retracted = [r for r in rows if r["retracted"]]
    real = [r for r in rows if r["real"] and not r["retracted"]]
    wrong = [r for r in rows if not r["real"] and not r["retracted"]]
    fv = [r for r in wrong if r["false_verified"]]
    print(f"False VERIFIED (fabricated or wrong): {pct(len(fv), len(wrong))}")
    by_cat = Counter(r["category"] for r in fv)
    for c in sorted(by_cat):
        total = sum(1 for r in wrong if r["category"] == c)
        print(f"  {c:<24} {pct(by_cat[c], total)}")
    unflagged = [r for r in retracted if r["unflagged_retraction"]]
    print(f"Retracted, accepted without a retraction flag: {pct(len(unflagged), len(retracted))}")
    print(f"  ... and verified at all: {pct(sum(r['verdict'] == 'verified' for r in retracted), len(retracted))}")
    hits = [r for r in real if not r["real_miss"]]
    print(f"Real-citation recall (verified or plausible): {pct(len(hits), len(real))}")
    print(f"  verified: {pct(sum(r['verdict'] == 'verified' for r in real), len(real))}")
    ww = [r for r in real if r["wrong_work"]]
    print(f"  accepted but matched another work than the one cited: {len(ww)}")
    spent = sum(u.get("cost_usd") or 0 for u in usage.values()) if usage else 0
    print(f"OpenAlex spend this run: ${spent:.4f}")
    criteria(wrong, real, retracted)

    if show_failures:
        by_n = {c.get("n"): (c, r) for c, r in zip(cases, results)}
        for title, picked in [
            ("FALSE VERIFIED", fv),
            ("RETRACTED, UNFLAGGED", unflagged),
            ("REAL MISSES", [r for r in real if r["real_miss"]]),
            ("REAL, ANOTHER WORK", ww),
        ]:
            if not picked:
                continue
            print(f"\n== {title} ({len(picked)})")
            for row in picked:
                case, res = by_n[row["n"]]
                bm = res.get("best_match") or {}
                work = bm.get("work") or {}
                print(f"#{row['n']} [{row['category']}] {row['verdict']} {bm.get('confidence', '-')}  {case['why']}")
                print(f"   claim: {claim_line(case['claim'])[:220]}")
                if work:
                    print(f"   match: {work.get('title', '')[:120]} ({(work.get('issued') or '')[:4]}) {work.get('canonical_id')}")
                    print(f"   checks: {bm.get('checks')}")
                print(f"   reason: {res.get('reason_text', '')[:220]}")
    return len(fv)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", nargs="?", default=ROOT / "bench/redteam/corpus.jsonl", type=Path)
    ap.add_argument("--bin", default=ROOT / "target/release/aspero", type=Path)
    ap.add_argument("--cache-dir", default=ROOT / "target/redteam-cache", type=Path,
                    help="answer cache, with --no-tape")
    ap.add_argument("--tape", default=ROOT / "target/redteam-tape.sqlite", type=Path,
                    help="HTTP tape: requests recorded once, replayed after")
    ap.add_argument("--no-tape", action="store_true", help="run live through --cache-dir")
    ap.add_argument("--replay-only", action="store_true",
                    help="offline: a request not on the tape fails, unsent")
    ap.add_argument("--results", default=ROOT / "target/redteam/results.json", type=Path)
    ap.add_argument("--rescore", action="store_true", help="score saved results, run nothing")
    ap.add_argument("--category", action="append", help="only these categories (prefix match)")
    ap.add_argument("--failures", action="store_true", help="list every failure")
    args = ap.parse_args()
    if args.no_tape and args.replay_only:
        ap.error("--replay-only needs the tape")

    cases = [json.loads(l) for l in args.corpus.read_text().splitlines() if l.strip()]
    if args.category:
        cases = [c for c in cases if c["category"].startswith(tuple(args.category))]
    if args.rescore:
        saved = json.loads(args.results.read_text())
        by_input = {r["input"]: r for r in saved["results"]}
        results = [by_input[claim_line(c["claim"])] for c in cases]
        usage = saved.get("usage", {})
    else:
        if not args.bin.exists():
            sys.exit(f"{args.bin} missing: cargo build --release -p aspero")
        tape = None if args.no_tape else args.tape
        results, usage = run(cases, args.bin, args.cache_dir, tape, args.replay_only)
        args.results.parent.mkdir(parents=True, exist_ok=True)
        args.results.write_text(json.dumps({"results": results, "usage": usage}))
    false_verified = report(cases, results, usage, args.failures)
    sys.exit(1 if false_verified else 0)


if __name__ == "__main__":
    main()
