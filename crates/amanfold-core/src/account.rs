//! Account creation, unlock, password change and recovery.
//!
//! The server stores an [`AccountBundle`]: salts, KDF parameters, public keys
//! and two encrypted copies of the account private keys, one opened by the
//! master password + Secret Key, one by the 24-word recovery code. Neither copy
//! can be opened with anything the server holds.

use bip39::{Language, Mnemonic};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    Error, FORMAT_VERSION, Result,
    aead::{Context, Envelope, SymmetricKey},
    kdf::{self, KdfParams, SALT_LEN, SecretKey},
    keys::{AccountKeys, PublicKeys},
    secret::{SecretBytes, SecretString},
};

/// Minimum master password length, in Unicode scalar values after NFKC.
/// Strength estimation beyond this lives in the UI layer.
pub const MIN_PASSWORD_CHARS: usize = 10;

/// What the server stores for an account. Contains no plaintext secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBundle {
    /// Format version.
    pub format: u16,
    /// Account id.
    pub account_id: Uuid,
    /// Argon2id parameters for the password path.
    pub kdf: KdfParams,
    /// Argon2id salt.
    #[serde(with = "serde_bytes")]
    pub salt: [u8; SALT_LEN],
    /// Account public keys.
    pub public: PublicKeys,
    /// Account private keys, wrapped by the Account Unlock Key.
    pub keys_by_password: Envelope,
    /// Account private keys, wrapped by the recovery key.
    pub keys_by_recovery: Envelope,
}

/// Result of [`create_account`]. Show the Secret Key and recovery code to the
/// person once, in the recovery kit, then drop this value.
#[derive(Debug)]
pub struct NewAccount {
    /// Upload this to the server.
    pub bundle: AccountBundle,
    /// Goes in the recovery kit and the device keychain.
    pub secret_key: SecretKey,
    /// Goes in the recovery kit only.
    pub recovery_code: RecoveryCode,
    /// Unlocked keys for the current session.
    pub keys: AccountKeys,
    /// OPAQUE login key: register it with [`crate::auth::client_registration_start`].
    pub auth_key: SecretBytes<32>,
}

/// What the server hands out before login, so the client can run Argon2id.
///
/// Unauthenticated, so the client still enforces the KDF floor. For unknown
/// usernames the server must answer with [`crate::auth::fake_prelogin`] so
/// this request does not reveal which accounts exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreLogin {
    /// Account id.
    pub account_id: Uuid,
    /// Argon2id parameters.
    pub kdf: KdfParams,
    /// Argon2id salt.
    #[serde(with = "serde_bytes")]
    pub salt: [u8; SALT_LEN],
}

impl PreLogin {
    /// Run the master-password derivation (one Argon2id pass) for login and unlock.
    pub fn derive(&self, password: &str, secret_key: &SecretKey) -> Result<kdf::AccountSecrets> {
        kdf::derive_account_secrets(
            password,
            secret_key,
            &self.salt,
            &self.kdf,
            self.account_id.as_bytes(),
        )
    }
}

impl AccountBundle {
    /// The pre-login parameters for this account.
    pub fn prelogin(&self) -> PreLogin {
        PreLogin {
            account_id: self.account_id,
            kdf: self.kdf,
            salt: self.salt,
        }
    }
}

fn password_ctx(id: &Uuid) -> Context {
    Context::new("account-keys/password").with(id.as_bytes())
}
fn recovery_ctx(id: &Uuid) -> Context {
    Context::new("account-keys/recovery").with(id.as_bytes())
}

fn check_password(pw: &str) -> Result<()> {
    if kdf::normalize_password(pw).chars().count() < MIN_PASSWORD_CHARS {
        return Err(Error::InvalidArgument("master password too short"));
    }
    Ok(())
}

/// Create a new account.
pub fn create_account(password: &str) -> Result<NewAccount> {
    create_account_with(password, KdfParams::DEFAULT)
}

/// Create a new account with explicit KDF parameters (tuned per device class).
pub fn create_account_with(password: &str, params: KdfParams) -> Result<NewAccount> {
    check_password(password)?;
    let account_id = crate::secret::random_uuid();
    let secret_key = SecretKey::generate();
    let recovery_code = RecoveryCode::generate();
    let keys = AccountKeys::generate();
    let salt = kdf::new_salt();
    let secrets =
        kdf::derive_account_secrets(password, &secret_key, &salt, &params, account_id.as_bytes())?;
    let auk = secrets.unlock_key;
    let rk = recovery_code.derive_key(&account_id);
    let bundle = AccountBundle {
        format: FORMAT_VERSION,
        account_id,
        kdf: params,
        salt,
        public: keys.public(),
        keys_by_password: keys.wrap(&auk, &password_ctx(&account_id)),
        keys_by_recovery: keys.wrap(&rk, &recovery_ctx(&account_id)),
    };
    Ok(NewAccount {
        bundle,
        secret_key,
        recovery_code,
        keys,
        auth_key: secrets.auth_key,
    })
}

fn check_bundle(bundle: &AccountBundle) -> Result<()> {
    if bundle.format != FORMAT_VERSION {
        return Err(Error::Malformed("unsupported account format"));
    }
    Ok(())
}

/// The unlocked keys must match the public keys the server advertises,
/// otherwise the server is mixing bundles.
fn check_public(keys: &AccountKeys, bundle: &AccountBundle) -> Result<()> {
    if keys.public() != bundle.public {
        return Err(Error::Decrypt);
    }
    Ok(())
}

/// Unlock with master password + Secret Key.
pub fn unlock(
    bundle: &AccountBundle,
    password: &str,
    secret_key: &SecretKey,
) -> Result<AccountKeys> {
    check_bundle(bundle)?;
    let auk = kdf::derive_unlock_key(
        password,
        secret_key,
        &bundle.salt,
        &bundle.kdf,
        bundle.account_id.as_bytes(),
    )?;
    let keys = AccountKeys::unwrap(
        &auk,
        &password_ctx(&bundle.account_id),
        &bundle.keys_by_password,
    )?;
    check_public(&keys, bundle)?;
    Ok(keys)
}

/// Unlock with an unlock key already derived during login (see [`PreLogin::derive`]),
/// so Argon2id runs once per sign-in, not twice.
pub fn unlock_with_key(bundle: &AccountBundle, unlock_key: &SymmetricKey) -> Result<AccountKeys> {
    check_bundle(bundle)?;
    let keys = AccountKeys::unwrap(
        unlock_key,
        &password_ctx(&bundle.account_id),
        &bundle.keys_by_password,
    )?;
    check_public(&keys, bundle)?;
    Ok(keys)
}

/// Re-wrap the account keys under a new password and/or stronger KDF
/// parameters. Also used for silent KDF upgrades: call it with the same
/// password and [`KdfParams::DEFAULT`] when [`KdfParams::needs_upgrade`] says so.
///
/// The salt changes, so the OPAQUE login key changes too: derive it from the
/// new bundle's [`AccountBundle::prelogin`] and re-register with the server in
/// the same request that uploads the new bundle.
pub fn rewrap_password(
    bundle: &AccountBundle,
    keys: &AccountKeys,
    new_password: &str,
    secret_key: &SecretKey,
    params: KdfParams,
) -> Result<AccountBundle> {
    check_bundle(bundle)?;
    check_public(keys, bundle)?;
    check_password(new_password)?;
    let salt = kdf::new_salt();
    let auk = kdf::derive_unlock_key(
        new_password,
        secret_key,
        &salt,
        &params,
        bundle.account_id.as_bytes(),
    )?;
    Ok(AccountBundle {
        kdf: params,
        salt,
        keys_by_password: keys.wrap(&auk, &password_ctx(&bundle.account_id)),
        ..bundle.clone()
    })
}

/// Unlock with the recovery code from the printed kit (forgotten password).
/// Follow with [`rewrap_password`] to set a new password.
pub fn recover(bundle: &AccountBundle, code: &RecoveryCode) -> Result<AccountKeys> {
    check_bundle(bundle)?;
    let rk = code.derive_key(&bundle.account_id);
    let keys = AccountKeys::unwrap(
        &rk,
        &recovery_ctx(&bundle.account_id),
        &bundle.keys_by_recovery,
    )?;
    check_public(&keys, bundle)?;
    Ok(keys)
}

/// Replace the recovery code (for example after the old kit was lost).
pub fn rotate_recovery_code(
    bundle: &AccountBundle,
    keys: &AccountKeys,
) -> Result<(AccountBundle, RecoveryCode)> {
    check_bundle(bundle)?;
    check_public(keys, bundle)?;
    let code = RecoveryCode::generate();
    let rk = code.derive_key(&bundle.account_id);
    let b = AccountBundle {
        keys_by_recovery: keys.wrap(&rk, &recovery_ctx(&bundle.account_id)),
        ..bundle.clone()
    };
    Ok((b, code))
}

/// 24-word recovery code (256 bits of entropy, BIP-39 English word list).
///
/// It is already full-strength random, so it is stretched with HKDF only, not
/// Argon2: brute force is infeasible regardless.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode(SecretBytes<32>);

impl core::fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RecoveryCode(redacted)")
    }
}

impl RecoveryCode {
    /// Generate a new code.
    pub fn generate() -> Self {
        Self(SecretBytes::random())
    }

    /// The 24 words, space separated.
    pub fn to_words(&self) -> SecretString {
        let m = Mnemonic::from_entropy_in(Language::English, self.0.expose())
            .expect("32 bytes is valid entropy");
        SecretString::new(m.to_string())
    }

    /// Parse 24 words. Case and extra whitespace are ignored; the BIP-39
    /// checksum catches most typos.
    pub fn parse(words: &str) -> Result<Self> {
        let cleaned = Zeroizing::new(
            words
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        );
        let m = Mnemonic::parse_in_normalized(Language::English, &cleaned)
            .map_err(|_| Error::InvalidCode("recovery code"))?;
        let ent = Zeroizing::new(m.to_entropy());
        let arr: [u8; 32] = ent
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidCode("recovery code"))?;
        Ok(Self(SecretBytes::from_array(arr)))
    }

    fn derive_key(&self, account_id: &Uuid) -> SymmetricKey {
        let hk = Hkdf::<Sha256>::new(Some(account_id.as_bytes()), self.0.expose());
        let mut out = [0u8; 32];
        hk.expand(b"amanfold/v1/recovery-key", &mut out)
            .expect("valid HKDF length");
        SymmetricKey::from_array(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW: &str = "correct horse battery staple";

    #[test]
    fn create_and_unlock() {
        let acct = create_account(PW).unwrap();
        let keys = unlock(&acct.bundle, PW, &acct.secret_key).unwrap();
        assert_eq!(keys.public(), acct.keys.public());
    }

    #[test]
    fn wrong_password_or_secret_key_fails() {
        let acct = create_account(PW).unwrap();
        assert_eq!(
            unlock(
                &acct.bundle,
                "correct horse battery stapl3",
                &acct.secret_key
            )
            .unwrap_err(),
            Error::Decrypt
        );
        assert_eq!(
            unlock(&acct.bundle, PW, &SecretKey::generate()).unwrap_err(),
            Error::Decrypt
        );
    }

    #[test]
    fn short_password_rejected() {
        assert!(matches!(
            create_account("short"),
            Err(Error::InvalidArgument(_))
        ));
    }

    #[test]
    fn server_downgrade_of_kdf_params_is_refused() {
        let acct = create_account(PW).unwrap();
        let mut b = acct.bundle.clone();
        b.kdf = KdfParams {
            memory_kib: 1024,
            iterations: 1,
            lanes: 1,
        };
        assert!(matches!(
            unlock(&b, PW, &acct.secret_key),
            Err(Error::WeakKdfParams(_))
        ));
    }

    #[test]
    fn swapped_public_key_detected() {
        let acct = create_account(PW).unwrap();
        let mut b = acct.bundle.clone();
        b.public = AccountKeys::generate().public();
        assert_eq!(
            unlock(&b, PW, &acct.secret_key).unwrap_err(),
            Error::Decrypt
        );
    }

    #[test]
    fn bundles_cannot_be_mixed() {
        let a = create_account(PW).unwrap();
        let b = create_account(PW).unwrap();
        let mut mixed = a.bundle.clone();
        mixed.keys_by_password = b.bundle.keys_by_password.clone();
        assert!(unlock(&mixed, PW, &a.secret_key).is_err());
        // Password-wrapped copy cannot be used in the recovery slot either.
        let mut slot = a.bundle.clone();
        slot.keys_by_recovery = a.bundle.keys_by_password.clone();
        assert!(recover(&slot, &a.recovery_code).is_err());
    }

    #[test]
    fn change_password() {
        let acct = create_account(PW).unwrap();
        let nb = rewrap_password(
            &acct.bundle,
            &acct.keys,
            "a brand new passphrase",
            &acct.secret_key,
            KdfParams::DEFAULT,
        )
        .unwrap();
        assert!(unlock(&nb, PW, &acct.secret_key).is_err());
        assert_eq!(
            unlock(&nb, "a brand new passphrase", &acct.secret_key)
                .unwrap()
                .public(),
            acct.keys.public()
        );
        assert_ne!(nb.salt, acct.bundle.salt);
    }

    #[test]
    fn recovery_flow() {
        let acct = create_account(PW).unwrap();
        let words = acct.recovery_code.to_words();
        assert_eq!(words.expose().split(' ').count(), 24);
        let code = RecoveryCode::parse(&format!("  {}  ", words.expose().to_uppercase())).unwrap();
        let keys = recover(&acct.bundle, &code).unwrap();
        let nb = rewrap_password(
            &acct.bundle,
            &keys,
            "forgot it, new one now",
            &acct.secret_key,
            KdfParams::DEFAULT,
        )
        .unwrap();
        assert!(unlock(&nb, "forgot it, new one now", &acct.secret_key).is_ok());
        assert!(recover(&acct.bundle, &RecoveryCode::generate()).is_err());
    }

    #[test]
    fn recovery_code_typo_rejected() {
        let words = RecoveryCode::generate().to_words();
        let original: Vec<&str> = words.expose().split(' ').collect();
        // Substituting one word passes the 8-bit checksum about 1 time in 256.
        let list = Language::English.word_list();
        let accepted = list[..64]
            .iter()
            .filter(|w| **w != original[3])
            .filter(|w| {
                let mut t = original.clone();
                t[3] = w;
                RecoveryCode::parse(&t.join(" ")).is_ok()
            })
            .count();
        assert!(accepted <= 4, "checksum accepted {accepted} of 64 typos");
        assert!(RecoveryCode::parse("abandon abandon").is_err());
        assert!(RecoveryCode::parse("notaword ".repeat(24).trim()).is_err());
    }

    #[test]
    fn rotating_recovery_code_invalidates_old_one() {
        let acct = create_account(PW).unwrap();
        let (nb, code) = rotate_recovery_code(&acct.bundle, &acct.keys).unwrap();
        assert!(recover(&nb, &acct.recovery_code).is_err());
        assert!(recover(&nb, &code).is_ok());
    }
}
