//! The website's beta waitlist: double opt-in, no enumeration, leave works,
//! CORS only for configured origins and only on the waitlist routes.

use std::sync::Arc;

use amanfold_server::{
    AppState, Config, Secrets, app,
    mail::{Mail, MemoryMailer},
    store,
};
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn srv(cfg: Config) -> (Router, Arc<MemoryMailer>) {
    let mail = Arc::new(MemoryMailer::default());
    let state = AppState::new(
        cfg,
        Secrets::generate(),
        store::open(":memory:").unwrap(),
        mail.clone(),
    );
    (app(state), mail)
}

async fn post(app: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn links(mail: &MemoryMailer, to: &str) -> Vec<(String, String, bool)> {
    mail.sent()
        .into_iter()
        .filter(|(t, _)| t == to)
        .filter_map(|(_, m)| match m {
            Mail::WaitlistConfirm {
                confirm,
                leave,
                arabic,
            } => Some((confirm, leave, arabic)),
            _ => None,
        })
        .collect()
}

fn token(link: &str) -> String {
    link.split_once("#t=").unwrap().1.to_string()
}

#[tokio::test]
async fn double_opt_in_then_leave() {
    let (app, mail) = srv(Config::default());
    let (s, _) = post(
        &app,
        "/v1/waitlist",
        json!({"email": "Dana@Example.com", "lang": "ar", "plan": "family"}),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let l = links(&mail, "dana@example.com");
    assert_eq!(l.len(), 1);
    let (confirm, leave, arabic) = &l[0];
    assert!(arabic);
    assert!(
        confirm.starts_with("https://amanfold.com/ar/beta/confirm/#t="),
        "{confirm}"
    );
    assert!(
        leave.starts_with("https://amanfold.com/ar/beta/leave/#t="),
        "{leave}"
    );

    // A wrong or malformed token fails; the right one confirms (twice is fine).
    let (s, _) = post(&app, "/v1/waitlist/confirm", json!({"token": "nope"})).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = post(
        &app,
        "/v1/waitlist/confirm",
        json!({"token": "A".repeat(43)}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    for _ in 0..2 {
        let (s, _) = post(
            &app,
            "/v1/waitlist/confirm",
            json!({"token": token(confirm)}),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }

    // Joining again once confirmed: same answer, no new email.
    let (s, _) = post(&app, "/v1/waitlist", json!({"email": "dana@example.com"})).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(links(&mail, "dana@example.com").len(), 1);

    // Leaving deletes the entry; the old link no longer confirms.
    let (s, _) = post(&app, "/v1/waitlist/leave", json!({"token": token(leave)})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = post(
        &app,
        "/v1/waitlist/confirm",
        json!({"token": token(confirm)}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = post(&app, "/v1/waitlist/leave", json!({"token": token(leave)})).await;
    assert_eq!(s, StatusCode::OK);

    // After leaving, the form cannot be used to mail this address again.
    let (s, _) = post(&app, "/v1/waitlist", json!({"email": "dana@example.com"})).await;
    assert_eq!(s, StatusCode::ACCEPTED, "same answer as always");
    assert_eq!(links(&mail, "dana@example.com").len(), 1);
}

#[tokio::test]
async fn resending_replaces_the_old_link_and_bad_input_is_rejected() {
    let (app, mail) = srv(Config::default());
    post(&app, "/v1/waitlist", json!({"email": "a@example.com"})).await;
    post(
        &app,
        "/v1/waitlist",
        json!({"email": "a@example.com", "lang": "xx", "plan": "<b>"}),
    )
    .await;
    let l = links(&mail, "a@example.com");
    assert_eq!(l.len(), 2);
    let (s, _) = post(
        &app,
        "/v1/waitlist/confirm",
        json!({"token": token(&l[0].0)}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "first link was replaced");
    let (s, _) = post(
        &app,
        "/v1/waitlist/confirm",
        json!({"token": token(&l[1].0)}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let (s, _) = post(&app, "/v1/waitlist", json!({"email": "not an email"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(
        &app,
        "/v1/waitlist",
        json!({"email": "x@example.com", "pad": "a".repeat(8000)}),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn one_address_cannot_be_mail_bombed() {
    let (app, mail) = srv(Config::default());
    let mut limited = false;
    for _ in 0..10 {
        let (s, _) = post(&app, "/v1/waitlist", json!({"email": "b@example.com"})).await;
        limited |= s == StatusCode::TOO_MANY_REQUESTS;
    }
    assert!(limited);
    assert_eq!(
        links(&mail, "b@example.com").len() as i64,
        amanfold_server::waitlist::MAX_SENDS
    );
}

#[tokio::test]
async fn global_daily_budget_is_separate_from_account_mail() {
    let mut cfg = Config::default();
    cfg.limits.waitlist_global = amanfold_server::limits::Rule::new(2, 86_400);
    cfg.limits.mail_ip = amanfold_server::limits::Rule::new(100, 3600);
    let (app, mail) = srv(cfg);
    for e in ["1@example.com", "2@example.com"] {
        let (s, _) = post(&app, "/v1/waitlist", json!({"email": e})).await;
        assert_eq!(s, StatusCode::ACCEPTED);
    }
    let (s, _) = post(&app, "/v1/waitlist", json!({"email": "3@example.com"})).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert!(links(&mail, "3@example.com").is_empty());
    // Account sign-up mail is unaffected.
    let (s, _) = post(&app, "/v1/signup/email", json!({"email": "3@example.com"})).await;
    assert_eq!(s, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn cors_only_for_listed_origins_and_only_on_the_waitlist() {
    let cfg = Config {
        allowed_origins: vec!["https://amanfold.com".into()],
        ..Config::default()
    };
    let (app, _) = srv(cfg);
    let preflight = |origin: &str, path: &str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri(path)
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(Body::empty())
            .unwrap()
    };
    let allow = |r: &axum::response::Response| {
        r.headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap().to_string())
    };
    let r = app
        .clone()
        .oneshot(preflight("https://amanfold.com", "/v1/waitlist"))
        .await
        .unwrap();
    assert_eq!(allow(&r).as_deref(), Some("https://amanfold.com"));
    let r = app
        .clone()
        .oneshot(preflight("https://evil.example", "/v1/waitlist"))
        .await
        .unwrap();
    assert_eq!(allow(&r), None);
    let r = app
        .clone()
        .oneshot(preflight("https://amanfold.com", "/v1/prelogin"))
        .await
        .unwrap();
    assert_eq!(allow(&r), None);
}

#[test]
fn export_lists_only_confirmed() {
    let c = store::open(":memory:").unwrap();
    c.execute_batch(
        "INSERT INTO waitlist VALUES ('a@x.com','en','free',1,x'01',5);
         INSERT INTO waitlist VALUES ('b@x.com','ar','family',0,x'02',6);
         INSERT INTO waitlist VALUES ('c@x.com','en','free',1,x'03',7);",
    )
    .unwrap();
    // Export lists confirmed entries only.
    let rows = amanfold_server::waitlist::export(&c).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ("a@x.com".into(), "en".into(), "free".into(), 5));
    // The purge drops the expired unconfirmed entry and keeps confirmed ones.
    amanfold_server::waitlist::purge(&c).unwrap();
    let n: i64 = c
        .query_row("SELECT count(*) FROM waitlist", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}
