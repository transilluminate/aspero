#!/usr/bin/env bash
# Records Crossref REST API responses as test fixtures for aspero-crossref.
#
# Usage: ASPERO_MAILTO=you@example.org scripts/record-crossref.sh
#
# The mailto puts requests in Crossref's polite pool. The script fails if the
# address appears in any recorded file, so fixtures stay safe to publish.
set -euo pipefail

: "${ASPERO_MAILTO:?set ASPERO_MAILTO}"
out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/crossref/tests/fixtures"
mkdir -p "$out"
base="https://api.crossref.org"

# name  path-and-query (mailto is appended)
requests=(
  "work_by_doi             /works/10.1038/nature14539"
  "work_conference         /works/10.1145/3368089.3409741"
  "work_not_found          /works/10.5555/aspero-does-not-exist"
  "works_batch_doi         /works?filter=doi:10.1038/nature14539,doi:10.1145/3368089.3409741,doi:10.5555/aspero-does-not-exist&rows=20"
  "works_bibliographic     /works?query.bibliographic=Vaswani%202017%20Attention%20is%20all%20you%20need&rows=5"
  "works_bibliographic_ref /works?query.bibliographic=LeCun%20Y%2C%20Bengio%20Y%2C%20Hinton%20G.%20Deep%20learning.%20Nature.%202015%3B521%3A436-444.&rows=3"
  "works_search_filtered   /works?query=transformer&filter=from-pub-date:2017,until-pub-date:2018,type:journal-article&rows=3&cursor=*"
  "bad_filter              /works?filter=no-such-filter:1"
)

for entry in "${requests[@]}"; do
  read -r name path <<<"$entry"
  sep='?'; [[ "$path" == *\?* ]] && sep='&'
  curl -sS --globoff -H "User-Agent: aspero-fixture-recorder (mailto:$ASPERO_MAILTO)" \
    -D "$out/$name.headers" -o "$out/$name.body" "$base$path${sep}mailto=$ASPERO_MAILTO"
  # Keep only headers the provider reads.
  grep -iE '^(HTTP/|content-type|retry-after|x-rate-limit-|x-api-pool|x-concurrency)' "$out/$name.headers" \
    | tr -d '\r' >"$out/$name.headers.tmp" && mv "$out/$name.headers.tmp" "$out/$name.headers"
  printf '%-24s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
done

if grep -rqiF "$ASPERO_MAILTO" "$out"; then
  echo "mailto address found in fixtures; deleting them" >&2
  rm -rf "$out"
  exit 1
fi
echo "mailto not present in fixtures"
