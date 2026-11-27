//! End to end: the app-facing API (`amanfold-ffi`, exactly what the iOS,
//! Android and browser apps call) against the real HTTP server.

use std::sync::Arc;

use amanfold_ffi::{
    Account, FieldInput, ItemKind, LocalVault, LoginSession, create_account, recover_account,
};
use amanfold_server::{
    AppState, Config, Secrets, app,
    mail::{Mail, MemoryMailer},
    store,
    wire::{b64, unb64},
};
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const PW: &str = "correct horse battery staple";

struct Srv {
    app: Router,
    mail: Arc<MemoryMailer>,
    pin: Vec<u8>,
}

impl Srv {
    fn new() -> Self {
        Self::with(Config::default())
    }

    fn with(cfg: Config) -> Self {
        let mail = Arc::new(MemoryMailer::default());
        let secrets = Secrets::generate();
        let pin = secrets.opaque.public_key();
        let state = AppState::new(cfg, secrets, store::open(":memory:").unwrap(), mail.clone());
        Self {
            app: app(state),
            mail,
            pin,
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&[u8]>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {}", b64(t)));
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, v)
    }

    async fn post(&self, path: &str, token: Option<&[u8]>, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, token, Some(body)).await
    }

    async fn get(&self, path: &str, token: &[u8]) -> (StatusCode, Value) {
        self.call(Method::GET, path, Some(token), None).await
    }
}

fn bytes(v: &Value) -> Vec<u8> {
    unb64(v.as_str().expect("base64 string")).expect("valid base64")
}

fn f(name: &str, value: &str) -> FieldInput {
    FieldInput {
        name: name.into(),
        value: value.into(),
    }
}

/// One signed-in device.
struct Device {
    email: String,
    token: Vec<u8>,
    account: Arc<Account>,
}

struct SignedUp {
    device: Device,
    secret_key: String,
    recovery_words: String,
}

async fn signup(s: &Srv, email: &str) -> SignedUp {
    let (st, _) = s
        .post("/v1/signup/email", None, json!({ "email": email }))
        .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let code = s.mail.last_code(email).expect("code mailed");
    let na = create_account(PW.into()).unwrap();
    let id = na.account.account_id();
    let (st, r) = s
        .post(
            "/v1/signup/start",
            None,
            json!({ "email": email, "code": code, "account_id": id, "request": b64(&na.registration.request().unwrap()) }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let upload = na.registration.finish(bytes(&r["response"])).unwrap();
    let (st, r) = s
        .post(
            "/v1/signup/finish",
            None,
            json!({ "email": email, "code": code, "upload": b64(&upload), "bundle": b64(&na.bundle), "device_name": "Phone" }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["account_id"], json!(id));
    SignedUp {
        device: Device {
            email: email.into(),
            token: bytes(&r["api_token"]),
            account: na.account,
        },
        secret_key: na.secret_key,
        recovery_words: na.recovery_words,
    }
}

/// Full login on a new device; `Err(status)` if the server refused.
async fn try_login(
    s: &Srv,
    email: &str,
    password: &str,
    secret_key: &str,
) -> Result<Device, StatusCode> {
    let (st, r) = s
        .post("/v1/prelogin", None, json!({ "email": email }))
        .await;
    assert_eq!(st, StatusCode::OK);
    let session =
        LoginSession::new(bytes(&r["prelogin"]), password.into(), secret_key.into()).unwrap();
    let (st, r) = s
        .post(
            "/v1/login/start",
            None,
            json!({ "email": email, "request": b64(&session.start().unwrap()) }),
        )
        .await;
    if st != StatusCode::OK {
        return Err(st);
    }
    // The client checks the server's proof (and its pinned key) first.
    let out = session
        .finish(bytes(&r["response"]), Some(s.pin.clone()))
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let (st, r2) = s
        .post(
            "/v1/login/finish",
            None,
            json!({ "login_id": r["login_id"], "finalization": b64(&out.finalization), "device_name": "Laptop" }),
        )
        .await;
    if st != StatusCode::OK {
        return Err(st);
    }
    let account = session.unlock(bytes(&r2["bundle"])).unwrap();
    Ok(Device {
        email: email.into(),
        token: out.api_token,
        account,
    })
}

async fn login(s: &Srv, email: &str, secret_key: &str) -> Device {
    try_login(s, email, PW, secret_key).await.expect("login")
}

async fn create_vault(s: &Srv, d: &Device) -> (String, Arc<LocalVault>) {
    let nv = d.account.create_vault().unwrap();
    let (st, r) = s
        .post(
            "/v1/vaults",
            Some(&d.token),
            json!({ "grant": b64(&nv.grant) }),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{r}");
    assert_eq!(r["vault_id"], json!(nv.vault.id()));
    (nv.vault.id(), nv.vault)
}

async fn commit(s: &Srv, d: &Device, v: &LocalVault, records: Vec<Vec<u8>>) -> (StatusCode, Value) {
    let m = v.sign_manifest(d.account.clone());
    let recs: Vec<String> = records.iter().map(|r| b64(r)).collect();
    let (st, r) = s
        .post(
            &format!("/v1/vaults/{}/commit", v.id()),
            Some(&d.token),
            json!({ "manifest": b64(&m), "records": recs }),
        )
        .await;
    if st == StatusCode::OK {
        v.commit_accepted().unwrap();
    }
    (st, r)
}

/// Download, verify and merge; upload anything the server lacked.
async fn sync(s: &Srv, d: &Device, v: &LocalVault, writers: &[Vec<u8>]) -> Result<u64, String> {
    let (st, r) = s
        .get(&format!("/v1/vaults/{}/sync?since=0", v.id()), &d.token)
        .await;
    if st != StatusCode::OK {
        return Err(format!("sync {st}"));
    }
    if r["manifest"].is_null() {
        return Ok(0);
    }
    let recs: Vec<Vec<u8>> = r["records"].as_array().unwrap().iter().map(bytes).collect();
    let up = v
        .sync(bytes(&r["manifest"]), recs, writers.to_vec())
        .map_err(|e| format!("{e:?}"))?;
    if !up.is_empty() {
        let (st, r) = commit(s, d, v, up.into_iter().map(|u| u.record).collect()).await;
        if st != StatusCode::OK {
            return Err(format!("commit {st} {r}"));
        }
    }
    Ok(r["seq"].as_u64().unwrap())
}

/// Open a vault on a device from the grants the server holds for it.
async fn open_vault(s: &Srv, d: &Device, vault_id: &str, granter: &[u8]) -> Arc<LocalVault> {
    let (st, r) = s.get(&format!("/v1/vaults/{vault_id}"), &d.token).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let g = r["grants"].as_array().unwrap().last().unwrap();
    d.account
        .open_vault(bytes(&g["grant"]), granter.to_vec(), None)
        .unwrap()
}

fn titles(v: &LocalVault) -> Vec<String> {
    let mut t: Vec<String> = v
        .items(false)
        .iter()
        .flat_map(|i| {
            i.fields
                .iter()
                .filter(|f| f.name == "title")
                .map(|f| f.value.clone())
        })
        .collect();
    t.sort();
    t
}

async fn sign(s: &Srv, d: &Device, purpose: &str) -> (Vec<u8>, Vec<u8>) {
    let (st, r) = s
        .post(
            "/v1/account/challenge",
            Some(&d.token),
            json!({ "purpose": purpose }),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let ch = bytes(&r["challenge"]);
    let sig = d
        .account
        .sign_challenge(purpose.into(), ch.clone())
        .unwrap();
    (ch, sig)
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_devices_sign_up_log_in_and_sync() {
    let s = Srv::new();
    let su = signup(&s, "eslam@example.com").await;
    let phone = su.device;
    let (vid, pv) = create_vault(&s, &phone).await;
    let a = pv
        .create_item(
            ItemKind::Login,
            vec![f("title", "Bank"), f("password", "hunter2")],
        )
        .unwrap();
    let (st, r) = commit(&s, &phone, &pv, vec![a.record]).await;
    assert_eq!((st, r["seq"].as_u64()), (StatusCode::OK, Some(1)));

    // The server knows the vault, but holds only ciphertext.
    let laptop = login(&s, "Eslam@Example.com ", &su.secret_key).await;
    let me = phone.account.public_keys();
    let lv = open_vault(&s, &laptop, &vid, &me).await;
    sync(&s, &laptop, &lv, std::slice::from_ref(&me))
        .await
        .unwrap();
    assert_eq!(titles(&lv), ["Bank"]);

    // Laptop adds a card; phone gets it.
    let c = lv
        .create_item(
            ItemKind::Card,
            vec![f("title", "Visa"), f("card_number", "4111111111111111")],
        )
        .unwrap();
    let (st, _) = commit(&s, &laptop, &lv, vec![c.record]).await;
    assert_eq!(st, StatusCode::OK);
    sync(&s, &phone, &pv, std::slice::from_ref(&me))
        .await
        .unwrap();
    assert_eq!(titles(&pv), ["Bank", "Visa"]);

    // Incremental sync only sends what changed.
    let (_, r) = s
        .get(&format!("/v1/vaults/{vid}/sync?since=1"), &phone.token)
        .await;
    assert_eq!(r["records"].as_array().unwrap().len(), 1);
    assert_eq!(r["seq"], json!(2));

    // Devices and sign-out.
    let (_, r) = s.get("/v1/sessions", &phone.token).await;
    assert_eq!(r["sessions"].as_array().unwrap().len(), 2);
    let (st, _) = s
        .call(
            Method::DELETE,
            "/v1/sessions/current",
            Some(&laptop.token),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = s.get("/v1/vaults", &laptop.token).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, r) = s.get("/v1/vaults", &phone.token).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(r["vaults"][0]["role"], json!("owner"));
}

#[tokio::test]
async fn concurrent_writers_conflict_then_both_edits_survive() {
    let s = Srv::new();
    let su = signup(&s, "a@example.com").await;
    let phone = su.device;
    let (vid, pv) = create_vault(&s, &phone).await;
    let me = phone.account.public_keys();
    let x = pv
        .create_item(ItemKind::SecureNote, vec![f("title", "Wifi")])
        .unwrap();
    assert_eq!(
        commit(&s, &phone, &pv, vec![x.record]).await.0,
        StatusCode::OK
    );
    let laptop = login(&s, "a@example.com", &su.secret_key).await;
    let lv = open_vault(&s, &laptop, &vid, &me).await;
    sync(&s, &laptop, &lv, std::slice::from_ref(&me))
        .await
        .unwrap();

    // Both edit offline; phone commits first.
    let p = pv
        .create_item(ItemKind::Login, vec![f("title", "From phone")])
        .unwrap();
    let l = lv
        .create_item(ItemKind::Login, vec![f("title", "From laptop")])
        .unwrap();
    assert_eq!(
        commit(&s, &phone, &pv, vec![p.record]).await.0,
        StatusCode::OK
    );
    let (st, r) = commit(&s, &laptop, &lv, vec![l.record.clone()]).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(r["seq"], json!(2));

    // Laptop syncs (which re-uploads its pending item) and wins next round.
    sync(&s, &laptop, &lv, std::slice::from_ref(&me))
        .await
        .unwrap();
    let (st, r) = commit(&s, &laptop, &lv, vec![l.record]).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    sync(&s, &phone, &pv, std::slice::from_ref(&me))
        .await
        .unwrap();
    assert_eq!(titles(&pv), ["From laptop", "From phone", "Wifi"]);
    assert_eq!(titles(&lv), titles(&pv));
}

#[tokio::test]
async fn wrong_password_unknown_email_and_forged_messages_fail_alike() {
    let s = Srv::new();
    let su = signup(&s, "b@example.com").await;
    assert_eq!(
        try_login(&s, "b@example.com", "wrong password here", &su.secret_key)
            .await
            .err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    // Unknown address: pre-login answers with stable, plausible parameters.
    let (_, p1) = s
        .post(
            "/v1/prelogin",
            None,
            json!({ "email": "nobody@example.com" }),
        )
        .await;
    let (_, p2) = s
        .post(
            "/v1/prelogin",
            None,
            json!({ "email": "nobody@example.com" }),
        )
        .await;
    assert_eq!(p1, p2);
    let (_, real) = s
        .post("/v1/prelogin", None, json!({ "email": "b@example.com" }))
        .await;
    assert_eq!(bytes(&p1["prelogin"]).len(), bytes(&real["prelogin"]).len());
    assert_eq!(
        try_login(&s, "nobody@example.com", PW, &su.secret_key)
            .await
            .err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    // Forged finalization, unknown login id, garbage token.
    let (_, r) = s.post("/v1/login/start", None, json!({ "email": "b@example.com", "request": b64(&LoginSession::new(bytes(&real["prelogin"]), PW.into(), su.secret_key.clone()).unwrap().start().unwrap()) })).await;
    let (st, _) = s
        .post(
            "/v1/login/finish",
            None,
            json!({ "login_id": r["login_id"], "finalization": b64(&[0u8; 64]) }),
        )
        .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = s
        .post(
            "/v1/login/finish",
            None,
            json!({ "login_id": r["login_id"], "finalization": b64(&[0u8; 64]) }),
        )
        .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "login state is single use");
    assert_eq!(
        s.get("/v1/account", &[7u8; 32]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.get("/v1/account", b"short").await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn signup_needs_the_emailed_code_and_hides_existing_accounts() {
    let s = Srv::new();
    signup(&s, "c@example.com").await;
    // Second sign-up for the same address: same answer, owner gets a notice.
    let (st, _) = s
        .post(
            "/v1/signup/email",
            None,
            json!({ "email": "c@example.com" }),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    assert_eq!(s.mail.sent().last().unwrap().1, Mail::AlreadyRegistered);

    let (st, _) = s
        .post(
            "/v1/signup/email",
            None,
            json!({ "email": "d@example.com" }),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let na = create_account(PW.into()).unwrap();
    let start = |code: &str| json!({ "email": "d@example.com", "code": code, "account_id": na.account.account_id(), "request": b64(&[1u8; 32]) });
    // Five wrong guesses kill the code, so even the right one fails after.
    for _ in 0..5 {
        assert_eq!(
            s.post("/v1/signup/start", None, start("00000000")).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let real = s.mail.last_code("d@example.com").unwrap();
    assert_eq!(
        s.post("/v1/signup/start", None, start(&real)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.post("/v1/signup/start", None, json!({ "email": "d@example.com", "account_id": na.account.account_id(), "request": b64(&[1u8; 32]) })).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_attempts_are_rate_limited_per_email() {
    let s = Srv::new();
    let su = signup(&s, "e@example.com").await;
    for _ in 0..10 {
        let _ = try_login(&s, "e@example.com", "not the password!", &su.secret_key).await;
    }
    assert_eq!(
        try_login(&s, "e@example.com", PW, &su.secret_key)
            .await
            .err(),
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
}

#[tokio::test]
async fn server_rejects_inconsistent_or_unauthorised_uploads() {
    let s = Srv::new();
    let owner = signup(&s, "f@example.com").await.device;
    let other = signup(&s, "g@example.com").await.device;
    let (vid, v) = create_vault(&s, &owner).await;
    let a = v
        .create_item(ItemKind::Login, vec![f("title", "A")])
        .unwrap();
    assert_eq!(
        commit(&s, &owner, &v, vec![a.record.clone()]).await.0,
        StatusCode::OK
    );

    // A non-member cannot see or write the vault (and cannot tell it exists).
    assert_eq!(
        s.get(&format!("/v1/vaults/{vid}/sync"), &other.token)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        commit(&s, &other, &v, vec![]).await.0,
        StatusCode::NOT_FOUND
    );

    // Record not listed in the manifest (the manifest is signed before the
    // edit, so it lists the old digest).
    let m = v.sign_manifest(owner.account.clone());
    let b = v
        .create_item(ItemKind::Login, vec![f("title", "B")])
        .unwrap();
    let (st, _) = s
        .post(
            &format!("/v1/vaults/{vid}/commit"),
            Some(&owner.token),
            json!({ "manifest": b64(&m), "records": [b64(&b.record)] }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // A manifest that omits a stored item (here: a fresh replica that only
    // knows item B) is refused.
    let fresh = owner
        .account
        .open_vault(
            bytes(&s.get(&format!("/v1/vaults/{vid}"), &owner.token).await.1["grants"][0]["grant"]),
            owner.account.public_keys(),
            None,
        )
        .unwrap();
    let only_b = fresh
        .create_item(ItemKind::Login, vec![f("title", "B")])
        .unwrap();
    // (Pretend an earlier commit of it went through, so the sequence number
    // is right and only the omission is wrong.)
    let _ = fresh.sign_manifest(owner.account.clone());
    fresh.commit_accepted().unwrap();
    let (st, r) = commit(&s, &owner, &fresh, vec![only_b.record]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{r}");

    // Garbage and wrong-vault uploads.
    let (st, _) = s
        .post(
            &format!("/v1/vaults/{vid}/commit"),
            Some(&owner.token),
            json!({ "manifest": b64(b"junk"), "records": [] }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, other_vault) = create_vault(&s, &owner).await;
    let w = other_vault
        .create_item(ItemKind::Login, vec![f("title", "W")])
        .unwrap();
    let m = other_vault.sign_manifest(owner.account.clone());
    let (st, _) = s
        .post(
            &format!("/v1/vaults/{vid}/commit"),
            Some(&owner.token),
            json!({ "manifest": b64(&m), "records": [b64(&w.record)] }),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Nothing above changed the vault: an honest replica still syncs.
    let me = owner.account.public_keys();
    let check = open_vault(&s, &owner, &vid, &me).await;
    sync(&s, &owner, &check, &[me]).await.unwrap();
    assert_eq!(titles(&check), ["A"]);
}

#[tokio::test]
async fn family_sharing_roles_removal_and_rotation() {
    let s = Srv::new();
    let mum = signup(&s, "mum@example.com").await.device;
    let kid = signup(&s, "kid@example.com").await.device;
    let dad = signup(&s, "dad@example.com").await.device;
    let (vid, mv) = create_vault(&s, &mum).await;
    let n = mv
        .create_item(
            ItemKind::Login,
            vec![f("title", "Netflix"), f("password", "family pw")],
        )
        .unwrap();
    assert_eq!(
        commit(&s, &mum, &mv, vec![n.record]).await.0,
        StatusCode::OK
    );

    // Invite: kid as reader, dad as writer.
    for (who, role) in [(&kid, "reader"), (&dad, "writer")] {
        let (st, r) = s
            .post(
                &format!("/v1/vaults/{vid}/invites"),
                Some(&mum.token),
                json!({ "email": who.email, "role": role }),
            )
            .await;
        assert_eq!(st, StatusCode::CREATED, "{r}");
        let (_, inv) = s.get("/v1/invites", &who.token).await;
        let inv = &inv["invites"][0];
        // The invitee sees who invited them, with keys to verify.
        assert_eq!(
            bytes(&inv["from"]["public_keys"]),
            mum.account.public_keys()
        );
        let (st, _) = s
            .post(
                &format!("/v1/invites/{}/accept", inv["invite_id"].as_str().unwrap()),
                Some(&who.token),
                json!({}),
            )
            .await;
        assert_eq!(st, StatusCode::OK);
        // Pending until mum shares the key.
        assert_eq!(
            s.get(&format!("/v1/vaults/{vid}/sync"), &who.token).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert!(matches!(
        s.mail
            .sent()
            .iter()
            .filter(|(_, m)| matches!(m, Mail::Invitation { .. }))
            .count(),
        2
    ));

    // Mum fetches members' keys (after comparing safety numbers) and grants.
    let (_, ms) = s
        .get(&format!("/v1/vaults/{vid}/members"), &mum.token)
        .await;
    let members = ms["members"].as_array().unwrap().clone();
    assert_eq!(members.len(), 3);
    for m in members.iter().filter(|m| m["role"] != "owner") {
        let keys = bytes(&m["public_keys"]);
        let g = mum.account.grant_vault(mv.clone(), keys).unwrap();
        // A grant for the wrong member is refused.
        let wrong = if m["account_id"] == json!(kid.account.account_id()) {
            &dad
        } else {
            &kid
        };
        let (st, _) = s
            .call(
                Method::PUT,
                &format!(
                    "/v1/vaults/{vid}/members/{}/grant",
                    wrong.account.account_id()
                ),
                Some(&mum.token),
                Some(json!({ "grant": b64(&g) })),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = s
            .call(
                Method::PUT,
                &format!(
                    "/v1/vaults/{vid}/members/{}/grant",
                    m["account_id"].as_str().unwrap()
                ),
                Some(&mum.token),
                Some(json!({ "grant": b64(&g) })),
            )
            .await;
        assert_eq!(st, StatusCode::NO_CONTENT);
    }
    let writers = vec![mum.account.public_keys(), dad.account.public_keys()];
    let mum_keys = mum.account.public_keys();
    let kv = open_vault(&s, &kid, &vid, &mum_keys).await;
    sync(&s, &kid, &kv, &writers).await.unwrap();
    assert_eq!(titles(&kv), ["Netflix"]);

    // Reader cannot write; writer can.
    let k = kv
        .create_item(ItemKind::Login, vec![f("title", "Kid game")])
        .unwrap();
    assert_eq!(
        commit(&s, &kid, &kv, vec![k.record]).await.0,
        StatusCode::FORBIDDEN
    );
    let dv = open_vault(&s, &dad, &vid, &mum_keys).await;
    sync(&s, &dad, &dv, &writers).await.unwrap();
    let d = dv
        .create_item(ItemKind::Login, vec![f("title", "Electricity")])
        .unwrap();
    assert_eq!(
        commit(&s, &dad, &dv, vec![d.record]).await.0,
        StatusCode::OK
    );
    sync(&s, &mum, &mv, &writers).await.unwrap();
    assert_eq!(titles(&mv), ["Electricity", "Netflix"]);

    // Remove dad: access ends at once, and the key must rotate.
    let (st, r) = s
        .call(
            Method::DELETE,
            &format!("/v1/vaults/{vid}/members/{}", dad.account.account_id()),
            Some(&mum.token),
            None,
        )
        .await;
    assert_eq!(
        (st, r["needs_rotation"].clone()),
        (StatusCode::OK, json!(true))
    );
    assert_eq!(
        s.get(&format!("/v1/vaults/{vid}/sync"), &dad.token).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, info) = s.get(&format!("/v1/vaults/{vid}"), &mum.token).await;
    assert_eq!(info["needs_rotation"], json!(true));

    // Rotation must re-grant exactly the remaining members.
    let recs: Vec<String> = mv
        .rotate_key()
        .unwrap()
        .iter()
        .map(|r| b64(&r.record))
        .collect();
    let m = mv.sign_manifest(mum.account.clone());
    let g_mum = mum
        .account
        .grant_vault(mv.clone(), mum.account.public_keys())
        .unwrap();
    let g_kid = mum
        .account
        .grant_vault(mv.clone(), kid.account.public_keys())
        .unwrap();
    let g_dad = mum
        .account
        .grant_vault(mv.clone(), dad.account.public_keys())
        .unwrap();
    let rotate = |grants: Vec<String>| json!({ "manifest": b64(&m), "records": recs.clone(), "grants": grants });
    let path = format!("/v1/vaults/{vid}/rotate");
    assert_eq!(
        s.post(&path, Some(&mum.token), rotate(vec![b64(&g_mum)]))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        s.post(
            &path,
            Some(&mum.token),
            rotate(vec![b64(&g_mum), b64(&g_kid), b64(&g_dad)])
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        s.post(
            &path,
            Some(&kid.token),
            rotate(vec![b64(&g_mum), b64(&g_kid)])
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (st, r) = s
        .post(
            &path,
            Some(&mum.token),
            rotate(vec![b64(&g_mum), b64(&g_kid)]),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    mv.commit_accepted().unwrap();
    assert_eq!(r["epoch"], json!(1));

    // Kid moves to the new key and keeps syncing; dad's old key is useless.
    let (_, info) = s.get(&format!("/v1/vaults/{vid}"), &kid.token).await;
    assert_eq!(info["epoch"], json!(1));
    assert_eq!(info["needs_rotation"], json!(false));
    let newest = info["grants"].as_array().unwrap().last().unwrap();
    assert_eq!(newest["epoch"], json!(1));
    kid.account
        .accept_rotation(kv.clone(), bytes(&newest["grant"]), mum_keys.clone())
        .unwrap();
    sync(&s, &kid, &kv, std::slice::from_ref(&mum_keys))
        .await
        .unwrap();
    // The reader's refused edit never left the kid's device.
    assert_eq!(titles(&kv), ["Electricity", "Kid game", "Netflix"]);
    assert_eq!(titles(&mv), ["Electricity", "Netflix"]);
    assert_eq!(kv.epoch(), 1);
}

#[tokio::test]
async fn password_change_needs_the_account_key_and_signs_out_other_devices() {
    let s = Srv::new();
    let su = signup(&s, "h@example.com").await;
    let phone = su.device;
    let laptop = login(&s, "h@example.com", &su.secret_key).await;

    let change = phone
        .account
        .change_password("a brand new passphrase".into(), su.secret_key.clone())
        .unwrap();
    let (st, r) = s
        .post(
            "/v1/account/password/start",
            Some(&phone.token),
            json!({ "request": b64(&change.registration.request().unwrap()) }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let upload = change.registration.finish(bytes(&r["response"])).unwrap();

    // A stolen session token without the account's private keys cannot do it.
    let (ch, _) = sign(&s, &phone, "change-password").await;
    let stranger = create_account(PW.into()).unwrap().account;
    let forged = stranger
        .sign_challenge("change-password".into(), ch.clone())
        .unwrap();
    let body = |ch: &[u8], sig: &[u8]| json!({ "upload": b64(&upload), "bundle": b64(&change.bundle), "challenge": b64(ch), "signature": b64(sig) });
    assert_eq!(
        s.post(
            "/v1/account/password/finish",
            Some(&phone.token),
            body(&ch, &forged)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // A signature for another purpose does not count either.
    let (ch, sig) = sign(&s, &phone, "delete-account").await;
    assert_eq!(
        s.post(
            "/v1/account/password/finish",
            Some(&phone.token),
            body(&ch, &sig)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    let (ch, sig) = sign(&s, &phone, "change-password").await;
    assert_eq!(
        s.post(
            "/v1/account/password/finish",
            Some(&phone.token),
            body(&ch, &sig)
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    phone.account.update_bundle(change.bundle.clone()).unwrap();
    // Replay of the same challenge fails.
    assert_eq!(
        s.post(
            "/v1/account/password/finish",
            Some(&phone.token),
            body(&ch, &sig)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        s.get("/v1/account", &laptop.token).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(s.get("/v1/account", &phone.token).await.0, StatusCode::OK);
    assert!(
        try_login(&s, "h@example.com", PW, &su.secret_key)
            .await
            .is_err()
    );
    try_login(
        &s,
        "h@example.com",
        "a brand new passphrase",
        &su.secret_key,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn forgotten_password_recovery_with_email_code_and_recovery_words() {
    let s = Srv::new();
    let su = signup(&s, "i@example.com").await;
    let (vid, v) = create_vault(&s, &su.device).await;
    let x = v
        .create_item(ItemKind::Login, vec![f("title", "Email")])
        .unwrap();
    assert_eq!(
        commit(&s, &su.device, &v, vec![x.record]).await.0,
        StatusCode::OK
    );

    let (st, _) = s
        .post(
            "/v1/recovery/email",
            None,
            json!({ "email": "i@example.com" }),
        )
        .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let code = s.mail.last_code("i@example.com").unwrap();
    // Unknown address: same answer, no mail.
    let before = s.mail.sent().len();
    assert_eq!(
        s.post(
            "/v1/recovery/email",
            None,
            json!({ "email": "nope@example.com" })
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(s.mail.sent().len(), before);

    let (st, r) = s
        .post(
            "/v1/recovery/start",
            None,
            json!({ "email": "i@example.com", "code": code }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let bundle = bytes(&r["bundle"]);
    let challenge = bytes(&r["challenge"]);
    let acct = recover_account(bundle, su.recovery_words.clone()).unwrap();
    let change = acct
        .change_password("recovered passphrase!".into(), su.secret_key.clone())
        .unwrap();
    let (st, r) = s
        .post("/v1/recovery/register", None, json!({ "email": "i@example.com", "code": code, "request": b64(&change.registration.request().unwrap()) }))
        .await;
    assert_eq!(st, StatusCode::OK);
    let upload = change.registration.finish(bytes(&r["response"])).unwrap();
    let sig = acct
        .sign_challenge("recover".into(), challenge.clone())
        .unwrap();
    let finish = |code: &str| json!({ "email": "i@example.com", "code": code, "upload": b64(&upload), "bundle": b64(&change.bundle), "challenge": b64(&challenge), "signature": b64(&sig) });
    assert_eq!(
        s.post("/v1/recovery/finish", None, finish("12345678"))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.post("/v1/recovery/finish", None, finish(&code)).await.0,
        StatusCode::NO_CONTENT
    );

    // Every old session is gone; the new password works and the vault opens.
    assert_eq!(
        s.get("/v1/account", &su.device.token).await.0,
        StatusCode::UNAUTHORIZED
    );
    let d = try_login(&s, "i@example.com", "recovered passphrase!", &su.secret_key)
        .await
        .unwrap();
    let me = d.account.public_keys();
    let v2 = open_vault(&s, &d, &vid, &me).await;
    sync(&s, &d, &v2, &[me]).await.unwrap();
    assert_eq!(titles(&v2), ["Email"]);
}

#[tokio::test]
async fn account_deletion_removes_everything() {
    let s = Srv::new();
    let owner = signup(&s, "j@example.com").await.device;
    let friend = signup(&s, "k@example.com").await.device;
    let (vid, v) = create_vault(&s, &owner).await;
    let x = v
        .create_item(ItemKind::Login, vec![f("title", "X")])
        .unwrap();
    assert_eq!(
        commit(&s, &owner, &v, vec![x.record]).await.0,
        StatusCode::OK
    );
    s.post(
        &format!("/v1/vaults/{vid}/invites"),
        Some(&owner.token),
        json!({ "email": "k@example.com", "role": "writer" }),
    )
    .await;

    let (ch, sig) = sign(&s, &owner, "delete-account").await;
    let (st, _) = s
        .post(
            "/v1/account/delete",
            Some(&owner.token),
            json!({ "challenge": b64(&ch), "signature": b64(&sig) }),
        )
        .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(
        s.get("/v1/account", &owner.token).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, inv) = s.get("/v1/invites", &friend.token).await;
    assert_eq!(inv["invites"].as_array().unwrap().len(), 0);
    // The address can sign up again.
    signup(&s, "j@example.com").await;
}

#[tokio::test]
async fn self_hosted_mode_without_email_verification() {
    let s = Srv::with(Config {
        require_email_verification: false,
        ..Config::default()
    });
    let na = create_account(PW.into()).unwrap();
    let (st, r) = s
        .post("/v1/signup/start", None, json!({ "email": "l@example.com", "account_id": na.account.account_id(), "request": b64(&na.registration.request().unwrap()) }))
        .await;
    assert_eq!(st, StatusCode::OK);
    let upload = na.registration.finish(bytes(&r["response"])).unwrap();
    let (st, _) = s
        .post(
            "/v1/signup/finish",
            None,
            json!({ "email": "l@example.com", "upload": b64(&upload), "bundle": b64(&na.bundle) }),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    login(&s, "l@example.com", &na.secret_key).await;
    let (_, info) = s.call(Method::GET, "/v1/server", None, None).await;
    assert_eq!(bytes(&info["opaque_public_key"]), s.pin);
    assert_eq!(info["email_verification"], json!(false));
    assert!(s.mail.sent().is_empty());
}

#[tokio::test]
async fn body_limits_allow_big_vault_uploads_only_where_needed() {
    let s = Srv::new();
    let d = signup(&s, "m@example.com").await.device;
    let (_, v) = create_vault(&s, &d).await;
    // Three ~900 KB notes: a ~3.6 MB commit, over the 2 MiB default.
    let big = "x".repeat(900_000);
    let recs: Vec<Vec<u8>> = (0..3)
        .map(|i| {
            v.create_item(
                ItemKind::SecureNote,
                vec![f("title", &format!("n{i}")), f("notes", &big)],
            )
            .unwrap()
            .record
        })
        .collect();
    let (st, r) = commit(&s, &d, &v, recs).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    // Elsewhere the default applies.
    let (st, _) = s
        .post(
            "/v1/prelogin",
            None,
            json!({ "email": "a@example.com", "pad": "y".repeat(3 << 20) }),
        )
        .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
}
