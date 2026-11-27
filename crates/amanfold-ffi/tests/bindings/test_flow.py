"""End-to-end flow through the generated Python bindings.

Exercises the same API the Swift and Kotlin apps use: sign-up, OPAQUE login
on a second device, verified sync, rollback detection, the local cache, quick
unlock, sharing, key rotation, recovery and the small tools.
"""

import sys

from amanfold import (
    AmanfoldError,
    FieldInput,
    ItemKind,
    LoginSession,
    TestServer,
    card_info,
    check_recovery_words,
    check_secret_key,
    create_account,
    default_password_rules,
    fingerprint,
    generate_passphrase,
    generate_password,
    quick_unlock,
    recover_account,
    totp_at,
    unlock_account,
)

PW = "تفاحة-حصان-بطارية-دبوس"


def raises(exc, fn, *args):
    try:
        fn(*args)
    except exc:
        return
    raise AssertionError(f"expected {exc.__name__}")


def register(server, new):
    req = new.registration.request()
    resp = server.registration_response(new.account.account_id(), req)
    upload = new.registration.finish(resp)
    # One-shot: a second finish is a programming error, reported clearly.
    raises(AmanfoldError.WrongStep, new.registration.finish, resp)
    return server.registration_record(upload)


def login(server, record, bundle, pw, sk):
    s = LoginSession(server.prelogin(bundle), pw, sk)
    resp = server.login_response(s.account_id(), record, s.start())
    out = s.finish(resp, server.public_key())
    assert server.login_finish(out.finalization) == out.session_key
    assert len(out.session_key) == 64
    return s.unlock(bundle)


def fields(view):
    return {f.name: (f.value, f.concealed) for f in view.fields}


def main():
    server = TestServer()

    # Device 1: sign up, register, make a vault with a login and a card.
    new = create_account(PW)
    record = register(server, new)
    acct = new.account
    bundle = new.bundle
    me = acct.public_keys()
    assert check_secret_key(new.secret_key.lower()) == new.secret_key
    check_recovery_words(new.recovery_words)
    assert fingerprint(me) == acct.fingerprint()

    nv = acct.create_vault()
    d1 = nv.vault
    login_item = d1.create_item(
        ItemKind.LOGIN,
        [
            FieldInput(name="title", value="Email"),
            FieldInput(name="username", value="eslam@example.com"),
            FieldInput(name="password", value="s3cret!"),
            FieldInput(name="url", value="https://mail.example"),
        ],
    )
    card = d1.create_item(
        ItemKind.CARD,
        [FieldInput(name="title", value="Travel"), FieldInput(name="card_number", value="٥٥٥٥ ٥٥٥٥ ٥٥٥٥ ٤٤٤٤")],
    )
    server_records = {login_item.id: login_item.record, card.id: card.record}
    manifest = d1.sign_manifest(acct)

    # Device 2: OPAQUE login, unlock, open the vault, verified sync.
    acct2 = login(server, record, bundle, PW, new.secret_key)
    assert acct2.fingerprint() == acct.fingerprint()
    d2 = acct2.open_vault(nv.grant, me, None)
    assert d2.sync(manifest, list(server_records.values()), [me]) == []
    got = fields(d2.item(card.id))
    assert got["card_number"] == ("5555555555554444", True)
    assert fields(d2.item(login_item.id))["username"] == ("eslam@example.com", False)

    # Wrong password: login fails before anything is unlocked.
    s = LoginSession(server.prelogin(bundle), "wrong password!!", new.secret_key)
    resp = server.login_response(s.account_id(), record, s.start())
    raises(AmanfoldError.Decrypt, s.finish, resp, server.public_key())
    raises(AmanfoldError.Decrypt, unlock_account, bundle, "wrong password!!", new.secret_key)
    raises(AmanfoldError.InvalidCode, LoginSession, server.prelogin(bundle), PW, "A1-NOPE")

    # Device 2 edits; device 1 picks it up.
    up = d2.update_item(login_item.id, [FieldInput(name="password", value="n3w!")], ["url"])
    server_records[up.id] = up.record
    m2 = d2.sign_manifest(acct2)
    d1.sync(m2, [up.record], [me])
    f = fields(d1.item(login_item.id))
    assert f["password"] == ("n3w!", True) and "url" not in f

    # The server replays the old manifest: rejected as a rollback.
    raises(AmanfoldError.Rollback, d1.sync, manifest, [login_item.record], [me])
    # A manifest checked against the wrong writers is rejected.
    raises(AmanfoldError.BadSignature, d1.sync, m2, [], [])

    # Restart: the cache restores items and the rollback state.
    d1b = acct.open_vault(nv.grant, me, d1.export_cache())
    assert [i.id for i in d1b.items(False)] == [i.id for i in d1.items(False)]
    raises(AmanfoldError.Rollback, d1b.sync, manifest, [login_item.record], [me])

    # Trash.
    t = d1.set_trashed(card.id, True)
    assert len(d1.items(False)) == 1 and len(d1.items(True)) == 2
    server_records[t.id] = t.record

    # Quick unlock.
    q = acct.enable_quick_unlock()
    assert quick_unlock(bundle, q.device_key, q.wrapped_keys).account_id() == acct.account_id()
    raises(AmanfoldError.Decrypt, quick_unlock, bundle, bytes(32), q.wrapped_keys)

    # Family sharing and removal.
    kid = create_account("kid passphrase 4567").account
    g = acct.grant_vault(d1, kid.public_keys())
    kv = kid.open_vault(g, me, None)
    # A grant is only accepted from the expected granter.
    raises(AmanfoldError.BadSignature, kid.open_vault, g, kid.public_keys(), None)
    m3 = d1.sign_manifest(acct)
    kv.sync(m3, list(server_records.values()), [me])
    assert len(kv.items(True)) == 2
    rotated = d1.rotate_key()
    assert d1.epoch() == 1 and len(rotated) == 2
    raises(AmanfoldError.Malformed, kv.sync, d1.sign_manifest(acct), [r.record for r in rotated], [me])
    # The owner's other device moves to the new key and reads the new records.
    acct2.accept_rotation(d2, acct.grant_vault(d1, me), me)
    assert d2.epoch() == 1

    # Forgot password: recover with the words, set a new password.
    r = recover_account(bundle, new.recovery_words)
    pc = r.change_password("a whole new passphrase", new.secret_key)
    req = pc.registration.request()
    rec2 = server.registration_record(pc.registration.finish(server.registration_response(r.account_id(), req)))
    r.update_bundle(pc.bundle)
    assert login(server, rec2, pc.bundle, "a whole new passphrase", new.secret_key).account_id() == acct.account_id()
    raises(AmanfoldError.Decrypt, unlock_account, pc.bundle, PW, new.secret_key)
    raises(AmanfoldError.InvalidArgument, r.update_bundle, kid.bundle())

    # Tools.
    rules = default_password_rules()
    assert len(generate_password(rules)) == rules.length
    assert len(generate_passphrase(6, "-").split("-")) == 6
    # RFC 6238 SHA-1 test vector.
    secret = "otpauth://totp/x?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&digits=8"
    assert totp_at(secret, 59).code == "94287082"
    ci = card_info("4111 1111 1111 1111")
    assert ci.luhn_valid and ci.masked == "•••• 1111" and ci.digits == "4111111111111111"

    print("python bindings: all checks passed")


if __name__ == "__main__":
    sys.exit(main())
