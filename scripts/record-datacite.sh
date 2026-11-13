#!/usr/bin/env bash
# Records DataCite REST API responses as fixtures for aspero-datacite.
#
# Usage: [ONLY=name] scripts/record-datacite.sh
#
# Keep the requests identical to what the provider sends (the tests assert
# the parameters): relevance only works with page-number paging.
set -euo pipefail

out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/datacite/tests/fixtures"
mkdir -p "$out"
base="https://api.datacite.org"

# name  path  [query params as name=value, URL-encoded by curl]
record() {
  local name=$1 path=$2; shift 2
  [[ -n "${ONLY:-}" && "$name" != "$ONLY" ]] && return 0
  local args=()
  for p in "$@"; do args+=(--data-urlencode "$p"); done
  curl -sS -G -H "Accept: application/vnd.api+json" -D "$out/$name.headers" -o "$out/$name.body" \
    "$base$path" "${args[@]+"${args[@]}"}"
  tr -d '\r' <"$out/$name.headers" | grep -iE '^(HTTP/|content-type|retry-after|x-ratelimit)' \
    >"$out/$name.headers.tmp" && mv "$out/$name.headers.tmp" "$out/$name.headers"
  printf '%-18s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
}

record doi_arxiv /dois/10.48550/arxiv.1706.03762
record doi_not_found /dois/10.5555/aspero-does-not-exist
record batch_doi /dois \
  'query=doi:("10.48550/arxiv.1706.03762" OR "10.5281/zenodo.24110911" OR "10.5555/aspero-does-not-exist")' \
  'page[size]=3'
record search /dois 'query=(attention is all you need)' 'sort=relevance' 'page[number]=1' 'page[size]=3'
record search_filtered /dois \
  'query=(transformer) AND publicationYear:[2017 TO 2018]' 'resource-type-id=preprint' \
  'sort=relevance' 'page[number]=1' 'page[size]=3'
record search_tree /dois \
  'query=(titles.title:(transformer) OR descriptions.description:"attention mechanism") AND types.resourceTypeGeneral:Dataset AND descriptions.descriptionType:Abstract AND (NOT creators.name:(smith)) AND publicationYear:[2020 TO *]' \
  'sort=relevance' 'page[number]=1' 'page[size]=3'
record search_tree_orcid /dois \
  'query=creators.nameIdentifiers.nameIdentifier:("https://orcid.org/0000-0001-6187-6610" OR "0000-0001-6187-6610") AND language:(en OR eng)' \
  'sort=relevance' 'page[number]=1' 'page[size]=3'
record search_since /dois 'query=(sepsis) AND created:[2026-11-01 TO *]' \
  'sort=relevance' 'page[number]=1' 'page[size]=3'
record bad_request /dois 'published=2017-2018'
