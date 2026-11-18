#!/bin/sh
# Merges PR <number> only once the CI workflow run for its CURRENT head
# commit has completed with success. Fails closed: no run, a failed run,
# a head that moved while waiting, or a 30-minute timeout all mean no
# merge. Right after a push, GitHub may still report the old head: if it
# says so, run it again.
#
#   scripts/merge-if-green.sh 123
set -u
pr=${1:?usage: scripts/merge-if-green.sh <pr-number>}
repo=${MERGE_REPO:-transilluminate/aspero}
sha=$(gh pr view "$pr" -R "$repo" --json headRefOid -q .headRefOid)
[ -n "$sha" ] || { echo "NOT MERGING: no head commit"; exit 1; }
run=""
for _ in $(seq 1 60); do
  run=$(gh run list -R "$repo" --workflow CI --commit "$sha" \
    --json status,conclusion -q '.[0]|"\(.status) \(.conclusion)"' 2>/dev/null)
  case "$run" in "completed "*) break ;; esac
  sleep 30
done
echo "CI for $(echo "$sha" | cut -c1-7): ${run:-none}"
now=$(gh pr view "$pr" -R "$repo" --json headRefOid -q .headRefOid)
if [ "$run" = "completed success" ] && [ "$now" = "$sha" ]; then
  gh pr merge "$pr" --merge -R "$repo" --match-head-commit "$sha" && echo MERGED
else
  echo "NOT MERGING"
  exit 1
fi
