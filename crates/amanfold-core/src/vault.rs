//! Vaults: vault keys, grants to members, and a local replica that merges
//! remote changes.
//!
//! A vault key is delivered to each member (including the owner's own
//! account) as a [`VaultGrant`]: the key sealed to the member's X25519 public
//! key and signed by the granter's Ed25519 key. The recipient checks the
//! signature against a granter key it already trusts (its own, or one whose
//! fingerprint was verified), so a malicious server cannot slip in its own
//! vault key or substitute a member's public key.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error, FORMAT_VERSION, Result,
    aead::{Context, KEY_LEN, SymmetricKey},
    item::Item,
    keys::{self, AccountKeys, PublicKeys, Sealed},
    sync::{self, EncryptedRecord},
};

/// A vault's symmetric key and its generation number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultKey {
    id: Uuid,
    epoch: u32,
    key: SymmetricKey,
}

impl VaultKey {
    /// A key for a new vault.
    pub fn generate() -> Self {
        Self {
            id: crate::secret::random_uuid(),
            epoch: 0,
            key: keys::new_symmetric_key(),
        }
    }
    /// Vault id.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Key generation (incremented on every rotation).
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    pub(crate) fn key(&self) -> &SymmetricKey {
        &self.key
    }
    /// The next key generation for the same vault (used when removing a member).
    pub fn next(&self) -> Self {
        Self {
            id: self.id,
            epoch: self.epoch + 1,
            key: keys::new_symmetric_key(),
        }
    }
}

/// A vault key delivered to one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultGrant {
    /// Format version.
    pub format: u16,
    /// Vault id.
    pub vault_id: Uuid,
    /// Key generation.
    pub epoch: u32,
    /// Who it is for.
    pub recipient: PublicKeys,
    /// Who created it.
    pub granter: PublicKeys,
    /// The vault key, sealed to the recipient.
    pub sealed: Sealed,
    /// Granter's signature over all of the above.
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

fn grant_ctx(vault: &Uuid, epoch: u32) -> Context {
    Context::new("vault-grant")
        .with(vault.as_bytes())
        .with(&epoch.to_be_bytes())
}

fn grant_signed_bytes(g: &VaultGrant) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(&g.format.to_be_bytes());
    m.extend_from_slice(g.vault_id.as_bytes());
    m.extend_from_slice(&g.epoch.to_be_bytes());
    m.extend_from_slice(&g.recipient.encryption);
    m.extend_from_slice(&g.recipient.signing);
    m.extend_from_slice(&g.granter.encryption);
    m.extend_from_slice(&g.granter.signing);
    m.extend_from_slice(&g.sealed.ephemeral);
    m.extend_from_slice(g.sealed.body.as_bytes());
    m
}

/// Give `recipient` access to a vault key.
pub fn grant(
    vault: &VaultKey,
    granter: &AccountKeys,
    recipient: &PublicKeys,
) -> Result<VaultGrant> {
    let ctx = grant_ctx(&vault.id, vault.epoch);
    let sealed = keys::seal_to(recipient, &ctx, vault.key.expose())?;
    let mut g = VaultGrant {
        format: FORMAT_VERSION,
        vault_id: vault.id,
        epoch: vault.epoch,
        recipient: *recipient,
        granter: granter.public(),
        sealed,
        signature: Vec::new(),
    };
    g.signature = granter.sign(&ctx, &grant_signed_bytes(&g)).to_vec();
    Ok(g)
}

/// Open a grant addressed to `me`, made by `trusted_granter`.
pub fn accept(g: &VaultGrant, me: &AccountKeys, trusted_granter: &PublicKeys) -> Result<VaultKey> {
    if g.format != FORMAT_VERSION {
        return Err(Error::Malformed("unsupported grant format"));
    }
    if g.granter != *trusted_granter {
        return Err(Error::BadSignature);
    }
    if g.recipient != me.public() {
        return Err(Error::Decrypt);
    }
    let ctx = grant_ctx(&g.vault_id, g.epoch);
    let sig: [u8; 64] = g
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| Error::BadSignature)?;
    trusted_granter.verify(&ctx, &grant_signed_bytes(g), &sig)?;
    let raw = me.open_sealed(&ctx, &g.sealed)?;
    let arr: [u8; KEY_LEN] = raw.as_slice().try_into().map_err(|_| Error::Decrypt)?;
    Ok(VaultKey {
        id: g.vault_id,
        epoch: g.epoch,
        key: SymmetricKey::from_array(arr),
    })
}

/// A decrypted local copy of a vault.
#[derive(Debug)]
pub struct Vault {
    key: VaultKey,
    items: BTreeMap<Uuid, (Item, SymmetricKey)>,
}

impl Vault {
    /// An empty local vault.
    pub fn new(key: VaultKey) -> Self {
        Self {
            key,
            items: BTreeMap::new(),
        }
    }

    /// The vault key.
    pub fn key(&self) -> &VaultKey {
        &self.key
    }

    /// Look up an item.
    pub fn get(&self, id: &Uuid) -> Option<&Item> {
        self.items.get(id).map(|(i, _)| i)
    }

    /// Items not in the trash.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items
            .values()
            .map(|(i, _)| i)
            .filter(|i| !i.is_deleted())
    }

    /// All items including trashed ones.
    pub fn all_items(&self) -> impl Iterator<Item = &Item> {
        self.items.values().map(|(i, _)| i)
    }

    /// Save a local edit (merged with any existing copy). Returns the record
    /// to upload.
    pub fn save(&mut self, item: Item) -> Result<EncryptedRecord> {
        let (merged, ik) = match self.items.remove(&item.id()) {
            Some((old, ik)) => (old.merge(&item)?, ik),
            None => (item, keys::new_symmetric_key()),
        };
        let rec = sync::encrypt_item(&self.key, &merged, &ik)?;
        self.items.insert(merged.id(), (merged, ik));
        Ok(rec)
    }

    /// Apply a record downloaded from the server. Returns a record to upload
    /// if the local copy had changes the server did not.
    pub fn apply_remote(&mut self, rec: &EncryptedRecord) -> Result<Option<EncryptedRecord>> {
        let (remote, rik) = sync::decrypt_item(&self.key, rec)?;
        match self.items.remove(&remote.id()) {
            None => {
                self.items.insert(remote.id(), (remote, rik));
                Ok(None)
            }
            Some((local, _)) => {
                let merged = local.merge(&remote)?;
                let upload = if merged != remote {
                    Some(sync::encrypt_item(&self.key, &merged, &rik)?)
                } else {
                    None
                };
                self.items.insert(merged.id(), (merged, rik));
                Ok(upload)
            }
        }
    }

    /// Rotate to a new vault key (after removing a member). Every item gets a
    /// fresh item key and is re-encrypted, so a removed member who kept old
    /// item keys cannot read future edits. Returns the records to upload; the
    /// caller then issues new grants to the remaining members only.
    pub fn rotate(&mut self) -> Result<Vec<EncryptedRecord>> {
        let next = self.key.next();
        let mut out = Vec::with_capacity(self.items.len());
        for (item, ik) in self.items.values_mut() {
            *ik = keys::new_symmetric_key();
            out.push(sync::encrypt_item(&next, item, ik)?);
        }
        self.key = next;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hlc::{Clock, Hlc},
        item::fields,
    };

    #[test]
    fn grant_to_self_and_member() {
        let owner = AccountKeys::generate();
        let member = AccountKeys::generate();
        let vk = VaultKey::generate();
        let g_self = grant(&vk, &owner, &owner.public()).unwrap();
        assert_eq!(accept(&g_self, &owner, &owner.public()).unwrap(), vk);
        let g = grant(&vk, &owner, &member.public()).unwrap();
        assert_eq!(accept(&g, &member, &owner.public()).unwrap(), vk);
        // Not addressed to owner.
        assert!(accept(&g, &owner, &owner.public()).is_err());
    }

    #[test]
    fn forged_grant_rejected() {
        let owner = AccountKeys::generate();
        let member = AccountKeys::generate();
        let server = AccountKeys::generate();
        let vk = VaultKey::generate();
        // Server makes its own grant with its own key.
        let forged = grant(&VaultKey::generate(), &server, &member.public()).unwrap();
        assert_eq!(
            accept(&forged, &member, &owner.public()).unwrap_err(),
            Error::BadSignature
        );
        // Server relabels the granter field.
        let mut relabelled = forged.clone();
        relabelled.granter = owner.public();
        assert_eq!(
            accept(&relabelled, &member, &owner.public()).unwrap_err(),
            Error::BadSignature
        );
        // Server tampers with a genuine grant's epoch.
        let mut g = grant(&vk, &owner, &member.public()).unwrap();
        g.epoch = 9;
        assert!(accept(&g, &member, &owner.public()).is_err());
    }

    #[test]
    fn two_devices_converge() {
        let vk = VaultKey::generate();
        let mut phone = Vault::new(vk.clone());
        let mut laptop = Vault::new(vk.clone());
        let mut pc = Clock::new(1);
        let mut lc = Clock::new(2);

        let item =
            Item::login("Bank", "eslam", "p0", "https://bank.example", pc.tick(1000)).unwrap();
        let id = item.id();
        let r1 = phone.save(item).unwrap();
        assert!(laptop.apply_remote(&r1).unwrap().is_none());

        // Offline edits on both.
        let mut a = phone.get(&id).unwrap().clone();
        a.set(fields::PASSWORD, "p1", pc.tick(2000)).unwrap();
        let ra = phone.save(a).unwrap();
        let mut b = laptop.get(&id).unwrap().clone();
        b.set(fields::USERNAME, "eslam2", lc.tick(2001)).unwrap();
        let rb = laptop.save(b).unwrap();

        // Server received phone's first. Laptop pulls it, merges, re-uploads.
        let up = laptop
            .apply_remote(&ra)
            .unwrap()
            .expect("laptop has newer username");
        phone.apply_remote(&rb).unwrap();
        phone.apply_remote(&up).unwrap();
        let (p, l) = (phone.get(&id).unwrap(), laptop.get(&id).unwrap());
        assert_eq!(p, l);
        assert_eq!(p.get(fields::PASSWORD), Some("p1"));
        assert_eq!(p.get(fields::USERNAME), Some("eslam2"));
    }

    #[test]
    fn rotation_re_keys_everything() {
        let owner = AccountKeys::generate();
        let removed = AccountKeys::generate();
        let mut v = Vault::new(VaultKey::generate());
        let old_key = v.key().clone();
        let t = Hlc {
            millis: 1,
            counter: 0,
            device: 1,
        };
        let old_rec = v
            .save(Item::note("family", "wifi password", t).unwrap())
            .unwrap();
        let new_recs = v.rotate().unwrap();
        assert_eq!(v.key().epoch(), 1);
        assert_ne!(new_recs[0].item_key, old_rec.item_key);
        // Removed member's old key cannot open new records.
        assert!(sync::decrypt_item(&old_key, &new_recs[0]).is_err());
        // Old item key does not open the new payload either.
        let (_, old_ik) = sync::decrypt_item(&old_key, &old_rec).unwrap();
        let mut forged = new_recs[0].clone();
        forged.item_key = crate::aead::wrap_key(
            old_key.key(),
            &Context::new("item-key")
                .with(old_key.id().as_bytes())
                .with(&1u32.to_be_bytes())
                .with(forged.item_id.as_bytes()),
            &old_ik,
        );
        assert!(sync::decrypt_item(v.key(), &forged).is_err());
        // Remaining member gets the new key.
        let g = grant(v.key(), &owner, &owner.public()).unwrap();
        assert_eq!(accept(&g, &owner, &owner.public()).unwrap().epoch(), 1);
        let _ = removed;
    }
}
