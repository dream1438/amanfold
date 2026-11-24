# Amanfold security core: design and internal review

Status: **pre-release, not independently audited.** This document describes
what `crates/amanfold-core` does today, what it protects against, what it does
not yet protect against, and the findings of the internal review. An
independent audit is planned before the beta label is removed.

## Goals

1. The server, and anyone who breaches it, learns nothing about vault contents:
   not passwords, not card numbers, not titles, URLs or field names.
2. A stolen server database cannot be brute-forced offline, even for users
   with weak master passwords.
3. A malicious server cannot trick a client into accepting its keys, weaker
   KDF settings, or data moved between items and vaults.
4. Edits made on several devices offline are never silently lost.
5. The design is public. Its strength comes from user secrets, not from
   hiding the code (Kerckhoffs's principle).

## Primitives

| Purpose | Algorithm | Crate |
| --- | --- | --- |
| Password stretching | Argon2id v1.3, 64 MiB, 3 passes, 4 lanes (floor 64 MiB / 3 passes) | `argon2` (RustCrypto) |
| Key derivation | HKDF-SHA256 | `hkdf`, `sha2` |
| Symmetric encryption | XChaCha20-Poly1305, random 192-bit nonce | `chacha20poly1305` |
| Key agreement | X25519 (low-order points rejected) | `x25519-dalek` |
| Signatures | Ed25519, strict verification | `ed25519-dalek` |
| Recovery code | 256-bit random, BIP-39 English words | `bip39` |
| One-time codes | HOTP/TOTP, RFC 4226 / 6238 | `hmac`, `sha1`, `sha2` |

No custom primitives. `#![forbid(unsafe_code)]` in the crate.

## Key hierarchy

```
master password (NFKC) ── Argon2id(salt) ──┐
                                           ├── HKDF(salt = Secret Key) ──► Account Unlock Key
128-bit Secret Key ────────────────────────┘                                     │ wraps
                                                                                 ▼
recovery code (256-bit) ── HKDF ──► Recovery Key ── wraps ──►  Account keys (X25519 + Ed25519)
                                                                                 │ seal + sign (VaultGrant)
                                                                                 ▼
                                                                         Vault Key (per vault, per epoch)
                                                                                 │ wraps
                                                                                 ▼
                                                                         Item Key (per item)
                                                                                 │ encrypts
                                                                                 ▼
                                                                         padded CBOR item
```

### Associated-data contexts

Every ciphertext is bound to a length-prefixed context string so it cannot be
moved or reinterpreted:

| Ciphertext | Context |
| --- | --- |
| Account keys (password copy) | `account-keys/password` ‖ account id |
| Account keys (recovery copy) | `account-keys/recovery` ‖ account id |
| Vault grant | `sealed` ‖ (`vault-grant` ‖ vault id ‖ epoch) ‖ ephemeral pk ‖ recipient pk |
| Item key | `item-key` ‖ vault id ‖ epoch ‖ item id |
| Item payload | `item-payload` ‖ vault id ‖ item id |

## What the server stores

* `AccountBundle`: account id, KDF parameters, salt, public keys, two wrapped
  copies of the account private keys.
* `VaultGrant` per member per vault epoch: sealed vault key and signature.
* `EncryptedRecord` per item: vault id, item id, key epoch, wrapped item key,
  payload padded to a size bucket (512 B, 1 KiB, ... 64 KiB, then 64 KiB steps).

## Threats and how they are handled

| Threat | Handling | Test |
| --- | --- | --- |
| Server breach, offline guessing | Secret Key adds 128 bits the server never sees | `kdf::derivation_depends_on_every_input` |
| Server sends weak KDF params (downgrade) | Floor enforced client-side before derivation | `account::server_downgrade_of_kdf_params_is_refused` |
| Server sends huge KDF params (DoS on phones) | Ceiling: 1 GiB, 32 passes, 16 lanes | `kdf::param_floor_and_ceiling` |
| Server swaps a user's public key | Unlock checks keys match the bundle; sharing needs a verified fingerprint and a signed grant | `account::swapped_public_key_detected`, `vault::forged_grant_rejected` |
| Server injects its own vault key | Grants are signed by a granter key the client already trusts | `vault::forged_grant_rejected` |
| Server moves ciphertext between items, vaults, slots | Context binding (above) | `sync::server_cannot_move_or_swap_records`, `account::bundles_cannot_be_mixed` |
| Bit flips or truncation | AEAD; every single-bit flip tested | `aead::every_bit_flip_is_detected`, `mutated_records_never_decrypt` |
| Malformed or hostile bytes | Errors, never panics (property test over random input) | `hostile_server_bytes_never_panic` |
| Size leaks item type | Payload padding to buckets | `sync::padding_hides_size` |
| Plaintext on the wire | Checked for titles, usernames, field names | `sync::no_plaintext_on_the_wire` |
| Removed family member keeps reading | Rotation gives every item a fresh item key under a new vault key epoch | `vault::rotation_re_keys_everything` |
| Concurrent offline edits lost | Per-field HLC merge; losers kept in history; merge is commutative, associative, idempotent | `item::merge_is_commutative_associative_idempotent` |
| Same password typed on another keyboard fails | NFKC normalisation (Arabic presentation forms, full-width Latin, combining marks) | `kdf::nfkc_makes_equivalent_passwords_equal` |
| Secrets in logs or crash reports | `Debug` redacted on every secret type; zeroized on drop | `secret::debug_is_redacted` |
| Typos in Secret Key or recovery code | Checksums on both | `kdf::secret_key_round_trip_and_typos`, `account::recovery_code_typo_rejected` |
| Accidental change to derivation | Known-answer test, cross-checked against an independent Python implementation (argon2-cffi + cryptography) | `kdf::derivation_known_answer` |

## Internal review findings (2026-11-24)

Reviewed by Claude, who also wrote the code. This is **not** an independent
review; treat it as a checklist for the auditor, not a clean bill of health.

Fixed during the review:

1. **Merge order dependence.** History recorded a losing value only when its
   text differed from the winner, so three devices could converge to
   different histories. Now compares value and timestamp. Caught by the
   associativity property test.
2. **Weak revocation in the plan.** The plan re-wrapped existing item keys on
   rotation. A removed member who kept old item keys could then read future
   edits if they ever got the ciphertext. Rotation now re-keys every item.
3. **WebAssembly randomness.** `uuid` and `getrandom` had no randomness source
   in the browser build. UUIDs now come from the same OS/`crypto.getRandomValues`
   CSPRNG as keys; the wasm32 build is checked in CI.

Open, to be addressed in later milestones:

| # | Issue | Severity | Plan |
| --- | --- | --- | --- |
| O1 | **Rollback and omission.** The server can withhold records or serve an older (validly encrypted) version to a fresh device. Merge protects devices that already have newer data, not new ones. | Medium | Signed per-vault manifest (item ids + latest HLC) checked on sync. |
| O2 | **Any vault member can write any item.** Fine for personal and family vaults; Teams read-only roles need record signatures. | Medium (Teams) | Sign each record with the author's Ed25519 key; server and clients enforce roles. |
| O3 | **Server login not implemented.** This crate covers encryption only. Authentication to the server (OPAQUE) is the next milestone; it must use a key separate from the unlock key. | High until done | `opaque-ke`, with a distinct HKDF label. |
| O4 | **Memory hygiene is best effort.** Secrets are zeroized on drop, but `String` reallocation, CBOR buffers inside `ciborium`, and the platform UI layer can leave copies. | Low | Pre-size buffers; platform keychain/Secure Enclave integration; mlock where available. |
| O5 | **Clock skew.** A device whose clock is far behind can lose same-field conflicts. Clients must call `Clock::observe` with remote timestamps after sync. | Low | FFI wrapper will do this automatically. |
| O6 | **Grant replay.** The server can re-serve an older, genuinely signed grant for an old epoch. Clients reject records whose epoch does not match their key, but need to learn the current epoch from a signed source. | Low | Include the current epoch in the signed manifest (O1). |
| O7 | **Fingerprint UX.** Protection against key substitution in sharing depends on people comparing fingerprints. | Medium | QR-code verification in the apps; trust-on-first-use with change warnings. |
| O8 | **Passkey private keys** are stored as an ordinary concealed field. | Low | Dedicated passkey type with CXF (FIDO Credential Exchange) import/export. |
| O9 | **No fuzzing harness yet**, only property tests. | Low | `cargo-fuzz` targets for CBOR decode, TOTP URI parse, Secret Key parse. |

## Out of scope for the core

Server authentication, transport (TLS), device biometrics, autofill domain
matching (lives in the extension and mobile apps), and breach monitoring.
