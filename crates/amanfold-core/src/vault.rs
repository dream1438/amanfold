//! Vaults: vault keys, grants to members, and a local replica that merges
//! remote changes.
//!
//! A vault key is delivered to each member (including the owner's own
//! account) as a [`VaultGrant`]: the key sealed to the member's X25519 public
//! key and signed by the granter's Ed25519 key. The recipient checks the
//! signature against a granter key it already trusts (its own, or one whose
//! fingerprint was verified), so a malicious server cannot slip in its own
//! vault key or substitute a member's public key.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Error, FORMAT_VERSION, Result,
    aead::{self, Context, Envelope, KEY_LEN, SymmetricKey},
    item::Item,
    keys::{self, AccountKeys, PublicKeys, Sealed},
    manifest,
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
    /// Digest of the newest record this device uploaded or accepted, per item.
    digests: BTreeMap<Uuid, [u8; 32]>,
    /// Items listed in a verified manifest: the server may never drop these.
    confirmed: BTreeSet<Uuid>,
    /// Highest manifest sequence number accepted (rollback protection).
    last_seq: Option<u64>,
}

impl Vault {
    /// An empty local vault.
    pub fn new(key: VaultKey) -> Self {
        Self {
            key,
            items: BTreeMap::new(),
            digests: BTreeMap::new(),
            confirmed: BTreeSet::new(),
            last_seq: None,
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
        // Merge before touching the map, so a failed merge loses nothing.
        let (merged, ik) = match self.items.get(&item.id()) {
            Some((old, ik)) => (old.merge(&item)?, ik.clone()),
            None => (item, keys::new_symmetric_key()),
        };
        let rec = sync::encrypt_item(&self.key, &merged, &ik)?;
        self.digests
            .insert(merged.id(), manifest::record_digest(&rec));
        self.items.insert(merged.id(), (merged, ik));
        Ok(rec)
    }

    /// Apply a record downloaded from the server. Returns a record to upload
    /// if the local copy had changes the server did not.
    pub fn apply_remote(&mut self, rec: &EncryptedRecord) -> Result<Option<EncryptedRecord>> {
        let (remote, rik) = sync::decrypt_item(&self.key, rec)?;
        // Merge and encrypt first: if either fails, the replica is unchanged.
        let (merged, upload) = match self.items.get(&remote.id()) {
            None => (remote, None),
            Some((local, _)) => {
                let merged = local.merge(&remote)?;
                let upload = if merged != remote {
                    Some(sync::encrypt_item(&self.key, &merged, &rik)?)
                } else {
                    None
                };
                (merged, upload)
            }
        };
        let digest = match &upload {
            Some(up) => manifest::record_digest(up),
            None => manifest::record_digest(rec),
        };
        self.digests.insert(merged.id(), digest);
        self.items.insert(merged.id(), (merged, rik));
        Ok(upload)
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
            let rec = sync::encrypt_item(&next, item, ik)?;
            self.digests
                .insert(item.id(), manifest::record_digest(&rec));
            out.push(rec);
        }
        self.key = next;
        Ok(out)
    }

    /// Sign a manifest of this device's view of the vault, to upload after
    /// its records. The server must accept it only if `seq` is exactly one
    /// more than the manifest it currently holds (compare-and-swap), so two
    /// writers cannot both win; the loser syncs again and retries.
    pub fn sign_manifest(&mut self, author: &AccountKeys) -> manifest::SignedManifest {
        let seq = self.last_seq.map_or(1, |s| s + 1);
        let m = manifest::SignedManifest::sign(&self.key, seq, self.digests.clone(), author);
        self.last_seq = Some(seq);
        self.confirmed.extend(self.digests.keys().copied());
        m
    }

    /// Verified sync: apply records downloaded from the server against the
    /// vault's signed manifest.
    ///
    /// * `manifest` must be signed by one of `trusted_writers` (the members
    ///   with write access, whose keys came from verified grants).
    /// * Its sequence number must not go backwards (rollback).
    /// * Every downloaded record must be the exact version the manifest lists.
    /// * Every item the manifest lists, and every item a previous manifest
    ///   listed, must be present afterwards (omission).
    ///
    /// Returns records to upload (local changes the server lacked); upload
    /// them, then [`Vault::sign_manifest`].
    pub fn sync_verified(
        &mut self,
        manifest: &manifest::SignedManifest,
        changed: &[EncryptedRecord],
        trusted_writers: &[PublicKeys],
    ) -> Result<Vec<EncryptedRecord>> {
        let body = manifest.verify(trusted_writers)?;
        if body.vault_id != self.key.id() {
            return Err(Error::Malformed("manifest for another vault"));
        }
        if body.epoch < self.key.epoch() {
            return Err(Error::Rollback("manifest uses an old vault key"));
        }
        if body.epoch > self.key.epoch() {
            return Err(Error::Malformed(
                "vault key rotated: fetch the new grant first",
            ));
        }
        if self.last_seq.is_some_and(|s| body.seq < s) {
            return Err(Error::Rollback("manifest older than one already seen"));
        }
        for rec in changed {
            if body.entries.get(&rec.item_id).map(|d| d.0) != Some(manifest::record_digest(rec)) {
                return Err(Error::Rollback("record does not match the manifest"));
            }
        }
        if self
            .confirmed
            .iter()
            .any(|id| !body.entries.contains_key(id))
        {
            return Err(Error::Rollback("server dropped an item"));
        }
        let mut uploads = Vec::new();
        for rec in changed {
            if let Some(up) = self.apply_remote(rec)? {
                uploads.push(up);
            }
        }
        if body.entries.keys().any(|id| !self.items.contains_key(id)) {
            return Err(Error::Rollback("server withheld an item"));
        }
        self.last_seq = Some(body.seq);
        self.confirmed.extend(body.entries.keys().copied());
        Ok(uploads)
    }
}

/// Plaintext of the local cache, sealed by [`Vault::export_cache`].
#[derive(Serialize, Deserialize)]
struct CacheBody {
    format: u16,
    vault_id: Uuid,
    epoch: u32,
    items: Vec<CachedItem>,
    digests: Vec<(Uuid, manifest::RecordDigest)>,
    confirmed: Vec<Uuid>,
    last_seq: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct CachedItem {
    item: Item,
    #[serde(with = "serde_bytes")]
    key: [u8; KEY_LEN],
}

impl Drop for CachedItem {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.key);
    }
}

fn cache_ctx(id: &Uuid, epoch: u32) -> Context {
    Context::new("vault-cache")
        .with(id.as_bytes())
        .with(&epoch.to_be_bytes())
}

impl Vault {
    /// Encrypt this replica for the device's own storage, so the vault opens
    /// offline and keeps its rollback state (manifest sequence number and
    /// confirmed items) across app restarts.
    pub fn export_cache(&self) -> Envelope {
        let body = CacheBody {
            format: FORMAT_VERSION,
            vault_id: self.key.id(),
            epoch: self.key.epoch(),
            items: self
                .items
                .values()
                .map(|(item, ik)| CachedItem {
                    item: item.clone(),
                    key: *ik.expose(),
                })
                .collect(),
            digests: self
                .digests
                .iter()
                .map(|(k, v)| (*k, manifest::RecordDigest(*v)))
                .collect(),
            confirmed: self.confirmed.iter().copied().collect(),
            last_seq: self.last_seq,
        };
        aead::seal(
            self.key.key(),
            &cache_ctx(&self.key.id(), self.key.epoch()),
            &sync::to_cbor(&body),
        )
    }

    /// Reopen a replica saved with [`Vault::export_cache`] under the same key.
    pub fn from_cache(key: VaultKey, cache: &Envelope) -> Result<Self> {
        let raw = aead::open(key.key(), &cache_ctx(&key.id(), key.epoch()), cache)?;
        let body: CacheBody = sync::from_cbor(&raw)?;
        if body.format != FORMAT_VERSION || body.vault_id != key.id() || body.epoch != key.epoch() {
            return Err(Error::Malformed("vault cache"));
        }
        let mut v = Vault::new(key);
        for c in &body.items {
            v.items.insert(
                c.item.id(),
                (c.item.clone(), SymmetricKey::from_array(c.key)),
            );
        }
        v.digests = body.digests.iter().map(|(k, d)| (*k, d.0)).collect();
        v.confirmed = body.confirmed.iter().copied().collect();
        v.last_seq = body.last_seq;
        Ok(v)
    }

    /// Switch to a newer key generation that another member created with
    /// [`Vault::rotate`] (delivered as a new grant). Keeps local items and the
    /// rollback state. Every local item key is replaced, so edits made before
    /// the new records arrive are not readable with the old keys a removed
    /// member may have kept.
    pub fn rekey(&mut self, next: VaultKey) -> Result<()> {
        if next.id() != self.key.id() {
            return Err(Error::InvalidArgument("key for another vault"));
        }
        if next.epoch() <= self.key.epoch() {
            return Err(Error::Rollback("vault key is not newer"));
        }
        for (_, ik) in self.items.values_mut() {
            *ik = keys::new_symmetric_key();
        }
        self.key = next;
        Ok(())
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

    #[test]
    fn cache_round_trip_keeps_rollback_state() {
        let owner = AccountKeys::generate();
        let mut v = Vault::new(VaultKey::generate());
        let mut c = Clock::new(1);
        v.save(Item::note("a", "secret body", c.tick(1)).unwrap())
            .unwrap();
        let m = v.sign_manifest(&owner);
        let cache = v.export_cache();
        let mut back = Vault::from_cache(v.key().clone(), &cache).unwrap();
        assert_eq!(back.items().count(), 1);
        assert_eq!(back.last_seq, Some(1));
        assert_eq!(back.confirmed, v.confirmed);
        assert_eq!(back.digests, v.digests);
        // The restored replica still rejects a replay of an older manifest.
        let _ = back.sign_manifest(&owner);
        assert!(matches!(
            back.sync_verified(&m, &[], &[owner.public()]),
            Err(Error::Rollback(_))
        ));
        // Wrong key or tampering fails.
        assert!(Vault::from_cache(VaultKey::generate(), &cache).is_err());
        assert!(Vault::from_cache(v.key().next(), &cache).is_err());
        let mut raw = cache.as_bytes().to_vec();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        let bad = Envelope::from_bytes(raw).unwrap();
        assert!(Vault::from_cache(v.key().clone(), &bad).is_err());
    }

    #[test]
    fn rekey_after_someone_else_rotates() {
        let mut a = Vault::new(VaultKey::generate());
        let mut b = Vault::new(a.key().clone());
        let mut c = Clock::new(1);
        let rec = a.save(Item::note("a", "x", c.tick(1)).unwrap()).unwrap();
        b.apply_remote(&rec).unwrap();
        let old_ik = b.items.values().next().unwrap().1.clone();
        let recs = a.rotate().unwrap();
        assert!(b.rekey(a.key().clone()).is_ok());
        assert_ne!(b.items.values().next().unwrap().1, old_ik);
        assert!(b.apply_remote(&recs[0]).unwrap().is_none());
        assert_eq!(b.items().count(), 1);
        // Older or foreign keys are refused.
        let older = b.key().clone();
        assert!(b.rekey(older).is_err());
        assert!(b.rekey(VaultKey::generate().next()).is_err());
    }

    #[test]
    fn failed_merge_keeps_the_local_item() {
        // A member with write access sends a record that reuses an existing
        // item id with a different kind. The merge is refused and the local
        // item must survive untouched.
        let vk = VaultKey::generate();
        let mut v = Vault::new(vk.clone());
        let mut c = Clock::new(1);
        let note = Item::note("mine", "keep me", c.tick(1)).unwrap();
        let id = note.id();
        v.save(note).unwrap();
        let before = v.export_cache();
        let hostile = Item::with_id(id, crate::item::ItemKind::Login, c.tick(2));
        let rec = sync::encrypt_item(&vk, &hostile, &keys::new_symmetric_key()).unwrap();
        assert!(matches!(v.apply_remote(&rec), Err(Error::Merge(_))));
        assert!(matches!(v.save(hostile), Err(Error::Merge(_))));
        assert_eq!(v.get(&id).unwrap().get(fields::NOTES), Some("keep me"));
        let after = Vault::from_cache(vk.clone(), &v.export_cache()).unwrap();
        let orig = Vault::from_cache(vk, &before).unwrap();
        assert_eq!(after.digests, orig.digests);
    }
}
