# Using the core from the apps

Every Amanfold app calls the security core through one API,
[`crates/amanfold-ffi`](../crates/amanfold-ffi/src/lib.rs). It is generated
for Swift (iOS, macOS) and Kotlin (Android) with
[UniFFI](https://mozilla.github.io/uniffi-rs/), and wrapped for JavaScript
(browser extension, web vault) in [`crates/amanfold-wasm`](../crates/amanfold-wasm/src/lib.rs).
The names are the same everywhere, in each language's style:
`create_account` is `createAccount` in Swift, Kotlin and JavaScript.

The apps never see a private key, a vault key or protocol state. They hold
objects and pass bytes between those objects and the server.

## The objects

| Object | What it is |
| --- | --- |
| `Account` | An unlocked account. Drop it when the app locks. |
| `LocalVault` | A decrypted vault on this device. |
| `LoginSession` | One sign-in on a device (OPAQUE login plus unlock). |
| `Registration` | Registering the login with the server (sign-up, password change). |

Everything else is plain data: byte strings for the server, strings for the
person, and small records (`ItemView`, `FieldInput`, `CardInfo`, ...).

## Flows

**Sign up.** `createAccount(password)` returns the account, a
`Registration`, the bundle to upload, and the Secret Key and 24 recovery
words for the recovery kit. Run `registration.request()` → server →
`registration.finish(response)` → upload with the bundle. Show the kit once.
Keep the Secret Key in the device keychain; never store the words.

**Sign in on a new device.** Ask the server for the pre-login answer, then
`LoginSession(prelogin, password, secretKey)`, `start()` → server →
`finish(response, pinnedServerKey)` → send `finalization`. Store
`serverPublicKey` the first time and pass it as `pinnedServerKey` after that.
Send `apiToken` as `Authorization: Bearer` (base64url) on later requests.
`unlock(bundle)` gives the `Account` without running Argon2id again.

**Offline unlock.** `unlockAccount(bundle, password, secretKey)`.

**Quick unlock (Face ID, fingerprint).** `account.enableQuickUnlock()` returns
`deviceKey` and `wrappedKeys`. Put `deviceKey` in the Keychain with a
biometry access control (iOS) or the Keystore with user authentication
(Android); `wrappedKeys` can go in ordinary storage. Later:
`quickUnlock(bundle, deviceKey, wrappedKeys)`. Delete either to turn it off.

**Vaults.** `account.createVault()` gives the vault and the owner's grant to
upload. On other devices: `account.openVault(grant, granterPublicKeys, cache)`.
For your own vaults the granter is your own `account.publicKeys()`; for a
shared vault it is the sharer's keys, after comparing safety numbers
(`account.fingerprint()` on both phones).

**Editing.** `createItem`, `updateItem`, `setTrashed` each return a
`SavedItem` (id plus encrypted record) to upload. Upload the records together
with `vault.signManifest(account)` in one commit. When the server accepts it,
call `vault.commitAccepted()`. If it refuses because another device wrote
first, sync and try again; nothing is lost. The server protocol is in
[SERVER.md](SERVER.md).

**Sync.** Download the vault's signed manifest and the changed records, then
`vault.sync(manifest, records, trustedWriters)`. `trustedWriters` are the
public keys of members allowed to write (for a personal vault, just yours).
Upload whatever it returns, then a new manifest. A `Rollback` error means the
server sent old or incomplete data: do not retry silently, tell the person.

**Restart.** Save `vault.exportCache()` in app storage after changes and pass
it to `openVault` next time. It is encrypted, and it keeps rollback
protection across restarts. If the vault key was rotated meanwhile, open with
the old grant and call `account.acceptRotation(vault, newGrant, granterKeys)`.

**Sharing and removal.** `account.grantVault(vault, theirPublicKeys)`. To
remove a member: `vault.rotateKey()`, upload the records, then grant the new
key to the remaining members only.

**Forgot password.** `recoverAccount(bundle, words)`, then
`account.changePassword(newPassword, secretKey)`: upload the new bundle with
the new registration, then `account.updateBundle(bundle)`. The server asks
for proof that the recovery worked: `account.signChallenge("recover",
challenge)`. The same call, with purposes `change-password`,
`rotate-recovery-code` and `delete-account`, protects those operations.

**Tools.** `generatePassword(rules)`, `generatePassphrase(words, separator)`,
`totpNow(secretOrUri)`, `cardInfo(number)` (Arabic-Indic and Persian digits
accepted), `checkSecretKey(text)`, `checkRecoveryWords(words)`.

## Errors

One error type, `AmanfoldError` (Kotlin `AmanfoldException`; in JavaScript an
`Error` whose `name` is the kind). The kinds that need care in the UI:

* `Decrypt`: wrong password, Secret Key, words or device key, or tampering.
* `Rollback`: the server misbehaved. Do not use its response.
* `BadSignature`: data did not come from who it claims (wrong granter, a
  manifest from someone who is not a writer).
* `WeakKdfParams`: the server asked for unsafe key-derivation settings.

## Building

```sh
# Swift / Kotlin: build the library, then generate bindings from it
cargo build -p amanfold-ffi --release
cargo run -p uniffi-bindgen -- generate --library target/release/libamanfold_ffi.so \
    --language kotlin --out-dir out/kotlin        # or --language swift

# Browser
cargo build -p amanfold-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/amanfold_wasm.wasm
```

For phones, build the library per target (`aarch64-apple-ios`,
`aarch64-linux-android`, ...) and package it as an XCFramework or AAR. That
packaging comes with the apps themselves.

## Tests

The same end-to-end flow runs through each generated binding against a test
server (the `test-server` feature, never shipped):
`crates/amanfold-ffi/tests/bindings/test_flow.py`, `FlowTest.kt`,
`FlowTest.swift`, and `crates/amanfold-wasm/tests/flow.cjs`. CI runs all
four; see [.github/workflows/ci.yml](../.github/workflows/ci.yml).
