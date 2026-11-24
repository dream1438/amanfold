//! Encrypted sync record format.
//!
//! One [`EncryptedRecord`] per item. The server stores it next to a revision
//! number it assigns, and returns records changed since a revision for delta
//! sync. The server sees only the vault id, item id, key epoch and padded
//! ciphertext length, never titles, URLs or field names.
//!
//! Plaintext layout before encryption:
//!
//! ```text
//! u32 big-endian length || CBOR(Item) || zero padding to the size bucket
//! ```
//!
//! Size buckets (512 B, 1 KiB, 2 KiB ... 64 KiB, then multiples of 64 KiB)
//! hide whether an item is a short PIN or a long note.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    Error, FORMAT_VERSION, Result,
    aead::{self, Context, Envelope, SymmetricKey},
    item::Item,
    vault::VaultKey,
};

/// Largest plaintext item accepted (attachments are stored separately).
pub const MAX_ITEM_BYTES: usize = 1024 * 1024;

/// An encrypted item as stored on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedRecord {
    /// Format version.
    pub format: u16,
    /// Vault this item belongs to.
    pub vault_id: Uuid,
    /// Which vault key generation wrapped the item key.
    pub key_epoch: u32,
    /// Item id.
    pub item_id: Uuid,
    /// Item key, wrapped by the vault key.
    pub item_key: Envelope,
    /// Item content, encrypted with the item key.
    pub payload: Envelope,
}

fn item_key_ctx(vault: &Uuid, epoch: u32, item: &Uuid) -> Context {
    Context::new("item-key")
        .with(vault.as_bytes())
        .with(&epoch.to_be_bytes())
        .with(item.as_bytes())
}
fn payload_ctx(vault: &Uuid, item: &Uuid) -> Context {
    Context::new("item-payload")
        .with(vault.as_bytes())
        .with(item.as_bytes())
}

/// Padded size for a plaintext of `n` bytes (including the 4-byte length).
pub fn bucket(n: usize) -> usize {
    const MIN: usize = 512;
    const STEP: usize = 64 * 1024;
    if n <= STEP {
        n.next_power_of_two().max(MIN)
    } else {
        n.div_ceil(STEP) * STEP
    }
}

fn pad(body: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if body.len() > MAX_ITEM_BYTES {
        return Err(Error::InvalidArgument("item too large"));
    }
    let total = bucket(body.len() + 4);
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out.resize(total, 0);
    Ok(out)
}

fn unpad(buf: &[u8]) -> Result<&[u8]> {
    if buf.len() < 4 {
        return Err(Error::Malformed("payload"));
    }
    let n = u32::from_be_bytes(buf[..4].try_into().expect("4 bytes")) as usize;
    buf.get(4..4 + n).ok_or(Error::Malformed("payload length"))
}

/// Serialise to CBOR into a zeroizing buffer.
pub fn to_cbor<T: Serialize>(v: &T) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::new());
    ciborium::into_writer(v, &mut *out).expect("serialising to memory cannot fail");
    out
}

/// Deserialise from CBOR.
pub fn from_cbor<T: DeserializeOwned>(b: &[u8]) -> Result<T> {
    ciborium::from_reader(b).map_err(|_| Error::Malformed("cbor"))
}

/// Encrypt an item with its item key and wrap the item key with the vault key.
pub fn encrypt_item(
    vault: &VaultKey,
    item: &Item,
    item_key: &SymmetricKey,
) -> Result<EncryptedRecord> {
    let id = item.id();
    let body = to_cbor(item);
    let padded = pad(&body)?;
    Ok(EncryptedRecord {
        format: FORMAT_VERSION,
        vault_id: vault.id(),
        key_epoch: vault.epoch(),
        item_id: id,
        item_key: aead::wrap_key(
            vault.key(),
            &item_key_ctx(&vault.id(), vault.epoch(), &id),
            item_key,
        ),
        payload: aead::seal(item_key, &payload_ctx(&vault.id(), &id), &padded),
    })
}

/// Decrypt a record. Returns the item and its item key.
pub fn decrypt_item(vault: &VaultKey, rec: &EncryptedRecord) -> Result<(Item, SymmetricKey)> {
    if rec.format != FORMAT_VERSION {
        return Err(Error::Malformed("unsupported record format"));
    }
    if rec.vault_id != vault.id() {
        return Err(Error::Decrypt);
    }
    if rec.key_epoch != vault.epoch() {
        return Err(Error::Malformed("record uses a different vault key epoch"));
    }
    let ik = aead::unwrap_key(
        vault.key(),
        &item_key_ctx(&rec.vault_id, rec.key_epoch, &rec.item_id),
        &rec.item_key,
    )?;
    let padded = aead::open(&ik, &payload_ctx(&rec.vault_id, &rec.item_id), &rec.payload)?;
    let item: Item = from_cbor(unpad(&padded)?)?;
    if item.id() != rec.item_id {
        return Err(Error::Decrypt);
    }
    Ok((item, ik))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hlc::Hlc, keys::new_symmetric_key};

    fn now() -> Hlc {
        Hlc {
            millis: 1,
            counter: 0,
            device: 1,
        }
    }

    #[test]
    fn round_trip() {
        let vk = VaultKey::generate();
        let item = Item::login("Mail", "me", "pw", "https://mail.example", now()).unwrap();
        let ik = new_symmetric_key();
        let rec = encrypt_item(&vk, &item, &ik).unwrap();
        let (back, ik2) = decrypt_item(&vk, &rec).unwrap();
        assert_eq!(back, item);
        assert_eq!(ik, ik2);
        // Survives CBOR transport.
        let wire = to_cbor(&rec);
        assert_eq!(
            decrypt_item(&vk, &from_cbor(&wire).unwrap()).unwrap().0,
            item
        );
    }

    #[test]
    fn no_plaintext_on_the_wire() {
        let vk = VaultKey::generate();
        let item = Item::login(
            "VerySecretBank",
            "eslam",
            "pw",
            "https://secretbank.example",
            now(),
        )
        .unwrap();
        let wire = to_cbor(&encrypt_item(&vk, &item, &new_symmetric_key()).unwrap());
        for needle in [
            "VerySecretBank",
            "eslam",
            "secretbank",
            "password",
            "username",
            "url",
        ] {
            assert!(
                !wire.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "{needle} leaked"
            );
        }
    }

    #[test]
    fn padding_hides_size() {
        let vk = VaultKey::generate();
        let small = Item::note("a", "b", now()).unwrap();
        let bigger = Item::note("a much longer title", &"x".repeat(100), now()).unwrap();
        let a = encrypt_item(&vk, &small, &new_symmetric_key()).unwrap();
        let b = encrypt_item(&vk, &bigger, &new_symmetric_key()).unwrap();
        assert_eq!(a.payload.as_bytes().len(), b.payload.as_bytes().len());
        assert_eq!(bucket(1), 512);
        assert_eq!(bucket(513), 1024);
        assert_eq!(bucket(64 * 1024 + 1), 128 * 1024);
    }

    #[test]
    fn server_cannot_move_or_swap_records() {
        let vk = VaultKey::generate();
        let a = Item::note("a", "1", now()).unwrap();
        let b = Item::note("b", "2", now()).unwrap();
        let ra = encrypt_item(&vk, &a, &new_symmetric_key()).unwrap();
        let rb = encrypt_item(&vk, &b, &new_symmetric_key()).unwrap();
        // Relabel a's ciphertext as item b.
        let mut swapped = ra.clone();
        swapped.item_id = rb.item_id;
        assert!(decrypt_item(&vk, &swapped).is_err());
        // Mix a's key with b's payload.
        let mut mixed = rb.clone();
        mixed.item_key = ra.item_key.clone();
        assert!(decrypt_item(&vk, &mixed).is_err());
        // Move to another vault.
        let other = VaultKey::generate();
        let mut moved = ra.clone();
        moved.vault_id = other.id();
        assert!(decrypt_item(&other, &moved).is_err());
        // Wrong epoch.
        let mut ep = ra.clone();
        ep.key_epoch += 1;
        assert!(decrypt_item(&vk, &ep).is_err());
    }

    #[test]
    fn unpad_rejects_bad_length() {
        assert!(unpad(&[0, 0, 1, 0, 1, 2]).is_err());
        assert!(unpad(&[0, 0]).is_err());
    }
}
