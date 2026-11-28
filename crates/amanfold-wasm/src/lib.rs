//! Browser bindings of the Amanfold security core, for the browser extension
//! and the web vault. A thin wrapper over `amanfold-ffi` (the same API the
//! Swift and Kotlin apps use), with JavaScript naming:
//!
//! * byte strings are `Uint8Array`, lists of them are arrays of `Uint8Array`;
//! * items, fields and tool results are plain objects (camelCase keys);
//! * errors are `Error` objects whose `name` is the error kind (`Decrypt`,
//!   `Rollback`, `BadSignature`, ...), so callers can branch on `e.name`.
//!
//! Randomness comes from `crypto.getRandomValues` and time from `Date.now`.
//! Run it in an extension service worker, not in a content script: pages
//! must never share memory with the vault.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::Arc;

use amanfold_ffi::{self as ffi, AmanfoldError, FieldInput, ItemKind};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

type JsResult<T> = Result<T, JsValue>;

fn kind(e: &AmanfoldError) -> &'static str {
    match e {
        AmanfoldError::Decrypt => "Decrypt",
        AmanfoldError::WeakKdfParams { .. } => "WeakKdfParams",
        AmanfoldError::InvalidCode { .. } => "InvalidCode",
        AmanfoldError::Malformed { .. } => "Malformed",
        AmanfoldError::InvalidPublicKey => "InvalidPublicKey",
        AmanfoldError::BadSignature => "BadSignature",
        AmanfoldError::Merge { .. } => "Merge",
        AmanfoldError::Rollback { .. } => "Rollback",
        AmanfoldError::InvalidArgument { .. } => "InvalidArgument",
        AmanfoldError::NotFound => "NotFound",
        AmanfoldError::WrongStep { .. } => "WrongStep",
    }
}

fn err(e: AmanfoldError) -> JsValue {
    let j = js_sys::Error::new(&e.to_string());
    j.set_name(kind(&e));
    j.into()
}

fn bad_input(what: &str) -> JsValue {
    err(AmanfoldError::InvalidArgument {
        reason: what.to_string(),
    })
}

fn to_js<T: serde::Serialize>(v: &T) -> JsResult<JsValue> {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|_| bad_input("could not convert result"))
}

fn from_js<T: serde::de::DeserializeOwned>(v: JsValue, what: &str) -> JsResult<T> {
    serde_wasm_bindgen::from_value(v).map_err(|_| bad_input(what))
}

fn byte_list(a: Array) -> JsResult<Vec<Vec<u8>>> {
    a.iter()
        .map(|v| {
            v.dyn_into::<Uint8Array>()
                .map(|u| u.to_vec())
                .map_err(|_| bad_input("expected an array of Uint8Array"))
        })
        .collect()
}

fn saved_list(v: Vec<ffi::SavedItem>) -> Array {
    v.into_iter().map(|s| JsValue::from(SavedItem(s))).collect()
}

fn item_kind(s: &str) -> JsResult<ItemKind> {
    from_js(JsValue::from_str(s), "item kind")
}

// ---------------------------------------------------------------------------

/// An encrypted record to upload.
#[wasm_bindgen]
pub struct SavedItem(ffi::SavedItem);

#[wasm_bindgen]
impl SavedItem {
    /// Item id.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.0.id.clone()
    }
    /// Record bytes for the server.
    #[wasm_bindgen(getter)]
    pub fn record(&self) -> Vec<u8> {
        self.0.record.clone()
    }
}

/// Result of [`create_account`].
#[wasm_bindgen]
pub struct NewAccount(ffi::NewAccount);

#[wasm_bindgen]
impl NewAccount {
    /// The unlocked account.
    #[wasm_bindgen(getter)]
    pub fn account(&self) -> Account {
        Account(self.0.account.clone())
    }
    /// OPAQUE registration to run with the server.
    #[wasm_bindgen(getter)]
    pub fn registration(&self) -> Registration {
        Registration(self.0.registration.clone())
    }
    /// Account bundle to upload.
    #[wasm_bindgen(getter)]
    pub fn bundle(&self) -> Vec<u8> {
        self.0.bundle.clone()
    }
    /// Secret Key for the recovery kit.
    #[wasm_bindgen(getter, js_name = secretKey)]
    pub fn secret_key(&self) -> String {
        self.0.secret_key.clone()
    }
    /// 24 recovery words for the recovery kit.
    #[wasm_bindgen(getter, js_name = recoveryWords)]
    pub fn recovery_words(&self) -> String {
        self.0.recovery_words.clone()
    }
}

/// Create a new account.
#[wasm_bindgen(js_name = createAccount)]
pub fn create_account(password: String) -> JsResult<NewAccount> {
    ffi::create_account(password).map(NewAccount).map_err(err)
}

/// OPAQUE registration (client side).
#[wasm_bindgen]
pub struct Registration(Arc<ffi::Registration>);

#[wasm_bindgen]
impl Registration {
    /// Step 1: request for the server.
    pub fn request(&self) -> JsResult<Vec<u8>> {
        self.0.request().map_err(err)
    }
    /// Step 2: upload for the server, from its response.
    pub fn finish(&self, response: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0.finish(response).map_err(err)
    }
}

/// What [`LoginSession::finish`] returns.
#[wasm_bindgen]
pub struct LoginResult(ffi::LoginResult);

#[wasm_bindgen]
impl LoginResult {
    /// Send to the server.
    #[wasm_bindgen(getter)]
    pub fn finalization(&self) -> Vec<u8> {
        self.0.finalization.clone()
    }
    /// Shared session key.
    #[wasm_bindgen(getter, js_name = sessionKey)]
    pub fn session_key(&self) -> Vec<u8> {
        self.0.session_key.clone()
    }
    /// Bearer token for API requests.
    #[wasm_bindgen(getter, js_name = apiToken)]
    pub fn api_token(&self) -> Vec<u8> {
        self.0.api_token.clone()
    }
    /// Server OPAQUE public key, to pin.
    #[wasm_bindgen(getter, js_name = serverPublicKey)]
    pub fn server_public_key(&self) -> Vec<u8> {
        self.0.server_public_key.clone()
    }
}

/// Signing in (OPAQUE login plus unlock from one Argon2id run).
#[wasm_bindgen]
pub struct LoginSession(Arc<ffi::LoginSession>);

#[wasm_bindgen]
impl LoginSession {
    /// Derive keys from the pre-login answer, password and Secret Key.
    #[wasm_bindgen(constructor)]
    pub fn new(prelogin: Vec<u8>, password: String, secret_key: String) -> JsResult<LoginSession> {
        ffi::LoginSession::new(prelogin, password, secret_key)
            .map(LoginSession)
            .map_err(err)
    }
    /// Account id.
    #[wasm_bindgen(js_name = accountId)]
    pub fn account_id(&self) -> String {
        self.0.account_id()
    }
    /// Step 1: login request.
    pub fn start(&self) -> JsResult<Vec<u8>> {
        self.0.start().map_err(err)
    }
    /// Step 2: check the server's response.
    pub fn finish(
        &self,
        response: Vec<u8>,
        pinned_server_key: Option<Vec<u8>>,
    ) -> JsResult<LoginResult> {
        self.0
            .finish(response, pinned_server_key)
            .map(LoginResult)
            .map_err(err)
    }
    /// Unlock the bundle with this session's key.
    pub fn unlock(&self, bundle: Vec<u8>) -> JsResult<Account> {
        self.0.unlock(bundle).map(Account).map_err(err)
    }
}

/// Unlock with master password and Secret Key.
#[wasm_bindgen(js_name = unlockAccount)]
pub fn unlock_account(bundle: Vec<u8>, password: String, secret_key: String) -> JsResult<Account> {
    ffi::unlock_account(bundle, password, secret_key)
        .map(Account)
        .map_err(err)
}

/// Unlock with the 24 recovery words.
#[wasm_bindgen(js_name = recoverAccount)]
pub fn recover_account(bundle: Vec<u8>, recovery_words: String) -> JsResult<Account> {
    ffi::recover_account(bundle, recovery_words)
        .map(Account)
        .map_err(err)
}

/// Unlock with a device key from [`Account::enable_quick_unlock`], without
/// the password. The extension keeps the device key in memory-only session
/// storage so the vault survives the service worker being stopped.
#[wasm_bindgen(js_name = quickUnlock)]
pub fn quick_unlock(
    bundle: Vec<u8>,
    device_key: Vec<u8>,
    wrapped_keys: Vec<u8>,
) -> JsResult<Account> {
    ffi::quick_unlock(bundle, device_key, wrapped_keys)
        .map(Account)
        .map_err(err)
}

/// Result of [`Account::enable_quick_unlock`].
#[wasm_bindgen]
pub struct QuickUnlock(ffi::QuickUnlock);

#[wasm_bindgen]
impl QuickUnlock {
    /// Device key. Keep it only in memory-only storage.
    #[wasm_bindgen(getter, js_name = deviceKey)]
    pub fn device_key(&self) -> Vec<u8> {
        self.0.device_key.clone()
    }
    /// Account keys wrapped under the device key.
    #[wasm_bindgen(getter, js_name = wrappedKeys)]
    pub fn wrapped_keys(&self) -> Vec<u8> {
        self.0.wrapped_keys.clone()
    }
}

/// Result of [`Account::rotate_recovery_code`].
#[wasm_bindgen]
pub struct RecoveryRotation(ffi::RecoveryRotation);

#[wasm_bindgen]
impl RecoveryRotation {
    /// New bundle to upload.
    #[wasm_bindgen(getter)]
    pub fn bundle(&self) -> Vec<u8> {
        self.0.bundle.clone()
    }
    /// The new 24 words.
    #[wasm_bindgen(getter, js_name = recoveryWords)]
    pub fn recovery_words(&self) -> String {
        self.0.recovery_words.clone()
    }
}

/// Result of [`Account::create_vault`].
#[wasm_bindgen]
pub struct NewVault(ffi::NewVault);

#[wasm_bindgen]
impl NewVault {
    /// The vault.
    #[wasm_bindgen(getter)]
    pub fn vault(&self) -> LocalVault {
        LocalVault(self.0.vault.clone())
    }
    /// Owner's grant to upload.
    #[wasm_bindgen(getter)]
    pub fn grant(&self) -> Vec<u8> {
        self.0.grant.clone()
    }
}

/// Result of [`Account::change_password`].
#[wasm_bindgen]
pub struct PasswordChange(ffi::PasswordChange);

#[wasm_bindgen]
impl PasswordChange {
    /// New bundle.
    #[wasm_bindgen(getter)]
    pub fn bundle(&self) -> Vec<u8> {
        self.0.bundle.clone()
    }
    /// Re-registration.
    #[wasm_bindgen(getter)]
    pub fn registration(&self) -> Registration {
        Registration(self.0.registration.clone())
    }
}

/// An unlocked account.
#[wasm_bindgen]
pub struct Account(Arc<ffi::Account>);

#[wasm_bindgen]
impl Account {
    /// Account id.
    #[wasm_bindgen(js_name = accountId)]
    pub fn account_id(&self) -> String {
        self.0.account_id()
    }
    /// Sign a server challenge (account recovery).
    #[wasm_bindgen(js_name = signChallenge)]
    pub fn sign_challenge(&self, purpose: String, challenge: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0.sign_challenge(purpose, challenge).map_err(err)
    }
    /// Current bundle.
    pub fn bundle(&self) -> Vec<u8> {
        self.0.bundle()
    }
    /// Public keys.
    #[wasm_bindgen(js_name = publicKeys)]
    pub fn public_keys(&self) -> Vec<u8> {
        self.0.public_keys()
    }
    /// Safety number.
    pub fn fingerprint(&self) -> String {
        self.0.fingerprint()
    }
    /// Store a bundle the server accepted.
    #[wasm_bindgen(js_name = updateBundle)]
    pub fn update_bundle(&self, bundle: Vec<u8>) -> JsResult<()> {
        self.0.update_bundle(bundle).map_err(err)
    }
    /// New vault.
    #[wasm_bindgen(js_name = createVault)]
    pub fn create_vault(&self) -> JsResult<NewVault> {
        self.0.create_vault().map(NewVault).map_err(err)
    }
    /// Open a vault from a grant (and optional cache).
    #[wasm_bindgen(js_name = openVault)]
    pub fn open_vault(
        &self,
        grant: Vec<u8>,
        granter_public_keys: Vec<u8>,
        cache: Option<Vec<u8>>,
    ) -> JsResult<LocalVault> {
        self.0
            .open_vault(grant, granter_public_keys, cache)
            .map(LocalVault)
            .map_err(err)
    }
    /// Grant a vault to another account.
    #[wasm_bindgen(js_name = grantVault)]
    pub fn grant_vault(
        &self,
        vault: &LocalVault,
        recipient_public_keys: Vec<u8>,
    ) -> JsResult<Vec<u8>> {
        self.0
            .grant_vault(vault.0.clone(), recipient_public_keys)
            .map_err(err)
    }
    /// Move a vault to a newer key from a new grant.
    #[wasm_bindgen(js_name = acceptRotation)]
    pub fn accept_rotation(
        &self,
        vault: &LocalVault,
        grant: Vec<u8>,
        granter_public_keys: Vec<u8>,
    ) -> JsResult<()> {
        self.0
            .accept_rotation(vault.0.clone(), grant, granter_public_keys)
            .map_err(err)
    }
    /// Turn on quick unlock for this browser session.
    #[wasm_bindgen(js_name = enableQuickUnlock)]
    pub fn enable_quick_unlock(&self) -> JsResult<QuickUnlock> {
        self.0.enable_quick_unlock().map(QuickUnlock).map_err(err)
    }
    /// New recovery code (fresh recovery kit).
    #[wasm_bindgen(js_name = rotateRecoveryCode)]
    pub fn rotate_recovery_code(&self) -> JsResult<RecoveryRotation> {
        self.0
            .rotate_recovery_code()
            .map(RecoveryRotation)
            .map_err(err)
    }
    /// Set a new master password.
    #[wasm_bindgen(js_name = changePassword)]
    pub fn change_password(
        &self,
        new_password: String,
        secret_key: String,
    ) -> JsResult<PasswordChange> {
        self.0
            .change_password(new_password, secret_key)
            .map(PasswordChange)
            .map_err(err)
    }
}

/// A decrypted vault in this browser.
#[wasm_bindgen]
pub struct LocalVault(Arc<ffi::LocalVault>);

#[wasm_bindgen]
impl LocalVault {
    /// Vault id.
    pub fn id(&self) -> String {
        self.0.id()
    }
    /// Key generation.
    pub fn epoch(&self) -> u32 {
        self.0.epoch()
    }
    /// Items as plain objects, newest change first.
    pub fn items(&self, include_trashed: bool) -> JsResult<JsValue> {
        to_js(&self.0.items(include_trashed))
    }
    /// One item, or `undefined`.
    pub fn item(&self, id: String) -> JsResult<JsValue> {
        match self.0.item(id).map_err(err)? {
            Some(v) => to_js(&v),
            None => Ok(JsValue::UNDEFINED),
        }
    }
    /// Logins for the site of `pageUrl`, best match first:
    /// `[{item, strength}]` with strength `"Origin"`, `"Host"` or `"Site"`.
    /// Autofill may offer only these.
    #[wasm_bindgen(js_name = loginsForUrl)]
    pub fn logins_for_url(&self, page_url: String) -> JsResult<JsValue> {
        to_js(&self.0.logins_for_url(page_url))
    }
    /// Saved sites that `pageUrl` imitates (phishing warning).
    #[wasm_bindgen(js_name = lookalikeSites)]
    pub fn lookalike_sites(&self, page_url: String) -> Vec<String> {
        self.0.lookalike_sites(page_url)
    }
    /// Create an item. `kind` is `"Login"`, `"Card"`, `"SecureNote"`,
    /// `"Identity"` or `"Passkey"`; `fields` is `[{name, value}]`.
    #[wasm_bindgen(js_name = createItem)]
    pub fn create_item(&self, kind: String, fields: JsValue) -> JsResult<SavedItem> {
        let f: Vec<FieldInput> = from_js(fields, "fields must be [{name, value}]")?;
        self.0
            .create_item(item_kind(&kind)?, f)
            .map(SavedItem)
            .map_err(err)
    }
    /// Set and remove fields.
    #[wasm_bindgen(js_name = updateItem)]
    pub fn update_item(
        &self,
        id: String,
        set: JsValue,
        remove: Vec<String>,
    ) -> JsResult<SavedItem> {
        let f: Vec<FieldInput> = from_js(set, "fields must be [{name, value}]")?;
        self.0
            .update_item(id, f, remove)
            .map(SavedItem)
            .map_err(err)
    }
    /// Trash or restore.
    #[wasm_bindgen(js_name = setTrashed)]
    pub fn set_trashed(&self, id: String, trashed: bool) -> JsResult<SavedItem> {
        self.0.set_trashed(id, trashed).map(SavedItem).map_err(err)
    }
    /// Verified sync. Returns records to upload.
    pub fn sync(
        &self,
        manifest: Vec<u8>,
        changed: Array,
        trusted_writers: Array,
    ) -> JsResult<Array> {
        self.0
            .sync(manifest, byte_list(changed)?, byte_list(trusted_writers)?)
            .map(saved_list)
            .map_err(err)
    }
    /// Sign a manifest.
    #[wasm_bindgen(js_name = signManifest)]
    pub fn sign_manifest(&self, author: &Account) -> Vec<u8> {
        self.0.sign_manifest(author.0.clone())
    }
    /// The server accepted the last signed manifest.
    #[wasm_bindgen(js_name = commitAccepted)]
    pub fn commit_accepted(&self) -> JsResult<()> {
        self.0.commit_accepted().map_err(err)
    }
    /// Rotate the vault key (after removing a member).
    #[wasm_bindgen(js_name = rotateKey)]
    pub fn rotate_key(&self) -> JsResult<Array> {
        self.0.rotate_key().map(saved_list).map_err(err)
    }
    /// Encrypted snapshot for local storage (IndexedDB).
    #[wasm_bindgen(js_name = exportCache)]
    pub fn export_cache(&self) -> Vec<u8> {
        self.0.export_cache()
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// Default generator rules, as an object.
#[wasm_bindgen(js_name = defaultPasswordRules)]
pub fn default_password_rules() -> JsResult<JsValue> {
    to_js(&ffi::default_password_rules())
}

/// Random password from rules (`{length, lowercase, uppercase, digits, symbols, avoidAmbiguous}`).
#[wasm_bindgen(js_name = generatePassword)]
pub fn generate_password(rules: JsValue) -> JsResult<String> {
    ffi::generate_password(from_js(rules, "password rules")?).map_err(err)
}

/// Random passphrase.
#[wasm_bindgen(js_name = generatePassphrase)]
pub fn generate_passphrase(words: u32, separator: String) -> JsResult<String> {
    ffi::generate_passphrase(words, separator).map_err(err)
}

/// Current one-time code: `{code, secondsRemaining, period}`.
#[wasm_bindgen(js_name = totpNow)]
pub fn totp_now(secret: String) -> JsResult<JsValue> {
    to_js(&ffi::totp_now(secret).map_err(err)?)
}

/// One-time code at a Unix time (seconds).
#[wasm_bindgen(js_name = totpAt)]
pub fn totp_at(secret: String, unix_secs: f64) -> JsResult<JsValue> {
    if !(unix_secs.is_finite() && unix_secs >= 0.0) {
        return Err(bad_input("time"));
    }
    to_js(&ffi::totp_at(secret, unix_secs as u64).map_err(err)?)
}

/// Card number check: `{digits, brand, masked, luhnValid}`.
#[wasm_bindgen(js_name = cardInfo)]
pub fn card_info(number: String) -> JsResult<JsValue> {
    to_js(&ffi::card_info(number).map_err(err)?)
}

/// Canonical form of a typed Secret Key.
#[wasm_bindgen(js_name = checkSecretKey)]
pub fn check_secret_key(text: String) -> JsResult<String> {
    ffi::check_secret_key(text).map_err(err)
}

/// Check typed recovery words.
#[wasm_bindgen(js_name = checkRecoveryWords)]
pub fn check_recovery_words(words: String) -> JsResult<()> {
    ffi::check_recovery_words(words).map_err(err)
}

/// Safety number of public keys.
#[wasm_bindgen]
pub fn fingerprint(public_keys: Vec<u8>) -> JsResult<String> {
    ffi::fingerprint(public_keys).map_err(err)
}

/// How a saved URL matches a page: `"None"`, `"Site"`, `"Host"` or `"Origin"`.
#[wasm_bindgen(js_name = siteMatch)]
pub fn site_match(saved_url: String, page_url: String) -> JsResult<JsValue> {
    to_js(&ffi::site_match(saved_url, page_url))
}

/// A page address as shown to people: `{secure, host, displayHost, site}`,
/// or `undefined` for anything but `http`/`https`.
#[wasm_bindgen(js_name = pageSite)]
pub fn page_site(page_url: String) -> JsResult<JsValue> {
    match ffi::page_site(page_url) {
        Some(s) => to_js(&s),
        None => Ok(JsValue::UNDEFINED),
    }
}

/// Core version.
#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    ffi::core_version()
}

// ---------------------------------------------------------------------------

/// Server halves, for the JavaScript test only.
#[cfg(feature = "test-server")]
#[wasm_bindgen]
pub struct TestServer(Arc<ffi::test_server::TestServer>);

#[cfg(feature = "test-server")]
#[wasm_bindgen]
impl TestServer {
    /// New server.
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> TestServer {
        TestServer(ffi::test_server::TestServer::new())
    }
    /// OPAQUE public key.
    #[wasm_bindgen(js_name = publicKey)]
    pub fn public_key(&self) -> Vec<u8> {
        self.0.public_key()
    }
    /// Pre-login answer.
    pub fn prelogin(&self, bundle: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0.prelogin(bundle).map_err(err)
    }
    /// Registration step 1.
    #[wasm_bindgen(js_name = registrationResponse)]
    pub fn registration_response(&self, account_id: String, request: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0
            .registration_response(account_id, request)
            .map_err(err)
    }
    /// Registration step 2.
    #[wasm_bindgen(js_name = registrationRecord)]
    pub fn registration_record(&self, upload: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0.registration_record(upload).map_err(err)
    }
    /// Login step 1.
    #[wasm_bindgen(js_name = loginResponse)]
    pub fn login_response(
        &self,
        account_id: String,
        record: Vec<u8>,
        request: Vec<u8>,
    ) -> JsResult<Vec<u8>> {
        self.0
            .login_response(account_id, record, request)
            .map_err(err)
    }
    /// Login step 2.
    #[wasm_bindgen(js_name = loginFinish)]
    pub fn login_finish(&self, finalization: Vec<u8>) -> JsResult<Vec<u8>> {
        self.0.login_finish(finalization).map_err(err)
    }
}
