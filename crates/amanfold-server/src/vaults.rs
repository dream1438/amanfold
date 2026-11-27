//! Vaults: creation, verified sync, compare-and-swap commits and key rotation.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use amanfold_core::{
    FORMAT_VERSION,
    keys::PublicKeys,
    manifest::{self, SignedManifest},
    sync::{EncryptedRecord, MAX_ITEM_BYTES, from_cbor, to_cbor},
    vault::VaultGrant,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AppState, Authed,
    error::{ApiError, ApiResult},
    now_secs,
    store::{self, Role},
    wire::B64,
};

type St = State<Arc<AppState>>;

/// Largest stored record: the biggest padded item plus envelope overhead.
pub const MAX_RECORD_BYTES: usize = MAX_ITEM_BYTES + 128 * 1024;
/// Most records in one commit.
pub const MAX_RECORDS_PER_COMMIT: usize = 50_000;
/// Most vaults one account may own.
pub const MAX_OWNED_VAULTS: i64 = 500;

pub(crate) fn caller_keys(c: &Connection, account: &Uuid) -> ApiResult<PublicKeys> {
    let (pk, _) = store::account_keys(c, account)?.ok_or(ApiError::Unauthorized)?;
    Ok(from_cbor(&pk)?)
}

/// The caller's role in a vault; `NotFound` for non-members so vault ids
/// cannot be probed.
pub(crate) fn require_member(
    c: &Connection,
    vault: &Uuid,
    account: &Uuid,
    need_active: bool,
) -> ApiResult<(store::VaultRow, Role, bool)> {
    let (role, active) = store::membership(c, vault, account)?.ok_or(ApiError::NotFound)?;
    let row = store::vault(c, vault)?.ok_or(ApiError::NotFound)?;
    if need_active && !active {
        return Err(ApiError::Forbidden(
            "waiting for the vault key to be shared",
        ));
    }
    Ok((row, role, active))
}

/// Parse an uploaded grant and check its envelope (not its signature: only
/// the recipient can verify that, and does).
pub(crate) fn parse_grant(
    b: &[u8],
    vault: &Uuid,
    epoch: u32,
    granter: &PublicKeys,
) -> ApiResult<(VaultGrant, Vec<u8>)> {
    let g: VaultGrant = from_cbor(b)?;
    if g.format != FORMAT_VERSION {
        return Err(ApiError::BadRequest("unsupported grant format"));
    }
    if g.vault_id != *vault {
        return Err(ApiError::BadRequest("grant is for another vault"));
    }
    if g.epoch != epoch {
        return Err(ApiError::BadRequest("grant is for another key epoch"));
    }
    if g.granter != *granter {
        return Err(ApiError::BadRequest("grant must be made by the uploader"));
    }
    if g.signature.len() != 64 {
        return Err(ApiError::BadRequest("grant signature"));
    }
    let canon = to_cbor(&g).to_vec();
    Ok((g, canon))
}

/// Check an uploaded manifest: signed by `signer`, for this vault, epoch and
/// sequence number.
fn parse_manifest(
    b: &[u8],
    vault: &Uuid,
    epoch: u32,
    current_seq: u64,
    signer: &PublicKeys,
) -> ApiResult<(SignedManifest, Vec<u8>)> {
    let m: SignedManifest = from_cbor(b)?;
    if m.body.signer != *signer {
        return Err(ApiError::BadRequest(
            "manifest must be signed by the uploader",
        ));
    }
    m.verify(std::slice::from_ref(signer))
        .map_err(|_| ApiError::BadRequest("manifest signature"))?;
    if m.body.vault_id != *vault {
        return Err(ApiError::BadRequest("manifest is for another vault"));
    }
    if m.body.seq != current_seq + 1 {
        return Err(ApiError::Conflict { seq: current_seq });
    }
    if m.body.epoch != epoch {
        // A writer on an old key: it must sync, take the new grant and retry.
        return Err(ApiError::Conflict { seq: current_seq });
    }
    let canon = to_cbor(&m).to_vec();
    Ok((m, canon))
}

struct Upload {
    item_id: Uuid,
    digest: [u8; 32],
    bytes: Vec<u8>,
}

/// Check uploaded records: each must be for this vault and epoch, listed in
/// the manifest with exactly its digest, and not too large.
fn parse_records(recs: &[B64], vault: &Uuid, m: &SignedManifest) -> ApiResult<Vec<Upload>> {
    if recs.len() > MAX_RECORDS_PER_COMMIT {
        return Err(ApiError::TooLarge("too many records in one request"));
    }
    let mut seen = BTreeSet::new();
    recs.iter()
        .map(|b| {
            if b.0.len() > MAX_RECORD_BYTES {
                return Err(ApiError::TooLarge("record"));
            }
            let r: EncryptedRecord = from_cbor(&b.0)?;
            if r.format != FORMAT_VERSION {
                return Err(ApiError::BadRequest("unsupported record format"));
            }
            if r.vault_id != *vault || r.key_epoch != m.body.epoch {
                return Err(ApiError::BadRequest(
                    "record is for another vault or key epoch",
                ));
            }
            if !seen.insert(r.item_id) {
                return Err(ApiError::BadRequest("record uploaded twice"));
            }
            let digest = manifest::record_digest(&r);
            if m.body.entries.get(&r.item_id).map(|d| d.0) != Some(digest) {
                return Err(ApiError::BadRequest("record does not match the manifest"));
            }
            Ok(Upload {
                item_id: r.item_id,
                digest,
                bytes: to_cbor(&r).to_vec(),
            })
        })
        .collect()
}

/// After writing, the stored records must be exactly what the manifest lists.
fn check_stored_matches(c: &Connection, vault: &Uuid, m: &SignedManifest) -> ApiResult<u64> {
    let mut s =
        c.prepare("SELECT item_id, digest, length(record) FROM records WHERE vault_id = ?1")?;
    let mut stored = BTreeMap::new();
    let mut bytes = 0u64;
    for row in s.query_map(params![vault.as_bytes()], |r| {
        Ok((
            store::uuid_col(r.get(0)?),
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (id, d, n) = row?;
        bytes += n as u64;
        stored.insert(id, d);
    }
    if stored.len() != m.body.entries.len()
        || m.body
            .entries
            .iter()
            .any(|(id, d)| stored.get(id).map(Vec::as_slice) != Some(&d.0[..]))
    {
        return Err(ApiError::BadRequest(
            "manifest does not match the vault: sync first, and never drop items",
        ));
    }
    Ok(bytes)
}

fn vault_json(id: &Uuid, row: &store::VaultRow, role: Role, active: bool) -> Value {
    json!({
        "vault_id": id,
        "owner_id": row.owner_id,
        "role": role,
        "active": active,
        "epoch": row.epoch,
        "seq": row.seq,
        "needs_rotation": row.needs_rotation,
        "bytes": row.bytes,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/vaults` body.
#[derive(Deserialize)]
pub struct CreateReq {
    /// The owner's grant to itself (from `Account::create_vault`).
    grant: B64,
}

/// `POST /v1/vaults`: create a vault owned by the caller.
pub async fn create(
    State(st): St,
    a: Authed,
    Json(req): Json<CreateReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = st
        .db(move |c| {
            let me = caller_keys(c, &a.account_id)?;
            let g: VaultGrant = from_cbor(&req.grant.0)?;
            let (g, canon) = parse_grant(&req.grant.0, &g.vault_id, 0, &me)?;
            if g.recipient != me {
                return Err(ApiError::BadRequest("the first grant must be to the owner"));
            }
            let owned: i64 = c.query_row(
                "SELECT count(*) FROM vaults WHERE owner_id = ?1",
                params![a.account_id.as_bytes()],
                |r| r.get(0),
            )?;
            if owned >= MAX_OWNED_VAULTS {
                return Err(ApiError::TooLarge("too many vaults"));
            }
            let tx = c.transaction()?;
            let now = now_secs() as i64;
            let n = tx.execute(
                "INSERT OR IGNORE INTO vaults (id, owner_id, epoch, created_at) VALUES (?1, ?2, 0, ?3)",
                params![g.vault_id.as_bytes(), a.account_id.as_bytes(), now],
            )?;
            if n == 0 {
                return Err(ApiError::Exists("vault"));
            }
            tx.execute(
                "INSERT INTO members (vault_id, account_id, role, active, added_at) VALUES (?1, ?2, 'owner', 1, ?3)",
                params![g.vault_id.as_bytes(), a.account_id.as_bytes(), now],
            )?;
            tx.execute(
                "INSERT INTO grants (vault_id, account_id, epoch, body) VALUES (?1, ?2, 0, ?3)",
                params![g.vault_id.as_bytes(), a.account_id.as_bytes(), canon],
            )?;
            tx.commit()?;
            Ok(g.vault_id)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "vault_id": id }))))
}

/// `GET /v1/vaults`: vaults the caller belongs to.
pub async fn list(State(st): St, a: Authed) -> ApiResult<Json<Value>> {
    let v = st
        .db(move |c| {
            let mut s = c.prepare(
                "SELECT m.vault_id, m.role, m.active FROM members m WHERE m.account_id = ?1 ORDER BY m.added_at",
            )?;
            let rows = s
                .query_map(params![a.account_id.as_bytes()], |r| {
                    Ok((store::uuid_col(r.get(0)?), r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::new();
            for (id, role, active) in rows {
                if let Some(row) = store::vault(c, &id)? {
                    out.push(vault_json(&id, &row, Role::parse(&role)?, active != 0));
                }
            }
            Ok(out)
        })
        .await?;
    Ok(Json(json!({ "vaults": v })))
}

/// `GET /v1/vaults/{id}`: the vault's state and the caller's grants (every
/// epoch, oldest first, so a device can open an old cache and then move to
/// the newest key).
pub async fn get(State(st): St, a: Authed, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let v = st
        .db(move |c| {
            let (row, role, active) = require_member(c, &id, &a.account_id, false)?;
            let mut s = c.prepare(
                "SELECT epoch, body FROM grants WHERE vault_id = ?1 AND account_id = ?2 ORDER BY epoch",
            )?;
            let grants = s
                .query_map(params![id.as_bytes(), a.account_id.as_bytes()], |r| {
                    Ok(json!({ "epoch": r.get::<_, i64>(0)?, "grant": B64(r.get(1)?) }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut v = vault_json(&id, &row, role, active);
            v["grants"] = Value::Array(grants);
            Ok(v)
        })
        .await?;
    Ok(Json(v))
}

/// `GET /v1/vaults/{id}/sync` query.
#[derive(Deserialize)]
pub struct SyncQuery {
    /// Sequence number this device last synced; 0 or absent for everything.
    since: Option<u64>,
}

/// `GET /v1/vaults/{id}/sync?since=N`: the current signed manifest and every
/// record written after manifest `N`. Pass both to `LocalVault::sync`.
pub async fn sync(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
    Query(q): Query<SyncQuery>,
) -> ApiResult<Json<Value>> {
    let since = q.since.unwrap_or(0).min(i64::MAX as u64) as i64;
    let v = st
        .db(move |c| {
            let (row, _, _) = require_member(c, &id, &a.account_id, true)?;
            let mut s = c.prepare(
                "SELECT record FROM records WHERE vault_id = ?1 AND seq > ?2 ORDER BY item_id",
            )?;
            let records = s
                .query_map(params![id.as_bytes(), since], |r| Ok(B64(r.get(0)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "epoch": row.epoch,
                "seq": row.seq,
                "manifest": row.manifest.map(B64),
                "records": records,
            }))
        })
        .await?;
    Ok(Json(v))
}

/// `POST /v1/vaults/{id}/commit` body.
#[derive(Deserialize)]
pub struct CommitReq {
    /// From `LocalVault::sign_manifest`, signed after the records below.
    manifest: B64,
    /// Changed records (`SavedItem.record`).
    #[serde(default)]
    records: Vec<B64>,
}

/// `POST /v1/vaults/{id}/commit`: upload changed records and the new signed
/// manifest atomically. The manifest's sequence number must be exactly one
/// more than the stored one; otherwise `409 conflict` with the current
/// number, and the device syncs and retries.
pub async fn commit(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<CommitReq>,
) -> ApiResult<Json<Value>> {
    let max_bytes = st.cfg.max_vault_bytes;
    let seq = st
        .db(move |c| {
            let tx = c.transaction()?;
            let (row, role, _) = require_member(&tx, &id, &a.account_id, true)?;
            if !role.can_write() {
                return Err(ApiError::Forbidden("read-only member"));
            }
            let me = caller_keys(&tx, &a.account_id)?;
            let (m, canon) = parse_manifest(&req.manifest.0, &id, row.epoch, row.seq, &me)?;
            let uploads = parse_records(&req.records, &id, &m)?;
            let seq = m.body.seq;
            for u in &uploads {
                tx.execute(
                    "INSERT INTO records (vault_id, item_id, seq, digest, record) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (vault_id, item_id) DO UPDATE SET seq = ?3, digest = ?4, record = ?5",
                    params![id.as_bytes(), u.item_id.as_bytes(), seq as i64, u.digest.to_vec(), u.bytes],
                )?;
            }
            let bytes = check_stored_matches(&tx, &id, &m)?;
            if bytes > max_bytes {
                return Err(ApiError::TooLarge("vault storage limit reached"));
            }
            tx.execute(
                "UPDATE vaults SET seq = ?1, manifest = ?2, bytes = ?3 WHERE id = ?4",
                params![seq as i64, canon, bytes as i64, id.as_bytes()],
            )?;
            tx.commit()?;
            Ok(seq)
        })
        .await?;
    Ok(Json(json!({ "seq": seq })))
}

/// `POST /v1/vaults/{id}/rotate` body.
#[derive(Deserialize)]
pub struct RotateReq {
    /// Manifest for the new epoch (after `LocalVault::rotate_key`).
    manifest: B64,
    /// Every record, re-encrypted under the new key.
    records: Vec<B64>,
    /// A new-epoch grant for every remaining member with access, including
    /// the uploader.
    grants: Vec<B64>,
}

/// `POST /v1/vaults/{id}/rotate`: replace the vault key after removing a
/// member. Atomic: every record, the manifest and the remaining members'
/// grants change together.
pub async fn rotate(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<RotateReq>,
) -> ApiResult<Json<Value>> {
    let max_bytes = st.cfg.max_vault_bytes;
    let (epoch, seq) = st
        .db(move |c| {
            let tx = c.transaction()?;
            let (row, role, _) = require_member(&tx, &id, &a.account_id, true)?;
            if !role.can_manage() {
                return Err(ApiError::Forbidden("only owners and managers rotate the key"));
            }
            let me = caller_keys(&tx, &a.account_id)?;
            let new_epoch = row.epoch + 1;
            let (m, canon) = parse_manifest(&req.manifest.0, &id, new_epoch, row.seq, &me)?;
            let uploads = parse_records(&req.records, &id, &m)?;
            if uploads.len() != m.body.entries.len() {
                return Err(ApiError::BadRequest("rotation must re-upload every record"));
            }
            // Remaining members with access must each get exactly one grant.
            let mut s = tx.prepare(
                "SELECT m.account_id, a.public_keys FROM members m JOIN accounts a ON a.id = m.account_id
                 WHERE m.vault_id = ?1 AND m.active = 1",
            )?;
            let mut members: BTreeMap<Vec<u8>, Uuid> = BTreeMap::new();
            for r in s.query_map(params![id.as_bytes()], |r| {
                Ok((store::uuid_col(r.get(0)?), r.get::<_, Vec<u8>>(1)?))
            })? {
                let (acc, pk) = r?;
                let pk: PublicKeys = from_cbor(&pk)?;
                members.insert(to_cbor(&pk).to_vec(), acc);
            }
            drop(s);
            let mut grants = BTreeMap::new();
            for g in &req.grants {
                let (g, canon) = parse_grant(&g.0, &id, new_epoch, &me)?;
                let acc = members
                    .get(to_cbor(&g.recipient).as_slice())
                    .ok_or(ApiError::BadRequest("grant for someone who is not a member"))?;
                if grants.insert(*acc, canon).is_some() {
                    return Err(ApiError::BadRequest("two grants for one member"));
                }
            }
            if grants.len() != members.len() {
                return Err(ApiError::BadRequest("every remaining member needs a new grant"));
            }
            tx.execute("DELETE FROM records WHERE vault_id = ?1", params![id.as_bytes()])?;
            let seq = m.body.seq;
            for u in &uploads {
                tx.execute(
                    "INSERT INTO records (vault_id, item_id, seq, digest, record) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id.as_bytes(), u.item_id.as_bytes(), seq as i64, u.digest.to_vec(), u.bytes],
                )?;
            }
            // Every item the vault had must still be there (checked against
            // the old manifest's entries via the new one).
            if let Some(old) = &row.manifest {
                let old: SignedManifest = from_cbor(old)?;
                if old.body.entries.keys().any(|k| !m.body.entries.contains_key(k)) {
                    return Err(ApiError::BadRequest("rotation dropped an item"));
                }
            }
            let bytes = check_stored_matches(&tx, &id, &m)?;
            if bytes > max_bytes {
                return Err(ApiError::TooLarge("vault storage limit reached"));
            }
            for (acc, g) in &grants {
                tx.execute(
                    "INSERT INTO grants (vault_id, account_id, epoch, body) VALUES (?1, ?2, ?3, ?4)",
                    params![id.as_bytes(), acc.as_bytes(), new_epoch as i64, g],
                )?;
            }
            tx.execute(
                "UPDATE vaults SET epoch = ?1, seq = ?2, manifest = ?3, bytes = ?4, needs_rotation = 0 WHERE id = ?5",
                params![new_epoch as i64, seq as i64, canon, bytes as i64, id.as_bytes()],
            )?;
            tx.commit()?;
            Ok((new_epoch, seq))
        })
        .await?;
    tracing::info!(vault = %id, epoch, "vault key rotated");
    Ok(Json(json!({ "epoch": epoch, "seq": seq })))
}

/// `DELETE /v1/vaults/{id}`: delete a vault and everything in it (owner only).
pub async fn delete(State(st): St, a: Authed, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    st.db(move |c| {
        let (_, role, _) = require_member(c, &id, &a.account_id, false)?;
        if role != Role::Owner {
            return Err(ApiError::Forbidden("only the owner deletes a vault"));
        }
        c.execute("DELETE FROM vaults WHERE id = ?1", params![id.as_bytes()])?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
