#!/usr/bin/env bash
# Records PMC open-data (AWS S3, pmc-oa-opendata) responses as fixtures for
# aspero-pmc: a version listing, then the latest version's metadata JSON.
#
# Usage: [ONLY=name] scripts/record-pmc.sh
#
# The bucket is public and takes no key or contact address; requests carry
# only the PMCID.
set -euo pipefail

out="$(cd "$(dirname "$0")/.." && pwd)/crates/providers/pmc/tests/fixtures"
mkdir -p "$out"
base="https://pmc-oa-opendata.s3.amazonaws.com"

# name  path-and-query
requests=(
  "list_oa         /?list-type=2&prefix=PMC8114386.&delimiter=/"
  "meta_oa         /PMC8114386.1/PMC8114386.1.json"
  "list_manuscript /?list-type=2&prefix=PMC2918907.&delimiter=/"
  "meta_manuscript /PMC2918907.1/PMC2918907.1.json"
  "list_missing    /?list-type=2&prefix=PMC5008955.&delimiter=/"
  "meta_missing    /PMC5008955.1/PMC5008955.1.json"
)

for entry in "${requests[@]}"; do
  read -r name path <<<"$entry"
  [[ -n "${ONLY:-}" && "$name" != "$ONLY" ]] && continue
  curl -sS -H "User-Agent: aspero-fixture-recorder" \
    -D "$out/$name.all-headers" -o "$out/$name.body" "$base$path"
  tr -d '\r' <"$out/$name.all-headers" \
    | grep -iE '^(HTTP/|content-type|retry-after)' >"$out/$name.headers"
  rm "$out/$name.all-headers"
  printf '%-16s %s\n' "$name" "$(head -1 "$out/$name.headers")"
  sleep 1
done
