#!/usr/bin/env bash
# Records doi.org content-negotiation responses as fixtures for aspero-doi.
#
# Usage: ASPERO_MAILTO=you@example.org scripts/record-doi.sh
#
# doi.org redirects to the DOI's registration agency (Crossref, DataCite,
# mEDRA, ...); like the provider's HTTP client, curl follows the redirects,
# and only the final response's headers are kept. Fails if the mailto
# appears in any recorded file.
set -euo pipefail

: "${ASPERO_MAILTO:?set ASPERO_MAILTO}"
out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/doi/tests/fixtures"
mkdir -p "$out"

# name  doi
requests=(
  "csl_crossref   10.1038/nature14539"
  "csl_datacite   10.48550/arXiv.1706.03762"
  "csl_not_found  10.5555/aspero-does-not-exist"
)

for entry in "${requests[@]}"; do
  read -r name doi <<<"$entry"
  curl -sS -L -H "Accept: application/vnd.citationstyles.csl+json" \
    -H "User-Agent: aspero-fixture-recorder (mailto:$ASPERO_MAILTO)" \
    -D "$out/$name.all-headers" -o "$out/$name.body" "https://doi.org/$doi"
  # Keep the last header block (after redirects), only headers we read.
  tr -d '\r' <"$out/$name.all-headers" \
    | awk '/^HTTP\//{block=""} {block=block $0 "\n"} END{printf "%s", block}' \
    | grep -iE '^(HTTP/|content-type|retry-after|x-rate-limit|ratelimit)' >"$out/$name.headers"
  rm "$out/$name.all-headers"
  printf '%-14s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
done

if grep -rqiF "$ASPERO_MAILTO" "$out"; then
  echo "mailto address found in fixtures; deleting them" >&2
  rm -rf "$out"
  exit 1
fi
echo "mailto not present in fixtures"
