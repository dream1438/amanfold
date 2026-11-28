//! # Amanfold sync server
//!
//! Stores what the security core produces and nothing more: account bundles
//! (public keys and wrapped private keys), OPAQUE records, signed vault
//! grants, encrypted item records and signed manifests. It never sees a
//! master password, a Secret Key, a recovery code, a vault key or an item.
//!
//! What the server does enforce:
//!
//! * login with OPAQUE, sessions, rate limits, email verification;
//! * who may read and write each vault (owner, manager, writer, reader);
//! * that uploads are well formed, belong where they are filed, match the
//!   signed manifest, and that manifests advance one step at a time
//!   (compare-and-swap), so concurrent writers serialise;
//! * that a key rotation replaces every record and re-grants exactly the
//!   remaining members.
//!
//! Clients still verify everything themselves (see `docs/SECURITY.md`); the
//! server's checks keep honest clients consistent and stop a client bug from
//! corrupting a shared vault, but a malicious server is assumed.
//!
//! See `docs/SERVER.md` for the API.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use amanfold_core::{auth::ServerKeys, secret::SecretBytes};
use axum::{
    Router,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts},
    http::{HeaderValue, Method, header, request::Parts},
    routing::{delete, get, post, put},
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use zeroize::Zeroizing;

pub mod accounts;
pub mod error;
pub mod limits;
pub mod mail;
pub mod sharing;
pub mod store;
pub mod vaults;
pub mod waitlist;
pub mod wire;

use error::{ApiError, ApiResult};
use limits::{Limits, RateLimiter};
use mail::Mailer;

/// Server settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// Require an emailed code to sign up and to recover. Turn off only for
    /// local development or a private self-hosted server.
    pub require_email_verification: bool,
    /// Take the client address from `X-Forwarded-For` (set when behind a
    /// reverse proxy that overwrites the header).
    pub trust_proxy: bool,
    /// A session unused for this long expires (seconds).
    pub session_idle_secs: u64,
    /// A session expires this long after login regardless of use (seconds).
    pub session_max_secs: u64,
    /// Storage limit per vault (bytes of encrypted records).
    pub max_vault_bytes: u64,
    /// Largest request body (bytes). Key rotation uploads the whole vault.
    pub max_body_bytes: usize,
    /// Rate limits.
    pub limits: Limits,
    /// The public website, for links in waitlist emails.
    pub site_url: String,
    /// Web origins allowed to call the waitlist endpoints from a browser
    /// (CORS). Empty: none. The rest of the API never sends CORS headers.
    pub allowed_origins: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            require_email_verification: true,
            trust_proxy: false,
            session_idle_secs: 30 * 86_400,
            session_max_secs: 180 * 86_400,
            max_vault_bytes: 512 * 1024 * 1024,
            max_body_bytes: 64 * 1024 * 1024,
            limits: Limits::default(),
            site_url: "https://amanfold.com".into(),
            allowed_origins: Vec::new(),
        }
    }
}

/// Long-term server secrets.
pub struct Secrets {
    /// OPAQUE server keys (the public half is pinned by apps).
    pub opaque: ServerKeys,
    /// Key for fake pre-login answers to unknown emails.
    pub prelogin: Zeroizing<[u8; 32]>,
}

impl Secrets {
    /// Fresh secrets (once per deployment).
    pub fn generate() -> Self {
        Self {
            opaque: ServerKeys::generate(),
            prelogin: Zeroizing::new(*SecretBytes::<32>::random().expose()),
        }
    }

    /// Serialise for a secrets file or secret manager.
    pub fn to_json(&self) -> Zeroizing<String> {
        Zeroizing::new(
            serde_json::json!({
                "opaque": wire::b64(&self.opaque.to_bytes()),
                "prelogin": wire::b64(&self.prelogin[..]),
            })
            .to_string(),
        )
    }

    /// Load from [`Secrets::to_json`] output.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(s).map_err(|_| "secrets: not JSON")?;
        let get = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .and_then(wire::unb64)
                .map(Zeroizing::new)
                .ok_or_else(|| format!("secrets: missing {k}"))
        };
        let opaque =
            ServerKeys::from_bytes(&get("opaque")?).map_err(|_| "secrets: bad opaque keys")?;
        let p = get("prelogin")?;
        let prelogin: [u8; 32] = p
            .as_slice()
            .try_into()
            .map_err(|_| "secrets: bad prelogin key")?;
        Ok(Self {
            opaque,
            prelogin: Zeroizing::new(prelogin),
        })
    }
}

pub(crate) struct PendingLogin {
    pub account_id: Option<Uuid>,
    pub credential_id: Uuid,
    pub state: amanfold_core::auth::ServerLoginState,
    pub expires: u64,
}

pub(crate) struct PendingChallenge {
    pub account_id: Uuid,
    pub purpose: &'static str,
    pub expires: u64,
}

/// Shared server state.
pub struct AppState {
    pub(crate) cfg: Config,
    pub(crate) secrets: Secrets,
    db: Arc<Mutex<Connection>>,
    pub(crate) logins: Mutex<HashMap<Uuid, PendingLogin>>,
    pub(crate) challenges: Mutex<HashMap<[u8; 32], PendingChallenge>>,
    pub(crate) limiter: RateLimiter,
    pub(crate) mailer: Arc<dyn Mailer>,
}

impl AppState {
    /// New state over an open database.
    pub fn new(
        cfg: Config,
        secrets: Secrets,
        db: Connection,
        mailer: Arc<dyn Mailer>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            secrets,
            db: Arc::new(Mutex::new(db)),
            logins: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            limiter: RateLimiter::default(),
            mailer,
        })
    }

    /// Run `f` on the database off the async threads.
    pub(crate) async fn db<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> ApiResult<T> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let mut c = db.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut c)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("db task: {e}")))?
    }
}

/// The HTTP API.
pub fn app(state: Arc<AppState>) -> Router {
    let big = DefaultBodyLimit::max(state.cfg.max_body_bytes);
    let origins: Vec<HeaderValue> = state
        .cfg
        .allowed_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o.trim()).ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::POST])
        .allow_headers([header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(86_400));
    let waitlist = Router::new()
        .route("/v1/waitlist", post(waitlist::join))
        .route("/v1/waitlist/confirm", post(waitlist::confirm))
        .route("/v1/waitlist/leave", post(waitlist::leave))
        .layer(DefaultBodyLimit::max(4 * 1024))
        .layer(cors);
    Router::new()
        .merge(waitlist)
        .route("/v1/server", get(accounts::server_info))
        // Sign-up
        .route("/v1/signup/email", post(accounts::signup_email))
        .route("/v1/signup/start", post(accounts::signup_start))
        .route("/v1/signup/finish", post(accounts::signup_finish))
        // Login
        .route("/v1/prelogin", post(accounts::prelogin))
        .route("/v1/login/start", post(accounts::login_start))
        .route("/v1/login/finish", post(accounts::login_finish))
        // Sessions
        .route("/v1/sessions", get(accounts::list_sessions))
        .route("/v1/sessions/current", delete(accounts::logout))
        .route("/v1/sessions/{id}", delete(accounts::revoke_session))
        // Account
        .route("/v1/account", get(accounts::get_account))
        .route("/v1/account/challenge", post(accounts::challenge))
        .route("/v1/account/password/start", post(accounts::password_start))
        .route(
            "/v1/account/password/finish",
            post(accounts::password_finish),
        )
        .route(
            "/v1/account/recovery-code",
            put(accounts::rotate_recovery_code),
        )
        .route("/v1/account/delete", post(accounts::delete_account))
        // Recovery (forgotten password)
        .route("/v1/recovery/email", post(accounts::recovery_email))
        .route("/v1/recovery/start", post(accounts::recovery_start))
        .route("/v1/recovery/register", post(accounts::recovery_register))
        .route("/v1/recovery/finish", post(accounts::recovery_finish))
        // Vaults and sync
        .route("/v1/vaults", get(vaults::list).post(vaults::create))
        .route("/v1/vaults/{id}", get(vaults::get).delete(vaults::delete))
        .route("/v1/vaults/{id}/sync", get(vaults::sync))
        .route("/v1/vaults/{id}/commit", post(vaults::commit).layer(big))
        .route("/v1/vaults/{id}/rotate", post(vaults::rotate).layer(big))
        // Sharing
        .route("/v1/vaults/{id}/members", get(sharing::members))
        .route(
            "/v1/vaults/{id}/members/{account}",
            delete(sharing::remove_member),
        )
        .route(
            "/v1/vaults/{id}/members/{account}/grant",
            put(sharing::put_grant),
        )
        .route(
            "/v1/vaults/{id}/members/{account}/role",
            put(sharing::set_role),
        )
        .route(
            "/v1/vaults/{id}/invites",
            get(sharing::vault_invites).post(sharing::invite),
        )
        .route("/v1/invites", get(sharing::my_invites))
        .route("/v1/invites/{id}/accept", post(sharing::accept_invite))
        .route("/v1/invites/{id}", delete(sharing::delete_invite))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Request context
// ---------------------------------------------------------------------------

/// A request authenticated with a session token.
#[derive(Debug, Clone, Copy)]
pub struct Authed {
    /// The account.
    pub account_id: Uuid,
    /// The session.
    pub session_id: Uuid,
}

impl FromRequestParts<Arc<AppState>> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> ApiResult<Self> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(wire::unb64)
            .filter(|t| t.len() == 32)
            .ok_or(ApiError::Unauthorized)?;
        let hash = token_hash(&token);
        let cfg = state.cfg.clone();
        state
            .db(move |c| {
                let now = now_secs();
                let row: Option<(Vec<u8>, Vec<u8>, i64, i64)> = c
                    .query_row(
                        "SELECT id, account_id, created_at, last_used FROM sessions WHERE token_hash = ?1",
                        params![hash],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?;
                let (sid, aid, created, used) = row.ok_or(ApiError::Unauthorized)?;
                let (created, used) = (created as u64, used as u64);
                if now >= created + cfg.session_max_secs || now >= used + cfg.session_idle_secs {
                    c.execute("DELETE FROM sessions WHERE id = ?1", params![sid])?;
                    return Err(ApiError::Unauthorized);
                }
                if now > used + 60 {
                    c.execute(
                        "UPDATE sessions SET last_used = ?1 WHERE id = ?2",
                        params![now as i64, sid],
                    )?;
                }
                Ok(Authed {
                    account_id: store::uuid_col(aid),
                    session_id: store::uuid_col(sid),
                })
            })
            .await
    }
}

/// The client's address, for rate limits.
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl FromRequestParts<Arc<AppState>> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> ApiResult<Self> {
        if state.cfg.trust_proxy {
            if let Some(ip) = parts
                .headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next_back())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            {
                return Ok(ClientIp(rate_key(&ip)));
            }
        }
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| rate_key(&c.0.ip().to_string()))
            .unwrap_or_else(|| "unknown".into());
        Ok(ClientIp(ip))
    }
}

/// The rate-limit key for an address. One IPv6 customer usually holds a
/// whole /64 (2^64 addresses), so IPv6 clients are counted per /64; counting
/// per address would let one client get unlimited budgets.
fn rate_key(ip: &str) -> String {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
        Ok(std::net::IpAddr::V4(v4)) => v4.to_string(),
        Err(_) => ip.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn token_hash(token: &[u8]) -> Vec<u8> {
    Sha256::new()
        .chain_update(b"amanfold/v1/session-token")
        .chain_update(token)
        .finalize()
        .to_vec()
}

pub(crate) fn random32() -> [u8; 32] {
    *SecretBytes::<32>::random().expose()
}

pub(crate) fn random_uuid() -> Uuid {
    amanfold_core::secret::random_uuid()
}

/// Normalise an email address for lookups: trimmed, NFKC, lower-case.
pub fn normalize_email(raw: &str) -> ApiResult<String> {
    let e: String = raw.trim().nfkc().collect::<String>().to_lowercase();
    // A plain addr-spec only: no display names, quotes, comments or lists, so
    // the stored string is exactly the address mail is delivered to (a
    // quoted `"victim@corp.com"<me@evil.com>` would otherwise be stored as
    // one address and delivered to another).
    let local_ok = |c: char| c.is_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c);
    let domain_ok = |c: char| c.is_alphanumeric() || c == '-' || c == '.';
    let ok = e.len() <= 254
        && e.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty()
                && l.len() <= 64
                && l.chars().all(local_ok)
                && !l.starts_with('.')
                && !l.ends_with('.')
                && !l.contains("..")
                && d.chars().all(domain_ok)
                && d.contains('.')
                && !d.starts_with('.')
                && !d.ends_with('.')
                && !d.contains("..")
                && !d.starts_with('-')
        });
    if ok {
        Ok(e)
    } else {
        Err(ApiError::BadRequest("invalid email address"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_normalise() {
        assert_eq!(
            normalize_email("  Eslam@Example.COM ").unwrap(),
            "eslam@example.com"
        );
        // Full-width letters fold to ASCII.
        assert_eq!(normalize_email("ａ@example.com").unwrap(), "a@example.com");
        for bad in [
            "",
            "no-at",
            "@example.com",
            "a@b",
            "a b@example.com",
            "a@.com",
            "\"victim@corp.com\"<attacker@evil.com>",
            "a,b@example.com",
            "a@example.com,b@example.com",
            "(x)a@example.com",
            "a..b@example.com",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ipv6_clients_share_a_64() {
        assert_eq!(
            rate_key("2001:db8:1:2:aaaa::1"),
            rate_key("2001:db8:1:2:ffff::9")
        );
        assert_ne!(rate_key("2001:db8:1:2::1"), rate_key("2001:db8:1:3::1"));
        assert_eq!(rate_key("::ffff:192.0.2.1"), "192.0.2.1");
        assert_eq!(rate_key("192.0.2.1"), "192.0.2.1");
    }

    #[test]
    fn secrets_round_trip() {
        let s = Secrets::generate();
        let back = Secrets::from_json(&s.to_json()).unwrap();
        assert_eq!(s.opaque.public_key(), back.opaque.public_key());
        assert_eq!(*s.prelogin, *back.prelogin);
        assert!(Secrets::from_json("{}").is_err());
    }
}
