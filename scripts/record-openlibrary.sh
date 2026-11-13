#!/usr/bin/env bash
# Records Open Library responses as fixtures for aspero-openlibrary.
#
# Usage: ASPERO_MAILTO=you@example.org [ONLY=name] scripts/record-openlibrary.sh
#
# Open Library asks API clients to identify themselves in the User-Agent.
# Keep requests identical to what the provider sends (tests assert them).
set -euo pipefail

: "${ASPERO_MAILTO:?set ASPERO_MAILTO}"
out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/openlibrary/tests/fixtures"
mkdir -p "$out"
base="https://openlibrary.org"
fields="key,title,subtitle,author_name,first_publish_year,publish_year,isbn,publisher,number_of_pages_median"

record() {
  local name=$1 path=$2; shift 2
  [[ -n "${ONLY:-}" && "$name" != "$ONLY" ]] && return 0
  local args=()
  for p in "$@"; do args+=(--data-urlencode "$p"); done
  curl -sS -G -A "aspero-fixture-recorder (mailto:$ASPERO_MAILTO)" \
    -D "$out/$name.headers" -o "$out/$name.body" "$base$path" "${args[@]+"${args[@]}"}"
  tr -d '\r' <"$out/$name.headers" | grep -iE '^(HTTP/|content-type|retry-after)' \
    >"$out/$name.headers.tmp" && mv "$out/$name.headers.tmp" "$out/$name.headers"
  printf '%-16s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
}

record books_isbn /api/books 'bibkeys=ISBN:9780262035613,ISBN:9780000000002' 'format=json' 'jscmd=data'
record search /search.json 'q=(deep learning goodfellow)' "fields=$fields" 'limit=3' 'page=1'
record search_filtered /search.json 'q=(deep learning) AND first_publish_year:[2016 TO 2016]' \
  "fields=$fields" 'limit=3' 'page=1'
record search_fields /search.json 'q=' 'author=Goodfellow' 'title=deep learning' 'sort=new' \
  "fields=$fields" 'limit=3' 'page=1'
record search_tree /search.json \
  'q=(title:(hobbit) OR title:(silmarillion)) AND author_name:(tolkien) AND (NOT title:(companion)) AND language:eng' \
  "fields=$fields" 'limit=3' 'page=1'
record search_tree_ids /search.json \
  'q=(isbn:9780262035613 OR isbn:9780131103627) AND author_name:(goodfellow)' \
  "fields=$fields" 'limit=3' 'page=1'

if grep -rqiF "$ASPERO_MAILTO" "$out"; then
  echo "mailto address found in fixtures; deleting them" >&2
  rm -rf "$out"
  exit 1
fi
echo "mailto not present in fixtures"
