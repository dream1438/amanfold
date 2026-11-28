# Amanfold

Amanfold is a zero-knowledge password, passkey and payment-card manager for
individuals, families and teams. This repository holds the shared **security
core**, written once in Rust and used by every Amanfold app (iOS, Android,
browser extension, desktop and web).

> **Beta software. Not yet independently audited.** Do not store real secrets
> in it yet. See [docs/SECURITY.md](docs/SECURITY.md).

## What is in `crates/amanfold-core`

| Module | Does |
| --- | --- |
| `kdf` | Master password (NFKC) + 128-bit Secret Key → Argon2id + HKDF → Account Unlock Key; KDF floor/ceiling against server downgrades |
| `account` | Sign-up, unlock, password change, silent KDF upgrade, 24-word recovery code |
| `auth` | OPAQUE login (client and server halves), fake responses for unknown accounts, server key pinning |
| `manifest` | Signed vault manifests: detect old, withheld or dropped data |
| `keys` | X25519/Ed25519 account keys, public-key sealing, safety-number fingerprints |
| `vault` | Vault keys, signed grants for family and team members, local replica, key rotation on member removal |
| `item` | Logins, cards, notes, identities, passkeys; per-field conflict-free merge with history |
| `sync` | Encrypted, size-padded record format the server stores |
| `site` | Which saved logins belong to which page: Public Suffix List matching, no HTTPS downgrade, look-alike domain detection |
| `card` | Luhn check, brand detection, expiry parsing; accepts Arabic-Indic and Persian digits |
| `totp` | RFC 6238 one-time codes from secrets or `otpauth://` URIs |
| `generator` | Unbiased random passwords and passphrases |

## The other crates

| Crate | Does |
| --- | --- |
| `crates/amanfold-ffi` | The one API every app calls; Swift and Kotlin bindings generated with UniFFI. Keys never leave Rust. |
| `crates/amanfold-wasm` | The same API for the browser extension and web vault (WebAssembly) |
| `crates/uniffi-bindgen` | Generates the Swift, Kotlin and Python bindings |
| `crates/amanfold-server` | The sync server: accounts, OPAQUE login, encrypted vault sync, family and team sharing. Stores ciphertext only. |

The browser extension (Chrome, Edge, Firefox) is in [`extension/`](extension/README.md),
the Android app in [`android/`](android/README.md) and the iOS app in [`ios/`](ios/README.md).

How the apps use it: [docs/BINDINGS.md](docs/BINDINGS.md). The server and
its API: [docs/SERVER.md](docs/SERVER.md).

## Build and test

```sh
cargo test --all-features        # core, bindings and server tests, incl. property tests
cargo clippy --all-targets --all-features -- -D warnings
cargo build -p amanfold-wasm --target wasm32-unknown-unknown --release
```

CI also runs the same end-to-end flow through the generated Python, Kotlin
and Swift bindings and the WebAssembly build.

Argon2 is compiled with optimisations even in debug builds, so the tests run
with the real production KDF settings.

## Using it (sketch)

```rust
use amanfold_core::{account, vault::{self, Vault}, item::Item, hlc::Clock};

let acct = account::create_account("my long master passphrase")?;
// Print acct.secret_key.to_display() and acct.recovery_code.to_words() in the recovery kit.
let vk = vault::VaultKey::generate();
let grant = vault::grant(&vk, &acct.keys, &acct.keys.public())?;   // upload
let mut personal = Vault::new(vk);
let mut clock = Clock::random();
let rec = personal.save(Item::login("Email", "me@example.com", "pw", "https://mail.example", clock.tick(now_ms))?)?;
// upload `rec`; on other devices: vault.apply_remote(&rec)?
```

## Roadmap for the core

1. ✅ Crypto, item model, sync format (this milestone)
2. ✅ Server authentication with OPAQUE; signed vault manifests (rollback protection)
3. ✅ App bindings: Swift/Kotlin (UniFFI), WebAssembly for the browser; quick unlock, encrypted local cache
4. ✅ Sync server: sign-up with email codes, OPAQUE login, sessions, recovery, verified sync, sharing with roles, key rotation
5. ✅ Browser extension (beta): safe autofill, cards, one-time codes, English and Arabic
6. ✅ Android and iOS apps (beta): system autofill, fingerprint / Face ID unlock, English and Arabic
7. Import/export (CSV, Bitwarden, 1Password, FIDO CXF), fuzzing harness
8. Trusted-contact recovery (Shamir), attachments

## License

Apache-2.0. See [LICENSE](LICENSE).
