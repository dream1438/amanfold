//! # Amanfold security core
//!
//! The one place where Amanfold touches secrets. Every client (iOS, Android,
//! browser extension, desktop, web) links this crate, so the cryptography is
//! written, reviewed and tested once.
//!
//! Key hierarchy (see `docs/SECURITY.md` in the repository for the full design):
//!
//! ```text
//! master password ─┐
//!                  ├─ Argon2id + HKDF ─► Account Unlock Key (never stored)
//! Secret Key ──────┘                          │ wraps
//!                                             ▼
//!                          Account key pair (X25519 + Ed25519)
//!                                             │ seals (to own or a member's public key)
//!                                             ▼
//!                                  Vault Key (one per vault)
//!                                             │ wraps
//!                                             ▼
//!                                  Item Key (one per item)
//!                                             │ encrypts
//!                                             ▼
//!                                  item fields + metadata
//! ```
//!
//! The server only ever sees the output of [`account::AccountBundle`],
//! [`vault::VaultGrant`] and [`sync::EncryptedRecord`]: ciphertext, public
//! keys, KDF parameters and salts.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod account;
pub mod aead;
pub mod auth;
pub mod card;
pub mod error;
pub mod generator;
pub mod hlc;
pub mod item;
pub mod kdf;
pub mod keys;
pub mod manifest;
pub mod secret;
pub mod sync;
pub mod totp;
pub mod vault;

pub use error::{Error, Result};

/// Version of the on-disk and on-wire formats produced by this crate.
pub const FORMAT_VERSION: u16 = 1;
