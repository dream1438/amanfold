# Amanfold for Android (beta)

Kotlin and Jetpack Compose, on the shared Rust security core (UniFFI
bindings). Android 9 (API 28) and later.

| Module | What it is |
| --- | --- |
| `client/` | Plain Kotlin (JVM): server API, account and vault session (personal and shared vaults), sync loop, encrypted account file, autofill field classification and fill policy. Tested on the JVM against a real `amanfold-server`. |
| `app/` | The app: onboarding with the printed recovery kit, vault (logins, cards, notes, one-time codes), password generator, family and team sharing with safety-number checks, settings, fingerprint/face unlock, the Autofill Service. English and Arabic (right to left). |

## Build

Needs JDK 17+, Rust, and for the app the Android SDK with an NDK and
[`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk):

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --version 4.1.2 --locked
cd android
./gradlew :client:test          # no Android SDK needed
./gradlew :app:assembleDebug    # builds the core per ABI, then the APK
```

The debug app (`com.amanfold.app.debug`) talks to `http://10.0.2.2:8080`
(the host machine from the emulator); start a dev server with
`AMANFOLD_DEV=1 AMANFOLD_NO_EMAIL_VERIFICATION=1 cargo run -p amanfold-server -- serve`.
Release builds use `https://api.amanfold.com` and refuse plain `http`.

## Autofill

Settings → Autofill → turn on, or in the phone's settings pick Amanfold as
the autofill service. See [docs/SECURITY.md](../docs/SECURITY.md#mobile-apps-milestone-6)
for how apps and web pages are matched (package name plus signing
certificate for apps; the core's site rules for pages in known browsers).

## Release

Signing and the Play listing need the Google Play developer account. The
release build is unsigned until a keystore is configured.
