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
| `card` | Luhn check, brand detection, expiry parsing; accepts Arabic-Indic and Persian digits |
| `totp` | RFC 6238 one-time codes from secrets or `otpauth://` URIs |
| `generator` | Unbiased random passwords and passphrases |

## Build and test

```sh
cargo test                       # 87 tests incl. property tests
cargo clippy --all-targets -- -D warnings
cargo build -p amanfold-core --target wasm32-unknown-unknown --release
```

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
3. UniFFI bindings for Swift/Kotlin, wasm-bindgen for the extension
4. Import/export (CSV, Bitwarden, 1Password, FIDO CXF), fuzzing harness
5. Trusted-contact recovery (Shamir), attachments

## License

Apache-2.0. See [LICENSE](LICENSE).
