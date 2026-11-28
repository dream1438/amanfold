# Amanfold security core: design and internal review

Status: **pre-release, not independently audited.** This document describes
what `crates/amanfold-core` and the sync server (`crates/amanfold-server`, see
[SERVER.md](SERVER.md)) do today, what they protect against, what they do
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
| Server login | OPAQUE (RFC 9807): Ristretto255 OPRF, 3DH with SHA-512, identity KSF (input pre-stretched) | `opaque-ke` |
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
                                           ├── HKDF(salt = Secret Key) ─┬─► OPAQUE login key (server login)
128-bit Secret Key ────────────────────────┘                            └─► Account Unlock Key
                                                                                 │ wraps
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

## Login (OPAQUE)

1. Client asks for the account's `PreLogin` (account id, salt, KDF params).
   For unknown usernames the server answers with `auth::fake_prelogin`
   (HMAC of the username under a server secret), so the endpoint does not
   reveal which emails have accounts.
2. Client runs Argon2id once and derives two keys under different HKDF
   labels: the unlock key (stays on device) and the OPAQUE login key.
3. OPAQUE login. The server holds an OPAQUE record from which the login key
   cannot be recovered without an online guess for each try; with the Secret
   Key mixed in, guessing is hopeless even with the record. Unknown accounts
   get an indistinguishable fake response.
4. Both sides get a 64-byte session key. The client checks the server's
   OPAQUE public key against the key pinned in the app.
5. Only then does the server hand out the `AccountBundle`; the client unlocks
   it with the unlock key from step 2 (no second Argon2id run).

A password change or KDF upgrade changes the salt, so the client
re-registers with OPAQUE in the same request that uploads the new bundle.

## Signed vault manifests

After each write, the writing device signs a manifest: vault id, key epoch,
sequence number, and every item id with the SHA-256 digest of its current
record. On sync, a device:

* accepts only manifests signed by members it trusts (keys from verified grants);
* refuses a sequence number lower than the last it accepted (rollback);
* refuses any downloaded record that is not the exact version listed;
* refuses if an item listed now, or listed in any manifest it accepted
  before, is missing (withheld or dropped data);
* learns the current key epoch from the signed manifest, so an old grant or
  old epoch is detected.

* refuses a manifest that lists a version of an item it already has but
  does not come with that record (the server hiding someone else's newer
  edit, which this device would otherwise overwrite).

The server accepts a new manifest only if its sequence number is exactly one
more than the stored one, which serialises concurrent writers. A device
records its own manifest as confirmed only after the server accepted it
(`commit_accepted`), so a refused commit leaves its rollback state untouched.

## App bindings (milestone 3)

Apps never call the core's internals. They use one app-facing API
(`crates/amanfold-ffi`), generated for Swift and Kotlin with UniFFI and
wrapped for JavaScript in `crates/amanfold-wasm`. See [BINDINGS.md](BINDINGS.md).

* **Keys stay in Rust.** Account private keys, vault keys, item keys, the
  unlock key, the OPAQUE login key and protocol state live inside opaque
  objects (`Account`, `LocalVault`, `LoginSession`, `Registration`). No call
  returns them.
* **What crosses the boundary** is server-bound bytes (bundle, grants,
  records, manifests, OPAQUE messages), values for the person (Secret Key and
  recovery words once at sign-up, field values when shown, generated
  passwords), the OPAQUE session key, and the quick-unlock device key.
* **Server data is verified in Rust.** Apps hand over raw bytes; parsing,
  signature checks, rollback checks and merging all happen in the core.
* **Quick unlock** (Face ID, fingerprint) wraps the account keys under a
  random 32-byte device key. The app keeps the device key in the iOS Keychain
  (biometry access control) or Android Keystore (user authentication), and
  the wrapped keys in ordinary storage. The master password is never stored.
  Bound to the account id; a key from another account or device fails.
* **Local cache.** `LocalVault::export_cache` seals the replica (items, item
  keys, record digests, confirmed items, last manifest sequence number) under
  the vault key with its own context. Restoring it keeps rollback protection
  across app restarts, which an unencrypted record cache would not.
* **Key rotation by another member** (`Account::accept_rotation`) keeps the
  local items and rollback state, and replaces every local item key, so an
  edit made before the new records arrive is not readable with keys a
  removed member kept.
* **Clock.** The wrapper reads the device clock and feeds every remote item's
  timestamp into the hybrid logical clock after each sync (closes O5).
* **One-shot protocol objects** report `WrongStep` when used out of order and
  stay usable for the correct next step.
* **Test server** halves of the protocols are behind the `test-server`
  feature, off by default; CI checks the shipping web build does not contain
  them.
* **Browser.** Randomness comes from `crypto.getRandomValues`, time from
  `Date.now`. The extension must run the core in its service worker, never in
  a content script.

## Browser extension and autofill (milestone 5)

The extension is described in [extension/README.md](../extension/README.md).
Security-relevant design:

* **Where keys live.** The core runs in the extension's background service
  worker only. Content scripts (which share a process with web pages) never
  hold vault data; they receive values for one fill the person started, in
  one frame, and nothing else. Messages are accepted from extension pages
  (checked by the sender URL the browser sets) and, from content scripts,
  only "a login form was submitted". No `externally_connectable`.
* **Site matching is in the core** (`amanfold_core::site`), so every app uses
  the same rules: registrable domain from the Public Suffix List (ICANN and
  private sections), ASCII/punycode comparison, no match from an HTTPS login
  to an HTTP page, same host and port only for IP addresses, `localhost` and
  unknown suffixes, and only `http`/`https` pages. `LocalVault::logins_for_url`
  is the only list autofill may offer.
* **Look-alike warning.** `LocalVault::lookalike_sites` flags pages whose
  name folds to a saved site's name under common homoglyphs (Cyrillic, Greek,
  digits, `rn`→`m`), is one edit away, uses another suffix, or adds words
  with hyphens. Such pages never match anyway; the warning also blocks
  filling a login there by hand.
* **Frames.** A login fills a frame only if the frame and every ancestor up
  to the top page match the login. Cards fill frames on the top page's site;
  a frame from another site needs a confirmation naming its host, and must
  itself be a child of same-site frames.
* **No automatic fill.** Values are sent only after the popup button or the
  keyboard shortcut. Fields must be rendered, opaque enough (product of
  ancestor opacities ≥ 0.2), at least 4×4 px, on screen and hit-testable at
  three points (not covered). Inside frames, Chrome's IntersectionObserver v2
  `isVisible` must also be true.
* **Navigation races.** Values go to the exact frame document (`documentId`
  in Chrome; `frameId` in Firefox) and the content script re-checks its
  origin before filling.
* **Service worker lifetime.** Browsers stop idle service workers. The
  extension enables quick unlock at unlock time and keeps the device key in
  `chrome.storage.session` (memory only, trusted contexts only), so the vault
  can reopen without the password; locking, auto-lock and quitting the
  browser delete it. This is the same trust as keeping the vault open.
* **Server key pinning** is trust-on-first-use from `GET /v1/server` at
  sign-up or first sign-in, then enforced on every login. Production builds
  must ship the production key pinned (`PINNED` in `session.ts`); see E6.

### Internal review findings (milestone 5, browser extension, 2026-11-28)

Fixed while building:

* Edits, sync and locking could interleave; every request now runs through
  one queue, so a lock can never free the vault in the middle of a sync and a
  sync can never drop an edit made during it.
* Guessed card fields ("security code") on two-step verification pages were
  taken for card CVV fields; guessed card roles now count only in a form
  that has a card number.

Open (extension):

| # | Issue | Severity | Plan |
| --- | --- | --- | --- |
| E1 | **Firefox not yet run.** Built from the same code with a Firefox manifest; Firefox has no IntersectionObserver v2, so frame visibility is not checked there beyond the in-frame checks. | Medium | Test in Firefox; Playwright-driven Firefox run in CI. |
| E2 | **Clipboard** is cleared after 30 s only while the popup stays open. | Low | Offscreen document (Chrome) to clear it later. |
| E3 | **Secret Key and session token on disk** in `storage.local`, like other managers keep their device secrets. Malware with access to the browser profile can copy them, but still needs the master password. | Low | OS keychain via native messaging (desktop app). |
| E4 | **Only the personal vault** is opened; shared vaults wait for the sharing UI with safety-number checks. | — | Next milestone. |
| E5 | **Field detection is heuristic** beyond `autocomplete` attributes; a miss means no fill, never a fill elsewhere. | Low | Grow the detection test page from real sites. |
| E6 | **Server key pinned on first use** until a production server exists. | Medium | Pin the production key in release builds. |

## Mobile apps (milestone 6)

The apps are described in [android/README.md](../android/README.md) and
[ios/README.md](../ios/README.md). Both keep every key inside the Rust core
and follow the browser extension's rules; what differs:

* **Account file at rest.** What the phone remembers (encrypted bundle,
  Secret Key, session token, encrypted cache, pending records) is one file,
  sealed with AES-256-GCM under a key that never leaves the phone: an
  Android Keystore key (hardware-backed where available), or a Keychain item
  `AfterFirstUnlockThisDeviceOnly` on iOS. Both are excluded from backups
  (`allowBackup=false` plus data-extraction rules on Android; this-device-only
  Keychain classes and `kSecAttrSynchronizable = false` on iOS).
* **Biometric unlock** wraps the account keys under a random device key
  (`enable_quick_unlock`). On Android that key is encrypted by a Keystore key
  that needs a strong biometric for every use (`BiometricPrompt` with a
  `CryptoObject`) and is invalidated when a fingerprint or face is added. On
  iOS it is a Keychain item with `.biometryCurrentSet`, gone when biometrics
  change or the passcode is removed. A changed or failed key turns biometric
  unlock off and asks for the master password.
* **Screens.** Android sets `FLAG_SECURE` (no screenshots or app-switcher
  preview); iOS covers the window whenever the app is not active. Copied
  values are marked sensitive and cleared after 60 seconds; on iOS they stay
  on this device (no Universal Clipboard).
* **Android autofill.** The service never puts a value in a fill response
  before the person unlocks and picks a login in Amanfold's own screen
  (dataset authentication). Web pages are matched with
  `logins_for_url` only when the requesting app is a browser on a fixed list
  and the field is in the focused document of the top page; a login frame
  from another site gets nothing, and a card frame from another site needs a
  confirmation naming its host. For apps, the identity is the package name
  **plus the SHA-256 of its signing certificate**
  (`androidapp://pkg?cert=…`, `android_app_url`), so a look-alike app with
  the same package name but another signer never matches. Apps never get web
  logins automatically, whatever domain they claim. Filling another login
  needs a search and a confirmation, and can be remembered for that app.
* **iOS AutoFill.** iOS gives the extension the page's domain (from Safari,
  or an app's verified associated domains), and the extension offers only
  `logins_for_url` for it, with the look-alike, punycode and http warnings.
  The QuickType bar's credential identity store is left empty, so site names
  and usernames never sit outside the encrypted vault.

### Internal review findings (milestone 6, mobile apps, 2026-11-28)

Checked while building:

* Logins not matched to the app or page are never in the first list; filling
  one needs a search and a confirmation naming the target.
* A password captured by Android's "save" prompt is not put in the pending
  intent; it stays in the service's memory under a random token, is taken
  once, and is dropped after ten minutes.

Open (mobile):

| # | Issue | Severity | Plan |
| --- | --- | --- | --- |
| A1 | **Not yet run on real phones.** Both apps compile and their logic is tested against a real server in CI, but autofill, biometrics and the Keystore/Keychain need devices. | High | Device testing before the beta goes out (TestFlight, Play internal testing). |
| A2 | **Trusted browser list on Android is by package name.** A sideloaded app reusing a browser's package name would be trusted to report page addresses. | Medium | Pin the signing certificates of the listed browsers. |
| A3 | **Browsers that report no scheme** are assumed to show `https` pages. | Low | Use the scheme where the browser reports it (already done); review per browser. |
| A4 | **Cards on iOS** cannot be offered through AutoFill (the platform supports passwords and passkeys only); people copy them in the app. | — | Revisit with new iOS APIs. |
| A5 | **Argon2id in the iOS AutoFill extension.** Extensions have a tight memory limit; unlocking with the master password there runs Argon2id with 64 MiB. Face ID unlock avoids it. | Medium | Measure on older iPhones; if needed, ask people to unlock in the app. |
| A6 | **Release builds are not minified** (R8 off) so the JNA/UniFFI classes stay intact. | Low | Add keep rules and turn R8 on. |
| A7 | **Server key pinned on first use**, as in E6. | Medium | Ship the production key pinned. |

## What the server stores

* `AccountBundle`: account id, KDF parameters, salt, public keys, two wrapped
  copies of the account private keys.
* `VaultGrant` per member per vault epoch: sealed vault key and signature.
* OPAQUE record per account, and its own OPAQUE server keys (in an HSM or secret manager).
* `SignedManifest` per vault (latest only).
* `EncryptedRecord` per item: vault id, item id, key epoch, wrapped item key,
  payload padded to a size bucket (512 B, 1 KiB, ... 64 KiB, then 64 KiB steps).
* Email address, hashes of session tokens and of emailed codes, vault
  memberships and roles, open invitations. Full table list in [SERVER.md](SERVER.md).

What the server can still learn (metadata): which emails have accounts, how
many items each vault has and their padded sizes, when devices sync, and who
shares vaults with whom.

## Sync server (milestone 4)

`crates/amanfold-server` (Rust, axum, SQLite) is built on this core. Its
checks are a second line of defence; clients never rely on them.

* **Login.** OPAQUE server half with fake answers for unknown emails; rate
  limits per email (10 per 15 minutes) and per address. The API token is
  derived from the OPAQUE session key on both sides (`auth::api_token`) and
  only its hash is stored.
* **Proof of key possession.** Changing the password, replacing the recovery
  code, deleting the account and forgotten-password recovery need a fresh
  server challenge signed with the account's Ed25519 key
  (`auth::sign_challenge`, bound to account id and purpose). A stolen session
  token, or access to the email inbox, is not enough.
* **Email codes** for sign-up and recovery: 8 digits, 15 minutes, 5 tries,
  stored hashed. Sign-up and recovery give the same answer whether or not the
  address has an account.
* **Writes** are compare-and-swap on the manifest sequence number; manifests
  must be signed by the uploader; uploads must match the manifest; nothing
  may be dropped. Readers cannot write. Rotation must re-grant exactly the
  remaining members.

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
| Stolen server database used to guess passwords | OPAQUE record needs an online guess per try; input includes the Secret Key | `auth::wrong_key_fails`, `opaque_signup_login_unlock_and_password_change` |
| Impostor server or stolen record replayed elsewhere | Record bound to account id and server identity; server key pinned in apps | `auth::record_is_bound_to_account_and_server`, `auth::pinning_detects_key_change` |
| Login endpoint reveals which emails are registered | Fake OPAQUE response and fake pre-login parameters for unknown accounts | `auth::unknown_account_gets_a_response_but_cannot_log_in`, `auth::fake_prelogin_is_stable_and_distinct` |
| Server serves an old item version | Manifest digest check | `manifest::old_record_version_is_rejected` |
| Server withholds or drops items | Manifest lists all items; previously confirmed items must stay | `manifest::withheld_item_is_detected`, `manifest::server_cannot_drop_a_confirmed_item` |
| Server replays an old manifest | Sequence number never goes back | `manifest::old_manifest_is_rejected_after_newer_one` |
| Server forges or edits a manifest | Ed25519 signature from a trusted member | `manifest::untrusted_or_tampered_manifest_rejected` |
| Device stolen, quick-unlock data copied | Device key in hardware-backed keychain; wrapped keys useless without it; bound to account | `account::device_quick_unlock` |
| Local cache edited or rolled back on disk | Sealed under the vault key with a vault/epoch context | `vault::cache_round_trip_keeps_rollback_state` |
| App restart forgets the last manifest seen | Sequence number and confirmed items kept in the sealed cache | `vault::cache_round_trip_keeps_rollback_state`, binding tests |
| Hostile member reuses an item id with another kind | Merge refused; local item and digests untouched | `vault::failed_merge_keeps_the_local_item` |
| Old item keys reused after someone else rotates | `rekey` replaces every local item key | `vault::rekey_after_someone_else_rotates` |
| App uses a protocol object out of order | `WrongStep` error, state kept | `amanfold-ffi` `tests/api.rs` |
| Same checks on every platform | End-to-end tests through the generated Python, Kotlin and Swift bindings and the wasm build | `crates/amanfold-ffi/tests/bindings/*`, `crates/amanfold-wasm/tests/flow.cjs` |
| Server hides a newer version of an item the device already has | Listed digest must match the last confirmed version or come with the record | `manifest::hidden_newer_version_of_a_known_item_is_detected` |
| Device stuck after another device wrote first | Own manifest confirmed only after the server accepts it | `manifest::refused_commit_leaves_rollback_state_alone`, server `concurrent_writers_conflict_then_both_edits_survive` |
| Stolen session token used to change the password or delete the account | Signed challenge with the account key required | server `password_change_needs_the_account_key_and_signs_out_other_devices` |
| Email inbox taken over, password reset attempted | Recovery needs the emailed code and a signature from the key unlocked with the 24 words | server `forgotten_password_recovery_with_email_code_and_recovery_words` |
| Online password guessing | 10 login attempts per email per 15 minutes, plus per-address limits | server `login_attempts_are_rate_limited_per_email` |
| Guessing an emailed code | 5 tries, then the code is void | server `signup_needs_the_emailed_code_and_hides_existing_accounts` |
| Member drops items, uploads records that do not match, or writes as a reader | Server checks against the signed manifest and roles; clients check again | server `server_rejects_inconsistent_or_unauthorised_uploads`, `family_sharing_roles_removal_and_rotation` |
| Removed member keeps access | Access ends at the server at once; rotation must re-grant only remaining members | server `family_sharing_roles_removal_and_rotation` |
| Accidental change to derivation | Known-answer test, cross-checked against an independent Python implementation (argon2-cffi + cryptography) | `kdf::derivation_known_answer` |

## Internal review findings (milestone 4, sync server, 2026-11-27)

Found by running the real apps' API against the real server:

1. **Server could hide a newer edit (fixed in the core).** `sync_verified`
   only checked that every listed item existed locally. A server could list
   another device's newer version but not send it; the device kept its old
   copy and its next upload silently replaced the other device's edit. Now
   a listed digest must match the last confirmed version, the device's own
   version, or a record sent with it. Regression test added.
2. **Device stuck after a save conflict (fixed in the core).**
   `sign_manifest` marked the device's own items as confirmed and advanced
   the sequence number before the server accepted them. After a `409` (two
   devices saving at once, which is normal), the honest server's manifest
   looked like dropped data and the device refused every later sync. Signing
   no longer changes rollback state; apps call `commit_accepted` after a
   successful upload. New API: `LocalVault.commitAccepted()`.
3. **Account takeover with a session token.** A password change through the
   server needed only a session; it now also needs a signed challenge.
4. **Recovery needed a way to prove the recovery kit** without the old
   password: added `auth::sign_challenge` / `Account.signChallenge`.

## Internal review findings (milestone 3, 2026-11-27)

1. **Data loss on a failed merge (fixed).** `Vault::save` and
   `Vault::apply_remote` removed the local item from the map before merging.
   A record that reused an item id with a different kind (only a vault member
   can produce one) made the merge fail and dropped the local item from
   memory and from the next cache export. Both now merge first and change
   nothing on failure. Regression test added.
2. **Login session bricked by an early `finish` (fixed during development).**
   Calling `finish` before `start` consumed the session's state. It now
   reports `WrongStep` and the session stays usable.
3. **Rollback state was memory-only.** Without the sealed cache, every app
   restart reset the "highest manifest seen" check, which would have let a
   server replay old data after a restart. Added `export_cache`/`from_cache`.
4. **Rotation by another member** previously had no client path that kept
   rollback state; added `rekey` with fresh item keys.
5. **Secrets as platform strings** (Swift `String`, Kotlin `String`, JS
   strings) cannot be wiped. Kept to what the person must see (O4 updated).

## Internal review findings (milestone 2, 2026-11-24)

1. **Login key separated from unlock key** by HKDF label, so the OPAQUE
   exchange gives the server nothing that helps decrypt the vault.
2. **Account enumeration** through pre-login closed with deterministic fake
   parameters, and through OPAQUE with its built-in fake responses.
3. **Dependency advisory** found by `cargo audit` and traced as unreachable (O10).

## Internal review findings (milestone 1, 2026-11-24)

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
| O1 | ~~Rollback and omission.~~ **Mostly fixed (milestone 2)** by signed manifests. Remaining: a "freeze" (server shows a device an old but consistent state and hides all newer manifests) and a brand-new device's first sync, which has no earlier sequence number to compare. | Low | Signed timestamps in manifests with a staleness warning; device-to-device checks. |
| O2 | **Any vault member can write any item.** Fine for personal and family vaults; Teams read-only roles need record signatures. | Medium (Teams) | Sign each record with the author's Ed25519 key; server and clients enforce roles. |
| O3 | ~~Server login not implemented.~~ **Fixed (milestone 2)**: OPAQUE with a login key separate from the unlock key. ~~Server must still rate-limit login attempts.~~ **Done (milestone 4).** | | |
| O4 | **Memory hygiene is best effort.** Secrets are zeroized on drop inside Rust, but `String` reallocation, CBOR buffers inside `ciborium`, and anything handed to Swift, Kotlin or JavaScript (field values, generated passwords, the Secret Key and recovery words at sign-up, the session and device keys) can leave copies the core cannot wipe. | Low | Pre-size buffers; reveal fields one at a time in the UI; mlock where available. |
| O5 | ~~Clock skew.~~ **Fixed (milestone 3)**: the app-facing `LocalVault` observes every remote timestamp after sync and on open. A device clock that is far behind can still order its own edits before older remote ones until it syncs. | Low | |
| O6 | ~~Grant replay.~~ **Fixed (milestone 2)**: the signed manifest carries the current epoch; a device on an older epoch is told to fetch the new grant. | | |
| O7 | **Fingerprint UX.** Protection against key substitution in sharing depends on people comparing fingerprints. | Medium | QR-code verification in the apps; trust-on-first-use with change warnings. |
| O8 | **Passkey private keys** are stored as an ordinary concealed field. | Low | Dedicated passkey type with CXF (FIDO Credential Exchange) import/export. |
| O9 | **No fuzzing harness yet**, only property tests. | Low | `cargo-fuzz` targets for CBOR decode, TOTP URI parse, Secret Key parse, OPAQUE messages. |
| O10 | **Dependency advisory RUSTSEC-2026-0387** (crypto-bigint 0.5 timing). Pulled in only as a trait dependency of `elliptic-curve` 0.13 via `opaque-ke`. Our Ristretto255 suite uses curve25519-dalek arithmetic; the only `elliptic-curve` code on that path is SHA-512 message expansion. Not reachable; ignored in `.cargo/audit.toml` with this reason. Also flagged (warning only): RUSTSEC-2026-0374, `basic-toml` unmaintained, used by UniFFI's template engine at build time to read `uniffi.toml`; not in the code apps run. | Low | Upgrade when `opaque-ke` moves to `elliptic-curve` 0.14, and UniFFI off `basic-toml`. |
| O11 | **Manifest writers.** Any member in `trusted_writers` can sign a manifest. Removing a member must also remove them from every device's trusted list, which needs the membership list itself to be signed. | Medium (families/Teams) | Signed membership log per vault (with O2). |
| O12 | **Release builds abort on panic** (`panic = "abort"`), so a bug that panics inside the core closes the app instead of returning an error. Chosen so a panic can never leave half-updated secret state behind; property tests cover hostile input. | Low | Fuzzing (O9). |
| O13 | **Cache across a rotation** opens only with the old grant; apps must open with the old grant and then call `accept_rotation`, or the rollback state for that vault restarts. The server keeps every epoch's grant for current members, so the old one stays available. | Low | Document in app integration guide; keep last grant locally. |
| O14 | **Server state in one process.** Rate limits, pending logins and challenges are in memory; a restart clears them, and several server instances would each count separately. | Low (beta) | Shared store (Redis) and Postgres before scaling out. |
| O15 | **Metadata.** The server sees emails, item counts and padded sizes, sync times and the sharing graph. | Low | Document in the privacy policy; consider per-vault item-count padding. |
| O16 | **Deleting an owner's account deletes shared vaults** it owns, including family vaults, because there is no ownership transfer yet. | Medium (families) | Ownership transfer to a manager before deletion. |
| O17 | **Rotation by a manager.** After a manager rotates, other members receive a grant signed by the manager; apps must treat verified managers as trusted granters (part of O11's signed membership log). | Medium (Teams) | With O11. |
| O18 | **Dependency review for the server** (axum, rusqlite with bundled SQLite, lettre with rustls) runs in CI's `cargo audit`; not yet reviewed by hand. | Low | Review before launch. |

## Out of scope

Transport (TLS is terminated by a reverse proxy in front of the server),
device biometrics, autofill domain matching (lives in the extension and
mobile apps), billing, and breach monitoring.
