//! The app-facing API of the Amanfold security core.
//!
//! Every Amanfold app (iOS, Android, desktop, browser extension, web vault)
//! calls the core through this one API, so security decisions live in Rust
//! and are made the same way everywhere. Swift, Kotlin and Python bindings are
//! generated from it with UniFFI; the browser crate (`amanfold-wasm`) wraps the
//! same functions for JavaScript.
//!
//! Design rules:
//!
//! * Private keys, vault keys, item keys and protocol state never cross the
//!   boundary. Apps hold opaque objects ([`Account`], [`LocalVault`],
//!   [`LoginSession`], [`Registration`]) that keep them inside Rust.
//! * What does cross is either for the server (opaque byte strings: account
//!   bundle, grants, encrypted records, manifests, OPAQUE messages), for the
//!   person (the Secret Key, recovery words, field values they asked to see),
//!   or for the platform's secure storage (the quick-unlock device key).
//! * Server data is always parsed and verified here, never by the app.
//! * Only the client halves of the protocols are included. The server halves
//!   are behind the `test-server` feature, for binding tests only.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::{Arc, Mutex, MutexGuard};

use amanfold_core::{
    Error as CoreError, account,
    aead::{Envelope, SymmetricKey},
    auth, card,
    generator::{self, PasswordRules as CoreRules},
    hlc::Clock,
    item::{self, FieldValue, Item},
    kdf::{AccountSecrets, SecretKey},
    keys::{AccountKeys, PublicKeys},
    manifest::SignedManifest,
    site,
    sync::{self, EncryptedRecord},
    totp::Totp,
    vault::{self, Vault, VaultGrant},
};
use uuid::Uuid;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!("amanfold");

#[cfg(feature = "test-server")]
pub mod test_server;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every error the core can report. Show `Decrypt` and `Rollback` to the
/// person with care: the first usually means a typo, the second means the
/// server misbehaved and its response must not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum AmanfoldError {
    /// Wrong password, Secret Key, recovery code or device key, or the data
    /// was tampered with.
    #[error("decryption failed")]
    Decrypt,
    /// The server sent key-derivation settings below the floor or above the
    /// ceiling (a downgrade or denial-of-service attempt, or a bug).
    #[error("key derivation parameters rejected: {reason}")]
    WeakKdfParams {
        /// Details.
        reason: String,
    },
    /// A Secret Key or recovery code was typed incorrectly.
    #[error("invalid {reason}")]
    InvalidCode {
        /// Which code.
        reason: String,
    },
    /// Bytes from the server or storage could not be parsed.
    #[error("malformed data: {reason}")]
    Malformed {
        /// Details.
        reason: String,
    },
    /// A public key was invalid.
    #[error("invalid public key")]
    InvalidPublicKey,
    /// A signature did not verify: the data did not come from who it claims.
    #[error("signature verification failed")]
    BadSignature,
    /// Two versions of an item could not be merged.
    #[error("merge conflict: {reason}")]
    Merge {
        /// Details.
        reason: String,
    },
    /// The server served older data than this device already saw, or left
    /// data out. Do not use the response; tell the person and report it.
    #[error("possible rollback or withheld data: {reason}")]
    Rollback {
        /// Details.
        reason: String,
    },
    /// The app passed an invalid value.
    #[error("invalid argument: {reason}")]
    InvalidArgument {
        /// Details.
        reason: String,
    },
    /// No item with that id in this vault.
    #[error("item not found")]
    NotFound,
    /// A one-shot protocol object was used out of order or twice.
    #[error("wrong step: {reason}")]
    WrongStep {
        /// Details.
        reason: String,
    },
}

impl From<CoreError> for AmanfoldError {
    fn from(e: CoreError) -> Self {
        let r = |s: &str| s.to_string();
        match e {
            CoreError::Decrypt => Self::Decrypt,
            CoreError::WeakKdfParams(s) => Self::WeakKdfParams { reason: r(s) },
            CoreError::InvalidCode(s) => Self::InvalidCode { reason: r(s) },
            CoreError::Malformed(s) => Self::Malformed { reason: r(s) },
            CoreError::InvalidPublicKey => Self::InvalidPublicKey,
            CoreError::BadSignature => Self::BadSignature,
            CoreError::Merge(s) => Self::Merge { reason: r(s) },
            CoreError::Rollback(s) => Self::Rollback { reason: r(s) },
            CoreError::InvalidArgument(s) => Self::InvalidArgument { reason: r(s) },
        }
    }
}

/// Result type of this API.
pub type Result<T> = core::result::Result<T, AmanfoldError>;

fn invalid(reason: &str) -> AmanfoldError {
    AmanfoldError::InvalidArgument {
        reason: reason.to_string(),
    }
}

fn wrong_step(reason: &str) -> AmanfoldError {
    AmanfoldError::WrongStep {
        reason: reason.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn enc<T: serde::Serialize>(v: &T) -> Vec<u8> {
    sync::to_cbor(v).to_vec()
}

fn dec<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T> {
    Ok(sync::from_cbor(b)?)
}

fn parse_id(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|_| invalid("item id"))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding the lock cannot leave half-written secrets
    // behind (every update builds the new value first), so keep going.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

fn public_keys(b: &[u8]) -> Result<PublicKeys> {
    dec(b)
}

// ---------------------------------------------------------------------------
// Plain data types
// ---------------------------------------------------------------------------

/// What kind of item this is. Fixed at creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ItemKind {
    /// Website or app login.
    Login,
    /// Payment card.
    Card,
    /// Secure note.
    SecureNote,
    /// Name, address, phone, email.
    Identity,
    /// Passkey.
    Passkey,
}

impl From<ItemKind> for item::ItemKind {
    fn from(k: ItemKind) -> Self {
        match k {
            ItemKind::Login => Self::Login,
            ItemKind::Card => Self::Card,
            ItemKind::SecureNote => Self::SecureNote,
            ItemKind::Identity => Self::Identity,
            ItemKind::Passkey => Self::Passkey,
        }
    }
}

impl From<item::ItemKind> for ItemKind {
    fn from(k: item::ItemKind) -> Self {
        match k {
            item::ItemKind::Login => Self::Login,
            item::ItemKind::Card => Self::Card,
            item::ItemKind::SecureNote => Self::SecureNote,
            item::ItemKind::Identity => Self::Identity,
            item::ItemKind::Passkey => Self::Passkey,
        }
    }
}

/// A field to set. Standard names: `title`, `notes`, `username`, `password`,
/// `url`, `totp`, `cardholder`, `card_number`, `exp_month`, `exp_year`,
/// `cvv`, `pin`, `rp_id`, `credential_id`, `user_handle`, `private_key`.
/// Custom fields start with `custom:`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FieldInput {
    /// Field name.
    pub name: String,
    /// New value.
    pub value: String,
}

/// A field as shown to the person.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FieldView {
    /// Field name.
    pub name: String,
    /// Value. Hide `concealed` values until the person asks to see them.
    pub value: String,
    /// Hidden by default (passwords, card numbers, CVV, keys).
    pub concealed: bool,
}

/// A decrypted item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ItemView {
    /// Item id.
    pub id: String,
    /// Kind.
    pub kind: ItemKind,
    /// Fields that have a value, sorted by name.
    pub fields: Vec<FieldView>,
    /// In the trash.
    pub trashed: bool,
    /// Creation time, Unix milliseconds.
    pub created_ms: u64,
    /// Last change, Unix milliseconds.
    pub modified_ms: u64,
}

impl ItemView {
    fn of(i: &Item) -> Self {
        Self {
            id: i.id().to_string(),
            kind: i.kind().into(),
            fields: i
                .fields()
                .iter()
                .filter_map(|(name, f)| {
                    let concealed = matches!(f.value, FieldValue::Concealed(_));
                    f.value.as_str().map(|v| FieldView {
                        name: name.clone(),
                        value: v.to_string(),
                        concealed,
                    })
                })
                .collect(),
            trashed: i.is_deleted(),
            created_ms: i.created().millis,
            modified_ms: i.last_modified().millis,
        }
    }
}

/// An encrypted record to upload, with the id the server files it under.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SavedItem {
    /// Item id.
    pub id: String,
    /// Encrypted record bytes for the server.
    pub record: Vec<u8>,
}

impl SavedItem {
    fn of(r: &EncryptedRecord) -> Self {
        Self {
            id: r.item_id.to_string(),
            record: enc(r),
        }
    }
}

/// Password generator settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct PasswordRules {
    /// Length.
    pub length: u32,
    /// a-z.
    pub lowercase: bool,
    /// A-Z.
    pub uppercase: bool,
    /// 0-9.
    pub digits: bool,
    /// Symbols.
    pub symbols: bool,
    /// Leave out look-alikes (0 O o 1 l I).
    pub avoid_ambiguous: bool,
}

/// A one-time code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct TotpCode {
    /// The code.
    pub code: String,
    /// Seconds until it changes.
    pub seconds_remaining: u64,
    /// Period in seconds.
    pub period: u64,
}

/// Payment card brand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(missing_docs)]
pub enum CardBrand {
    Visa,
    Mastercard,
    Amex,
    Discover,
    Diners,
    Jcb,
    UnionPay,
    Maestro,
    Unknown,
}

/// What the core can tell from a typed card number.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CardInfo {
    /// ASCII digits only (Arabic-Indic and Persian digits are converted).
    pub digits: String,
    /// Brand.
    pub brand: CardBrand,
    /// Display form, for example `•••• 1111`.
    pub masked: String,
    /// Luhn checksum passes.
    pub luhn_valid: bool,
}

// ---------------------------------------------------------------------------
// Sign-up and OPAQUE registration
// ---------------------------------------------------------------------------

/// Result of [`create_account`].
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NewAccount {
    /// The unlocked account.
    pub account: Arc<Account>,
    /// Register the login with the server.
    pub registration: Arc<Registration>,
    /// Upload to the server together with the registration upload.
    pub bundle: Vec<u8>,
    /// Show once, print in the recovery kit, and keep in the device keychain.
    pub secret_key: String,
    /// Show once and print in the recovery kit only. Never store it.
    pub recovery_words: String,
}

/// Create a new account. Runs Argon2id once (about a second on a phone).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn create_account(password: String) -> Result<NewAccount> {
    let a = account::create_account(&password)?;
    Ok(NewAccount {
        bundle: enc(&a.bundle),
        secret_key: a.secret_key.to_display().expose().to_string(),
        recovery_words: a.recovery_code.to_words().expose().to_string(),
        registration: Arc::new(Registration::new(a.bundle.account_id, a.auth_key)),
        account: Arc::new(Account::new(a.keys, a.bundle)),
    })
}

/// Client side of OPAQUE registration (sign-up or password change): two
/// messages, in order.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct Registration {
    account_id: Uuid,
    auth_key: amanfold_core::secret::SecretBytes<32>,
    state: Mutex<Option<auth::ClientRegistrationState>>,
    started: Mutex<bool>,
}

impl Registration {
    fn new(account_id: Uuid, auth_key: amanfold_core::secret::SecretBytes<32>) -> Self {
        Self {
            account_id,
            auth_key,
            state: Mutex::new(None),
            started: Mutex::new(false),
        }
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl Registration {
    /// Step 1: the registration request to send to the server.
    pub fn request(&self) -> Result<Vec<u8>> {
        let mut started = lock(&self.started);
        if *started {
            return Err(wrong_step("registration already started"));
        }
        let (st, req) = auth::client_registration_start(&self.auth_key)?;
        *lock(&self.state) = Some(st);
        *started = true;
        Ok(req)
    }

    /// Step 2: given the server's response, the upload that completes
    /// registration. Send it with the account bundle.
    pub fn finish(&self, response: Vec<u8>) -> Result<Vec<u8>> {
        let st = lock(&self.state)
            .take()
            .ok_or_else(|| wrong_step("call request() first, once"))?;
        Ok(auth::client_registration_finish(
            st,
            &self.auth_key,
            &response,
            &self.account_id,
        )?)
    }
}

// ---------------------------------------------------------------------------
// Login and unlock
// ---------------------------------------------------------------------------

/// What [`LoginSession::finish`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct LoginResult {
    /// Send to the server to complete the login.
    pub finalization: Vec<u8>,
    /// Shared session key (64 bytes).
    pub session_key: Vec<u8>,
    /// Bearer token for later API requests (32 bytes, derived from the
    /// session key; the server derives the same one). Keep it in memory or
    /// the device keychain, and send it as `Authorization: Bearer`
    /// (base64url, no padding).
    pub api_token: Vec<u8>,
    /// The server's OPAQUE public key. Store it and pass it as
    /// `pinned_server_key` on later logins.
    pub server_public_key: Vec<u8>,
}

enum LoginStep {
    Ready,
    Started(Box<auth::ClientLoginState>),
    Done,
}

/// Signing in on a device: one Argon2id run gives both the OPAQUE login key
/// and the key that unlocks the account.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct LoginSession {
    account_id: Uuid,
    secrets: AccountSecrets,
    step: Mutex<LoginStep>,
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl LoginSession {
    /// Derive keys from the server's pre-login answer, the master password
    /// and the Secret Key. Rejects KDF settings below the floor.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new(prelogin: Vec<u8>, password: String, secret_key: String) -> Result<Arc<Self>> {
        let pre: account::PreLogin = dec(&prelogin)?;
        let sk = SecretKey::parse(&secret_key)?;
        let secrets = pre.derive(&password, &sk)?;
        Ok(Arc::new(Self {
            account_id: pre.account_id,
            secrets,
            step: Mutex::new(LoginStep::Ready),
        }))
    }

    /// Account id from the pre-login answer.
    pub fn account_id(&self) -> String {
        self.account_id.to_string()
    }

    /// Step 1: the login request to send to the server.
    pub fn start(&self) -> Result<Vec<u8>> {
        let mut step = lock(&self.step);
        if !matches!(*step, LoginStep::Ready) {
            return Err(wrong_step("login already started"));
        }
        let (st, req) = auth::client_login_start(&self.secrets.auth_key)?;
        *step = LoginStep::Started(Box::new(st));
        Ok(req)
    }

    /// Step 2: check the server's response. Fails with `Decrypt` on a wrong
    /// password or Secret Key, or a server that is not the real one. Pass the
    /// stored server key as `pinned_server_key` after the first login.
    pub fn finish(
        &self,
        response: Vec<u8>,
        pinned_server_key: Option<Vec<u8>>,
    ) -> Result<LoginResult> {
        let mut step = lock(&self.step);
        let st = match std::mem::replace(&mut *step, LoginStep::Done) {
            LoginStep::Started(st) => *st,
            other => {
                *step = other;
                return Err(wrong_step("call start() first, once"));
            }
        };
        let out = auth::client_login_finish(
            st,
            &self.secrets.auth_key,
            &response,
            &self.account_id,
            pinned_server_key.as_deref(),
        )?;
        Ok(LoginResult {
            finalization: out.finalization,
            api_token: auth::api_token(&out.session_key).expose().to_vec(),
            session_key: out.session_key.expose().to_vec(),
            server_public_key: out.server_public_key,
        })
    }

    /// Unlock the account bundle with the key from this session's Argon2id
    /// run (no second run). Works offline, before or without `start`.
    pub fn unlock(&self, bundle: Vec<u8>) -> Result<Arc<Account>> {
        let b: account::AccountBundle = dec(&bundle)?;
        if b.account_id != self.account_id {
            return Err(AmanfoldError::Decrypt);
        }
        let keys = account::unlock_with_key(&b, &self.secrets.unlock_key)?;
        Ok(Arc::new(Account::new(keys, b)))
    }
}

/// Unlock a stored account bundle with the master password and Secret Key
/// (offline, without a server round trip).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn unlock_account(
    bundle: Vec<u8>,
    password: String,
    secret_key: String,
) -> Result<Arc<Account>> {
    let b: account::AccountBundle = dec(&bundle)?;
    let sk = SecretKey::parse(&secret_key)?;
    let keys = account::unlock(&b, &password, &sk)?;
    Ok(Arc::new(Account::new(keys, b)))
}

/// Unlock with the 24 recovery words (forgotten password). Follow with
/// [`Account::change_password`].
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn recover_account(bundle: Vec<u8>, recovery_words: String) -> Result<Arc<Account>> {
    let b: account::AccountBundle = dec(&bundle)?;
    let code = account::RecoveryCode::parse(&recovery_words)?;
    let keys = account::recover(&b, &code)?;
    Ok(Arc::new(Account::new(keys, b)))
}

/// Quick unlock (Face ID, fingerprint) with a device key from
/// [`Account::enable_quick_unlock`].
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn quick_unlock(
    bundle: Vec<u8>,
    device_key: Vec<u8>,
    wrapped_keys: Vec<u8>,
) -> Result<Arc<Account>> {
    let b: account::AccountBundle = dec(&bundle)?;
    let dk: [u8; 32] = device_key
        .as_slice()
        .try_into()
        .map_err(|_| invalid("device key length"))?;
    let dk = SymmetricKey::from_array(dk);
    let env = Envelope::from_bytes(wrapped_keys)?;
    let keys = account::unlock_with_device_key(&b, &dk, &env)?;
    Ok(Arc::new(Account::new(keys, b)))
}

// ---------------------------------------------------------------------------
// Unlocked account
// ---------------------------------------------------------------------------

/// Result of [`Account::create_vault`].
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NewVault {
    /// The empty vault.
    pub vault: Arc<LocalVault>,
    /// The owner's own grant: upload it, and pass it to
    /// [`Account::open_vault`] on other devices.
    pub grant: Vec<u8>,
}

/// Result of [`Account::change_password`].
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PasswordChange {
    /// The new bundle. Upload it with the registration upload, then call
    /// [`Account::update_bundle`] once the server accepted both.
    pub bundle: Vec<u8>,
    /// Re-register the login: the password and salt changed.
    pub registration: Arc<Registration>,
}

/// Result of [`Account::rotate_recovery_code`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryRotation {
    /// The new bundle (upload, then [`Account::update_bundle`]).
    pub bundle: Vec<u8>,
    /// The new 24 words for a fresh recovery kit.
    pub recovery_words: String,
}

/// Result of [`Account::enable_quick_unlock`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct QuickUnlock {
    /// Store in the iOS Keychain with a biometry access control, or the
    /// Android Keystore with user authentication. Never in plain storage.
    pub device_key: Vec<u8>,
    /// Ordinary app storage is fine: useless without the device key.
    pub wrapped_keys: Vec<u8>,
}

/// An unlocked account. Holds the private keys in memory until dropped;
/// drop it when the app locks.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct Account {
    keys: AccountKeys,
    bundle: Mutex<account::AccountBundle>,
}

impl Account {
    fn new(keys: AccountKeys, bundle: account::AccountBundle) -> Self {
        Self {
            keys,
            bundle: Mutex::new(bundle),
        }
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl Account {
    /// Account id.
    pub fn account_id(&self) -> String {
        lock(&self.bundle).account_id.to_string()
    }

    /// The account bundle as this device knows it.
    pub fn bundle(&self) -> Vec<u8> {
        enc(&*lock(&self.bundle))
    }

    /// Public keys, for others to share vaults with this account.
    pub fn public_keys(&self) -> Vec<u8> {
        enc(&self.keys.public())
    }

    /// Safety number to compare in person before sharing.
    pub fn fingerprint(&self) -> String {
        self.keys.public().fingerprint()
    }

    /// Replace the stored bundle after the server accepted a new one (from
    /// a password change or recovery-code rotation). Must be this account's.
    pub fn update_bundle(&self, bundle: Vec<u8>) -> Result<()> {
        let b: account::AccountBundle = dec(&bundle)?;
        let mut cur = lock(&self.bundle);
        if b.account_id != cur.account_id || b.public != self.keys.public() {
            return Err(invalid("bundle belongs to another account"));
        }
        *cur = b;
        Ok(())
    }

    /// Create a new vault owned by this account.
    pub fn create_vault(&self) -> Result<NewVault> {
        let vk = vault::VaultKey::generate();
        let g = vault::grant(&vk, &self.keys, &self.keys.public())?;
        Ok(NewVault {
            vault: Arc::new(LocalVault::new(Vault::new(vk))),
            grant: enc(&g),
        })
    }

    /// Open a vault from a grant. `granter_public_keys` is who must have
    /// signed it: this account's own keys for its vaults, or the keys of a
    /// family or team member whose safety number was checked. `cache` is a
    /// previous [`LocalVault::export_cache`]; it keeps the vault usable
    /// offline and keeps rollback protection across restarts.
    pub fn open_vault(
        &self,
        grant: Vec<u8>,
        granter_public_keys: Vec<u8>,
        cache: Option<Vec<u8>>,
    ) -> Result<Arc<LocalVault>> {
        let g: VaultGrant = dec(&grant)?;
        let granter = public_keys(&granter_public_keys)?;
        let vk = vault::accept(&g, &self.keys, &granter)?;
        let v = match cache {
            Some(c) => Vault::from_cache(vk, &Envelope::from_bytes(c)?)?,
            None => Vault::new(vk),
        };
        Ok(Arc::new(LocalVault::new(v)))
    }

    /// Share `vault` with another account: returns their grant to upload.
    /// Check `recipient_public_keys` against the safety number first.
    pub fn grant_vault(
        &self,
        vault: Arc<LocalVault>,
        recipient_public_keys: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let to = public_keys(&recipient_public_keys)?;
        let v = lock(&vault.inner);
        Ok(enc(&vault::grant(v.key(), &self.keys, &to)?))
    }

    /// Move `vault` to a newer key that another member made after removing
    /// someone (a new grant arrived). Keeps local items and rollback state.
    pub fn accept_rotation(
        &self,
        vault: Arc<LocalVault>,
        grant: Vec<u8>,
        granter_public_keys: Vec<u8>,
    ) -> Result<()> {
        let g: VaultGrant = dec(&grant)?;
        let granter = public_keys(&granter_public_keys)?;
        let vk = vault::accept(&g, &self.keys, &granter)?;
        lock(&vault.inner).rekey(vk)?;
        Ok(())
    }

    /// Set a new master password (also after recovery). Runs Argon2id.
    pub fn change_password(
        &self,
        new_password: String,
        secret_key: String,
    ) -> Result<PasswordChange> {
        let sk = SecretKey::parse(&secret_key)?;
        let cur = lock(&self.bundle).clone();
        let nb = account::rewrap_password(
            &cur,
            &self.keys,
            &new_password,
            &sk,
            amanfold_core::kdf::KdfParams::DEFAULT,
        )?;
        let secrets = nb.prelogin().derive(&new_password, &sk)?;
        Ok(PasswordChange {
            bundle: enc(&nb),
            registration: Arc::new(Registration::new(nb.account_id, secrets.auth_key)),
        })
    }

    /// Replace the recovery code (old kit lost). The old words stop working
    /// once the server has the new bundle.
    pub fn rotate_recovery_code(&self) -> Result<RecoveryRotation> {
        let cur = lock(&self.bundle).clone();
        let (nb, code) = account::rotate_recovery_code(&cur, &self.keys)?;
        Ok(RecoveryRotation {
            bundle: enc(&nb),
            recovery_words: code.to_words().expose().to_string(),
        })
    }

    /// Sign a challenge from the server, proving this device holds the
    /// account's private keys. Account recovery uses it with purpose
    /// `"recover"`: after [`recover_account`], the signature lets the server
    /// accept a new password without the old one.
    pub fn sign_challenge(&self, purpose: String, challenge: Vec<u8>) -> Result<Vec<u8>> {
        let id = lock(&self.bundle).account_id;
        Ok(auth::sign_challenge(&self.keys, &id, &purpose, &challenge)?.to_vec())
    }

    /// Turn on quick unlock for this device. See [`QuickUnlock`].
    pub fn enable_quick_unlock(&self) -> Result<QuickUnlock> {
        let (dk, env) = account::wrap_for_device(&lock(&self.bundle), &self.keys)?;
        Ok(QuickUnlock {
            device_key: dk.expose().to_vec(),
            wrapped_keys: env.as_bytes().to_vec(),
        })
    }
}

// ---------------------------------------------------------------------------
// Vaults
// ---------------------------------------------------------------------------

/// A decrypted vault on this device. Edits return encrypted records to
/// upload; [`LocalVault::sync`] applies what the server sends back.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct LocalVault {
    inner: Mutex<Vault>,
    clock: Mutex<Clock>,
}

impl LocalVault {
    fn new(v: Vault) -> Self {
        let mut clock = Clock::random();
        let now = now_ms();
        for i in v.all_items() {
            clock.observe(i.last_modified(), now);
        }
        Self {
            inner: Mutex::new(v),
            clock: Mutex::new(clock),
        }
    }

    fn edit(
        &self,
        id: &str,
        f: impl FnOnce(&mut Item, &mut Clock) -> Result<()>,
    ) -> Result<SavedItem> {
        let id = parse_id(id)?;
        let mut v = lock(&self.inner);
        let mut it = v.get(&id).cloned().ok_or(AmanfoldError::NotFound)?;
        f(&mut it, &mut lock(&self.clock))?;
        Ok(SavedItem::of(&v.save(it)?))
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl LocalVault {
    /// Vault id.
    pub fn id(&self) -> String {
        lock(&self.inner).key().id().to_string()
    }

    /// Key generation (goes up each time a member is removed).
    pub fn epoch(&self) -> u32 {
        lock(&self.inner).key().epoch()
    }

    /// Items, newest change first.
    pub fn items(&self, include_trashed: bool) -> Vec<ItemView> {
        let v = lock(&self.inner);
        let mut out: Vec<ItemView> = v
            .all_items()
            .filter(|i| include_trashed || !i.is_deleted())
            .map(ItemView::of)
            .collect();
        out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// Logins saved for the site of `page_url`, best match first (then most
    /// recently changed). Trashed items are left out. This is the only list
    /// autofill may offer on that page.
    pub fn logins_for_url(&self, page_url: String) -> Vec<LoginMatch> {
        let Some(page) = site::page_site(&page_url) else {
            return Vec::new();
        };
        let v = lock(&self.inner);
        let mut out: Vec<LoginMatch> = v
            .all_items()
            .filter(|i| !i.is_deleted() && i.kind() == item::ItemKind::Login)
            .filter_map(|i| {
                let best = item_urls(i)
                    .filter_map(site::saved_site)
                    .map(|s| site::match_sites(&s, &page))
                    .max()
                    .unwrap_or(site::SiteMatch::None);
                (best != site::SiteMatch::None).then(|| LoginMatch {
                    item: ItemView::of(i),
                    strength: best.into(),
                })
            })
            .collect();
        out.sort_by(|a, b| {
            b.strength
                .cmp(&a.strength)
                .then(b.item.modified_ms.cmp(&a.item.modified_ms))
                .then(a.item.id.cmp(&b.item.id))
        });
        out
    }

    /// Logins saved for the Android app `app_url` (from
    /// [`android_app_url`]): same package and same signing certificate.
    /// Trashed items are left out. This is the only list Android autofill
    /// may offer inside that app.
    pub fn logins_for_app(&self, app_url: String) -> Vec<LoginMatch> {
        let v = lock(&self.inner);
        let mut out: Vec<LoginMatch> = v
            .all_items()
            .filter(|i| !i.is_deleted() && i.kind() == item::ItemKind::Login)
            .filter(|i| item_urls(i).any(|u| site::app_matches(u, &app_url)))
            .map(|i| LoginMatch {
                item: ItemView::of(i),
                strength: SiteMatch::Origin,
            })
            .collect();
        out.sort_by(|a, b| {
            b.item
                .modified_ms
                .cmp(&a.item.modified_ms)
                .then(a.item.id.cmp(&b.item.id))
        });
        out
    }

    /// Saved sites (registrable domains) that `page_url` imitates without
    /// being them, for a phishing warning. Empty when the page is one of the
    /// saved sites or resembles none.
    pub fn lookalike_sites(&self, page_url: String) -> Vec<String> {
        let Some(page) = site::page_site(&page_url) else {
            return Vec::new();
        };
        let v = lock(&self.inner);
        let saved: Vec<site::SiteInfo> = v
            .all_items()
            .filter(|i| !i.is_deleted() && i.kind() == item::ItemKind::Login)
            .flat_map(|i| {
                item_urls(i)
                    .filter_map(site::saved_site)
                    .collect::<Vec<_>>()
            })
            .collect();
        if saved
            .iter()
            .any(|s| site::match_sites(s, &page) != site::SiteMatch::None)
        {
            return Vec::new();
        }
        let mut out: Vec<String> = saved
            .iter()
            .filter(|s| site::is_lookalike(s, &page))
            .map(|s| s.site.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// One item.
    pub fn item(&self, id: String) -> Result<Option<ItemView>> {
        let id = parse_id(&id)?;
        Ok(lock(&self.inner).get(&id).map(ItemView::of))
    }

    /// Create an item. Card numbers are validated and stored as digits.
    pub fn create_item(&self, kind: ItemKind, fields: Vec<FieldInput>) -> Result<SavedItem> {
        let mut clock = lock(&self.clock);
        let mut it = Item::new(kind.into(), clock.tick(now_ms()));
        for f in &fields {
            it.set(&f.name, &f.value, clock.tick(now_ms()))?;
        }
        drop(clock);
        Ok(SavedItem::of(&lock(&self.inner).save(it)?))
    }

    /// Change fields: set `set`, then remove `remove`.
    pub fn update_item(
        &self,
        id: String,
        set: Vec<FieldInput>,
        remove: Vec<String>,
    ) -> Result<SavedItem> {
        self.edit(&id, |it, clock| {
            for f in &set {
                it.set(&f.name, &f.value, clock.tick(now_ms()))?;
            }
            for name in &remove {
                it.remove(name, clock.tick(now_ms()));
            }
            Ok(())
        })
    }

    /// Move to or restore from the trash.
    pub fn set_trashed(&self, id: String, trashed: bool) -> Result<SavedItem> {
        self.edit(&id, |it, clock| {
            it.set_deleted(trashed, clock.tick(now_ms()));
            Ok(())
        })
    }

    /// Apply what the server returned: its current signed manifest and the
    /// records that changed. `trusted_writers` are the public keys of members
    /// allowed to write (from verified grants; for a personal vault, just
    /// this account's). Fails with `Rollback` if the server served old data
    /// or left something out. Returns records to upload (local changes the
    /// server lacked); upload them, then [`LocalVault::sign_manifest`].
    pub fn sync(
        &self,
        manifest: Vec<u8>,
        changed: Vec<Vec<u8>>,
        trusted_writers: Vec<Vec<u8>>,
    ) -> Result<Vec<SavedItem>> {
        let m: SignedManifest = dec(&manifest)?;
        let recs = changed
            .iter()
            .map(|r| dec::<EncryptedRecord>(r))
            .collect::<Result<Vec<_>>>()?;
        let writers = trusted_writers
            .iter()
            .map(|k| public_keys(k))
            .collect::<Result<Vec<_>>>()?;
        let mut v = lock(&self.inner);
        let up = v.sync_verified(&m, &recs, &writers)?;
        let now = now_ms();
        let mut clock = lock(&self.clock);
        for i in v.all_items() {
            clock.observe(i.last_modified(), now);
        }
        Ok(up.iter().map(SavedItem::of).collect())
    }

    /// Sign this device's view of the vault, to upload together with its
    /// records. If the server refuses it (409: another device wrote first),
    /// sync and sign again; nothing is lost.
    pub fn sign_manifest(&self, author: Arc<Account>) -> Vec<u8> {
        enc(&lock(&self.inner).sign_manifest(&author.keys))
    }

    /// The server accepted the last signed manifest. Call it after a
    /// successful upload, so the next edit can be committed without a sync
    /// in between.
    pub fn commit_accepted(&self) -> Result<()> {
        Ok(lock(&self.inner).commit_accepted()?)
    }

    /// After removing a member: new vault key, every item re-encrypted.
    /// Upload the records, then grant the new key to remaining members only.
    pub fn rotate_key(&self) -> Result<Vec<SavedItem>> {
        Ok(lock(&self.inner)
            .rotate()?
            .iter()
            .map(SavedItem::of)
            .collect())
    }

    /// Encrypted snapshot for this device's own storage (open it again with
    /// [`Account::open_vault`]).
    pub fn export_cache(&self) -> Vec<u8> {
        lock(&self.inner).export_cache().as_bytes().to_vec()
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// Recommended generator settings (20 characters, all classes, no look-alikes).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn default_password_rules() -> PasswordRules {
    let d = CoreRules::default();
    PasswordRules {
        length: d.length as u32,
        lowercase: d.lowercase,
        uppercase: d.uppercase,
        digits: d.digits,
        symbols: d.symbols,
        avoid_ambiguous: d.avoid_ambiguous,
    }
}

/// Random password.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn generate_password(rules: PasswordRules) -> Result<String> {
    let r = CoreRules {
        length: rules.length as usize,
        lowercase: rules.lowercase,
        uppercase: rules.uppercase,
        digits: rules.digits,
        symbols: rules.symbols,
        avoid_ambiguous: rules.avoid_ambiguous,
    };
    Ok(generator::password(&r)?.expose().to_string())
}

/// Random passphrase of `words` words.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn generate_passphrase(words: u32, separator: String) -> Result<String> {
    Ok(generator::passphrase(words as usize, &separator)?
        .expose()
        .to_string())
}

/// Current one-time code from a TOTP secret or `otpauth://` URI.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn totp_now(secret: String) -> Result<TotpCode> {
    totp_at(secret, now_ms() / 1000)
}

/// One-time code at a given Unix time.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn totp_at(secret: String, unix_secs: u64) -> Result<TotpCode> {
    let t = Totp::parse(&secret)?;
    Ok(TotpCode {
        code: t.code_at(unix_secs),
        seconds_remaining: t.seconds_remaining(unix_secs),
        period: t.period,
    })
}

/// Validate and describe a typed card number.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn card_info(number: String) -> Result<CardInfo> {
    let digits = card::to_ascii_digits(&number)?;
    let brand = match card::brand(&digits) {
        card::Brand::Visa => CardBrand::Visa,
        card::Brand::Mastercard => CardBrand::Mastercard,
        card::Brand::Amex => CardBrand::Amex,
        card::Brand::Discover => CardBrand::Discover,
        card::Brand::Diners => CardBrand::Diners,
        card::Brand::Jcb => CardBrand::Jcb,
        card::Brand::UnionPay => CardBrand::UnionPay,
        card::Brand::Maestro => CardBrand::Maestro,
        card::Brand::Unknown => CardBrand::Unknown,
    };
    Ok(CardInfo {
        masked: card::masked(&digits),
        luhn_valid: card::luhn_valid(&digits),
        brand,
        digits,
    })
}

/// Check a typed Secret Key and return its canonical form. Typos are caught
/// by its checksum; common look-alikes (0/O, 1/I, 8/B) are corrected.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn check_secret_key(text: String) -> Result<String> {
    Ok(SecretKey::parse(&text)?.to_display().expose().to_string())
}

/// Check typed recovery words (the checksum catches most typos).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn check_recovery_words(words: String) -> Result<()> {
    account::RecoveryCode::parse(&words)?;
    Ok(())
}

/// Safety number of someone's public keys, to compare with what their own
/// app shows before sharing a vault with them.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn fingerprint(public_keys_bytes: Vec<u8>) -> Result<String> {
    Ok(public_keys(&public_keys_bytes)?.fingerprint())
}

/// Who signed a vault grant (their public keys), so an app can look them up
/// among the people whose safety numbers were checked before opening it
/// with [`Account::open_vault`]. Not verified here: `open_vault` checks the
/// signature against the keys the app passes in.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn grant_granter(grant: Vec<u8>) -> Result<Vec<u8>> {
    let g: VaultGrant = dec(&grant)?;
    Ok(enc(&g.granter))
}

// ---------------------------------------------------------------------------
// Sites (autofill matching)
// ---------------------------------------------------------------------------

/// How a saved URL matches a page, from no match to best. Autofill offers a
/// login only for `Site` or better. See `amanfold_core::site` for the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SiteMatch {
    /// Different site: never fill.
    None,
    /// Same registrable domain.
    Site,
    /// Same host, other scheme or port.
    Host,
    /// Same scheme, host and port.
    Origin,
}

impl From<site::SiteMatch> for SiteMatch {
    fn from(m: site::SiteMatch) -> Self {
        match m {
            site::SiteMatch::None => Self::None,
            site::SiteMatch::Site => Self::Site,
            site::SiteMatch::Host => Self::Host,
            site::SiteMatch::Origin => Self::Origin,
        }
    }
}

/// A page address as the apps should show it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct SiteView {
    /// `https`.
    pub secure: bool,
    /// Host in ASCII (punycode) form.
    pub host: String,
    /// Host in Unicode. When it differs from `host`, show both.
    pub display_host: String,
    /// Registrable domain (or the host when there is none).
    pub site: String,
}

impl From<site::SiteInfo> for SiteView {
    fn from(s: site::SiteInfo) -> Self {
        Self {
            secure: s.secure,
            host: s.host,
            display_host: s.display_host,
            site: s.site,
        }
    }
}

/// A login that matches a page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LoginMatch {
    /// The login.
    pub item: ItemView,
    /// How well its best URL matches.
    pub strength: SiteMatch,
}

/// How a login saved for `saved_url` matches the page at `page_url`.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn site_match(saved_url: String, page_url: String) -> SiteMatch {
    site::match_url(&saved_url, &page_url).into()
}

/// Parse a page address (`http` or `https` only).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn page_site(page_url: String) -> Option<SiteView> {
    site::page_site(&page_url).map(Into::into)
}

/// Address of an Android app for a login (`androidapp://<package>?cert=...`):
/// its package name and the SHA-256 digest of its signing certificate.
/// Save it in the login's `url` (or `url.N`) field.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn android_app_url(package_name: String, signing_cert_sha256: Vec<u8>) -> Result<String> {
    site::android_app_url(&package_name, &signing_cert_sha256)
        .ok_or_else(|| invalid("android app identity"))
}

/// The package name in a saved Android app address, for display; `None`
/// for anything else (for example a website).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn android_app_package(saved_url: String) -> Option<String> {
    site::android_app_package(&saved_url).map(str::to_owned)
}

fn item_urls(i: &Item) -> impl Iterator<Item = &str> {
    i.fields().iter().filter_map(|(name, f)| {
        let is_url = name == item::fields::URL
            || name
                .strip_prefix("url.")
                .is_some_and(|n| n.parse::<u32>().is_ok());
        if is_url { f.value.as_str() } else { None }
    })
}

/// Version of the security core.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
