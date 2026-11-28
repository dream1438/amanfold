# Amanfold browser extension

Chrome, Edge and Firefox (Manifest V3). The vault opens only inside the
extension's background service worker, through the Rust security core
compiled to WebAssembly. Pages never see vault data unless the person fills
a form, and then only the one frame that passed the checks below receives
values.

> **Beta, not independently audited.** See [../docs/SECURITY.md](../docs/SECURITY.md).

## What it does

* Sign up (with the recovery kit shown once), sign in on another browser with
  the Secret Key, unlock, lock, auto-lock, sign out.
* Logins, cards and notes: add, edit, move to trash, search, copy, reveal.
* Password and passphrase generator.
* One-time codes (TOTP) shown live and filled into one-time-code fields.
* Fill from the popup or with **Ctrl+Shift+L** (**⌘+Shift+L** on Mac).
* Offers to save or update a login after you submit a form.
* Toolbar badge: number of logins for the open site, `!` on a look-alike site.
* English and Arabic (right to left). Text lives in `src/shared/locales.ts`;
  a missing translation is a compile error.

## How autofill stays safe

Common password-manager failures, and what this extension does instead:

| Problem | Here |
| --- | --- |
| Filling on page load lets hidden forms and scripts harvest passwords. | Nothing is filled until you press Fill or the shortcut. |
| Hidden decoy fields receive passwords. | Only fields that are rendered, not transparent, at least 4 px, on screen and not covered by another element are filled (`src/content/visible.ts`). In Chrome, fields inside frames must also pass the browser's own visibility check (IntersectionObserver v2), which catches transparent or covered frames. |
| A page embeds the real site in a frame and overlays it (clickjacking), or a site embeds another site's login frame. | A login fills only into frames on the login's site **whose every parent frame is on that site too** (`src/background/policy.ts`). |
| Look-alike domains (`paypa1.com`, `pаypal.com` with a Cyrillic а, `paypal-secure.com`) get offered your login. | Matching uses the Public Suffix List inside the Rust core (`crates/amanfold-core/src/site.rs`). Look-alikes are flagged with a red warning, and even choosing the login by hand is refused there. International domain names show their real (punycode) address when it differs. |
| `example.com.evil.io`, `user.github.io` vs `other.github.io`, `http://` versions of `https://` sites. | Never match. A login saved for HTTPS is never offered on HTTP. |
| Card iframes from payment providers either never fill or fill silently. | Card fields in a frame from another site are filled only after you confirm that frame's address. |
| The frame navigates between the check and the fill. | Values are sent to the exact frame document (Chrome `documentId`), and the content script checks its origin again before filling. |

Content scripts can only report "a login form was submitted". They cannot
ask for passwords, cards or item lists. Web pages cannot message the
extension at all.

## What is stored where

* IndexedDB of the extension origin (disk, not readable by content scripts): account bundle (private keys wrapped under
  your master password and Secret Key), the Secret Key, the vault grant, the
  encrypted vault cache, and encrypted records waiting to upload. Nothing in
  it opens without the master password. The session token lets someone with
  disk access download ciphertext only.
* `chrome.storage.session` (memory, cleared when the browser quits, not
  readable by content scripts): the quick-unlock device key, so the vault
  survives the browser stopping the service worker. Locking deletes it.
* Save prompts are kept in memory for 5 minutes, never on disk.

## Build

Needs Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli`
0.2.130 and Node 22.

```sh
cd extension
npm ci
npm run wasm                    # security core → pkg/
npm run build                   # → dist/chrome and dist/firefox
node scripts/build.mjs --server https://your-server.example --zip   # store zips
```

The default server is `http://127.0.0.1:8080` (a local `amanfold-server`, see
[../docs/SERVER.md](../docs/SERVER.md)). The server address can also be changed
on the welcome screen; it must be HTTPS except on this computer.

Load it:

* Chrome / Edge: `chrome://extensions` → Developer mode → Load unpacked →
  `extension/dist/chrome`.
* Firefox: `about:debugging` → This Firefox → Load Temporary Add-on →
  `extension/dist/firefox/manifest.json`.

Run a local server for it:

```sh
cargo build -p amanfold-server
target/debug/amanfold-server keygen > /tmp/amanfold-secrets.json
AMANFOLD_SECRETS_FILE=/tmp/amanfold-secrets.json AMANFOLD_DEV=1 \
AMANFOLD_NO_EMAIL_VERIFICATION=1 target/debug/amanfold-server serve
```

## Tests

```sh
npm run typecheck
npm test        # fill rules, card expiry formats, base64, translations
npm run e2e     # real Chromium + real server, see test/e2e/run.mjs
```

The end-to-end test signs up, adds a login with a one-time code and a card,
and checks: field detection (including Arabic labels and shadow DOM), that
decoy, off-screen and covered fields stay empty, that a same-site frame is
filled and a frame from another site is not, the look-alike warning and
refusal, one-time codes, card selects and an `MM / YY` box, asking before a
payment-provider frame, the save prompt, the vault reopening after the
service worker is stopped, lock and unlock, Arabic right-to-left, and a
second browser signing in with the Secret Key and receiving every item.

## Not yet

* Firefox is built from the same code but not yet tested in Firefox itself.
* Shared family and team vaults (the server and core support them; the
  extension opens only the personal vault so far).
* Passkeys, identities, import from other managers, an in-page suggestion
  menu (left out on purpose for now: in-page menus are the main target of
  extension clickjacking), clearing the clipboard after the popup closes.
* The production server address and its pinned OPAQUE key (until then the
  key is pinned on first contact).
