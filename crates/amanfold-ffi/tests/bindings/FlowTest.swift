// End-to-end flow through the generated Swift bindings (what the iOS and
// macOS apps call). Run by CI on macOS: see .github/workflows/ci.yml.

import Foundation

let PW = "تفاحة-حصان-بطارية-دبوس"

func check(_ cond: Bool, _ what: String) {
    if !cond { fatalError("check failed: \(what)") }
}

func expectError(_ what: String, _ matches: (AmanfoldError) -> Bool, _ body: () throws -> Void) {
    do {
        try body()
    } catch let e as AmanfoldError {
        if matches(e) { return }
        fatalError("\(what): wrong error \(e)")
    } catch {
        fatalError("\(what): unexpected \(error)")
    }
    fatalError("\(what): no error")
}

func run() throws {
    let server = TestServer()

    let new = try createAccount(password: PW)
    let acct = new.account
    let reg = new.registration
    let upload = try reg.finish(response: try server.registrationResponse(accountIdText: acct.accountId(), request: try reg.request()))
    let record = try server.registrationRecord(upload: upload)
    let me = acct.publicKeys()

    let nv = try acct.createVault()
    let saved = try nv.vault.createItem(kind: .login, fields: [
        FieldInput(name: "title", value: "Bank"),
        FieldInput(name: "password", value: "hunter2!"),
    ])
    let manifest = nv.vault.signManifest(author: acct)

    let s = try LoginSession(prelogin: try server.prelogin(bundle: new.bundle), password: PW, secretKey: new.secretKey)
    let resp = try server.loginResponse(accountIdText: s.accountId(), record: record, request: try s.start())
    let out = try s.finish(response: resp, pinnedServerKey: server.publicKey())
    check(try server.loginFinish(finalization: out.finalization) == out.sessionKey, "session keys match")
    let acct2 = try s.unlock(bundle: new.bundle)
    let v2 = try acct2.openVault(grant: nv.grant, granterPublicKeys: me, cache: nil)
    check(try v2.sync(manifest: manifest, changed: [saved.record], trustedWriters: [me]).isEmpty, "nothing to upload")
    let pw = try v2.item(id: saved.id)!.fields.first { $0.name == "password" }!
    check(pw.value == "hunter2!" && pw.concealed, "password field")

    let edit = try v2.updateItem(id: saved.id, set: [FieldInput(name: "password", value: "n3w")], remove: [])
    let m2 = v2.signManifest(author: acct2)
    _ = try nv.vault.sync(manifest: m2, changed: [edit.record], trustedWriters: [me])
    expectError("rollback", { if case .Rollback = $0 { return true }; return false }) {
        _ = try nv.vault.sync(manifest: manifest, changed: [saved.record], trustedWriters: [me])
    }

    let again = try acct.openVault(grant: nv.grant, granterPublicKeys: me, cache: nv.vault.exportCache())
    check(again.items(includeTrashed: false).count == 1, "cache restores items")

    let q = try acct.enableQuickUnlock()
    check(try quickUnlock(bundle: new.bundle, deviceKey: q.deviceKey, wrappedKeys: q.wrappedKeys).accountId() == acct.accountId(), "quick unlock")

    expectError("wrong password", { if case .Decrypt = $0 { return true }; return false }) {
        _ = try unlockAccount(bundle: new.bundle, password: "not my password", secretKey: new.secretKey)
    }

    check(try cardInfo(number: "٥٥٥٥ ٥٥٥٥ ٥٥٥٥ ٤٤٤٤").brand == .mastercard, "card brand")
    check(try totpAt(secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", unixSecs: 59).code == "287082", "totp")

    print("swift bindings: all checks passed")
}

try run()
