//! Sharing with family and team members: invitations, members, roles and grants.
//!
//! Flow: a manager invites an email address; the invitee (once signed up)
//! accepts, which makes them a pending member whose public keys the manager
//! can fetch; the manager compares safety numbers with them, then uploads a
//! grant sealed to their keys, which makes them active. The server only
//! relays keys and grants; it cannot make a grant itself, because grants are
//! signed by the granter and checked by the recipient.

use std::sync::Arc;

use amanfold_core::{keys::PublicKeys, sync::from_cbor};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AppState, Authed,
    error::{ApiError, ApiResult},
    mail::Mail,
    normalize_email, now_secs, random_uuid,
    store::{self, Role},
    vaults::{caller_keys, parse_grant, require_member},
    wire::B64,
};

type St = State<Arc<AppState>>;

const INVITE_TTL: u64 = 14 * 86_400;

fn invitable(by: Role, role: Role) -> ApiResult<()> {
    match (by, role) {
        (_, Role::Owner) => Err(ApiError::BadRequest("a vault has one owner")),
        (Role::Owner, _) => Ok(()),
        (Role::Manager, Role::Writer | Role::Reader) => Ok(()),
        _ => Err(ApiError::Forbidden("not allowed to give that role")),
    }
}

/// `POST /v1/vaults/{id}/invites` body.
#[derive(Deserialize)]
pub struct InviteReq {
    email: String,
    role: Role,
}

/// `POST /v1/vaults/{id}/invites`: invite an email address. The email goes
/// out whether or not the address has an account yet.
pub async fn invite(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<InviteReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let email = normalize_email(&req.email)?;
    st.limiter.hit(
        &format!("invite:{}", a.account_id),
        st.cfg.limits.invites_account,
    )?;
    let role = req.role;
    let e = email.clone();
    let (invite_id, from) = st
        .db(move |c| {
            let (_, my_role, _) = require_member(c, &id, &a.account_id, true)?;
            if !my_role.can_manage() {
                return Err(ApiError::Forbidden("only owners and managers invite"));
            }
            invitable(my_role, role)?;
            let (_, my_email) = store::account_keys(c, &a.account_id)?.ok_or(ApiError::Unauthorized)?;
            if my_email == e {
                return Err(ApiError::BadRequest("cannot invite yourself"));
            }
            let already: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM members m JOIN accounts a ON a.id = m.account_id
                 WHERE m.vault_id = ?1 AND a.email = ?2)",
                params![id.as_bytes(), e],
                |r| r.get(0),
            )?;
            if already {
                return Err(ApiError::Exists("member"));
            }
            let invite_id = random_uuid();
            c.execute(
                "INSERT INTO invites (id, vault_id, email, role, invited_by, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (vault_id, email) DO UPDATE SET id = ?1, role = ?4, invited_by = ?5, expires_at = ?6",
                params![
                    invite_id.as_bytes(),
                    id.as_bytes(),
                    e,
                    role.as_str(),
                    a.account_id.as_bytes(),
                    (now_secs() + INVITE_TTL) as i64
                ],
            )?;
            Ok((invite_id, my_email))
        })
        .await?;
    st.mailer.send(&email, Mail::Invitation { from });
    Ok((StatusCode::CREATED, Json(json!({ "invite_id": invite_id }))))
}

/// `GET /v1/vaults/{id}/invites`: open invitations for a vault (managers).
pub async fn vault_invites(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let v = st
        .db(move |c| {
            let (_, role, _) = require_member(c, &id, &a.account_id, true)?;
            if !role.can_manage() {
                return Err(ApiError::Forbidden("only owners and managers see invitations"));
            }
            let mut s = c.prepare(
                "SELECT id, email, role, expires_at FROM invites WHERE vault_id = ?1 AND expires_at > ?2",
            )?;
            let rows = s
                .query_map(params![id.as_bytes(), now_secs() as i64], |r| {
                    Ok(json!({
                        "invite_id": store::uuid_col(r.get(0)?),
                        "email": r.get::<_, String>(1)?,
                        "role": r.get::<_, String>(2)?,
                        "expires_at": r.get::<_, i64>(3)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "invites": v })))
}

/// `GET /v1/invites`: invitations addressed to the caller's email. Each
/// carries the inviter's public keys: compare their safety number before
/// trusting the grant they will send.
pub async fn my_invites(State(st): St, a: Authed) -> ApiResult<Json<Value>> {
    let v = st
        .db(move |c| {
            let (_, email) =
                store::account_keys(c, &a.account_id)?.ok_or(ApiError::Unauthorized)?;
            let mut s = c.prepare(
                "SELECT i.id, i.vault_id, i.role, i.expires_at, i.invited_by, a.email, a.public_keys
                 FROM invites i JOIN accounts a ON a.id = i.invited_by
                 WHERE i.email = ?1 AND i.expires_at > ?2",
            )?;
            let rows = s
                .query_map(params![email, now_secs() as i64], |r| {
                    Ok(json!({
                        "invite_id": store::uuid_col(r.get(0)?),
                        "vault_id": store::uuid_col(r.get(1)?),
                        "role": r.get::<_, String>(2)?,
                        "expires_at": r.get::<_, i64>(3)?,
                        "from": {
                            "account_id": store::uuid_col(r.get(4)?),
                            "email": r.get::<_, String>(5)?,
                            "public_keys": B64(r.get(6)?),
                        },
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "invites": v })))
}

/// `POST /v1/invites/{id}/accept`: join as a pending member. Access starts
/// when a manager uploads a grant for the caller's keys.
pub async fn accept_invite(
    State(st): St,
    a: Authed,
    Path(invite): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let vault = st
        .db(move |c| {
            let tx = c.transaction()?;
            let (_, email) = store::account_keys(&tx, &a.account_id)?.ok_or(ApiError::Unauthorized)?;
            let row: Option<(Vec<u8>, String, String, i64)> = tx
                .query_row(
                    "SELECT vault_id, email, role, expires_at FROM invites WHERE id = ?1",
                    params![invite.as_bytes()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            let (vault, to, role, exp) = row.ok_or(ApiError::NotFound)?;
            if to != email || now_secs() >= exp as u64 {
                return Err(ApiError::NotFound);
            }
            let vault = store::uuid_col(vault);
            tx.execute(
                "INSERT OR IGNORE INTO members (vault_id, account_id, role, active, added_at) VALUES (?1, ?2, ?3, 0, ?4)",
                params![vault.as_bytes(), a.account_id.as_bytes(), role, now_secs() as i64],
            )?;
            tx.execute("DELETE FROM invites WHERE id = ?1", params![invite.as_bytes()])?;
            tx.commit()?;
            Ok(vault)
        })
        .await?;
    Ok(Json(json!({ "vault_id": vault })))
}

/// `DELETE /v1/invites/{id}`: decline (invitee) or cancel (owner, manager).
pub async fn delete_invite(
    State(st): St,
    a: Authed,
    Path(invite): Path<Uuid>,
) -> ApiResult<StatusCode> {
    st.db(move |c| {
        let (_, email) = store::account_keys(c, &a.account_id)?.ok_or(ApiError::Unauthorized)?;
        let row: Option<(Vec<u8>, String)> = c
            .query_row(
                "SELECT vault_id, email FROM invites WHERE id = ?1",
                params![invite.as_bytes()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (vault, to) = row.ok_or(ApiError::NotFound)?;
        let vault = store::uuid_col(vault);
        let manager = matches!(store::membership(c, &vault, &a.account_id)?, Some((r, true)) if r.can_manage());
        if to != email && !manager {
            return Err(ApiError::NotFound);
        }
        c.execute("DELETE FROM invites WHERE id = ?1", params![invite.as_bytes()])?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/vaults/{id}/members`: members with roles and public keys. Use
/// the keys of verified writers as `trusted_writers` when syncing.
pub async fn members(State(st): St, a: Authed, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let v = st
        .db(move |c| {
            require_member(c, &id, &a.account_id, false)?;
            let mut s = c.prepare(
                "SELECT m.account_id, a.email, m.role, m.active, a.public_keys FROM members m
                 JOIN accounts a ON a.id = m.account_id WHERE m.vault_id = ?1 ORDER BY m.added_at",
            )?;
            let rows = s
                .query_map(params![id.as_bytes()], |r| {
                    Ok(json!({
                        "account_id": store::uuid_col(r.get(0)?),
                        "email": r.get::<_, String>(1)?,
                        "role": r.get::<_, String>(2)?,
                        "active": r.get::<_, i64>(3)? != 0,
                        "public_keys": B64(r.get(4)?),
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "members": v })))
}

/// `PUT /v1/vaults/{id}/members/{account}/grant` body.
#[derive(Deserialize)]
pub struct GrantReq {
    /// From `Account::grant_vault`, for the member's public keys.
    grant: B64,
}

/// `PUT /v1/vaults/{id}/members/{account}/grant`: give a member the current
/// vault key. Must be addressed to that member's keys and made by the caller.
pub async fn put_grant(
    State(st): St,
    a: Authed,
    Path((id, member)): Path<(Uuid, Uuid)>,
    Json(req): Json<GrantReq>,
) -> ApiResult<StatusCode> {
    st.db(move |c| {
        let tx = c.transaction()?;
        let (row, role, _) = require_member(&tx, &id, &a.account_id, true)?;
        if !role.can_manage() {
            return Err(ApiError::Forbidden(
                "only owners and managers share the key",
            ));
        }
        if store::membership(&tx, &id, &member)?.is_none() {
            return Err(ApiError::NotFound);
        }
        let me = caller_keys(&tx, &a.account_id)?;
        let (g, canon) = parse_grant(&req.grant.0, &id, row.epoch, &me)?;
        let (pk, _) = store::account_keys(&tx, &member)?.ok_or(ApiError::NotFound)?;
        let theirs: PublicKeys = from_cbor(&pk)?;
        if g.recipient != theirs {
            return Err(ApiError::BadRequest("grant is not for this member's keys"));
        }
        tx.execute(
            "INSERT INTO grants (vault_id, account_id, epoch, body) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (vault_id, account_id, epoch) DO UPDATE SET body = ?4",
            params![id.as_bytes(), member.as_bytes(), row.epoch as i64, canon],
        )?;
        tx.execute(
            "UPDATE members SET active = 1 WHERE vault_id = ?1 AND account_id = ?2",
            params![id.as_bytes(), member.as_bytes()],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /v1/vaults/{id}/members/{account}/role` body.
#[derive(Deserialize)]
pub struct RoleReq {
    role: Role,
}

/// `PUT /v1/vaults/{id}/members/{account}/role`: change a member's role (owner only).
pub async fn set_role(
    State(st): St,
    a: Authed,
    Path((id, member)): Path<(Uuid, Uuid)>,
    Json(req): Json<RoleReq>,
) -> ApiResult<StatusCode> {
    st.db(move |c| {
        let (_, role, _) = require_member(c, &id, &a.account_id, true)?;
        if role != Role::Owner {
            return Err(ApiError::Forbidden("only the owner changes roles"));
        }
        if req.role == Role::Owner || member == a.account_id {
            return Err(ApiError::BadRequest("a vault has one owner"));
        }
        let n = c.execute(
            "UPDATE members SET role = ?1 WHERE vault_id = ?2 AND account_id = ?3",
            params![req.role.as_str(), id.as_bytes(), member.as_bytes()],
        )?;
        if n == 0 {
            return Err(ApiError::NotFound);
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/vaults/{id}/members/{account}`: remove a member, or leave.
/// Their access ends now; if they ever held the key, the vault is marked
/// `needs_rotation` and an owner or manager device should rotate it.
pub async fn remove_member(
    State(st): St,
    a: Authed,
    Path((id, member)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    let needs = st
        .db(move |c| {
            let tx = c.transaction()?;
            let (_, my_role, _) = require_member(&tx, &id, &a.account_id, false)?;
            let (their_role, active) =
                store::membership(&tx, &id, &member)?.ok_or(ApiError::NotFound)?;
            if their_role == Role::Owner {
                return Err(ApiError::Forbidden(
                    "the owner cannot leave; delete the vault instead",
                ));
            }
            let allowed = member == a.account_id
                || my_role == Role::Owner
                || (my_role == Role::Manager && matches!(their_role, Role::Writer | Role::Reader));
            if !allowed {
                return Err(ApiError::Forbidden("not allowed to remove this member"));
            }
            tx.execute(
                "DELETE FROM members WHERE vault_id = ?1 AND account_id = ?2",
                params![id.as_bytes(), member.as_bytes()],
            )?;
            tx.execute(
                "DELETE FROM grants WHERE vault_id = ?1 AND account_id = ?2",
                params![id.as_bytes(), member.as_bytes()],
            )?;
            if active {
                tx.execute(
                    "UPDATE vaults SET needs_rotation = 1 WHERE id = ?1",
                    params![id.as_bytes()],
                )?;
            }
            tx.commit()?;
            Ok(active)
        })
        .await?;
    Ok(Json(json!({ "needs_rotation": needs })))
}
