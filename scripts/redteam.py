#!/usr/bin/env python3
"""Red-team `aspero verify`: run bench/redteam/corpus.jsonl and score it.

    scripts/redteam.py [corpus.jsonl] [--bin target/release/aspero]
                       [--results target/redteam/results.json] [--rescore]
                       [--category c:wrong-journal] [--failures]

Each corpus line is a bench case (`claim`, `expect`, `why`, optional `ids`,
`retracted`) plus `category` and `real`. The claims go to the release CLI in
one batch (`aspero verify - --format json`), through a dedicated cache
(`target/redteam-cache`, so re-runs are free), with today's OpenAlex spend
capped (`ASPERO_OPENALEX_CAP_USD`, 0.75 unless set). `OPENALEX_API_KEY` and
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
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VERDICTS = ["verified", "plausible", "mismatch", "not_found", "unverifiable"]
# Wholly invented works and identifiers: never verified, whatever else.
INVENTED = ("a:", "e:", "f:", "i:")


def claim_line(claim):
    if isinstance(claim, str):
        assert "\n" not in claim, claim
        return claim
    return json.dumps(claim, ensure_ascii=False)


def run(cases, binary, cache_dir):
    env = dict(os.environ)
    env.setdefault("ASPERO_OPENALEX_CAP_USD", "0.75")
    env["ASPERO_CACHE_DIR"] = str(cache_dir)
    stdin = "\n".join(claim_line(c["claim"]) for c in cases) + "\n"
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


def matched_ids(result):
    work = (result.get("best_match") or {}).get("work") or {}
    return {i.lower() for i in work.get("ids", [])}


def judge(case, result):
    verdict = result.get("verdict", "error")
    flagged = retraction_flagged(result)
    real = case.get("real", False)
    retracted_case = case["category"].startswith("g:")
    row = {
        "n": case.get("n"),
        "category": case["category"],
        "verdict": verdict,
        "ok": verdict in case["expect"],
        "false_verified": not real and not retracted_case and verdict == "verified",
        "unflagged_retraction": retracted_case
        and verdict in ("verified", "plausible") and not flagged,
        "real_miss": real and verdict not in ("verified", "plausible"),
        "wrong_work": False,
        "flagged": flagged,
        # A plausible verdict naming the fields to check.
        "names_field": bool((result.get("reason") or {}).get("differs")),
    }
    if case.get("ids") and verdict in ("verified", "plausible") and real:
        row["wrong_work"] = not ({i.lower() for i in case["ids"]} & matched_ids(result))
    return row


def pct(a, b):
    return f"{a}/{b} ({100 * a / b:.1f}%)" if b else "0/0"


def criteria(wrong, real, retracted):
    """Prints each acceptance criterion with PASS or FAIL."""
    def share(picked, pool):
        return len(picked) / len(pool) if pool else 0.0

    invented = [r for r in wrong if r["category"].startswith(INVENTED)]
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

    wrong = [r for r in rows if not r["category"].startswith(("real", "g:"))]
    real = [r for r in rows if r["category"].startswith("real")]
    retracted = [r for r in rows if r["category"].startswith("g:")]
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
    ap.add_argument("--cache-dir", default=ROOT / "target/redteam-cache", type=Path)
    ap.add_argument("--results", default=ROOT / "target/redteam/results.json", type=Path)
    ap.add_argument("--rescore", action="store_true", help="score saved results, run nothing")
    ap.add_argument("--category", action="append", help="only these categories (prefix match)")
    ap.add_argument("--failures", action="store_true", help="list every failure")
    args = ap.parse_args()

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
        results, usage = run(cases, args.bin, args.cache_dir)
        args.results.parent.mkdir(parents=True, exist_ok=True)
        args.results.write_text(json.dumps({"results": results, "usage": usage}))
    false_verified = report(cases, results, usage, args.failures)
    sys.exit(1 if false_verified else 0)


if __name__ == "__main__":
    main()
