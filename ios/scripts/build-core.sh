#!/usr/bin/env bash
# Builds the Rust security core for Apple platforms and generates its Swift
# bindings:
#   AmanfoldKit/Frameworks/AmanfoldFFI.xcframework   (iOS, iOS Simulator, macOS)
#   AmanfoldKit/Sources/Amanfold/Amanfold.swift      (generated, not committed)
# Needs Xcode and the Rust targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin x86_64-apple-darwin
# Pass --test-server to include the test server halves (for tests only;
# never for a build that ships).
set -euo pipefail
cd "$(dirname "$0")/../.."
root=$(pwd)
kit="$root/ios/AmanfoldKit"
features=()
if [[ "${1:-}" == "--test-server" ]]; then features=(--features test-server); fi

profile=release
targets=(aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin x86_64-apple-darwin)
for t in "${targets[@]}"; do
  cargo build -p amanfold-ffi --release --target "$t" "${features[@]}"
done

out="$root/target/apple"
rm -rf "$out" && mkdir -p "$out/headers" "$out/sim" "$out/mac"

# Bindings come from the library's metadata; any slice works.
cargo run -q -p uniffi-bindgen -- generate --no-format \
  --library "$root/target/aarch64-apple-darwin/$profile/libamanfold_ffi.a" \
  --language swift --out-dir "$out/swift"
cp "$out/swift/AmanfoldFFI.h" "$out/headers/"
cp "$out/swift/AmanfoldFFI.modulemap" "$out/headers/module.modulemap"

lipo -create "$root/target/aarch64-apple-ios-sim/$profile/libamanfold_ffi.a" \
  "$root/target/x86_64-apple-ios/$profile/libamanfold_ffi.a" -output "$out/sim/libamanfold_ffi.a"
lipo -create "$root/target/aarch64-apple-darwin/$profile/libamanfold_ffi.a" \
  "$root/target/x86_64-apple-darwin/$profile/libamanfold_ffi.a" -output "$out/mac/libamanfold_ffi.a"

rm -rf "$kit/Frameworks/AmanfoldFFI.xcframework"
mkdir -p "$kit/Frameworks" "$kit/Sources/Amanfold"
xcodebuild -create-xcframework \
  -library "$root/target/aarch64-apple-ios/$profile/libamanfold_ffi.a" -headers "$out/headers" \
  -library "$out/sim/libamanfold_ffi.a" -headers "$out/headers" \
  -library "$out/mac/libamanfold_ffi.a" -headers "$out/headers" \
  -output "$kit/Frameworks/AmanfoldFFI.xcframework"
cp "$out/swift/Amanfold.swift" "$kit/Sources/Amanfold/Amanfold.swift"
echo "built $kit/Frameworks/AmanfoldFFI.xcframework"
