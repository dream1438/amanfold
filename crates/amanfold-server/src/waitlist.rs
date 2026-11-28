//! Beta waitlist for the website: double opt-in, nothing else.
//!
//! Someone enters an email on amanfold.com; we mail them a link. Only after
//! they open it is the address kept as a confirmed sign-up. The link carries
//! a random token; the server stores its SHA-256, never the token. The same
//! link (and the one in every later waitlist email) lets them leave, which
//! deletes the row.
//!
//! Stored per entry: the address, the chosen language and plan, whether it
//! is confirmed, the token hash and the day it was created. No IP address,
//! no browser details, no tracking. Unconfirmed entries are deleted after
//! seven days.
//!
//! Every request gets the same answer whether or not the address is already
//! on the list, so the endpoint cannot be used to find out who signed up.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    AppState, ClientIp,
    error::{ApiError, ApiResult},
    mail::Mail,
    normalize_email, now_secs, random32,
    wire::{b64, unb64},
};

type St = State<Arc<AppState>>;

/// Unconfirmed sign-ups are forgotten after this long (seconds).
pub const PENDING_TTL: u64 = 7 * 86_400;

/// Languages the site and the emails are written in.
pub const LANGS: [&str; 2] = ["en", "ar"];

/// Plans someone can say they are interested in.
pub const PLANS: [&str; 4] = ["free", "premium", "family", "teams"];

fn token_hash(token: &[u8]) -> Vec<u8> {
    Sha256::new()
        .chain_update(b"amanfold/v1/waitlist-token")
        .chain_update(token)
        .finalize()
        .to_vec()
}

fn pick<'a>(v: Option<&str>, allowed: &[&'a str], default: &'a str) -> &'a str {
    v.and_then(|v| allowed.iter().find(|a| **a == v).copied())
        .unwrap_or(default)
}

/// `POST /v1/waitlist` body.
#[derive(Deserialize)]
pub struct JoinReq {
    email: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    plan: Option<String>,
}

/// `POST /v1/waitlist {email, lang?, plan?}` → `202 {}` always (after a
/// valid address). Mails a confirmation link unless the address is already
/// confirmed.
pub async fn join(
    st: St,
    ip: ClientIp,
    Json(req): Json<JoinReq>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let email = normalize_email(&req.email)?;
    let lang = pick(req.lang.as_deref(), &LANGS, "en");
    let plan = pick(req.plan.as_deref(), &PLANS, "free");
    st.limiter
        .hit(&format!("mail-ip:{}", ip.0), st.cfg.limits.mail_ip)?;
    st.limiter
        .hit(&format!("waitlist-email:{email}"), st.cfg.limits.mail_email)?;

    let token = random32();
    let hash = token_hash(&token);
    let to = email.clone();
    let send = st
        .db(move |c| {
            let now = now_secs() as i64;
            c.execute(
                "DELETE FROM waitlist WHERE confirmed = 0 AND created_at < ?1",
                params![now - PENDING_TTL as i64],
            )?;
            let confirmed: Option<i64> = c
                .query_row(
                    "SELECT confirmed FROM waitlist WHERE email = ?1",
                    params![email],
                    |r| r.get(0),
                )
                .optional()?;
            match confirmed {
                // Already on the list: say nothing new, change nothing.
                Some(1) => Ok(false),
                Some(_) => {
                    c.execute(
                        "UPDATE waitlist SET lang = ?2, plan = ?3, token_hash = ?4, created_at = ?5
                         WHERE email = ?1",
                        params![email, lang, plan, hash, now],
                    )?;
                    Ok(true)
                }
                None => {
                    c.execute(
                        "INSERT INTO waitlist (email, lang, plan, confirmed, token_hash, created_at)
                         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
                        params![email, lang, plan, hash, now],
                    )?;
                    Ok(true)
                }
            }
        })
        .await?;
    if send {
        let site = st.cfg.site_url.trim_end_matches('/');
        let prefix = if lang == "en" {
            String::new()
        } else {
            format!("/{lang}")
        };
        let t = b64(&token);
        st.mailer.send(
            &to,
            Mail::WaitlistConfirm {
                confirm: format!("{site}{prefix}/beta/confirm/#t={t}"),
                leave: format!("{site}{prefix}/beta/leave/#t={t}"),
                arabic: lang == "ar",
            },
        );
    }
    Ok((StatusCode::ACCEPTED, Json(json!({}))))
}

/// Body of confirm and leave.
#[derive(Deserialize)]
pub struct TokenReq {
    token: String,
}

fn parse_token(st: &AppState, ip: &ClientIp, t: &str) -> ApiResult<Vec<u8>> {
    st.limiter
        .hit(&format!("verify-ip:{}", ip.0), st.cfg.limits.verify_ip)?;
    let raw = unb64(t.trim())
        .filter(|b| b.len() == 32)
        .ok_or(ApiError::VerificationFailed)?;
    Ok(token_hash(&raw))
}

/// `POST /v1/waitlist/confirm {token}` → `{}`; `verification_failed` if the
/// link is unknown or expired. Confirming twice is fine.
pub async fn confirm(st: St, ip: ClientIp, Json(req): Json<TokenReq>) -> ApiResult<Json<Value>> {
    let hash = parse_token(&st, &ip, &req.token)?;
    let n = st
        .db(move |c| {
            let oldest = now_secs() as i64 - PENDING_TTL as i64;
            Ok(c.execute(
                "UPDATE waitlist SET confirmed = 1
                 WHERE token_hash = ?1 AND (confirmed = 1 OR created_at >= ?2)",
                params![hash, oldest],
            )?)
        })
        .await?;
    if n == 0 {
        return Err(ApiError::VerificationFailed);
    }
    Ok(Json(json!({})))
}

/// `POST /v1/waitlist/leave {token}` → `{}`. Deletes the entry. Leaving
/// with an unknown or already used link also answers `{}`.
pub async fn leave(st: St, ip: ClientIp, Json(req): Json<TokenReq>) -> ApiResult<Json<Value>> {
    let hash = parse_token(&st, &ip, &req.token)?;
    st.db(move |c| {
        c.execute("DELETE FROM waitlist WHERE token_hash = ?1", params![hash])?;
        Ok(())
    })
    .await?;
    Ok(Json(json!({})))
}

/// Confirmed sign-ups, for the operator's export (`amanfold-server waitlist`).
/// Each row is `(email, lang, plan, created_at)`.
pub fn export(c: &rusqlite::Connection) -> rusqlite::Result<Vec<(String, String, String, i64)>> {
    let mut q = c.prepare(
        "SELECT email, lang, plan, created_at FROM waitlist WHERE confirmed = 1 ORDER BY created_at",
    )?;
    let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_choices_fall_back() {
        assert_eq!(pick(Some("ar"), &LANGS, "en"), "ar");
        assert_eq!(pick(Some("<script>"), &LANGS, "en"), "en");
        assert_eq!(pick(None, &PLANS, "free"), "free");
        assert_eq!(pick(Some("family"), &PLANS, "free"), "family");
    }
}
