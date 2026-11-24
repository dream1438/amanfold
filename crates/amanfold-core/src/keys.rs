//! Account key pair and public-key encryption ("sealing").
//!
//! Each account has an X25519 key pair (to receive vault keys) and an Ed25519
//! key pair (to sign what it shares). Sealing is an HPKE-style construction:
//! ephemeral X25519 + HKDF-SHA256 + XChaCha20-Poly1305, with both public keys
//! and the caller's context bound into the key derivation and the AEAD.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::{
    Error, Result,
    aead::{self, Context, Envelope, SymmetricKey},
    secret::SecretBytes,
};

/// Public half of an account's keys. Safe to store on the server.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeys {
    /// X25519 encryption public key.
    #[serde(with = "serde_bytes")]
    pub encryption: [u8; 32],
    /// Ed25519 signing public key.
    #[serde(with = "serde_bytes")]
    pub signing: [u8; 32],
}

impl core::fmt::Debug for PublicKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PublicKeys({})", self.fingerprint())
    }
}

impl PublicKeys {
    /// Safety-number style fingerprint people compare in person or by QR
    /// before sharing, so a malicious server cannot substitute its own key.
    /// 12 groups of five digits, covering 192 bits of a SHA-256 hash.
    pub fn fingerprint(&self) -> String {
        let d = Sha256::new()
            .chain_update(b"amanfold/v1/fingerprint")
            .chain_update(self.encryption)
            .chain_update(self.signing)
            .finalize();
        let mut out = String::new();
        // Each group encodes 16 bits of the hash as a zero-padded number.
        for (i, chunk) in d.chunks(2).take(12).enumerate() {
            if i > 0 {
                out.push(' ');
            }
            let n = u32::from(chunk[0]) << 8 | u32::from(chunk[1]);
            out.push_str(&format!("{n:05}"));
        }
        out
    }

    /// Verify a signature made with the matching [`AccountKeys::sign`].
    pub fn verify(&self, ctx: &Context, msg: &[u8], sig: &[u8; 64]) -> Result<()> {
        let vk = VerifyingKey::from_bytes(&self.signing).map_err(|_| Error::InvalidPublicKey)?;
        let data = [ctx.as_bytes(), msg].concat();
        vk.verify_strict(&data, &Signature::from_bytes(sig))
            .map_err(|_| Error::BadSignature)
    }
}

/// An account's private keys. Exists only in memory while unlocked.
pub struct AccountKeys {
    encryption: StaticSecret,
    signing: SigningKey,
}

impl core::fmt::Debug for AccountKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AccountKeys(redacted)")
    }
}

impl AccountKeys {
    /// Generate new random keys.
    pub fn generate() -> Self {
        Self {
            encryption: StaticSecret::random_from_rng(rand_core::OsRng),
            signing: SigningKey::generate(&mut rand_core::OsRng),
        }
    }

    /// Public half.
    pub fn public(&self) -> PublicKeys {
        PublicKeys {
            encryption: PublicKey::from(&self.encryption).to_bytes(),
            signing: self.signing.verifying_key().to_bytes(),
        }
    }

    /// Sign `msg` under a domain-separating context.
    pub fn sign(&self, ctx: &Context, msg: &[u8]) -> [u8; 64] {
        let data = [ctx.as_bytes(), msg].concat();
        self.signing.sign(&data).to_bytes()
    }

    /// Serialise the private keys (64 bytes) for wrapping.
    pub(crate) fn to_bytes(&self) -> Zeroizing<[u8; 64]> {
        let mut out = Zeroizing::new([0u8; 64]);
        out[..32].copy_from_slice(self.encryption.as_bytes());
        out[32..].copy_from_slice(self.signing.as_bytes());
        out
    }

    pub(crate) fn from_bytes(b: &[u8]) -> Result<Self> {
        if b.len() != 64 {
            return Err(Error::Malformed("account keys"));
        }
        let mut e = Zeroizing::new([0u8; 32]);
        let mut s = Zeroizing::new([0u8; 32]);
        e.copy_from_slice(&b[..32]);
        s.copy_from_slice(&b[32..]);
        Ok(Self {
            encryption: StaticSecret::from(*e),
            signing: SigningKey::from_bytes(&s),
        })
    }

    /// Wrap these keys under a symmetric key (unlock key or recovery key).
    pub(crate) fn wrap(&self, kek: &SymmetricKey, ctx: &Context) -> Envelope {
        aead::seal(kek, ctx, &self.to_bytes()[..])
    }

    pub(crate) fn unwrap(kek: &SymmetricKey, ctx: &Context, env: &Envelope) -> Result<Self> {
        Self::from_bytes(&aead::open(kek, ctx, env)?)
    }

    /// Open a [`Sealed`] message addressed to this account.
    pub fn open_sealed(&self, ctx: &Context, sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>> {
        let eph = PublicKey::from(sealed.ephemeral);
        let shared = self.encryption.diffie_hellman(&eph);
        if !shared.was_contributory() {
            return Err(Error::Decrypt);
        }
        let recipient = PublicKey::from(&self.encryption).to_bytes();
        let key = seal_key(shared.as_bytes(), &sealed.ephemeral, &recipient, ctx);
        aead::open(
            &key,
            &seal_ctx(ctx, &sealed.ephemeral, &recipient),
            &sealed.body,
        )
    }
}

/// A message encrypted to someone's X25519 public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    /// Sender's one-time public key.
    #[serde(with = "serde_bytes")]
    pub ephemeral: [u8; 32],
    /// AEAD envelope.
    pub body: Envelope,
}

fn seal_key(
    shared: &[u8; 32],
    eph: &[u8; 32],
    recipient: &[u8; 32],
    ctx: &Context,
) -> SymmetricKey {
    let salt = [eph.as_slice(), recipient.as_slice()].concat();
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut out = [0u8; 32];
    hk.expand(
        &[b"amanfold/v1/seal-key".as_slice(), ctx.as_bytes()].concat(),
        &mut out,
    )
    .expect("valid HKDF length");
    SymmetricKey::from_array(out)
}

fn seal_ctx(ctx: &Context, eph: &[u8; 32], recipient: &[u8; 32]) -> Context {
    Context::new("sealed")
        .with(ctx.as_bytes())
        .with(eph)
        .with(recipient)
}

/// Encrypt `plaintext` so only the holder of `recipient`'s private key can read it.
pub fn seal_to(recipient: &PublicKeys, ctx: &Context, plaintext: &[u8]) -> Result<Sealed> {
    let eph_secret = StaticSecret::random_from_rng(rand_core::OsRng);
    let eph = PublicKey::from(&eph_secret).to_bytes();
    let shared = eph_secret.diffie_hellman(&PublicKey::from(recipient.encryption));
    if !shared.was_contributory() {
        return Err(Error::InvalidPublicKey);
    }
    let key = seal_key(shared.as_bytes(), &eph, &recipient.encryption, ctx);
    Ok(Sealed {
        ephemeral: eph,
        body: aead::seal(&key, &seal_ctx(ctx, &eph, &recipient.encryption), plaintext),
    })
}

/// Convenience: a random symmetric key (vault key, item key).
pub fn new_symmetric_key() -> SymmetricKey {
    SecretBytes::random()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_round_trip() {
        let alice = AccountKeys::generate();
        let ctx = Context::new("t");
        let s = seal_to(&alice.public(), &ctx, b"vault key").unwrap();
        assert_eq!(&*alice.open_sealed(&ctx, &s).unwrap(), b"vault key");
    }

    #[test]
    fn seal_wrong_recipient_or_context() {
        let alice = AccountKeys::generate();
        let mallory = AccountKeys::generate();
        let ctx = Context::new("t").with(b"vault-1");
        let s = seal_to(&alice.public(), &ctx, b"secret").unwrap();
        assert!(mallory.open_sealed(&ctx, &s).is_err());
        assert!(
            alice
                .open_sealed(&Context::new("t").with(b"vault-2"), &s)
                .is_err()
        );
        let mut swapped = s.clone();
        swapped.ephemeral = mallory.public().encryption;
        assert!(alice.open_sealed(&ctx, &swapped).is_err());
    }

    #[test]
    fn low_order_public_key_rejected() {
        let zero = PublicKeys {
            encryption: [0u8; 32],
            signing: AccountKeys::generate().public().signing,
        };
        assert_eq!(
            seal_to(&zero, &Context::new("t"), b"x").unwrap_err(),
            Error::InvalidPublicKey
        );
        let alice = AccountKeys::generate();
        let s = Sealed {
            ephemeral: [0u8; 32],
            body: aead::seal(&new_symmetric_key(), &Context::new("t"), b"x"),
        };
        assert!(alice.open_sealed(&Context::new("t"), &s).is_err());
    }

    #[test]
    fn signatures() {
        let a = AccountKeys::generate();
        let ctx = Context::new("grant");
        let sig = a.sign(&ctx, b"msg");
        assert!(a.public().verify(&ctx, b"msg", &sig).is_ok());
        assert_eq!(
            a.public().verify(&ctx, b"msh", &sig),
            Err(Error::BadSignature)
        );
        assert_eq!(
            a.public().verify(&Context::new("other"), b"msg", &sig),
            Err(Error::BadSignature)
        );
        assert!(
            AccountKeys::generate()
                .public()
                .verify(&ctx, b"msg", &sig)
                .is_err()
        );
    }

    #[test]
    fn key_bytes_round_trip() {
        let a = AccountKeys::generate();
        let b = AccountKeys::from_bytes(&a.to_bytes()[..]).unwrap();
        assert_eq!(a.public(), b.public());
    }

    #[test]
    fn fingerprint_shape() {
        let fp = AccountKeys::generate().public().fingerprint();
        assert_eq!(fp.len(), 12 * 5 + 11);
        assert!(fp.chars().all(|c| c.is_ascii_digit() || c == ' '));
        assert_ne!(fp, AccountKeys::generate().public().fingerprint());
    }
}
