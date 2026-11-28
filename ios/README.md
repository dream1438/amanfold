# Amanfold for iPhone and iPad (beta)

SwiftUI, on the shared Rust security core (UniFFI bindings, built into an
XCFramework). iOS 17 and later.

| Folder | What it is |
| --- | --- |
| `AmanfoldKit/` | Swift package shared by the app and the extension: server API, session and sync loop, encrypted account file, Keychain helpers, strings, shared views. `swift test` runs it on macOS against a real `amanfold-server`. |
| `App/` | The app: onboarding with the printed recovery kit, vault (logins, cards, notes, one-time codes), password generator, settings, Face ID / Touch ID unlock. |
| `AutoFill/` | The AutoFill credential provider for Safari and apps. |
| `project.yml` | [XcodeGen](https://github.com/yonaskolb/XcodeGen) project (the `.xcodeproj` is generated, not committed). |
| `scripts/` | `build-core.sh` builds the core for iOS, the simulator and macOS; `strings.py` makes the English and Arabic strings from the Android ones. |

## Build

On a Mac with Xcode 16+:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin x86_64-apple-darwin
ios/scripts/build-core.sh
cargo build -p amanfold-server
AMANFOLD_SERVER_BIN=$PWD/target/debug/amanfold-server swift test --package-path ios/AmanfoldKit
brew install xcodegen
cd ios && AMANFOLD_TEAM_ID=<your team id> xcodegen && open Amanfold.xcodeproj
```

Debug builds allow `http://localhost` servers; release builds need `https`.

## Signing and entitlements

Running on a phone needs the Apple Developer team, with these set up for
`com.amanfold.app` and `com.amanfold.app.autofill`:

* App Group `group.com.amanfold.app` (the encrypted account file is shared
  with the extension),
* Keychain sharing group `com.amanfold.shared` (file key and Face ID key),
* the AutoFill Credential Provider capability.

People turn on AutoFill in Settings → General → AutoFill & Passwords →
Amanfold; the app's settings link there.

## Language

English and Arabic. iOS picks the app's language from the phone, or per app
in Settings → Amanfold → Language.
