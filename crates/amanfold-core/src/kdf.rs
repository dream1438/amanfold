//! Key derivation: master password + Secret Key → Account Unlock Key.
//!
//! 1. The password is normalised to Unicode NFKC so the same password typed on
//!    any keyboard (Arabic, Latin, CJK...) always gives the same bytes.
//! 2. Argon2id stretches it with a per-account random salt.
//! 3. HKDF-SHA256 mixes in the 128-bit Secret Key. Someone who steals the
//!    server database has the salt and parameters but not the Secret Key, so
//!    they face a 128-bit search, not a password guess.
//!
//! KDF parameters come from the server at login, so the client enforces a
//! floor (no downgrade attack) and a ceiling (no memory-exhaustion attack).

use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    Error, Result,
    aead::SymmetricKey,
    secret::{SecretBytes, SecretString},
};

/// Length of the per-account Argon2 salt.
pub const SALT_LEN: usize = 16;

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub memory_kib: u32,
    /// Number of passes.
    pub iterations: u32,
    /// Degree of parallelism.
    pub lanes: u32,
}

impl KdfParams {
    /// Minimum accepted values. Never lower these.
    pub const FLOOR: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        lanes: 1,
    };
    /// Maximum accepted values (protects low-end phones from a hostile server).
    pub const CEILING: Self = Self {
        memory_kib: 1024 * 1024,
        iterations: 32,
        lanes: 16,
    };
    /// Default for new accounts: 64 MiB, 3 passes, 4 lanes.
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        lanes: 4,
    };

    /// Check the parameters are within the floor and ceiling.
    pub fn validate(&self) -> Result<()> {
        let (f, c) = (Self::FLOOR, Self::CEILING);
        if self.memory_kib < f.memory_kib || self.iterations < f.iterations || self.lanes < f.lanes
        {
            return Err(Error::WeakKdfParams("below security floor"));
        }
        if self.memory_kib > c.memory_kib || self.iterations > c.iterations || self.lanes > c.lanes
        {
            return Err(Error::WeakKdfParams("above safety ceiling"));
        }
        if self.memory_kib < 8 * self.lanes {
            return Err(Error::WeakKdfParams("memory too small for lane count"));
        }
        Ok(())
    }

    /// Whether an account on these parameters should be silently re-keyed to
    /// `target` at next unlock (the "automatic upgrade" promise in the plan).
    pub fn needs_upgrade(&self, target: &Self) -> bool {
        self.memory_kib < target.memory_kib || self.iterations < target.iterations
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Normalise a master password to NFKC.
pub fn normalize_password(pw: &str) -> Zeroizing<String> {
    Zeroizing::new(pw.nfkc().collect())
}

// ---------------------------------------------------------------------------
// Secret Key
// ---------------------------------------------------------------------------

const SECRET_KEY_LEN: usize = 16;
const SECRET_KEY_PREFIX: &str = "A1";
const BASE32: data_encoding::Encoding = data_encoding::BASE32_NOPAD;

/// The 128-bit device-generated Secret Key, printed in the recovery kit.
///
/// Text form: `A1-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXX` (base32 of 16 random bytes
/// plus a 2-byte checksum, so typos are caught before any derivation runs).
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey(SecretBytes<SECRET_KEY_LEN>);

impl core::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretKey(redacted)")
    }
}

impl SecretKey {
    /// Generate a new Secret Key.
    pub fn generate() -> Self {
        Self(SecretBytes::random())
    }

    fn checksum(b: &[u8; SECRET_KEY_LEN]) -> [u8; 2] {
        let d = Sha256::new()
            .chain_update(b"amanfold/v1/secret-key-checksum")
            .chain_update(b)
            .finalize();
        [d[0], d[1]]
    }

    /// Human-readable form for the recovery kit.
    pub fn to_display(&self) -> SecretString {
        let mut raw = Zeroizing::new([0u8; SECRET_KEY_LEN + 2]);
        raw[..SECRET_KEY_LEN].copy_from_slice(self.0.expose());
        raw[SECRET_KEY_LEN..].copy_from_slice(&Self::checksum(self.0.expose()));
        let enc = Zeroizing::new(BASE32.encode(&raw[..]));
        let mut out = String::from(SECRET_KEY_PREFIX);
        for chunk in enc.as_bytes().chunks(5) {
            out.push('-');
            out.push_str(core::str::from_utf8(chunk).expect("base32 is ASCII"));
        }
        SecretString::new(out)
    }

    /// Parse the text form. Case, spaces and dashes are ignored, and the
    /// commonly confused digits 0, 1 and 8 are read as O, I and B.
    pub fn parse(s: &str) -> Result<Self> {
        let mut cleaned = Zeroizing::new(String::with_capacity(s.len()));
        for c in s.chars().filter(|c| !c.is_whitespace() && *c != '-') {
            cleaned.push(match c.to_ascii_uppercase() {
                '0' => 'O',
                '1' => 'I',
                '8' => 'B',
                o => o,
            });
        }
        // The prefix "A1" may itself have been normalised to "AI".
        let body = cleaned
            .strip_prefix("A1")
            .or_else(|| cleaned.strip_prefix("AI"))
            .ok_or(Error::InvalidCode("secret key"))?;
        let raw = Zeroizing::new(
            BASE32
                .decode(body.as_bytes())
                .map_err(|_| Error::InvalidCode("secret key"))?,
        );
        if raw.len() != SECRET_KEY_LEN + 2 {
            return Err(Error::InvalidCode("secret key"));
        }
        let mut key = [0u8; SECRET_KEY_LEN];
        key.copy_from_slice(&raw[..SECRET_KEY_LEN]);
        if Self::checksum(&key) != raw[SECRET_KEY_LEN..] {
            key.zeroize();
            return Err(Error::InvalidCode("secret key"));
        }
        Ok(Self(SecretBytes::from_array(key)))
    }

    pub(crate) fn expose(&self) -> &[u8; SECRET_KEY_LEN] {
        self.0.expose()
    }
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

/// Both secrets that come out of one master-password derivation.
///
/// They come from the same Argon2id run (so login plus unlock costs one
/// stretch, not two) but from different HKDF labels, so neither reveals the
/// other: the server sees an OPAQUE exchange built on `auth_key` and never
/// learns anything that helps it compute `unlock_key`.
#[derive(Debug)]
pub struct AccountSecrets {
    /// Unwraps the account private keys. Never leaves the device.
    pub unlock_key: SymmetricKey,
    /// Input to the OPAQUE login protocol. Never sent to the server.
    pub auth_key: SecretBytes<32>,
}

/// Run Argon2id once and derive the unlock key and the login key.
///
/// `account_id` is mixed in so two accounts with the same password, Secret Key
/// and salt (which should never happen) still get different keys.
pub fn derive_account_secrets(
    password: &str,
    secret_key: &SecretKey,
    salt: &[u8; SALT_LEN],
    params: &KdfParams,
    account_id: &[u8],
) -> Result<AccountSecrets> {
    params.validate()?;
    let pw = normalize_password(password);
    if pw.is_empty() {
        return Err(Error::InvalidArgument("empty master password"));
    }
    let argon = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.memory_kib, params.iterations, params.lanes, Some(32))
            .map_err(|_| Error::WeakKdfParams("rejected by argon2"))?,
    );
    let mut stretched = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(pw.as_bytes(), salt, &mut stretched[..])
        .map_err(|_| Error::WeakKdfParams("rejected by argon2"))?;

    let hk = Hkdf::<Sha256>::new(Some(secret_key.expose()), &stretched[..]);
    let expand = |label: &[u8]| {
        let mut out = [0u8; 32];
        hk.expand(&[label, account_id].concat(), &mut out)
            .expect("32 bytes is a valid HKDF length");
        out
    };
    Ok(AccountSecrets {
        unlock_key: SymmetricKey::from_array(expand(b"amanfold/v1/account-unlock-key")),
        auth_key: SecretBytes::from_array(expand(b"amanfold/v1/opaque-login-key")),
    })
}

/// Derive only the Account Unlock Key. See [`derive_account_secrets`].
pub fn derive_unlock_key(
    password: &str,
    secret_key: &SecretKey,
    salt: &[u8; SALT_LEN],
    params: &KdfParams,
    account_id: &[u8],
) -> Result<SymmetricKey> {
    derive_account_secrets(password, secret_key, salt, params, account_id).map(|s| s.unlock_key)
}

/// Generate a fresh Argon2 salt.
pub fn new_salt() -> [u8; SALT_LEN] {
    *SecretBytes::<SALT_LEN>::random().expose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_floor_and_ceiling() {
        assert!(KdfParams::DEFAULT.validate().is_ok());
        let weak = KdfParams {
            memory_kib: 1024,
            ..KdfParams::DEFAULT
        };
        assert_eq!(
            weak.validate(),
            Err(Error::WeakKdfParams("below security floor"))
        );
        let few = KdfParams {
            iterations: 1,
            ..KdfParams::DEFAULT
        };
        assert!(few.validate().is_err());
        let huge = KdfParams {
            memory_kib: 64 * 1024 * 1024,
            ..KdfParams::DEFAULT
        };
        assert_eq!(
            huge.validate(),
            Err(Error::WeakKdfParams("above safety ceiling"))
        );
    }

    #[test]
    fn upgrade_detection() {
        let stronger = KdfParams {
            memory_kib: 128 * 1024,
            ..KdfParams::DEFAULT
        };
        assert!(KdfParams::DEFAULT.needs_upgrade(&stronger));
        assert!(!stronger.needs_upgrade(&KdfParams::DEFAULT));
    }

    #[test]
    fn nfkc_makes_equivalent_passwords_equal() {
        // "é" as one code point vs "e" + combining acute.
        assert_eq!(
            *normalize_password("caf\u{e9}"),
            *normalize_password("cafe\u{301}")
        );
        // Full-width Latin letters typed on an East Asian keyboard.
        assert_eq!(*normalize_password("\u{ff21}\u{ff22}"), "AB");
        // Arabic presentation form vs standard letter.
        assert_eq!(*normalize_password("\u{fe8d}"), "\u{627}");
    }

    #[test]
    fn secret_key_round_trip_and_typos() {
        let sk = SecretKey::generate();
        let text = sk.to_display();
        assert!(text.expose().starts_with("A1-"));
        assert_eq!(SecretKey::parse(text.expose()).unwrap(), sk);
        assert_eq!(
            SecretKey::parse(&text.expose().to_lowercase().replace('-', " ")).unwrap(),
            sk
        );
        // Change one character: checksum or decoding must reject it.
        let mut chars: Vec<char> = text.expose().chars().collect();
        let i = 5;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(
            SecretKey::parse(&typo),
            Err(Error::InvalidCode("secret key"))
        );
        assert!(SecretKey::parse("A1-ABC").is_err());
        assert!(SecretKey::parse("").is_err());
    }

    #[test]
    fn derivation_depends_on_every_input() {
        let sk = SecretKey::generate();
        let salt = new_salt();
        let p = KdfParams::DEFAULT;
        let base = derive_unlock_key("correct horse", &sk, &salt, &p, b"acct").unwrap();
        assert_eq!(
            base,
            derive_unlock_key("correct horse", &sk, &salt, &p, b"acct").unwrap()
        );
        assert_ne!(
            base,
            derive_unlock_key("correct horsf", &sk, &salt, &p, b"acct").unwrap()
        );
        assert_ne!(
            base,
            derive_unlock_key("correct horse", &SecretKey::generate(), &salt, &p, b"acct").unwrap()
        );
        assert_ne!(
            base,
            derive_unlock_key("correct horse", &sk, &new_salt(), &p, b"acct").unwrap()
        );
        assert_ne!(
            base,
            derive_unlock_key("correct horse", &sk, &salt, &p, b"acct2").unwrap()
        );
        let p2 = KdfParams { iterations: 4, ..p };
        assert_ne!(
            base,
            derive_unlock_key("correct horse", &sk, &salt, &p2, b"acct").unwrap()
        );
    }

    #[test]
    fn derivation_rejects_weak_params_and_empty_password() {
        let sk = SecretKey::generate();
        let weak = KdfParams {
            memory_kib: 8,
            iterations: 1,
            lanes: 1,
        };
        assert!(derive_unlock_key("pw", &sk, &new_salt(), &weak, b"a").is_err());
        assert!(derive_unlock_key("", &sk, &new_salt(), &KdfParams::DEFAULT, b"a").is_err());
    }

    #[test]
    fn login_key_is_independent_of_unlock_key() {
        let sk = SecretKey::generate();
        let s = derive_account_secrets("pw-pw-pw-pw", &sk, &[2u8; 16], &KdfParams::DEFAULT, b"a")
            .unwrap();
        assert_ne!(s.unlock_key.expose(), s.auth_key.expose());
    }

    /// Pinned output: if this changes, existing vaults would no longer unlock.
    #[test]
    fn derivation_known_answer() {
        let sk = SecretKey(SecretBytes::from_array([7u8; 16]));
        let k = derive_unlock_key(
            "كلمة سر Amanfold",
            &sk,
            &[1u8; 16],
            &KdfParams::DEFAULT,
            b"acct-1",
        )
        .unwrap();
        assert_eq!(hex::encode(k.expose()), KAT_UNLOCK_KEY);
    }
    const KAT_UNLOCK_KEY: &str = "2ca7563a4e35271a3ff96f896b25687c7915ae0eb8244372b0927a8b18fd87ef";
}
