#!/usr/bin/env bash
# Regenerates the FFI's Swift bindings, as scripts/xcframework.sh does (the
# same features, without Xcode), and fails if contracts/aspero_ffi.swift
# isn't them byte for byte: the committed snapshot of the Swift API, which
# the release's aspero_ffi.swift asset is. An exported item's doc comment
# feeds its UniFFI checksum, and enum cases keep their places, so a diff
# here is a change Swift callers see.
#
# Usage: scripts/swift-bindings.sh          (check)
#        ASPERO_BLESS=1 scripts/swift-bindings.sh   (rewrite the snapshot)
#   CARGO   the cargo to run (default: cargo)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
snapshot="$root/contracts/aspero_ffi.swift"
cargo="${CARGO:-cargo}"
cd "$root"

case "$(uname -s)" in
    Darwin) lib=target/debug/libaspero_ffi.dylib ;;
    *) lib=target/debug/libaspero_ffi.so ;;
esac
"$cargo" build -q -p aspero-ffi --lib --features mcp
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
"$cargo" run -q -p aspero-ffi --features bindgen --bin uniffi-bindgen -- generate \
    --library "$lib" --language swift --out-dir "$out"

if [[ -n "${ASPERO_BLESS:-}" ]]; then
    cp "$out/aspero_ffi.swift" "$snapshot"
    echo "wrote $snapshot"
elif ! cmp -s "$out/aspero_ffi.swift" "$snapshot"; then
    diff -u "$snapshot" "$out/aspero_ffi.swift" | head -200 || true
    echo "error: the Swift API changed: contracts/aspero_ffi.swift differs from the generated" \
        "bindings (ASPERO_BLESS=1 scripts/swift-bindings.sh, if intended)" >&2
    exit 1
fi
