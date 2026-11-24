//! Vault item model and conflict-free merge.
//!
//! An item is a set of named fields. Each field carries the hybrid-logical
//! timestamp of its last edit, so two devices that edit different fields
//! offline both keep their edits, and when they edit the same field the newer
//! edit wins and the older value is kept in the item's history. Nothing is
//! ever silently lost.
//!
//! `merge` is commutative, associative and idempotent (checked with property
//! tests), so devices converge whatever order they sync in.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, Result, card, hlc::Hlc, secret::SecretString};

/// Maximum number of history entries kept per item.
pub const MAX_HISTORY: usize = 64;

/// What kind of item this is. Fixed at creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    /// Website or app login (may include a TOTP secret).
    Login,
    /// Payment card.
    Card,
    /// Free-form secure note.
    SecureNote,
    /// Name, address, phone, email.
    Identity,
    /// WebAuthn passkey.
    Passkey,
}

/// Standard field names. Custom fields use the `custom:` prefix.
pub mod fields {
    /// Item title shown in lists.
    pub const TITLE: &str = "title";
    /// Free-form notes.
    pub const NOTES: &str = "notes";
    /// Login username or email.
    pub const USERNAME: &str = "username";
    /// Login password.
    pub const PASSWORD: &str = "password";
    /// Primary URL. Further URLs use `url.2`, `url.3`...
    pub const URL: &str = "url";
    /// TOTP secret or `otpauth://` URI.
    pub const TOTP: &str = "totp";
    /// Name on the card.
    pub const CARDHOLDER: &str = "cardholder";
    /// Card number (digits only).
    pub const CARD_NUMBER: &str = "card_number";
    /// Expiry month, 1-12.
    pub const EXP_MONTH: &str = "exp_month";
    /// Expiry year, four digits.
    pub const EXP_YEAR: &str = "exp_year";
    /// Card security code.
    pub const CVV: &str = "cvv";
    /// Card PIN.
    pub const PIN: &str = "pin";
    /// Passkey relying-party id (domain).
    pub const RP_ID: &str = "rp_id";
    /// Passkey credential id (base64url).
    pub const CREDENTIAL_ID: &str = "credential_id";
    /// Passkey user handle (base64url).
    pub const USER_HANDLE: &str = "user_handle";
    /// Passkey private key (PKCS#8, base64url).
    pub const PRIVATE_KEY: &str = "private_key";
    /// Prefix for user-defined fields.
    pub const CUSTOM_PREFIX: &str = "custom:";
}

/// Fields that are hidden by default in the UI and never offered to
/// clipboard history or screenshots.
pub fn is_concealed_by_default(name: &str) -> bool {
    use fields::*;
    matches!(
        name,
        PASSWORD | TOTP | CARD_NUMBER | CVV | PIN | PRIVATE_KEY
    )
}

/// A field's value. Every variant is zeroized on drop and redacted in logs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FieldValue {
    /// Shown normally.
    Text(SecretString),
    /// Hidden until revealed.
    Concealed(SecretString),
    /// The field was removed (a tombstone, so the removal syncs).
    Removed,
}

impl FieldValue {
    /// The string value, if present.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::Concealed(s) => Some(s.expose()),
            Self::Removed => None,
        }
    }
}

/// A field with its last-modified timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    /// Current value.
    pub value: FieldValue,
    /// When it was last set.
    pub modified: Hlc,
}

/// A superseded value, kept so edits and conflicts are recoverable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When this value was set (not when it was replaced).
    pub modified: Hlc,
    /// Field name.
    pub field: String,
    /// The old value.
    pub value: FieldValue,
}

/// A vault item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    id: Uuid,
    kind: ItemKind,
    created: Hlc,
    fields: BTreeMap<String, Field>,
    /// Trash state as a last-writer-wins register.
    deleted: (bool, Hlc),
    history: Vec<HistoryEntry>,
}

impl Item {
    /// A new empty item.
    pub fn new(kind: ItemKind, now: Hlc) -> Self {
        Self::with_id(crate::secret::random_uuid(), kind, now)
    }

    /// A new item with a given id (used by importers that keep source ids stable).
    pub fn with_id(id: Uuid, kind: ItemKind, now: Hlc) -> Self {
        Self {
            id,
            kind,
            created: now,
            fields: BTreeMap::new(),
            deleted: (false, Hlc::ZERO),
            history: Vec::new(),
        }
    }

    /// Item id.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Item kind.
    pub fn kind(&self) -> ItemKind {
        self.kind
    }
    /// Creation timestamp.
    pub fn created(&self) -> Hlc {
        self.created
    }
    /// Whether the item is in the trash.
    pub fn is_deleted(&self) -> bool {
        self.deleted.0
    }
    /// Superseded values, oldest first.
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }
    /// Latest timestamp anywhere in the item.
    pub fn last_modified(&self) -> Hlc {
        self.fields
            .values()
            .map(|f| f.modified)
            .chain([self.created, self.deleted.1])
            .max()
            .unwrap_or(Hlc::ZERO)
    }

    /// Current value of a field, if set.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.get(name).and_then(|f| f.value.as_str())
    }

    /// Names of fields that currently have a value.
    pub fn field_names(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .filter(|(_, f)| f.value != FieldValue::Removed)
            .map(|(k, _)| k.as_str())
    }

    /// Raw field map, including tombstones.
    pub fn fields(&self) -> &BTreeMap<String, Field> {
        &self.fields
    }

    /// Set a field. Uses concealed or plain display based on the field name.
    /// Card numbers are validated and stored as digits only.
    pub fn set(&mut self, name: &str, value: &str, at: Hlc) -> Result<()> {
        if name.is_empty() {
            return Err(Error::InvalidArgument("empty field name"));
        }
        let value = SecretString::new(self.normalise(name, value)?);
        let v = if is_concealed_by_default(name) {
            FieldValue::Concealed(value)
        } else {
            FieldValue::Text(value)
        };
        self.put(name, v, at);
        Ok(())
    }

    /// Remove a field (records a tombstone so the removal syncs).
    pub fn remove(&mut self, name: &str, at: Hlc) {
        if self.fields.contains_key(name) {
            self.put(name, FieldValue::Removed, at);
        }
    }

    /// Move to or restore from the trash.
    pub fn set_deleted(&mut self, deleted: bool, at: Hlc) {
        if at > self.deleted.1 {
            self.deleted = (deleted, at);
        }
    }

    fn normalise(&self, name: &str, value: &str) -> Result<String> {
        Ok(match (self.kind, name) {
            (ItemKind::Card, fields::CARD_NUMBER) => card::normalise_number(value)?,
            (ItemKind::Card, fields::EXP_MONTH) => card::parse_month(value)?.to_string(),
            (ItemKind::Card, fields::EXP_YEAR) => card::parse_year(value)?.to_string(),
            _ => value.to_owned(),
        })
    }

    fn put(&mut self, name: &str, value: FieldValue, at: Hlc) {
        match self.fields.get(name) {
            Some(old) if old.modified >= at => {
                // A newer value already exists (stale local write); keep it,
                // remember ours in history.
                self.push_history(HistoryEntry {
                    modified: at,
                    field: name.to_owned(),
                    value,
                });
            }
            Some(old) if old.value == value => {
                self.fields.insert(
                    name.to_owned(),
                    Field {
                        value,
                        modified: at,
                    },
                );
            }
            Some(old) => {
                let prev = HistoryEntry {
                    modified: old.modified,
                    field: name.to_owned(),
                    value: old.value.clone(),
                };
                self.fields.insert(
                    name.to_owned(),
                    Field {
                        value,
                        modified: at,
                    },
                );
                self.push_history(prev);
            }
            None => {
                self.fields.insert(
                    name.to_owned(),
                    Field {
                        value,
                        modified: at,
                    },
                );
            }
        }
    }

    fn push_history(&mut self, e: HistoryEntry) {
        if e.value == FieldValue::Removed {
            return;
        }
        let mut set: BTreeSet<HistoryEntry> = self.history.drain(..).collect();
        set.insert(e);
        self.history = cap_history(set);
    }

    /// Merge two versions of the same item. See the module docs.
    pub fn merge(&self, other: &Self) -> Result<Self> {
        if self.id != other.id {
            return Err(Error::Merge("different item ids"));
        }
        if self.kind != other.kind {
            return Err(Error::Merge("different item kinds"));
        }
        let mut history: BTreeSet<HistoryEntry> =
            self.history.iter().chain(&other.history).cloned().collect();
        let mut fields = BTreeMap::new();
        let names: BTreeSet<&String> = self.fields.keys().chain(other.fields.keys()).collect();
        for name in names {
            let f = match (self.fields.get(name), other.fields.get(name)) {
                (Some(a), Some(b)) => {
                    let (win, lose) = if (a.modified, &a.value) >= (b.modified, &b.value) {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    // Compare value AND timestamp: recording only value changes would
                    // make the result depend on merge order.
                    if lose != win && lose.value != FieldValue::Removed {
                        history.insert(HistoryEntry {
                            modified: lose.modified,
                            field: name.clone(),
                            value: lose.value.clone(),
                        });
                    }
                    win.clone()
                }
                (Some(a), None) | (None, Some(a)) => a.clone(),
                (None, None) => unreachable!(),
            };
            fields.insert(name.clone(), f);
        }
        let deleted = if (self.deleted.1, self.deleted.0) >= (other.deleted.1, other.deleted.0) {
            self.deleted
        } else {
            other.deleted
        };
        Ok(Self {
            id: self.id,
            kind: self.kind,
            created: self.created.min(other.created),
            fields,
            deleted,
            history: cap_history(history),
        })
    }
}

fn cap_history(set: BTreeSet<HistoryEntry>) -> Vec<HistoryEntry> {
    let n = set.len();
    set.into_iter()
        .skip(n.saturating_sub(MAX_HISTORY))
        .collect()
}

/// Builders for common item types.
impl Item {
    /// A login item.
    pub fn login(title: &str, username: &str, password: &str, url: &str, at: Hlc) -> Result<Self> {
        let mut i = Self::new(ItemKind::Login, at);
        i.set(fields::TITLE, title, at)?;
        for (k, v) in [
            (fields::USERNAME, username),
            (fields::PASSWORD, password),
            (fields::URL, url),
        ] {
            if !v.is_empty() {
                i.set(k, v, at)?;
            }
        }
        Ok(i)
    }

    /// A payment card item. The number is Luhn-checked.
    pub fn card(
        title: &str,
        cardholder: &str,
        number: &str,
        exp_month: &str,
        exp_year: &str,
        cvv: &str,
        at: Hlc,
    ) -> Result<Self> {
        let mut i = Self::new(ItemKind::Card, at);
        i.set(fields::TITLE, title, at)?;
        i.set(fields::CARD_NUMBER, number, at)?;
        for (k, v) in [
            (fields::CARDHOLDER, cardholder),
            (fields::EXP_MONTH, exp_month),
            (fields::EXP_YEAR, exp_year),
            (fields::CVV, cvv),
        ] {
            if !v.is_empty() {
                i.set(k, v, at)?;
            }
        }
        Ok(i)
    }

    /// A secure note.
    pub fn note(title: &str, body: &str, at: Hlc) -> Result<Self> {
        let mut i = Self::new(ItemKind::SecureNote, at);
        i.set(fields::TITLE, title, at)?;
        i.set(fields::NOTES, body, at)?;
        Ok(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn t(ms: u64, dev: u64) -> Hlc {
        Hlc {
            millis: ms,
            counter: 0,
            device: dev,
        }
    }

    #[test]
    fn concurrent_edits_to_different_fields_both_survive() {
        let base =
            Item::login("Bank", "eslam", "old-pass", "https://bank.example", t(1, 0)).unwrap();
        let mut phone = base.clone();
        let mut laptop = base.clone();
        phone.set(fields::PASSWORD, "new-pass", t(10, 1)).unwrap();
        laptop
            .set(fields::USERNAME, "eslam@example.com", t(11, 2))
            .unwrap();
        let m = phone.merge(&laptop).unwrap();
        assert_eq!(m.get(fields::PASSWORD), Some("new-pass"));
        assert_eq!(m.get(fields::USERNAME), Some("eslam@example.com"));
    }

    #[test]
    fn same_field_conflict_keeps_loser_in_history() {
        let base = Item::login("Bank", "u", "p0", "", t(1, 0)).unwrap();
        let mut a = base.clone();
        let mut b = base.clone();
        a.set(fields::PASSWORD, "from-a", t(10, 1)).unwrap();
        b.set(fields::PASSWORD, "from-b", t(20, 2)).unwrap();
        let m = a.merge(&b).unwrap();
        assert_eq!(m.get(fields::PASSWORD), Some("from-b"));
        let old: Vec<_> = m
            .history()
            .iter()
            .filter_map(|h| h.value.as_str())
            .collect();
        assert!(old.contains(&"from-a") && old.contains(&"p0"));
    }

    #[test]
    fn removal_syncs_and_newer_set_beats_removal() {
        let base = Item::login("X", "u", "p", "https://x", t(1, 0)).unwrap();
        let mut a = base.clone();
        a.remove(fields::URL, t(5, 1));
        assert_eq!(a.merge(&base).unwrap().get(fields::URL), None);
        let mut b = base.clone();
        b.set(fields::URL, "https://y", t(6, 2)).unwrap();
        assert_eq!(a.merge(&b).unwrap().get(fields::URL), Some("https://y"));
    }

    #[test]
    fn delete_and_restore() {
        let mut a = Item::note("n", "b", t(1, 0)).unwrap();
        let mut b = a.clone();
        a.set_deleted(true, t(5, 1));
        assert!(a.merge(&b).unwrap().is_deleted());
        b.set_deleted(false, t(6, 2));
        assert!(!a.merge(&b).unwrap().is_deleted());
    }

    #[test]
    fn merge_rejects_mismatches() {
        let a = Item::note("a", "", t(1, 0)).unwrap();
        let b = Item::note("b", "", t(1, 0)).unwrap();
        assert!(a.merge(&b).is_err());
        let c = Item::with_id(a.id(), ItemKind::Login, t(1, 0));
        assert!(a.merge(&c).is_err());
    }

    #[test]
    fn stale_local_write_does_not_override() {
        let mut a = Item::login("X", "u", "p", "", t(1, 0)).unwrap();
        a.set(fields::PASSWORD, "new", t(10, 1)).unwrap();
        a.set(fields::PASSWORD, "stale", t(5, 1)).unwrap();
        assert_eq!(a.get(fields::PASSWORD), Some("new"));
        assert!(
            a.history()
                .iter()
                .any(|h| h.value.as_str() == Some("stale"))
        );
    }

    #[test]
    fn card_fields_validated() {
        let c = Item::card(
            "Visa",
            "ESLAM",
            "4111 1111 1111 1111",
            "07",
            "2029",
            "123",
            t(1, 0),
        )
        .unwrap();
        assert_eq!(c.get(fields::CARD_NUMBER), Some("4111111111111111"));
        assert_eq!(c.get(fields::EXP_MONTH), Some("7"));
        assert!(Item::card("Bad", "", "4111 1111 1111 1112", "", "", "", t(1, 0)).is_err());
        assert!(Item::card("Bad", "", "4111111111111111", "13", "", "", t(1, 0)).is_err());
    }

    #[test]
    fn history_is_capped() {
        let mut a = Item::login("X", "u", "p", "", t(1, 0)).unwrap();
        for i in 0..200 {
            a.set(fields::PASSWORD, &format!("p{i}"), t(10 + i, 1))
                .unwrap();
        }
        assert_eq!(a.history().len(), MAX_HISTORY);
        assert_eq!(a.history().last().unwrap().value.as_str(), Some("p198"));
    }

    // ---- property tests: convergence ----

    #[derive(Debug, Clone)]
    enum Op {
        Set(u8, u8),
        Remove(u8),
        Delete(bool),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0u8..4, 0u8..4).prop_map(|(f, v)| Op::Set(f, v)),
            (0u8..4).prop_map(Op::Remove),
            any::<bool>().prop_map(Op::Delete),
        ]
    }

    fn replica(base: &Item, dev: u64, ops: &[(Op, u64)]) -> Item {
        let mut it = base.clone();
        for (o, ms) in ops {
            let at = t(*ms, dev);
            match o {
                Op::Set(f, v) => it.set(&format!("f{f}"), &format!("v{v}"), at).unwrap(),
                Op::Remove(f) => it.remove(&format!("f{f}"), at),
                Op::Delete(d) => it.set_deleted(*d, at),
            }
        }
        it
    }

    fn ops() -> impl Strategy<Value = Vec<(Op, u64)>> {
        prop::collection::vec((op(), 1u64..50), 0..12)
    }

    proptest! {
        #[test]
        fn merge_is_commutative_associative_idempotent(a in ops(), b in ops(), c in ops()) {
            let base = Item::with_id(Uuid::nil(), ItemKind::Login, Hlc::ZERO);
            let (ra, rb, rc) = (replica(&base, 1, &a), replica(&base, 2, &b), replica(&base, 3, &c));
            prop_assert_eq!(ra.merge(&rb).unwrap(), rb.merge(&ra).unwrap());
            prop_assert_eq!(
                ra.merge(&rb).unwrap().merge(&rc).unwrap(),
                ra.merge(&rb.merge(&rc).unwrap()).unwrap()
            );
            prop_assert_eq!(ra.merge(&ra).unwrap(), ra.clone());
        }
    }
}
