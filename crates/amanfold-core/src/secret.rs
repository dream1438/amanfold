//! Containers for secret material: wiped from memory on drop, never printed.

use core::fmt;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A fixed-size secret byte array, zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes<const N: usize>(pub(crate) [u8; N]);

impl<const N: usize> SecretBytes<N> {
    /// Fresh random bytes from the operating system CSPRNG.
    pub fn random() -> Self {
        let mut b = [0u8; N];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
        Self(b)
    }
    /// Wrap existing bytes. The caller should zeroize its own copy.
    pub fn from_array(b: [u8; N]) -> Self {
        Self(b)
    }
    /// Borrow the raw bytes.
    pub fn expose(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> PartialEq for SecretBytes<N> {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}
impl<const N: usize> Eq for SecretBytes<N> {}

impl<const N: usize> fmt::Debug for SecretBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes<{N}>(redacted)")
    }
}

/// A secret string (password, card number, CVV...), zeroized on drop and
/// redacted in `Debug` output so it never lands in logs or crash reports.
#[derive(
    Clone,
    Default,
    Zeroize,
    ZeroizeOnDrop,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// Borrow the secret value. Call only where it is actually needed.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}
impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(redacted)")
    }
}

/// A random (version 4) UUID from the OS CSPRNG.
pub fn random_uuid() -> uuid::Uuid {
    uuid::Builder::from_random_bytes(*SecretBytes::<16>::random().expose()).into_uuid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted() {
        let s = SecretString::new("hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
        let b = SecretBytes::<4>::from_array([1, 2, 3, 4]);
        assert!(!format!("{b:?}").contains('1'));
    }

    #[test]
    fn random_bytes_differ() {
        assert_ne!(SecretBytes::<32>::random(), SecretBytes::<32>::random());
    }
}
