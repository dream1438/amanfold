//! The app-facing API from Rust: the same flow the binding tests run.

#![cfg(feature = "test-server")]

use amanfold_ffi::{test_server::TestServer, *};

fn f(name: &str, value: &str) -> FieldInput {
    FieldInput {
        name: name.into(),
        value: value.into(),
    }
}

#[test]
fn signup_login_sync_rollback() {
    let server = TestServer::new();
    let new = create_account("correct horse battery".into()).unwrap();
    let id = new.account.account_id();
    let req = new.registration.request().unwrap();
    assert!(matches!(
        new.registration.request(),
        Err(AmanfoldError::WrongStep { .. })
    ));
    let up = new
        .registration
        .finish(server.registration_response(id.clone(), req).unwrap())
        .unwrap();
    let record = server.registration_record(up).unwrap();
    let me = new.account.public_keys();

    let nv = new.account.create_vault().unwrap();
    let saved = nv
        .vault
        .create_item(
            ItemKind::Login,
            vec![f("title", "Bank"), f("password", "pw")],
        )
        .unwrap();
    let m1 = nv.vault.sign_manifest(new.account.clone());

    let s = LoginSession::new(
        server.prelogin(new.bundle.clone()).unwrap(),
        "correct horse battery".into(),
        new.secret_key.clone(),
    )
    .unwrap();
    assert!(matches!(
        s.finish(vec![], None),
        Err(AmanfoldError::WrongStep { .. })
    ));
    let resp = server
        .login_response(id.clone(), record, s.start().unwrap())
        .unwrap();
    let out = s.finish(resp, Some(server.public_key())).unwrap();
    assert_eq!(
        server.login_finish(out.finalization).unwrap(),
        out.session_key
    );
    let acct2 = s.unlock(new.bundle.clone()).unwrap();
    let v2 = acct2
        .open_vault(nv.grant.clone(), me.clone(), None)
        .unwrap();
    v2.sync(m1.clone(), vec![saved.record.clone()], vec![me.clone()])
        .unwrap();
    let view = v2.item(saved.id.clone()).unwrap().unwrap();
    assert_eq!(view.kind, ItemKind::Login);
    assert!(
        view.fields
            .iter()
            .any(|x| x.name == "password" && x.concealed)
    );

    let e = v2
        .update_item(saved.id.clone(), vec![f("password", "new")], vec![])
        .unwrap();
    let m2 = v2.sign_manifest(acct2.clone());
    nv.vault.sync(m2, vec![e.record], vec![me.clone()]).unwrap();
    assert!(matches!(
        nv.vault.sync(m1, vec![saved.record], vec![me.clone()]),
        Err(AmanfoldError::Rollback { .. })
    ));
    assert!(matches!(
        nv.vault.item("not-a-uuid".into()),
        Err(AmanfoldError::InvalidArgument { .. })
    ));
    assert!(matches!(
        nv.vault.set_trashed(uuid_like(), true),
        Err(AmanfoldError::NotFound)
    ));
}

fn uuid_like() -> String {
    "00000000-0000-4000-8000-000000000000".into()
}

#[test]
fn errors_cross_the_boundary_as_kinds() {
    assert!(matches!(
        unlock_account(vec![1, 2, 3], "x".into(), "y".into()),
        Err(AmanfoldError::Malformed { .. })
    ));
    assert!(matches!(
        check_secret_key("A1-NOPE".into()),
        Err(AmanfoldError::InvalidCode { .. })
    ));
    assert!(matches!(
        quick_unlock(vec![], vec![0; 31], vec![]),
        Err(AmanfoldError::Malformed { .. }) | Err(AmanfoldError::InvalidArgument { .. })
    ));
}
