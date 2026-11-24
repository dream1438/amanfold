//! End-to-end flows through the public API only, as a client app would use it.

use amanfold_core::{
    account::{self, RecoveryCode},
    hlc::Clock,
    item::{Item, fields},
    kdf::{KdfParams, SecretKey},
    sync::{self, EncryptedRecord},
    vault::{self, Vault},
};
use proptest::prelude::*;

const PW: &str = "تفاحة-حصان-بطارية-دبوس";

#[test]
fn signup_save_sync_new_device_unlock() {
    // Device 1: sign up, create the personal vault, add a login and a card.
    let acct = account::create_account(PW).unwrap();
    let vk = vault::VaultKey::generate();
    let self_grant = vault::grant(&vk, &acct.keys, &acct.keys.public()).unwrap();
    let mut d1 = Vault::new(vk);
    let mut c1 = Clock::new(1);
    let login = Item::login(
        "Email",
        "eslam@example.com",
        "s3cret!",
        "https://mail.example",
        c1.tick(1_000),
    )
    .unwrap();
    let card = Item::card(
        "Travel card",
        "ESLAM",
        "5555 5555 5555 4444",
        "12",
        "2030",
        "321",
        c1.tick(1_001),
    )
    .unwrap();
    let card_id = card.id();
    // "Upload": everything the server stores is serialised to bytes.
    let server_bundle = sync::to_cbor(&acct.bundle).to_vec();
    let server_grant = sync::to_cbor(&self_grant).to_vec();
    let server_records: Vec<Vec<u8>> = [d1.save(login).unwrap(), d1.save(card).unwrap()]
        .iter()
        .map(|r| sync::to_cbor(r).to_vec())
        .collect();

    // Recovery kit text, as printed.
    let kit_secret_key = acct.secret_key.to_display();
    let kit_words = acct.recovery_code.to_words();
    drop(acct);

    // Device 2: types the master password and the Secret Key from the kit.
    let bundle: account::AccountBundle = sync::from_cbor(&server_bundle).unwrap();
    let sk = SecretKey::parse(kit_secret_key.expose()).unwrap();
    let keys = account::unlock(&bundle, PW, &sk).unwrap();
    let grant: vault::VaultGrant = sync::from_cbor(&server_grant).unwrap();
    let vk2 = vault::accept(&grant, &keys, &keys.public()).unwrap();
    let mut d2 = Vault::new(vk2);
    for r in &server_records {
        let rec: EncryptedRecord = sync::from_cbor(r).unwrap();
        assert!(d2.apply_remote(&rec).unwrap().is_none());
    }
    assert_eq!(d2.items().count(), 2);
    assert_eq!(
        d2.get(&card_id).unwrap().get(fields::CARD_NUMBER),
        Some("5555555555554444")
    );

    // Forgot password: recover with the 24 words and set a new one.
    let code = RecoveryCode::parse(kit_words.expose()).unwrap();
    let rk = account::recover(&bundle, &code).unwrap();
    let nb = account::rewrap_password(
        &bundle,
        &rk,
        "a whole new passphrase",
        &sk,
        KdfParams::DEFAULT,
    )
    .unwrap();
    assert!(account::unlock(&nb, "a whole new passphrase", &sk).is_ok());
    assert!(account::unlock(&nb, PW, &sk).is_err());
}

#[test]
fn family_member_sharing_and_removal() {
    let owner = account::create_account("owner passphrase 123").unwrap();
    let kid = account::create_account("kid passphrase 4567").unwrap();
    // Owner verified kid's fingerprint in person before sharing.
    let kid_fp = kid.keys.public().fingerprint();
    assert_eq!(kid_fp, kid.bundle.public.fingerprint());

    let mut family = Vault::new(vault::VaultKey::generate());
    let rec = family
        .save(Item::note("Wi-Fi", "router password", Clock::new(9).tick(5)).unwrap())
        .unwrap();
    let g = vault::grant(family.key(), &owner.keys, &kid.bundle.public).unwrap();
    let kid_vk = vault::accept(&g, &kid.keys, &owner.bundle.public).unwrap();
    assert_eq!(
        sync::decrypt_item(&kid_vk, &rec)
            .unwrap()
            .0
            .get(fields::NOTES),
        Some("router password")
    );

    // Remove the kid: rotate, re-grant only to the owner.
    let new_recs = family.rotate().unwrap();
    assert!(sync::decrypt_item(&kid_vk, &new_recs[0]).is_err());
}

proptest! {
    /// Whatever bytes a malicious or broken server returns, the client errors
    /// cleanly instead of panicking.
    #[test]
    fn hostile_server_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let vk = vault::VaultKey::generate();
        if let Ok(rec) = sync::from_cbor::<EncryptedRecord>(&bytes) {
            let _ = sync::decrypt_item(&vk, &rec);
        }
        let _ = sync::from_cbor::<account::AccountBundle>(&bytes);
        let _ = sync::from_cbor::<vault::VaultGrant>(&bytes);
    }

    #[test]
    fn mutated_records_never_decrypt(i in 0usize..10_000, bit in 0u8..8) {
        let vk = vault::VaultKey::generate();
        let item = Item::note("t", "body", Clock::new(1).tick(1)).unwrap();
        let mut v = Vault::new(vk.clone());
        let rec = v.save(item.clone()).unwrap();
        let mut wire = sync::to_cbor(&rec).to_vec();
        let i = i % wire.len();
        wire[i] ^= 1 << bit;
        if let Ok(r) = sync::from_cbor::<EncryptedRecord>(&wire) {
            if let Ok((got, _)) = sync::decrypt_item(&vk, &r) {
                // Only acceptable if the flip hit a non-authenticated encoding
                // detail that decodes to the identical record.
                prop_assert_eq!(r, rec);
                prop_assert_eq!(got, item);
            }
        }
    }
}

/// Sign-up registers with OPAQUE; a new device logs in, then unlocks with the
/// key from the same Argon2id run. Password change re-registers.
#[test]
fn opaque_signup_login_unlock_and_password_change() {
    use amanfold_core::auth;
    let server = auth::ServerKeys::generate();
    let pinned = server.public_key();

    // Sign-up.
    let acct = account::create_account(PW).unwrap();
    let id = acct.bundle.account_id;
    let (st, req) = auth::client_registration_start(&acct.auth_key).unwrap();
    let resp = auth::server_registration_start(&server, &req, &id).unwrap();
    let upload = auth::client_registration_finish(st, &acct.auth_key, &resp, &id).unwrap();
    let mut record = auth::server_registration_finish(&upload).unwrap();
    let bundle = acct.bundle.clone();
    let sk = SecretKey::parse(acct.secret_key.to_display().expose()).unwrap();

    let login = |record: &[u8], pre: &account::PreLogin, pw: &str| {
        let secrets = pre.derive(pw, &sk)?;
        let (cst, req) = auth::client_login_start(&secrets.auth_key)?;
        let (sst, resp) = auth::server_login_start(&server, Some(record), &req, &pre.account_id)?;
        let out = auth::client_login_finish(
            cst,
            &secrets.auth_key,
            &resp,
            &pre.account_id,
            Some(&pinned),
        )?;
        let skey = auth::server_login_finish(sst, &out.finalization, &pre.account_id)?;
        assert_eq!(skey, out.session_key);
        Ok::<_, amanfold_core::Error>(secrets)
    };

    // New device: pre-login, login, then unlock without a second Argon2 run.
    let secrets = login(&record, &bundle.prelogin(), PW).unwrap();
    let keys = account::unlock_with_key(&bundle, &secrets.unlock_key).unwrap();
    assert_eq!(keys.public(), acct.keys.public());
    assert!(login(&record, &bundle.prelogin(), "wrong password here").is_err());

    // Password change: new bundle and new OPAQUE record, uploaded together.
    let nb = account::rewrap_password(
        &bundle,
        &keys,
        "new passphrase 2026",
        &sk,
        KdfParams::DEFAULT,
    )
    .unwrap();
    let new_auth = nb
        .prelogin()
        .derive("new passphrase 2026", &sk)
        .unwrap()
        .auth_key;
    let (st, req) = auth::client_registration_start(&new_auth).unwrap();
    let resp = auth::server_registration_start(&server, &req, &id).unwrap();
    let up = auth::client_registration_finish(st, &new_auth, &resp, &id).unwrap();
    record = auth::server_registration_finish(&up).unwrap();
    assert!(login(&record, &nb.prelogin(), PW).is_err());
    let s2 = login(&record, &nb.prelogin(), "new passphrase 2026").unwrap();
    assert!(account::unlock_with_key(&nb, &s2.unlock_key).is_ok());
}
