#!/usr/bin/env bash
# Records OpenAlex responses as test fixtures for aspero-openalex.
#
# Usage: OPENALEX_API_KEY=… [ONLY=name] scripts/record-openalex.sh
#
# The key is sent as a Bearer header (never in the URL) and the script fails
# if it appears in any recorded file. Lookups are free; each search costs
# about $0.001 of the daily allowance.
set -euo pipefail

: "${OPENALEX_API_KEY:?set OPENALEX_API_KEY}"
out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/openalex/tests/fixtures"
mkdir -p "$out"
base="https://api.openalex.org"

# name  path-and-query
requests=(
  "work_by_doi       /works/doi:10.1038/nature14539"
  "work_by_openalex  /works/W2919115771"
  "work_by_pmid      /works/pmid:26017442"
  "work_by_arxiv_doi /works/doi:10.48550/arxiv.1706.03762"
  "work_not_found    /works/doi:10.5555/aspero-does-not-exist"
  "works_batch_doi   /works?filter=doi:10.1038/nature14539|10.48550/arxiv.1706.03762|10.5555/aspero-does-not-exist&per-page=100"
  "works_batch_openalex /works?filter=openalex:W2919115771|W2626778328&per-page=100"
  "works_batch_pmid  /works?filter=pmid:26017442&per-page=100"
  "works_search      /works?search=attention%20is%20all%20you%20need&per-page=5"
  "works_search_filtered /works?search=transformer&filter=from_publication_date:2017-01-01,to_publication_date:2018-12-31,is_oa:true,type:preprint&per-page=3&cursor=*"
  "works_search_fields /works?sort=cited_by_count:desc&filter=raw_author_name.search:lecun,title.search:deep%20learning&per-page=3&cursor=*"
  "works_search_tree /works?search=protein%20structure&filter=title.search:alphafold%20OR%20%22structure%20prediction%22,raw_author_name.search:!jumper,is_retracted:false,language:en,from_publication_date:2021-01-01&per-page=3&cursor=*"
  "works_search_relation /works?sort=cited_by_count:desc&filter=cites:W2919115771,title.search:medical,type:article|review&per-page=3&cursor=*"
  "works_search_orcid /works?filter=authorships.author.orcid:0000-0002-3271-7861|0000-0001-6187-6610,has_abstract:true,is_oa:true&per-page=3&cursor=*"
  "sources_named     /sources?filter=display_name.search:nature%20machine&select=id&per-page=100"
  "bad_filter        /works?filter=no_such_field:1"
  "work_retracted    /works/doi:10.1016/S0140-6736(97)11096-0"
  "work_no_references /works/doi:10.1176/appi.books.9780890425596"
  "works_retracted_batch /works?filter=openalex:W2158048826|W2919115771|W3010930696,is_retracted:true&per-page=100&cursor=*"
  "works_batch_reregistered /works?filter=doi:10.1197/j.aem.2003.09.015|10.1038/nature14539|10.5555/aspero-does-not-exist&per-page=100"
  "work_reregistered /works/doi:10.1197/j.aem.2003.09.015"
  "works_cites_page  /works?select=id,publication_year,referenced_works&sort=cited_by_count:desc&filter=cites:W2293082134|W2568812702&per-page=200&cursor=*"
)

for entry in "${requests[@]}"; do
  read -r name path <<<"$entry"
  [[ -n "${ONLY:-}" && "$name" != "$ONLY" ]] && continue
  curl -sS --globoff -H "Authorization: Bearer $OPENALEX_API_KEY" \
    -H "User-Agent: aspero-fixture-recorder" \
    -D "$out/$name.headers" -o "$out/$name.body" "$base$path"
  # Keep only headers the provider reads; drop cookies and tracing noise.
  grep -iE '^(HTTP/|content-type|retry-after|x-ratelimit-|location)' "$out/$name.headers" \
    | tr -d '\r' >"$out/$name.headers.tmp" && mv "$out/$name.headers.tmp" "$out/$name.headers"
  printf '%-22s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 0.2
done

if grep -rqF "$OPENALEX_API_KEY" "$out"; then
  echo "API key found in fixtures; deleting them" >&2
  rm -rf "$out"
  exit 1
fi
echo "key not present in fixtures"
