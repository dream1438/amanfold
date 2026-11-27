//! Signed vault manifests: protection against rollback and withheld data.
//!
//! Encryption stops the server from reading or forging records, but on its
//! own it does not stop the server from serving an *older* valid record,
//! hiding a record, or serving an old vault-key epoch. A manifest closes that
//! gap: after each write, the writing device signs the list of every item in
//! the vault with the digest of its current record, plus a sequence number
//! and the key epoch. Readers check the signature against the members they
//! trust, refuse a sequence number lower than one they have seen, and check
//! that the records they download are exactly the ones listed.
//!
//! The server enforces that a new manifest's `seq` is exactly one more than
//! the stored one (compare-and-swap), which serialises concurrent writers.
//! Remaining gap: a server can withhold *all* updates from a device and keep
//! showing it an old but consistent state (a "freeze"). Devices that talk to
//! each other directly, or a signed timestamp in the manifest, can catch
//! that later.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    Error, FORMAT_VERSION, Result,
    aead::Context,
    keys::{AccountKeys, PublicKeys},
    sync::{EncryptedRecord, to_cbor},
    vault::VaultKey,
};

/// SHA-256 digest of a record, as listed in a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordDigest(#[serde(with = "serde_bytes")] pub [u8; 32]);

/// Digest of a record's canonical CBOR encoding.
pub fn record_digest(rec: &EncryptedRecord) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"amanfold/v1/record-digest")
        .chain_update(&*to_cbor(rec))
        .finalize()
        .into()
}

/// The signed content of a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBody {
    /// Format version.
    pub format: u16,
    /// Vault id.
    pub vault_id: Uuid,
    /// Current vault key epoch.
    pub epoch: u32,
    /// Strictly increasing per vault.
    pub seq: u64,
    /// Every item in the vault and the digest of its current record.
    pub entries: BTreeMap<Uuid, RecordDigest>,
    /// Who signed it.
    pub signer: PublicKeys,
}

/// A manifest and its signature, as stored by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    /// Signed content.
    pub body: ManifestBody,
    /// Ed25519 signature over the CBOR encoding of `body`.
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

fn ctx() -> Context {
    Context::new("vault-manifest")
}

impl SignedManifest {
    /// Build and sign a manifest.
    pub fn sign(
        vault: &VaultKey,
        seq: u64,
        digests: BTreeMap<Uuid, [u8; 32]>,
        author: &AccountKeys,
    ) -> Self {
        let body = ManifestBody {
            format: FORMAT_VERSION,
            vault_id: vault.id(),
            epoch: vault.epoch(),
            seq,
            entries: digests
                .into_iter()
                .map(|(k, v)| (k, RecordDigest(v)))
                .collect(),
            signer: author.public(),
        };
        let signature = author.sign(&ctx(), &to_cbor(&body)).to_vec();
        Self { body, signature }
    }

    /// Check the format, that the signer is trusted, and the signature.
    pub fn verify(&self, trusted_writers: &[PublicKeys]) -> Result<&ManifestBody> {
        if self.body.format != FORMAT_VERSION {
            return Err(Error::Malformed("unsupported manifest format"));
        }
        if !trusted_writers.contains(&self.body.signer) {
            return Err(Error::BadSignature);
        }
        let sig: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::BadSignature)?;
        self.body
            .signer
            .verify(&ctx(), &to_cbor(&self.body), &sig)?;
        Ok(&self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        hlc::Clock,
        item::{Item, fields},
        sync::from_cbor,
        vault::Vault,
    };

    struct World {
        owner: AccountKeys,
        phone: Vault,
        laptop: Vault,
        /// What the server holds.
        records: BTreeMap<Uuid, EncryptedRecord>,
        manifest: Option<SignedManifest>,
        clock: Clock,
    }

    impl World {
        fn new() -> Self {
            let vk = VaultKey::generate();
            Self {
                owner: AccountKeys::generate(),
                phone: Vault::new(vk.clone()),
                laptop: Vault::new(vk),
                records: BTreeMap::new(),
                manifest: None,
                clock: Clock::new(1),
            }
        }
        fn phone_writes(&mut self, title: &str) -> Uuid {
            let item = Item::note(title, "body", self.clock.tick(1000)).unwrap();
            let id = item.id();
            let rec = self.phone.save(item).unwrap();
            self.records.insert(id, rec);
            let m = self.phone.sign_manifest(&self.owner);
            self.phone.commit_accepted().unwrap();
            self.manifest = Some(m);
            id
        }
        fn laptop_syncs(
            &mut self,
            records: Vec<EncryptedRecord>,
            m: &SignedManifest,
        ) -> crate::Result<()> {
            self.laptop
                .sync_verified(m, &records, &[self.owner.public()])
                .map(|_| ())
        }
        fn all(&self) -> Vec<EncryptedRecord> {
            self.records.values().cloned().collect()
        }
    }

    #[test]
    fn honest_sync_passes() {
        let mut w = World::new();
        w.phone_writes("a");
        w.phone_writes("b");
        let m = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m).unwrap();
        assert_eq!(w.laptop.items().count(), 2);
        // Manifest survives transport.
        let back: SignedManifest = from_cbor(&to_cbor(&m)).unwrap();
        assert!(back.verify(&[w.owner.public()]).is_ok());
    }

    #[test]
    fn old_record_version_is_rejected() {
        let mut w = World::new();
        let id = w.phone_writes("a");
        let old = w.records[&id].clone();
        let mut item = w.phone.get(&id).unwrap().clone();
        item.set(fields::NOTES, "new body", w.clock.tick(2000))
            .unwrap();
        w.records.insert(id, w.phone.save(item).unwrap());
        let m = w.phone.sign_manifest(&w.owner);
        // Server serves the old (validly encrypted) record with the new manifest.
        assert!(matches!(
            w.laptop_syncs(vec![old], &m),
            Err(Error::Rollback(_))
        ));
    }

    #[test]
    fn withheld_item_is_detected() {
        let mut w = World::new();
        let a = w.phone_writes("a");
        w.phone_writes("b");
        let m = w.manifest.clone().unwrap();
        let only_a = vec![w.records[&a].clone()];
        assert!(matches!(
            w.laptop_syncs(only_a, &m),
            Err(Error::Rollback(_))
        ));
    }

    #[test]
    fn old_manifest_is_rejected_after_newer_one() {
        let mut w = World::new();
        w.phone_writes("a");
        let m1 = w.manifest.clone().unwrap();
        w.phone_writes("b");
        let m2 = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m2).unwrap();
        assert!(matches!(
            w.laptop_syncs(vec![], &m1),
            Err(Error::Rollback(_))
        ));
    }

    #[test]
    fn server_cannot_drop_a_confirmed_item() {
        let mut w = World::new();
        let a = w.phone_writes("a");
        let m = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m).unwrap();
        // A manifest signed by a trusted member that omits `a` (e.g. replayed
        // from a different vault state) is still refused.
        let mut digests = BTreeMap::new();
        digests.insert(crate::secret::random_uuid(), [0u8; 32]);
        let bad = SignedManifest::sign(w.phone.key(), 99, digests, &w.owner);
        let _ = a;
        assert!(matches!(
            w.laptop_syncs(vec![], &bad),
            Err(Error::Rollback(_))
        ));
    }

    #[test]
    fn untrusted_or_tampered_manifest_rejected() {
        let mut w = World::new();
        w.phone_writes("a");
        let m = w.manifest.clone().unwrap();
        let server = AccountKeys::generate();
        let forged = SignedManifest::sign(w.phone.key(), 5, BTreeMap::new(), &server);
        assert_eq!(
            forged.verify(&[w.owner.public()]).unwrap_err(),
            Error::BadSignature
        );
        let mut tampered = m.clone();
        tampered.body.seq += 10;
        assert_eq!(
            tampered.verify(&[w.owner.public()]).unwrap_err(),
            Error::BadSignature
        );
        let mut relabelled = forged.clone();
        relabelled.body.signer = w.owner.public();
        assert!(relabelled.verify(&[w.owner.public()]).is_err());
    }

    #[test]
    fn epoch_mismatch_handled() {
        let mut w = World::new();
        w.phone_writes("a");
        let recs = w.phone.rotate().unwrap();
        for r in &recs {
            w.records.insert(r.item_id, r.clone());
        }
        let m = w.phone.sign_manifest(&w.owner);
        // Laptop still on epoch 0: told to fetch the new grant, not silently accept.
        assert!(matches!(
            w.laptop_syncs(w.all(), &m),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn laptop_changes_flow_back() {
        let mut w = World::new();
        let id = w.phone_writes("a");
        let m = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m).unwrap();
        let mut item = w.laptop.get(&id).unwrap().clone();
        item.set(fields::TITLE, "renamed", Clock::new(2).tick(5000))
            .unwrap();
        let rec = w.laptop.save(item).unwrap();
        w.records.insert(id, rec);
        let m2 = w.laptop.sign_manifest(&w.owner);
        assert_eq!(m2.body.seq, m.body.seq + 1);
        w.phone
            .sync_verified(&m2, &[w.records[&id].clone()], &[w.owner.public()])
            .unwrap();
        assert_eq!(
            w.phone.get(&id).unwrap().get(fields::TITLE),
            Some("renamed")
        );
    }

    #[test]
    fn hidden_newer_version_of_a_known_item_is_detected() {
        let mut w = World::new();
        let id = w.phone_writes("a");
        let m = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m).unwrap();
        // Phone edits the item; the server lists the new version in the
        // manifest but does not send the record to the laptop.
        let mut item = w.phone.get(&id).unwrap().clone();
        item.set(fields::NOTES, "edited on phone", w.clock.tick(3000))
            .unwrap();
        w.records.insert(id, w.phone.save(item).unwrap());
        let m2 = w.phone.sign_manifest(&w.owner);
        assert!(matches!(
            w.laptop_syncs(vec![], &m2),
            Err(Error::Rollback(_))
        ));
        // Honest server: the record comes along and sync passes.
        w.laptop_syncs(w.all(), &m2).unwrap();
        // Nothing changed since: an empty sync against the same manifest passes.
        w.laptop_syncs(vec![], &m2).unwrap();
    }

    #[test]
    fn own_upload_then_empty_sync_passes() {
        let mut w = World::new();
        let id = w.phone_writes("a");
        let m = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m).unwrap();
        let mut item = w.laptop.get(&id).unwrap().clone();
        item.set(fields::TITLE, "renamed", Clock::new(2).tick(5000))
            .unwrap();
        w.records.insert(id, w.laptop.save(item).unwrap());
        let m2 = w.laptop.sign_manifest(&w.owner);
        // The laptop's own manifest comes back with nothing it lacks.
        w.laptop_syncs(vec![], &m2).unwrap();
    }

    #[test]
    fn refused_commit_leaves_rollback_state_alone() {
        let mut w = World::new();
        w.phone_writes("a");
        let m1 = w.manifest.clone().unwrap();
        w.laptop_syncs(w.all(), &m1).unwrap();
        // Both write; the phone's commit wins on the server.
        let p = w.phone_writes("from phone");
        let m2 = w.manifest.clone().unwrap();
        let item = Item::note("from laptop", "x", Clock::new(9).tick(9000)).unwrap();
        let lid = item.id();
        let lrec = w.laptop.save(item).unwrap();
        let refused = w.laptop.sign_manifest(&w.owner);
        assert_eq!(refused.body.seq, m2.body.seq);
        // The laptop syncs the winner's manifest: no false rollback alarm,
        // and its own item is still there to upload.
        w.laptop
            .sync_verified(&m2, &[w.records[&p].clone()], &[w.owner.public()])
            .unwrap();
        assert!(w.laptop.commit_accepted().is_err());
        assert!(w.laptop.get(&lid).is_some());
        w.records.insert(lid, lrec);
        let m3 = w.laptop.sign_manifest(&w.owner);
        assert_eq!(m3.body.seq, m2.body.seq + 1);
        assert!(m3.body.entries.contains_key(&lid));
        w.laptop.commit_accepted().unwrap();
        // A replay of the older manifest is now refused.
        assert!(matches!(
            w.laptop.sync_verified(&m2, &[], &[w.owner.public()]),
            Err(Error::Rollback(_))
        ));
    }
}
