#!/usr/bin/env bash
# Builds aspero-ffi for macOS apps: AsperoFFI.xcframework (arm64 and x86_64
# in one static library) and its Swift bindings, zipped for a Swift
# package's binaryTarget.
#
# Usage: scripts/xcframework.sh    (needs Xcode; output in target/xcframework)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/target/xcframework"
targets=(aarch64-apple-darwin x86_64-apple-darwin)
cd "$root"

rustup target add "${targets[@]}" >/dev/null
for t in "${targets[@]}"; do
    cargo build -q -p aspero-ffi --release --lib --target "$t"
done

rm -rf "$out"
mkdir -p "$out/lib" "$out/headers" "$out/bindings"
libs=()
for t in "${targets[@]}"; do libs+=("target/$t/release/libaspero_ffi.a"); done
lipo -create "${libs[@]}" -output "$out/lib/libaspero_ffi.a"

cargo run -q -p aspero-ffi --features bindgen --bin uniffi-bindgen -- generate \
    --library "target/${targets[0]}/release/libaspero_ffi.dylib" \
    --language swift --out-dir "$out/bindings"
cp "$out/bindings/aspero_ffiFFI.h" "$out/headers/"
cp "$out/bindings/aspero_ffiFFI.modulemap" "$out/headers/module.modulemap"

xcodebuild -create-xcframework \
    -library "$out/lib/libaspero_ffi.a" -headers "$out/headers" \
    -output "$out/AsperoFFI.xcframework" >/dev/null
(cd "$out" && zip -qry AsperoFFI.xcframework.zip AsperoFFI.xcframework)
cp "$out/bindings/aspero_ffi.swift" "$out/aspero_ffi.swift"

echo "$out/AsperoFFI.xcframework.zip"
echo "$out/aspero_ffi.swift"
echo "checksum: $(swift package compute-checksum "$out/AsperoFFI.xcframework.zip")"
