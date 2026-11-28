#!/bin/sh
# Wrap dist/safari in the native app Safari needs (macOS and iOS) and build
# it, without signing. Runs on macOS with Xcode.
#
#   npm run wasm && node scripts/build.mjs && sh scripts/safari.sh
#
# Publishing to the App Store needs an Apple Developer account: open
# build/safari/Amanfold/Amanfold.xcodeproj, choose the team, and archive.
set -eu
cd "$(dirname "$0")/.."

rm -rf build/safari
xcrun safari-web-extension-converter dist/safari \
  --project-location build/safari \
  --app-name Amanfold \
  --bundle-identifier com.amanfold.Amanfold \
  --swift --copy-resources --no-open --no-prompt --force

project=build/safari/Amanfold/Amanfold.xcodeproj
xcodebuild -project "$project" -list

xcodebuild -project "$project" -scheme "Amanfold (macOS)" -configuration Release \
  -derivedDataPath build/safari/derived CODE_SIGNING_ALLOWED=NO build
xcodebuild -project "$project" -scheme "Amanfold (iOS)" -configuration Release \
  -sdk iphonesimulator -derivedDataPath build/safari/derived CODE_SIGNING_ALLOWED=NO build

echo "built build/safari (macOS app and iOS simulator app, unsigned)"
