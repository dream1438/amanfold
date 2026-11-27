// End-to-end flow through the generated Kotlin bindings (what the Android
// app calls). Run by CI: see .github/workflows/ci.yml.

import com.amanfold.core.*

const val PW = "تفاحة-حصان-بطارية-دبوس"

inline fun <reified E : Throwable> assertThrows(block: () -> Unit) {
    try {
        block()
    } catch (e: Throwable) {
        if (e is E) return
        throw AssertionError("expected ${E::class.simpleName}, got $e")
    }
    throw AssertionError("expected ${E::class.simpleName}")
}

fun check(cond: Boolean, what: String) {
    if (!cond) throw AssertionError(what)
}

fun main() {
    val server = TestServer()

    // Sign-up and OPAQUE registration.
    val new = createAccount(PW)
    val acct = new.account
    val reg = new.registration
    val upload = reg.finish(server.registrationResponse(acct.accountId(), reg.request()))
    val record = server.registrationRecord(upload)
    val me = acct.publicKeys()

    // A personal vault with one login.
    val nv = acct.createVault()
    val saved = nv.vault.createItem(
        ItemKind.LOGIN,
        listOf(FieldInput("title", "Bank"), FieldInput("password", "hunter2!")),
    )
    val manifest = nv.vault.signManifest(acct)

    // Second phone: log in, unlock, sync.
    val s = LoginSession(server.prelogin(new.bundle), PW, new.secretKey)
    val out = s.finish(server.loginResponse(s.accountId(), record, s.start()), server.publicKey())
    check(server.loginFinish(out.finalization).contentEquals(out.sessionKey), "session keys match")
    val acct2 = s.unlock(new.bundle)
    val v2 = acct2.openVault(nv.grant, me, null)
    check(v2.sync(manifest, listOf(saved.record), listOf(me)).isEmpty(), "nothing to upload")
    val pw = v2.item(saved.id)!!.fields.first { it.name == "password" }
    check(pw.value == "hunter2!" && pw.concealed, "password field")

    // An edit, then the server replays the old manifest.
    val edit = v2.updateItem(saved.id, listOf(FieldInput("password", "n3w")), emptyList())
    val m2 = v2.signManifest(acct2)
    nv.vault.sync(m2, listOf(edit.record), listOf(me))
    assertThrows<AmanfoldException.Rollback> { nv.vault.sync(manifest, listOf(saved.record), listOf(me)) }

    // Restart from the cache; rollback protection survives.
    val again = acct.openVault(nv.grant, me, nv.vault.exportCache())
    check(again.items(false).size == 1, "cache restores items")
    assertThrows<AmanfoldException.Rollback> { again.sync(manifest, listOf(saved.record), listOf(me)) }

    // Quick unlock with a key the app keeps in the Android Keystore.
    val q = acct.enableQuickUnlock()
    check(quickUnlock(new.bundle, q.deviceKey, q.wrappedKeys).accountId() == acct.accountId(), "quick unlock")

    // Wrong password.
    assertThrows<AmanfoldException.Decrypt> { unlockAccount(new.bundle, "not my password", new.secretKey) }

    // Tools.
    check(cardInfo("٥٥٥٥ ٥٥٥٥ ٥٥٥٥ ٤٤٤٤").brand == CardBrand.MASTERCARD, "card brand")
    check(totpAt("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 59UL).code == "287082", "totp")

    println("kotlin bindings: all checks passed")
}
