#!/usr/bin/env bash
# Records Europe PMC REST responses as fixtures for aspero-europepmc.
#
# Usage: ASPERO_MAILTO=you@example.org [ONLY=name] scripts/record-europepmc.sh
#
# Keep requests identical to what the provider sends (tests assert them).
set -euo pipefail

: "${ASPERO_MAILTO:?set ASPERO_MAILTO}"
out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/europepmc/tests/fixtures"
mkdir -p "$out"
base="https://www.ebi.ac.uk/europepmc/webservices/rest/search"

record() {
  local name=$1; shift
  [[ -n "${ONLY:-}" && "$name" != "$ONLY" ]] && return 0
  local args=()
  for p in "$@"; do args+=(--data-urlencode "$p"); done
  curl -sS -G -A "aspero-fixture-recorder (mailto:$ASPERO_MAILTO)" \
    -D "$out/$name.headers" -o "$out/$name.body" "$base" "${args[@]}"
  tr -d '\r' <"$out/$name.headers" | grep -iE '^(HTTP/|content-type|retry-after)' \
    >"$out/$name.headers.tmp" && mv "$out/$name.headers.tmp" "$out/$name.headers"
  printf '%-16s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
}

record lookup_batch \
  'query=(EXT_ID:26017442 AND SRC:MED) OR PMCID:PMC6716493 OR DOI:"10.1016/s0140-6736(97)11096-0" OR (EXT_ID:99999999 AND SRC:MED)' \
  'resultType=core' 'format=json' 'pageSize=8'
record lookup_nbk 'query=BOOK_ID:NBK526056 OR BOOK_ID:NBK999999999' \
  'resultType=core' 'format=json' 'pageSize=4'
record search 'query=(protein structure prediction alphafold)' 'resultType=core' 'format=json' \
  'pageSize=3' 'cursorMark=*'
record search_filtered 'query=(protein structure prediction) AND PUB_YEAR:[2021 TO 2021] AND OPEN_ACCESS:y' \
  'resultType=core' 'format=json' 'pageSize=3' 'cursorMark=*'
record search_fields \
  'query=AUTH:"Jumper J" AND TITLE:(protein structure prediction) AND JOURNAL:"Nature"' \
  'resultType=core' 'format=json' 'pageSize=3' 'cursorMark=*'
record search_newest 'query=(alphafold)' 'resultType=core' 'format=json' 'pageSize=3' \
  'cursorMark=*' 'sort=P_PDATE_D desc'
record search_most_cited 'query=(alphafold)' 'resultType=core' 'format=json' 'pageSize=3' \
  'cursorMark=*' 'sort=CITED desc'
record search_tree \
  'query=(TITLE:"protein structure prediction" OR ABSTRACT:(alphafold)) AND LANG:eng AND HAS_ABSTRACT:Y AND (NOT PUB_TYPE:"Retracted Publication") AND (NOT TITLE:(review)) AND PUB_YEAR:[2021 TO 2023]' \
  'resultType=core' 'format=json' 'pageSize=3' 'cursorMark=*'
record search_tree_orcid 'query=AUTHORID:"0000-0001-6187-6610" AND (NOT LANG:ger)' 'resultType=core' 'format=json' \
  'pageSize=3' 'cursorMark=*' 'sort=P_PDATE_D desc'
record search_since 'query=(sepsis) AND FIRST_IDATE:[2026-11-01 TO *]' 'resultType=core' 'format=json' \
  'pageSize=3' 'cursorMark=*'

if grep -rqiF "$ASPERO_MAILTO" "$out"; then
  echo "mailto address found in fixtures; deleting them" >&2
  rm -rf "$out"
  exit 1
fi
echo "mailto not present in fixtures"
