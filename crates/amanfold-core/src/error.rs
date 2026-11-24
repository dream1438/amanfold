//! Error type.
//!
//! Decryption failures deliberately carry no detail (wrong key, tampered data
//! and wrong context all look the same) so errors cannot be used as an oracle.

/// Errors returned by the security core.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// Ciphertext failed authentication: wrong key, wrong context, or tampering.
    #[error("decryption failed")]
    Decrypt,
    /// KDF parameters are below the security floor or above the safety ceiling.
    /// A server that sends these is either buggy or attempting a downgrade.
    #[error("key derivation parameters rejected: {0}")]
    WeakKdfParams(&'static str),
    /// A Secret Key or recovery code was typed incorrectly.
    #[error("invalid {0}")]
    InvalidCode(&'static str),
    /// Input data is malformed or uses an unsupported format version.
    #[error("malformed data: {0}")]
    Malformed(&'static str),
    /// A public key was invalid (for example a low-order X25519 point).
    #[error("invalid public key")]
    InvalidPublicKey,
    /// A signature did not verify.
    #[error("signature verification failed")]
    BadSignature,
    /// Two items cannot be merged (different ids or kinds).
    #[error("merge conflict: {0}")]
    Merge(&'static str),
    /// The server served older data than this device has already seen, or
    /// left data out. Do not use the response; report it.
    #[error("possible rollback or withheld data: {0}")]
    Rollback(&'static str),
    /// Invalid argument passed by the caller.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
}

/// Result alias.
pub type Result<T> = core::result::Result<T, Error>;
