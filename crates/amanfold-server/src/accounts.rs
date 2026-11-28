//! Sign-up, OPAQUE login, sessions, password change, recovery and deletion.

use std::sync::Arc;

use amanfold_core::{
    FORMAT_VERSION,
    account::AccountBundle,
    auth,
    keys::PublicKeys,
    sync::{from_cbor, to_cbor},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    AppState, Authed, ClientIp, PendingChallenge, PendingLogin,
    error::{ApiError, ApiResult},
    mail::Mail,
    normalize_email, now_secs, random_uuid, random32, store, token_hash,
    wire::B64,
};

type St = State<Arc<AppState>>;

const CODE_TTL: u64 = 15 * 60;
const MAX_CODE_ATTEMPTS: i64 = 5;
const LOGIN_TTL: u64 = 120;
const CHALLENGE_TTL: u64 = 300;
const MAX_PENDING: usize = 50_000;

/// Challenge purposes. A signature for one cannot be used for another.
pub const PURPOSE_RECOVER: &str = "recover";
const PURPOSES: [&str; 3] = ["change-password", "rotate-recovery-code", "delete-account"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn code_hash(email: &str, purpose: &str, code: &str) -> Vec<u8> {
    Sha256::new()
        .chain_update(b"amanfold/v1/email-code\0")
        .chain_update(purpose.as_bytes())
        .chain_update(b"\0")
        .chain_update(email.as_bytes())
        .chain_update(b"\0")
        .chain_update(code.trim().as_bytes())
        .finalize()
        .to_vec()
}

fn new_code() -> String {
    let b = random32();
    let n = u64::from_le_bytes(b[..8].try_into().expect("8 bytes")) % 100_000_000;
    format!("{n:08}")
}

fn store_code(c: &Connection, email: &str, purpose: &str) -> ApiResult<String> {
    let code = new_code();
    c.execute(
        "INSERT INTO email_codes (email, purpose, code_hash, expires_at, attempts)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT (email, purpose) DO UPDATE SET code_hash = ?3, expires_at = ?4, attempts = 0",
        params![
            email,
            purpose,
            code_hash(email, purpose, &code),
            (now_secs() + CODE_TTL) as i64
        ],
    )?;
    Ok(code)
}

/// Check an emailed code. Each wrong guess counts; after five the code is dead.
fn check_code(
    c: &Connection,
    required: bool,
    email: &str,
    purpose: &str,
    code: Option<&str>,
    consume: bool,
) -> ApiResult<()> {
    if !required {
        return Ok(());
    }
    let code = code.ok_or(ApiError::VerificationFailed)?;
    let row: Option<(Vec<u8>, i64, i64)> = c
        .query_row(
            "SELECT code_hash, expires_at, attempts FROM email_codes WHERE email = ?1 AND purpose = ?2",
            params![email, purpose],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((hash, expires, attempts)) = row else {
        return Err(ApiError::VerificationFailed);
    };
    if attempts >= MAX_CODE_ATTEMPTS || now_secs() >= expires as u64 {
        c.execute(
            "DELETE FROM email_codes WHERE email = ?1 AND purpose = ?2",
            params![email, purpose],
        )?;
        return Err(ApiError::VerificationFailed);
    }
    if !bool::from(hash.ct_eq(&code_hash(email, purpose, code))) {
        c.execute(
            "UPDATE email_codes SET attempts = attempts + 1 WHERE email = ?1 AND purpose = ?2",
            params![email, purpose],
        )?;
        return Err(ApiError::VerificationFailed);
    }
    if consume {
        c.execute(
            "DELETE FROM email_codes WHERE email = ?1 AND purpose = ?2",
            params![email, purpose],
        )?;
    }
    Ok(())
}

/// Parse and sanity-check an uploaded account bundle; returns it and its
/// canonical encoding.
fn parse_bundle(b: &[u8]) -> ApiResult<(AccountBundle, Vec<u8>)> {
    let bundle: AccountBundle = from_cbor(b)?;
    if bundle.format != FORMAT_VERSION {
        return Err(ApiError::BadRequest("unsupported bundle format"));
    }
    bundle.kdf.validate()?;
    let canon = to_cbor(&bundle).to_vec();
    Ok((bundle, canon))
}

/// A replacement bundle must belong to the same account and keep its keys.
fn check_replacement(old: &AccountBundle, new: &AccountBundle) -> ApiResult<()> {
    if new.account_id != old.account_id || new.public != old.public {
        return Err(ApiError::BadRequest("bundle belongs to another account"));
    }
    Ok(())
}

fn device_name(s: Option<String>) -> String {
    let s: String = s
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if s.trim().is_empty() {
        "Unnamed device".into()
    } else {
        s
    }
}

fn insert_session(c: &Connection, account: &Uuid, token: &[u8], device: &str) -> ApiResult<Uuid> {
    let id = random_uuid();
    let now = now_secs() as i64;
    c.execute(
        "INSERT INTO sessions (id, account_id, token_hash, device_name, created_at, last_used)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            id.as_bytes(),
            account.as_bytes(),
            token_hash(token),
            device,
            now
        ],
    )?;
    Ok(id)
}

fn load_bundle(c: &Connection, account: &Uuid) -> ApiResult<AccountBundle> {
    let b: Vec<u8> = c
        .query_row(
            "SELECT bundle FROM accounts WHERE id = ?1",
            params![account.as_bytes()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(ApiError::Unauthorized)?;
    Ok(from_cbor(&b)?)
}

/// Account id, bundle and OPAQUE record.
type AccountRow = (Uuid, Vec<u8>, Vec<u8>);

fn account_by_email(c: &Connection, email: &str) -> ApiResult<Option<AccountRow>> {
    Ok(c.query_row(
        "SELECT id, bundle, opaque_record FROM accounts WHERE email = ?1",
        params![email],
        |r| Ok((store::uuid_col(r.get(0)?), r.get(1)?, r.get(2)?)),
    )
    .optional()?)
}

/// Outstanding challenges one account may hold.
const MAX_CHALLENGES_PER_ACCOUNT: usize = 5;

fn new_challenge(st: &AppState, account_id: Uuid, purpose: &'static str) -> ApiResult<[u8; 32]> {
    let ch = random32();
    let now = now_secs();
    let mut m = st.challenges.lock().unwrap_or_else(|e| e.into_inner());
    // A few outstanding challenges per account, the oldest replaced: one
    // account must not be able to fill the shared map and block everyone.
    let mine: Vec<([u8; 32], u64)> = m
        .iter()
        .filter(|(_, p)| p.account_id == account_id)
        .map(|(k, p)| (*k, p.expires))
        .collect();
    if mine.len() >= MAX_CHALLENGES_PER_ACCOUNT {
        if let Some((oldest, _)) = mine.iter().min_by_key(|(_, exp)| *exp) {
            m.remove(oldest);
        }
    }
    if m.len() >= MAX_PENDING {
        m.retain(|_, p| p.expires > now);
        if m.len() >= MAX_PENDING {
            return Err(ApiError::RateLimited);
        }
    }
    m.insert(
        ch,
        PendingChallenge {
            account_id,
            purpose,
            expires: now + CHALLENGE_TTL,
        },
    );
    Ok(ch)
}

/// Use up a challenge and check the signature over it with the account's
/// current public keys.
fn redeem_challenge(
    st: &AppState,
    account_id: &Uuid,
    public: &PublicKeys,
    purpose: &str,
    challenge: &[u8],
    signature: &[u8],
) -> ApiResult<()> {
    let key: [u8; 32] = challenge
        .try_into()
        .map_err(|_| ApiError::VerificationFailed)?;
    let p = st
        .challenges
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key)
        .ok_or(ApiError::VerificationFailed)?;
    if p.account_id != *account_id || p.purpose != purpose || now_secs() >= p.expires {
        return Err(ApiError::VerificationFailed);
    }
    auth::verify_challenge(public, account_id, purpose, challenge, signature)
        .map_err(|_| ApiError::VerificationFailed)
}

fn accepted() -> (StatusCode, Json<Value>) {
    (StatusCode::ACCEPTED, Json(json!({})))
}

// ---------------------------------------------------------------------------
// Server info
// ---------------------------------------------------------------------------

/// `GET /v1/server`: what apps need to talk to this server. Apps ship the
/// production OPAQUE public key pinned; this is for checking and for
/// self-hosted servers (trust on first use).
pub async fn server_info(State(st): St) -> Json<Value> {
    Json(json!({
        "server_identity": String::from_utf8_lossy(auth::SERVER_IDENTITY),
        "opaque_public_key": B64(st.secrets.opaque.public_key()),
        "format": FORMAT_VERSION,
        "version": env!("CARGO_PKG_VERSION"),
        "email_verification": st.cfg.require_email_verification,
    }))
}

// ---------------------------------------------------------------------------
// Sign-up
// ---------------------------------------------------------------------------

/// Body with just an email.
#[derive(Deserialize)]
pub struct EmailReq {
    email: String,
}

/// `POST /v1/signup/email`: mail a sign-up code. Same answer whether or not
/// the address already has an account (that owner gets a notice instead).
pub async fn signup_email(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<EmailReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("mail-ip:{}", ip.0), st.cfg.limits.mail_ip)?;
    st.limiter
        .hit(&format!("mail-email:{email}"), st.cfg.limits.mail_email)?;
    if !st.cfg.require_email_verification {
        return Ok(accepted());
    }
    let e = email.clone();
    let mail = st
        .db(move |c| {
            if account_by_email(c, &e)?.is_some() {
                Ok(Mail::AlreadyRegistered)
            } else {
                Ok(Mail::SignupCode {
                    code: store_code(c, &e, "signup")?,
                })
            }
        })
        .await?;
    st.mailer.send(&email, mail);
    Ok(accepted())
}

/// `POST /v1/signup/start` body.
#[derive(Deserialize)]
pub struct SignupStartReq {
    email: String,
    code: Option<String>,
    account_id: Uuid,
    /// OPAQUE registration request.
    request: B64,
}

/// `POST /v1/signup/start`: OPAQUE registration, server step 1.
pub async fn signup_start(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<SignupStartReq>,
) -> ApiResult<Json<Value>> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("verify-ip:{}", ip.0), st.cfg.limits.verify_ip)?;
    let required = st.cfg.require_email_verification;
    let (e, code, id) = (email.clone(), req.code.clone(), req.account_id);
    st.db(move |c| {
        check_code(c, required, &e, "signup", code.as_deref(), false)?;
        let taken: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE email = ?1 OR id = ?2)",
            params![e, id.as_bytes()],
            |r| r.get(0),
        )?;
        if taken {
            return Err(ApiError::Exists("account"));
        }
        Ok(())
    })
    .await?;
    let resp =
        auth::server_registration_start(&st.secrets.opaque, &req.request.0, &req.account_id)?;
    Ok(Json(json!({ "response": B64(resp) })))
}

/// `POST /v1/signup/finish` body.
#[derive(Deserialize)]
pub struct SignupFinishReq {
    email: String,
    code: Option<String>,
    /// OPAQUE registration upload.
    upload: B64,
    /// The new account bundle.
    bundle: B64,
    device_name: Option<String>,
}

/// Session handed out at sign-up (no second Argon2id run needed).
#[derive(Serialize)]
pub struct NewSession {
    account_id: Uuid,
    session_id: Uuid,
    /// Bearer token (base64url).
    api_token: B64,
}

/// `POST /v1/signup/finish`: store the OPAQUE record and the bundle. Returns
/// a session for the device that signed up.
pub async fn signup_finish(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<SignupFinishReq>,
) -> ApiResult<Json<NewSession>> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("verify-ip:{}", ip.0), st.cfg.limits.verify_ip)?;
    let (bundle, canon) = parse_bundle(&req.bundle.0)?;
    let record = auth::server_registration_finish(&req.upload.0)?;
    let required = st.cfg.require_email_verification;
    let device = device_name(req.device_name);
    let token = random32();
    let id = bundle.account_id;
    let public = to_cbor(&bundle.public).to_vec();
    let session_id = st
        .db(move |c| {
            // Check (and count a wrong guess) outside the transaction: an
            // error inside it would roll the attempt counter back, leaving
            // the code open to unlimited guessing.
            check_code(c, required, &email, "signup", req.code.as_deref(), false)?;
            let tx = c.transaction()?;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO accounts (id, email, bundle, public_keys, opaque_record, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id.as_bytes(), email, canon, public, record, now_secs() as i64],
            )?;
            if inserted == 0 {
                return Err(ApiError::Exists("account"));
            }
            check_code(&tx, required, &email, "signup", req.code.as_deref(), true)?;
            let sid = insert_session(&tx, &id, &token, &device)?;
            tx.commit()?;
            Ok(sid)
        })
        .await?;
    tracing::info!(account = %id, "account created");
    Ok(Json(NewSession {
        account_id: id,
        session_id,
        api_token: B64(token.to_vec()),
    }))
}

// ---------------------------------------------------------------------------
// Login
// ---------------------------------------------------------------------------

/// `POST /v1/prelogin`: KDF parameters and salt for an email. Unknown
/// addresses get stable fake parameters.
pub async fn prelogin(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<EmailReq>,
) -> ApiResult<Json<Value>> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("prelogin-ip:{}", ip.0), st.cfg.limits.prelogin_ip)?;
    let e = email.clone();
    let found = st.db(move |c| account_by_email(c, &e)).await?;
    let pre = match found {
        Some((_, b, _)) => from_cbor::<AccountBundle>(&b)?.prelogin(),
        None => auth::fake_prelogin(&st.secrets.prelogin, &email),
    };
    Ok(Json(json!({ "prelogin": B64(to_cbor(&pre).to_vec()) })))
}

/// `POST /v1/login/start` body.
#[derive(Deserialize)]
pub struct LoginStartReq {
    email: String,
    /// OPAQUE credential request.
    request: B64,
}

/// `POST /v1/login/start`: OPAQUE login, server step 1. Unknown emails get
/// OPAQUE's fake response, indistinguishable from a real one.
pub async fn login_start(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<LoginStartReq>,
) -> ApiResult<Json<Value>> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("login-ip:{}", ip.0), st.cfg.limits.login_ip)?;
    st.limiter
        .hit(&format!("login-email:{email}"), st.cfg.limits.login_email)?;
    let e = email.clone();
    let found = st.db(move |c| account_by_email(c, &e)).await?;
    let (account_id, credential_id, record) = match found {
        Some((id, _, rec)) => (Some(id), id, Some(rec)),
        None => (
            None,
            auth::fake_prelogin(&st.secrets.prelogin, &email).account_id,
            None,
        ),
    };
    let (state, resp) = auth::server_login_start(
        &st.secrets.opaque,
        record.as_deref(),
        &req.request.0,
        &credential_id,
    )?;
    let login_id = random_uuid();
    let now = now_secs();
    {
        let mut m = st.logins.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= MAX_PENDING {
            m.retain(|_, p| p.expires > now);
            if m.len() >= MAX_PENDING {
                return Err(ApiError::RateLimited);
            }
        }
        m.insert(
            login_id,
            PendingLogin {
                account_id,
                credential_id,
                state,
                expires: now + LOGIN_TTL,
            },
        );
    }
    Ok(Json(json!({ "login_id": login_id, "response": B64(resp) })))
}

/// `POST /v1/login/finish` body.
#[derive(Deserialize)]
pub struct LoginFinishReq {
    login_id: Uuid,
    /// OPAQUE credential finalization.
    finalization: B64,
    device_name: Option<String>,
}

/// `POST /v1/login/finish`: OPAQUE login, server step 2. On success the
/// session token is the one both sides derive from the OPAQUE session key
/// (`auth::api_token`); it is not sent. The account bundle comes back so the
/// device can unlock.
pub async fn login_finish(
    State(st): St,
    Json(req): Json<LoginFinishReq>,
) -> ApiResult<Json<Value>> {
    let pending = st
        .logins
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&req.login_id)
        .ok_or(ApiError::VerificationFailed)?;
    if now_secs() >= pending.expires {
        return Err(ApiError::VerificationFailed);
    }
    let key = auth::server_login_finish(pending.state, &req.finalization.0, &pending.credential_id)
        .map_err(|_| ApiError::VerificationFailed)?;
    let account_id = pending.account_id.ok_or(ApiError::VerificationFailed)?;
    let token = auth::api_token(&key);
    let token = token.expose().to_vec();
    let device = device_name(req.device_name);
    let (session_id, bundle) = st
        .db(move |c| {
            let sid = insert_session(c, &account_id, &token, &device)?;
            let b: Vec<u8> = c.query_row(
                "SELECT bundle FROM accounts WHERE id = ?1",
                params![account_id.as_bytes()],
                |r| r.get(0),
            )?;
            Ok((sid, b))
        })
        .await?;
    Ok(Json(json!({
        "account_id": account_id,
        "session_id": session_id,
        "bundle": B64(bundle),
    })))
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// `GET /v1/sessions`: this account's signed-in devices.
pub async fn list_sessions(State(st): St, a: Authed) -> ApiResult<Json<Value>> {
    let rows = st
        .db(move |c| {
            let mut s = c.prepare(
                "SELECT id, device_name, created_at, last_used FROM sessions WHERE account_id = ?1 ORDER BY created_at",
            )?;
            let rows = s
                .query_map(params![a.account_id.as_bytes()], |r| {
                    let id = store::uuid_col(r.get(0)?);
                    Ok(json!({
                        "session_id": id,
                        "device_name": r.get::<_, String>(1)?,
                        "created_at": r.get::<_, i64>(2)?,
                        "last_used": r.get::<_, i64>(3)?,
                        "current": id == a.session_id,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(Json(json!({ "sessions": rows })))
}

/// `DELETE /v1/sessions/current`: sign out this device.
pub async fn logout(State(st): St, a: Authed) -> ApiResult<StatusCode> {
    st.db(move |c| {
        c.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![a.session_id.as_bytes()],
        )?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/sessions/{id}`: sign out another device of this account.
pub async fn revoke_session(
    State(st): St,
    a: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let n = st
        .db(move |c| {
            Ok(c.execute(
                "DELETE FROM sessions WHERE id = ?1 AND account_id = ?2",
                params![id.as_bytes(), a.account_id.as_bytes()],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

/// `GET /v1/account`: the account bundle and email.
pub async fn get_account(State(st): St, a: Authed) -> ApiResult<Json<Value>> {
    let (email, bundle) = st
        .db(move |c| {
            Ok(c.query_row(
                "SELECT email, bundle FROM accounts WHERE id = ?1",
                params![a.account_id.as_bytes()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )?)
        })
        .await?;
    Ok(Json(json!({
        "account_id": a.account_id,
        "email": email,
        "bundle": B64(bundle),
    })))
}

/// `POST /v1/account/challenge` body.
#[derive(Deserialize)]
pub struct ChallengeReq {
    purpose: String,
}

/// `POST /v1/account/challenge`: a one-time challenge for an operation that
/// must be proven with the account's private keys (a stolen session token
/// alone cannot change the password, rotate the recovery code or delete the
/// account).
pub async fn challenge(
    State(st): St,
    a: Authed,
    Json(req): Json<ChallengeReq>,
) -> ApiResult<Json<Value>> {
    let purpose = PURPOSES
        .iter()
        .find(|p| **p == req.purpose)
        .copied()
        .ok_or(ApiError::BadRequest("unknown purpose"))?;
    let ch = new_challenge(&st, a.account_id, purpose)?;
    Ok(Json(json!({ "challenge": B64(ch.to_vec()) })))
}

/// Body with an OPAQUE registration request.
#[derive(Deserialize)]
pub struct RegisterReq {
    request: B64,
}

/// `POST /v1/account/password/start`: OPAQUE re-registration, server step 1.
pub async fn password_start(
    State(st): St,
    a: Authed,
    Json(req): Json<RegisterReq>,
) -> ApiResult<Json<Value>> {
    let resp = auth::server_registration_start(&st.secrets.opaque, &req.request.0, &a.account_id)?;
    Ok(Json(json!({ "response": B64(resp) })))
}

/// `POST /v1/account/password/finish` body.
#[derive(Deserialize)]
pub struct PasswordFinishReq {
    upload: B64,
    bundle: B64,
    challenge: B64,
    signature: B64,
}

/// `POST /v1/account/password/finish`: swap in the new OPAQUE record and
/// bundle together. Signs out every other device.
pub async fn password_finish(
    State(st): St,
    a: Authed,
    Json(req): Json<PasswordFinishReq>,
) -> ApiResult<StatusCode> {
    let (new, canon) = parse_bundle(&req.bundle.0)?;
    let record = auth::server_registration_finish(&req.upload.0)?;
    let id = a.account_id;
    let old = st.db(move |c| load_bundle(c, &id)).await?;
    check_replacement(&old, &new)?;
    redeem_challenge(
        &st,
        &id,
        &old.public,
        "change-password",
        &req.challenge.0,
        &req.signature.0,
    )?;
    st.db(move |c| {
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE accounts SET bundle = ?1, opaque_record = ?2 WHERE id = ?3",
            params![canon, record, id.as_bytes()],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE account_id = ?1 AND id != ?2",
            params![id.as_bytes(), a.session_id.as_bytes()],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    tracing::info!(account = %id, "password changed");
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /v1/account/recovery-code` body.
#[derive(Deserialize)]
pub struct RecoveryRotateReq {
    bundle: B64,
    challenge: B64,
    signature: B64,
}

/// `PUT /v1/account/recovery-code`: new bundle after a recovery-code
/// rotation. Only the recovery copy of the keys may change.
pub async fn rotate_recovery_code(
    State(st): St,
    a: Authed,
    Json(req): Json<RecoveryRotateReq>,
) -> ApiResult<StatusCode> {
    let (new, canon) = parse_bundle(&req.bundle.0)?;
    let id = a.account_id;
    let old = st.db(move |c| load_bundle(c, &id)).await?;
    check_replacement(&old, &new)?;
    if new.kdf != old.kdf || new.salt != old.salt || new.keys_by_password != old.keys_by_password {
        return Err(ApiError::BadRequest("only the recovery copy may change"));
    }
    redeem_challenge(
        &st,
        &id,
        &old.public,
        "rotate-recovery-code",
        &req.challenge.0,
        &req.signature.0,
    )?;
    st.db(move |c| {
        c.execute(
            "UPDATE accounts SET bundle = ?1 WHERE id = ?2",
            params![canon, id.as_bytes()],
        )?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Body with a signed challenge.
#[derive(Deserialize)]
pub struct ProofReq {
    challenge: B64,
    signature: B64,
}

/// `POST /v1/account/delete`: delete the account, its sessions, its
/// memberships and every vault it owns. Cannot be undone.
pub async fn delete_account(
    State(st): St,
    a: Authed,
    Json(req): Json<ProofReq>,
) -> ApiResult<StatusCode> {
    let id = a.account_id;
    let old = st.db(move |c| load_bundle(c, &id)).await?;
    redeem_challenge(
        &st,
        &id,
        &old.public,
        "delete-account",
        &req.challenge.0,
        &req.signature.0,
    )?;
    st.db(move |c| {
        let tx = c.transaction()?;
        let email: String = tx.query_row(
            "SELECT email FROM accounts WHERE id = ?1",
            params![id.as_bytes()],
            |r| r.get(0),
        )?;
        // Leaving shared vaults owned by others means their key must rotate.
        tx.execute(
            "UPDATE vaults SET needs_rotation = 1 WHERE owner_id != ?1 AND id IN
             (SELECT vault_id FROM members WHERE account_id = ?1)",
            params![id.as_bytes()],
        )?;
        tx.execute("DELETE FROM email_codes WHERE email = ?1", params![email])?;
        tx.execute("DELETE FROM invites WHERE email = ?1", params![email])?;
        // Cascades: sessions, owned vaults (records, members, grants, invites),
        // memberships and grants elsewhere.
        tx.execute("DELETE FROM accounts WHERE id = ?1", params![id.as_bytes()])?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    tracing::info!(account = %id, "account deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Recovery: forgotten master password
// ---------------------------------------------------------------------------
//
// Needs both the emailed code and the recovery kit. The server hands out
// the bundle (safe: its private keys are wrapped under the password plus
// Secret Key, or the 256-bit recovery code) and a challenge; the app
// unlocks the bundle with the 24 words, signs the challenge with the
// account key, and registers a new password.

/// `POST /v1/recovery/email`: mail a recovery code (only if the address has
/// an account; the answer is the same either way).
pub async fn recovery_email(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<EmailReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let email = normalize_email(&req.email)?;
    st.limiter
        .hit(&format!("mail-ip:{}", ip.0), st.cfg.limits.mail_ip)?;
    st.limiter
        .hit(&format!("mail-email:{email}"), st.cfg.limits.mail_email)?;
    if !st.cfg.require_email_verification {
        return Ok(accepted());
    }
    let e = email.clone();
    let code = st
        .db(move |c| match account_by_email(c, &e)? {
            Some(_) => Ok(Some(store_code(c, &e, PURPOSE_RECOVER)?)),
            None => Ok(None),
        })
        .await?;
    if let Some(code) = code {
        st.mailer.send(&email, Mail::RecoveryCode { code });
    }
    Ok(accepted())
}

/// Body with an email and its code.
#[derive(Deserialize)]
pub struct CodeReq {
    email: String,
    code: Option<String>,
}

async fn recovery_account(
    st: &AppState,
    ip: &ClientIp,
    email: &str,
    code: Option<String>,
    consume: bool,
) -> ApiResult<(Uuid, Vec<u8>)> {
    st.limiter
        .hit(&format!("verify-ip:{}", ip.0), st.cfg.limits.verify_ip)?;
    let required = st.cfg.require_email_verification;
    let e = email.to_string();
    st.db(move |c| {
        check_code(c, required, &e, PURPOSE_RECOVER, code.as_deref(), consume)?;
        let (id, bundle, _) = account_by_email(c, &e)?.ok_or(ApiError::VerificationFailed)?;
        Ok((id, bundle))
    })
    .await
}

/// `POST /v1/recovery/start`: the bundle and a challenge for the recovery key.
pub async fn recovery_start(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<CodeReq>,
) -> ApiResult<Json<Value>> {
    let email = normalize_email(&req.email)?;
    let (id, bundle) = recovery_account(&st, &ip, &email, req.code, false).await?;
    let ch = new_challenge(&st, id, PURPOSE_RECOVER)?;
    Ok(Json(json!({
        "account_id": id,
        "bundle": B64(bundle),
        "challenge": B64(ch.to_vec()),
    })))
}

/// `POST /v1/recovery/register` body.
#[derive(Deserialize)]
pub struct RecoveryRegisterReq {
    email: String,
    code: Option<String>,
    request: B64,
}

/// `POST /v1/recovery/register`: OPAQUE registration step 1 for the new password.
pub async fn recovery_register(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<RecoveryRegisterReq>,
) -> ApiResult<Json<Value>> {
    let email = normalize_email(&req.email)?;
    let (id, _) = recovery_account(&st, &ip, &email, req.code, false).await?;
    let resp = auth::server_registration_start(&st.secrets.opaque, &req.request.0, &id)?;
    Ok(Json(json!({ "response": B64(resp) })))
}

/// `POST /v1/recovery/finish` body.
#[derive(Deserialize)]
pub struct RecoveryFinishReq {
    email: String,
    code: Option<String>,
    upload: B64,
    bundle: B64,
    challenge: B64,
    signature: B64,
}

/// `POST /v1/recovery/finish`: with proof of the recovery key, store the new
/// password's record and bundle, and sign out every device.
pub async fn recovery_finish(
    State(st): St,
    ip: ClientIp,
    Json(req): Json<RecoveryFinishReq>,
) -> ApiResult<StatusCode> {
    let email = normalize_email(&req.email)?;
    let (new, canon) = parse_bundle(&req.bundle.0)?;
    let record = auth::server_registration_finish(&req.upload.0)?;
    let (id, old) = recovery_account(&st, &ip, &email, req.code.clone(), false).await?;
    let old: AccountBundle = from_cbor(&old)?;
    check_replacement(&old, &new)?;
    redeem_challenge(
        &st,
        &id,
        &old.public,
        PURPOSE_RECOVER,
        &req.challenge.0,
        &req.signature.0,
    )?;
    let required = st.cfg.require_email_verification;
    st.db(move |c| {
        let tx = c.transaction()?;
        check_code(
            &tx,
            required,
            &email,
            PURPOSE_RECOVER,
            req.code.as_deref(),
            true,
        )?;
        tx.execute(
            "UPDATE accounts SET bundle = ?1, opaque_record = ?2 WHERE id = ?3",
            params![canon, record, id.as_bytes()],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE account_id = ?1",
            params![id.as_bytes()],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    tracing::info!(account = %id, "account recovered");
    Ok(StatusCode::NO_CONTENT)
}
