//! Authenticated encryption: XChaCha20-Poly1305 with random 192-bit nonces.
//!
//! Every ciphertext is bound to a [`Context`] (passed as associated data) that
//! says what it is and where it belongs, for example "item payload for item X
//! in vault Y". A server that moves a ciphertext to another item, vault or
//! purpose makes decryption fail instead of silently confusing the client.

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{Error, Result, secret::SecretBytes};

/// Length of a symmetric key in bytes.
pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// Envelope algorithm identifier for XChaCha20-Poly1305.
const ALG_XCHACHA20POLY1305: u8 = 1;

/// A 256-bit symmetric key.
pub type SymmetricKey = SecretBytes<KEY_LEN>;

/// Domain-separated associated data.
///
/// Encoded as `"amanfold/v1/" label` followed by each part, every element
/// length-prefixed, so two different contexts can never encode to the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context(Vec<u8>);

impl Context {
    /// Start a context with a purpose label such as `"item"`.
    pub fn new(label: &str) -> Self {
        let mut c = Self(Vec::with_capacity(64));
        c.push(b"amanfold/v1");
        c.push(label.as_bytes());
        c
    }
    /// Append a part (an id, a public key...).
    pub fn with(mut self, part: &[u8]) -> Self {
        self.push(part);
        self
    }
    fn push(&mut self, part: &[u8]) {
        let len = u32::try_from(part.len()).expect("context part too long");
        self.0.extend_from_slice(&len.to_be_bytes());
        self.0.extend_from_slice(part);
    }
    /// Encoded bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// An encrypted blob: `alg (1) || nonce (24) || ciphertext+tag`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Envelope(#[serde(with = "serde_bytes")] Vec<u8>);

impl core::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Envelope({} bytes)", self.0.len())
    }
}

impl Envelope {
    /// Raw bytes for storage or transport.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Parse raw bytes (validated again on [`open`]).
    pub fn from_bytes(b: Vec<u8>) -> Result<Self> {
        if b.len() < 1 + NONCE_LEN + TAG_LEN {
            return Err(Error::Malformed("envelope too short"));
        }
        Ok(Self(b))
    }
}

/// Encrypt `plaintext` under `key`, bound to `ctx`.
pub fn seal(key: &SymmetricKey, ctx: &Context, plaintext: &[u8]) -> Envelope {
    let nonce = SecretBytes::<NONCE_LEN>::random();
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    let ct = cipher
        .encrypt(
            XNonce::from_slice(nonce.expose()),
            Payload {
                msg: plaintext,
                aad: ctx.as_bytes(),
            },
        )
        .expect("XChaCha20-Poly1305 encryption cannot fail for in-memory buffers");
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    out.push(ALG_XCHACHA20POLY1305);
    out.extend_from_slice(nonce.expose());
    out.extend_from_slice(&ct);
    Envelope(out)
}

/// Decrypt and authenticate an envelope. Any failure returns [`Error::Decrypt`].
pub fn open(key: &SymmetricKey, ctx: &Context, env: &Envelope) -> Result<Zeroizing<Vec<u8>>> {
    let b = &env.0;
    if b.len() < 1 + NONCE_LEN + TAG_LEN || b[0] != ALG_XCHACHA20POLY1305 {
        return Err(Error::Decrypt);
    }
    let cipher = XChaCha20Poly1305::new(key.expose().into());
    cipher
        .decrypt(
            XNonce::from_slice(&b[1..1 + NONCE_LEN]),
            Payload {
                msg: &b[1 + NONCE_LEN..],
                aad: ctx.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Decrypt)
}

/// Wrap (encrypt) one key with another.
pub fn wrap_key(kek: &SymmetricKey, ctx: &Context, key: &SymmetricKey) -> Envelope {
    seal(kek, ctx, key.expose())
}

/// Unwrap a key produced by [`wrap_key`].
pub fn unwrap_key(kek: &SymmetricKey, ctx: &Context, env: &Envelope) -> Result<SymmetricKey> {
    let pt = open(kek, ctx, env)?;
    let arr: [u8; KEY_LEN] = pt.as_slice().try_into().map_err(|_| Error::Decrypt)?;
    Ok(SymmetricKey::from_array(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let k = SymmetricKey::random();
        let ctx = Context::new("test").with(b"id");
        let env = seal(&k, &ctx, b"hello");
        assert_eq!(&*open(&k, &ctx, &env).unwrap(), b"hello");
    }

    #[test]
    fn nonces_are_unique() {
        let k = SymmetricKey::random();
        let ctx = Context::new("test");
        assert_ne!(seal(&k, &ctx, b"x"), seal(&k, &ctx, b"x"));
    }

    #[test]
    fn wrong_key_or_context_fails() {
        let k = SymmetricKey::random();
        let ctx = Context::new("test").with(b"a");
        let env = seal(&k, &ctx, b"hello");
        assert_eq!(
            open(&SymmetricKey::random(), &ctx, &env).unwrap_err(),
            Error::Decrypt
        );
        assert_eq!(
            open(&k, &Context::new("test").with(b"b"), &env).unwrap_err(),
            Error::Decrypt
        );
        assert_eq!(
            open(&k, &Context::new("other").with(b"a"), &env).unwrap_err(),
            Error::Decrypt
        );
    }

    #[test]
    fn every_bit_flip_is_detected() {
        let k = SymmetricKey::random();
        let ctx = Context::new("test");
        let env = seal(&k, &ctx, b"attack at dawn");
        for i in 0..env.0.len() {
            for bit in 0..8 {
                let mut t = env.clone();
                t.0[i] ^= 1 << bit;
                assert!(
                    open(&k, &ctx, &t).is_err(),
                    "flip at byte {i} bit {bit} not detected"
                );
            }
        }
        let mut short = env.clone();
        short.0.pop();
        assert!(open(&k, &ctx, &short).is_err());
    }

    #[test]
    fn context_encoding_is_unambiguous() {
        assert_ne!(Context::new("a").with(b"bc"), Context::new("ab").with(b"c"));
        assert_ne!(
            Context::new("x").with(b"ab").with(b""),
            Context::new("x").with(b"a").with(b"b")
        );
    }
}
