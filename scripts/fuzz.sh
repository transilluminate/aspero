#!/usr/bin/env bash
# Runs one fuzz target (fuzz/fuzz_targets) with its seeds and dictionary,
# under time and memory limits. Needs nightly and cargo-fuzz.
#
# Usage: scripts/fuzz.sh <target> [seconds] [libFuzzer options…]
#   seconds   total time (default 600)
#   e.g. scripts/fuzz.sh webpage 1800 -jobs=4 -workers=4
# New inputs go to fuzz/corpus/<target>, failures to fuzz/artifacts/<target>.
set -euo pipefail

target="${1:?usage: scripts/fuzz.sh <target> [seconds] [libFuzzer options…]}"
seconds="${2:-600}"
shift $(($# < 2 ? $# : 2))

case "$target" in
  webpage) dict=html ;;
  scan | id_parse | claim | titled | references) dict=ids ;;
  bib_read | docx) dict=bib ;;
  query | mcp_args) dict=query ;;
  *) echo "unknown target: $target" >&2; exit 2 ;;
esac

cd "$(dirname "$0")/../fuzz"
mkdir -p "corpus/$target"
exec cargo +nightly fuzz run --sanitizer none "$target" "corpus/$target" "seeds/$target" -- \
  -dict="dicts/$dict.dict" -max_total_time="$seconds" \
  -timeout=10 -rss_limit_mb=2048 -max_len=65536 "$@"
