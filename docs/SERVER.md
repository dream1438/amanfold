# Amanfold sync server

`crates/amanfold-server` stores encrypted vaults, runs the server half of
OPAQUE login, and relays sharing between family and team members. It is
built on the same security core as the apps and sees **ciphertext only**.

> **Beta, not independently audited.** See [SECURITY.md](SECURITY.md).

## What it stores

| Table | Contents | Readable by the server? |
| --- | --- | --- |
| `accounts` | email, account bundle (public keys, KDF settings, salt, private keys wrapped twice), OPAQUE record | Email and public keys only. The private keys are wrapped under the master password + Secret Key, or the 256-bit recovery code. The OPAQUE record cannot be guessed against offline (it needs the Secret Key). |
| `sessions` | SHA-256 of each bearer token, device name, times | No token is stored. |
| `email_codes` | hash of the current sign-up or recovery code, expiry, attempt count | No. |
| `vaults` | owner, key epoch, latest signed manifest, size | Item ids and record digests only. |
| `records` | one encrypted, size-padded record per item | No. |
| `members`, `grants` | who belongs to which vault with which role; each member's vault key sealed to their public key and signed by the granter | No: grants open only with the member's private key. |
| `invites` | email invited, role, inviter | Email and role. |

No master password, Secret Key, recovery code, vault key, item key, item
title, URL, username, password or card number ever reaches the server.

## What it enforces

The apps verify everything themselves and assume the server may be hostile
(signed manifests, signed grants, pinned server key). The server's own checks
keep honest devices consistent and stop a buggy or malicious *member* from
corrupting a shared vault:

* **Login**: OPAQUE; unknown emails get fake but stable answers; per-email
  and per-address rate limits; single-use login state that expires in 2 minutes.
* **Sessions**: the bearer token is derived by both sides from the OPAQUE
  session key (`auth::api_token`), so it never travels in the login messages.
  Sessions expire after 30 days unused or 180 days in total; devices can be
  listed and signed out.
* **Sensitive account changes** (new password, new recovery code, deleting
  the account) need a fresh challenge signed with the account's private key.
  A stolen session token alone cannot lock someone out.
* **Commits** are compare-and-swap: a manifest is accepted only if its
  sequence number is exactly one more than the stored one, it is signed by
  the uploader's own account key, every uploaded record is listed in it with
  its exact digest, and afterwards the stored records are exactly what it
  lists (nothing dropped). Otherwise `409 conflict` (sync and retry) or `400`.
* **Roles**: owner, manager, writer, reader. Readers cannot commit (enforced
  by the server only, see SECURITY.md O2). Managers can share with and remove
  writers and readers; only the owner changes roles or deletes the vault.
* **Rotation** after removing a member replaces every record, the manifest
  and the grants in one transaction, and must give a new grant to exactly
  the remaining members with access.
* **Limits**: 2 MiB per request except commits and rotations (64 MiB);
  512 MiB of records per vault; 500 owned vaults per account; 50,000 records
  per request.
* Every response has `Cache-Control: no-store`.

## API

JSON over HTTPS. Byte strings are unpadded base64url. Authenticated calls
send `Authorization: Bearer <base64url token>`. Errors are
`{"error": "<kind>", "message": "..."}` with kinds `bad_request`,
`unauthorized`, `verification_failed`, `forbidden`, `not_found`,
`conflict` (with `seq`), `exists`, `rate_limited`, `too_large`, `internal`.

### Sign up

1. `POST /v1/signup/email {email}` → `202`. Mails an 8-digit code (15
   minutes, 5 tries). If the address already has an account, its owner gets
   a notice instead; the answer is the same.
2. App: `createAccount(password)`.
3. `POST /v1/signup/start {email, code, account_id, request}` → `{response}`,
   where `request = registration.request()`.
4. `POST /v1/signup/finish {email, code, upload, bundle, device_name}` →
   `{account_id, session_id, api_token}`, where
   `upload = registration.finish(response)`.

### Sign in on a device

1. `POST /v1/prelogin {email}` → `{prelogin}`.
2. App: `LoginSession(prelogin, password, secretKey)`.
3. `POST /v1/login/start {email, request}` → `{login_id, response}`, where
   `request = session.start()`.
4. App: `out = session.finish(response, pinnedServerKey)`; fails on a wrong
   password or an impostor server before anything is sent.
5. `POST /v1/login/finish {login_id, finalization, device_name}` →
   `{account_id, session_id, bundle}`. The token is `out.apiToken`.
6. App: `account = session.unlock(bundle)`.

### Sessions and account

| Call | |
| --- | --- |
| `GET /v1/sessions` | Signed-in devices. |
| `DELETE /v1/sessions/current` | Sign out. |
| `DELETE /v1/sessions/{id}` | Sign out another device. |
| `GET /v1/account` | `{account_id, email, bundle}`. |
| `POST /v1/account/challenge {purpose}` | `{challenge}` for `change-password`, `rotate-recovery-code` or `delete-account`; sign it with `account.signChallenge(purpose, challenge)`. Valid 5 minutes, once. |
| `POST /v1/account/password/start {request}` | OPAQUE re-registration step 1 → `{response}`. |
| `POST /v1/account/password/finish {upload, bundle, challenge, signature}` | New password. Signs out every other device. Then `account.updateBundle(bundle)`. |
| `PUT /v1/account/recovery-code {bundle, challenge, signature}` | After `account.rotateRecoveryCode()`. Only the recovery copy may change. |
| `POST /v1/account/delete {challenge, signature}` | Deletes the account, its sessions, its memberships and every vault it owns. |

### Forgotten password

Needs the emailed code **and** the recovery kit (24 words + Secret Key).

1. `POST /v1/recovery/email {email}` → `202` (mail only if the account exists).
2. `POST /v1/recovery/start {email, code}` → `{account_id, bundle, challenge}`.
3. App: `acct = recoverAccount(bundle, words)`,
   `change = acct.changePassword(newPassword, secretKey)`,
   `signature = acct.signChallenge("recover", challenge)`.
4. `POST /v1/recovery/register {email, code, request}` → `{response}`.
5. `POST /v1/recovery/finish {email, code, upload, bundle, challenge, signature}`
   → `204`. Every device is signed out; sign in with the new password.

### Vaults and sync

| Call | |
| --- | --- |
| `POST /v1/vaults {grant}` | Create a vault with the owner's own grant from `account.createVault()`. |
| `GET /v1/vaults` | Vaults the caller belongs to, with role, epoch, seq, `needs_rotation`. |
| `GET /v1/vaults/{id}` | Same for one vault, plus the caller's grants for every epoch (oldest first). |
| `GET /v1/vaults/{id}/sync?since=N` | `{epoch, seq, manifest, records}`: the signed manifest and records written after manifest `N`. |
| `POST /v1/vaults/{id}/commit {manifest, records}` | Upload changes. `409 {seq}` if another device wrote first. |
| `POST /v1/vaults/{id}/rotate {manifest, records, grants}` | New vault key after removing a member. |
| `DELETE /v1/vaults/{id}` | Owner only. |

A device's sync loop:

```text
r = GET /sync?since=<seq from last sync, or 0>
if r.manifest: uploads = vault.sync(r.manifest, r.records, trustedWriters)
records = uploads + local edits since
m = vault.signManifest(account)
POST /commit {manifest: m, records}
  200 → vault.commitAccepted(); remember seq
  409 → start again (nothing is lost; the edits are re-signed next round)
```

If `r.epoch` is newer than `vault.epoch()`, fetch `GET /v1/vaults/{id}` and
call `account.acceptRotation(vault, newestGrant, granterKeys)` first.

### Sharing

1. `POST /v1/vaults/{id}/invites {email, role}` (owner or manager; role
   `manager`, `writer` or `reader`). An email goes out whether or not the
   address has an account.
2. Invitee: `GET /v1/invites` → each invite carries the inviter's public
   keys; compare safety numbers (`fingerprint(keys)`) in person or on a call.
   `POST /v1/invites/{id}/accept` → pending member.
3. Manager: `GET /v1/vaults/{id}/members` → each member's public keys and
   role. After comparing safety numbers,
   `PUT /v1/vaults/{id}/members/{account}/grant {grant}` with
   `account.grantVault(vault, theirKeys)`. The member is now active.
4. Member: `GET /v1/vaults/{id}`, `account.openVault(grant, granterKeys, null)`,
   then sync with the verified writers' keys as `trustedWriters`.

| Call | |
| --- | --- |
| `GET /v1/vaults/{id}/invites` | Open invitations (managers). |
| `DELETE /v1/invites/{id}` | Decline, or cancel. |
| `PUT /v1/vaults/{id}/members/{account}/role {role}` | Owner only. |
| `DELETE /v1/vaults/{id}/members/{account}` | Remove a member, or leave. Returns `needs_rotation`; an owner or manager device should then rotate. |

### Server info

`GET /v1/server` → `{server_identity, opaque_public_key, format, version,
email_verification}`. Production apps ship the OPAQUE public key pinned; this
endpoint is for checks and for self-hosted servers.

## Running it

For production (TLS, backups, monitoring, DNS) see [DEPLOY.md](DEPLOY.md).

```sh
cargo build -p amanfold-server --release
target/release/amanfold-server keygen > /secure/amanfold-secrets.json
# stderr shows the OPAQUE public key to pin in the apps
AMANFOLD_SECRETS_FILE=/secure/amanfold-secrets.json \
AMANFOLD_DB=/var/lib/amanfold/amanfold.db \
AMANFOLD_SMTP_URL='smtps://user:password@smtp.example.com' \
AMANFOLD_MAIL_FROM='Amanfold <no-reply@amanfold.com>' \
AMANFOLD_TRUST_PROXY=1 \
target/release/amanfold-server serve
```

| Variable | Default | |
| --- | --- | --- |
| `AMANFOLD_LISTEN` | `127.0.0.1:8080` | Listen address. Put a TLS reverse proxy (Caddy, nginx) in front. |
| `AMANFOLD_DB` | `amanfold.db` | SQLite file (WAL mode). Back it up with `sqlite3 .backup`. |
| `AMANFOLD_SECRETS_FILE` | required | From `keygen`. Losing it forces every user to sign in again with a new pinned key; leaking it lets someone impersonate the server (but not read vaults). Keep it in a secret manager, readable only by the service. |
| `AMANFOLD_SMTP_URL`, `AMANFOLD_MAIL_FROM` | | Any SMTP provider; TLS required. |
| `AMANFOLD_TRUST_PROXY` | `0` | `1` only behind a proxy that sets `X-Forwarded-For`; the rightmost address is used. |
| `AMANFOLD_DEV` | `0` | `1` writes emails (with codes) to the log instead of sending. Never in production. |
| `AMANFOLD_NO_EMAIL_VERIFICATION` | `0` | `1` for a private self-hosted server: sign-up and recovery skip the email code (recovery still needs the recovery kit). |
| `RUST_LOG` | `info` | Logs never contain request bodies, codes (outside dev mode) or tokens. |

## Limits of this version

* **One process.** Rate limits, pending logins and challenges live in memory,
  and SQLite allows one writer at a time. Fine for the beta; scaling out
  needs a shared store (Redis) and Postgres.
* **No billing yet.** Plans (Free, Premium $2.99, Family $4.99) are not
  enforced; every account can share.
* **No ownership transfer.** Deleting an account deletes the vaults it owns,
  including shared family vaults.
* **Sync is not paginated**; a vault is sent whole on first sync.
* **No push notifications**; devices poll `GET /v1/vaults`.
