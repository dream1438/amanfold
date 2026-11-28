package com.amanfold.client

import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import java.io.File
import java.net.ServerSocket
import java.nio.file.Files
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlin.test.fail
import kotlinx.coroutines.runBlocking

/**
 * Family sharing against a real amanfold-server: invitations with safety
 * numbers, pending members, roles, removal with key rotation, leaving.
 */
class SharingTest {
    private lateinit var server: Process
    private lateinit var url: String
    private lateinit var dir: File

    @BeforeTest
    fun start() {
        val bin = System.getProperty("amanfold.server") ?: fail("amanfold.server not set")
        dir = Files.createTempDirectory("amanfold-share").toFile()
        val secrets = File(dir, "secrets.json")
        val kg = ProcessBuilder(bin, "keygen").redirectError(ProcessBuilder.Redirect.DISCARD).start()
        secrets.writeBytes(kg.inputStream.readBytes())
        check(kg.waitFor() == 0)
        val port = ServerSocket(0).use { it.localPort }
        url = "http://127.0.0.1:$port"
        server = ProcessBuilder(bin, "serve").apply {
            environment()["AMANFOLD_SECRETS_FILE"] = secrets.path
            environment()["AMANFOLD_DB"] = File(dir, "db.sqlite").path
            environment()["AMANFOLD_LISTEN"] = "127.0.0.1:$port"
            environment()["AMANFOLD_DEV"] = "1"
            environment()["AMANFOLD_NO_EMAIL_VERIFICATION"] = "1"
            redirectErrorStream(true)
            redirectOutput(File(dir, "server.log"))
        }.start()
        val deadline = System.currentTimeMillis() + 20_000
        while (true) {
            try {
                Api(url).get("/v1/server")
                break
            } catch (e: ApiException) {
                if (System.currentTimeMillis() > deadline) fail("server did not start")
                Thread.sleep(100)
            }
        }
    }

    @AfterTest
    fun stop() {
        server.destroy()
        dir.deleteRecursively()
    }

    private suspend fun person(email: String): Amanfold {
        val c = Amanfold(MemoryStore(), ClientConfig("Test phone", allowLocalHttp = true))
        c.signup(url, email, "long master password for $email", null)
        return c
    }

    private fun titles(c: Amanfold) = c.items().map { i -> i.fields.first { it.name == "title" }.value }.sorted()

    private suspend fun join(owner: Amanfold, vault: String, who: Amanfold, email: String, role: Role) {
        owner.invite(vault, email, role)
        val inv = who.sharing().invites.single()
        assertEquals(owner.sharing().myFingerprint, inv.fromFingerprint)
        assertEquals(role, inv.role)
        // The app refuses if the safety number is not the one the person compared.
        assertFailsWith<FingerprintChangedException> { who.acceptInvite(inv.inviteId, "00000 00000") }
        who.acceptInvite(inv.inviteId, inv.fromFingerprint)
        val pending = owner.members(vault).members.single { it.email == email }
        assertFalse(pending.active)
        assertFalse(pending.verified)
        assertEquals(who.sharing().myFingerprint, pending.fingerprint)
        owner.verify(vault, pending.accountId, pending.fingerprint)
        assertTrue(owner.members(vault).members.single { it.email == email }.let { it.active && it.verified })
        who.sync()
    }

    /** Compare safety numbers with every other member (done in person or on a call). */
    private suspend fun verifyAll(c: Amanfold, vault: String) {
        for (m in c.members(vault).members) if (!m.me && !m.verified) c.verify(vault, m.accountId, m.fingerprint)
    }

    @Test
    fun familySharing() = runBlocking {
        val mum = person("mum@example.com")
        val dad = person("dad@example.com")
        val kid = person("kid@example.com")

        val fam = mum.createVault("Family", VaultKind.Family)
        mum.create(ItemKind.LOGIN, listOf(FieldInput("title", "Netflix"), FieldInput("password", "p1")), fam)
        mum.create(ItemKind.LOGIN, listOf(FieldInput("title", "Mum's bank")))
        // The vault's name note is hidden from the item list.
        assertEquals(listOf("Mum's bank", "Netflix"), titles(mum))
        assertEquals("Family", mum.vaults().single { it.shared }.name)

        join(mum, fam, dad, "dad@example.com", Role.Writer)
        join(mum, fam, kid, "kid@example.com", Role.Reader)
        verifyAll(dad, fam)

        // Both see the shared vault by name; nobody sees mum's personal items.
        assertEquals(listOf("Netflix"), titles(dad))
        val netflix = kid.items().single().id
        assertEquals(VaultTag(fam, "Family", VaultKind.Family, Role.Reader), kid.vaultOf(netflix))
        assertTrue(kid.vaultOf(netflix)!!.readOnly)

        // A writer's change reaches everyone who verified the writer.
        dad.create(ItemKind.LOGIN, listOf(FieldInput("title", "Electricity")), fam)
        mum.sync()
        kid.sync()
        assertEquals(listOf("Netflix"), titles(kid))
        assertTrue(kid.sharing().vaults.single().needsVerification)
        verifyAll(kid, fam)
        kid.sync()
        assertFalse(kid.sharing().vaults.single().needsVerification)
        assertEquals(listOf("Electricity", "Mum's bank", "Netflix"), titles(mum))
        assertEquals(listOf("Electricity", "Netflix"), titles(kid))
        assertFailsWith<ReadOnlyException> { kid.create(ItemKind.LOGIN, listOf(FieldInput("title", "Game")), fam) }
        assertFailsWith<ReadOnlyException> { kid.update(netflix, listOf(FieldInput("password", "mine"))) }

        // Moving an item into the shared vault.
        val bank = mum.items().single { it.fields.any { f -> f.value == "Mum's bank" } }.id
        val moved = mum.move(bank, fam)
        dad.sync()
        assertEquals(listOf("Electricity", "Mum's bank", "Netflix"), titles(dad))
        assertEquals(fam, mum.vaultOf(moved)!!.id)

        // Removing dad rotates the key: he loses the vault, the kid keeps it.
        val dadId = mum.members(fam).members.single { it.email == "dad@example.com" }.accountId
        mum.removeMember(fam, dadId)
        dad.sync()
        assertTrue(dad.items().isEmpty())
        assertTrue(dad.vaults().none { it.shared })
        mum.create(ItemKind.LOGIN, listOf(FieldInput("title", "After")), fam)
        kid.sync()
        assertEquals(listOf("After", "Electricity", "Mum's bank", "Netflix"), titles(kid))

        // The kid leaves.
        kid.leaveVault(fam)
        assertTrue(kid.items().isEmpty())
        kid.sync()
        assertTrue(kid.vaults().none { it.shared })
        assertEquals(1, mum.members(fam).members.size)
    }

    @Test
    fun vaultFromSomeoneUnverifiedWaits() = runBlocking {
        val mum = person("mum@example.com")
        val dad = person("dad@example.com")
        val gran = person("gran@example.com")
        val fam = mum.createVault("Family", VaultKind.Family)
        join(mum, fam, dad, "dad@example.com", Role.Manager)
        // Dad, a manager, lets gran in. Gran never compared dad's number.
        dad.invite(fam, "gran@example.com", Role.Reader)
        val inv = gran.sharing().invites.single()
        gran.acceptInvite(inv.inviteId, inv.fromFingerprint)
        val g = dad.members(fam).members.single { it.email == "gran@example.com" }
        dad.verify(fam, g.accountId, g.fingerprint)
        gran.sync()
        // Gran trusts dad (she compared his number when accepting), so it
        // opens; mum's changes wait until gran compares mum's number too.
        assertTrue(gran.vaults().any { it.id == fam })
        assertTrue(gran.sharing().vaults.single().needsVerification)
        verifyAll(gran, fam)
        gran.sync()
        assertFalse(gran.sharing().vaults.single().needsVerification)

        // A member granted by someone not verified on this device waits.
        val pat = person("pat@example.com")
        mum.invite(fam, "pat@example.com", Role.Reader)
        val pinv = pat.sharing().invites.single()
        pat.acceptInvite(pinv.inviteId, pinv.fromFingerprint) // trusts mum only
        val p = dad.members(fam).members.single { it.email == "pat@example.com" }
        dad.verify(fam, p.accountId, p.fingerprint) // dad grants, not mum
        pat.sync()
        val waiting = pat.sharing().waiting.single()
        assertEquals("dad@example.com", waiting.email)
        assertEquals(dad.sharing().myFingerprint, waiting.fingerprint)
        assertTrue(pat.vaults().none { it.id == fam })
        pat.verify(fam, waiting.accountId, waiting.fingerprint)
        assertTrue(pat.vaults().any { it.id == fam })
        assertTrue(pat.sharing().waiting.isEmpty())
    }
}
