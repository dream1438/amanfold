package com.amanfold.client

import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import com.amanfold.core.androidAppUrl
import java.io.File
import java.net.ServerSocket
import java.nio.file.Files
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.fail
import kotlinx.coroutines.runBlocking

/**
 * The client against a real amanfold-server (built by cargo), the same way
 * the phone talks to it: sign-up, recovery kit, a second device, sync both
 * ways, lock and quick unlock, the recovery flow and sign-out.
 */
class SessionTest {
    private lateinit var server: Process
    private lateinit var url: String
    private lateinit var dir: File

    private val pw = "تفاحة-حصان-بطارية-دبوس"

    @BeforeTest
    fun start() {
        val bin = System.getProperty("amanfold.server") ?: fail("amanfold.server not set")
        dir = Files.createTempDirectory("amanfold-test").toFile()
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
                if (System.currentTimeMillis() > deadline) fail("server did not start: " + File(dir, "server.log").readText())
                Thread.sleep(100)
            }
        }
    }

    @AfterTest
    fun stop() {
        server.destroy()
        dir.deleteRecursively()
    }

    private fun client(store: AccountStore = MemoryStore(), pins: Map<String, String> = emptyMap()) =
        Amanfold(store, ClientConfig("Test phone", pins, allowLocalHttp = true))

    private fun f(n: String, v: String) = FieldInput(n, v)

    @Test
    fun fullFlow() = runBlocking {
        // Sign-up on phone A.
        val a = client()
        assertEquals(VaultState.NoAccount, a.state.value)
        assertEquals(false, a.serverInfo(url).emailVerification)
        val kit = a.signup(url, "dana@example.com", pw, null)
        assertTrue(kit.secretKey.startsWith("A1-"))
        assertEquals(24, kit.recoveryWords.split(" ").size)
        assertEquals(VaultState.Unlocked, a.state.value)

        val id = a.create(
            ItemKind.LOGIN,
            listOf(f("title", "Bank"), f("username", "dana"), f("password", "hunter2!"), f("url", "https://www.bank.com/login")),
        )
        assertNull(a.syncStatus.value.problem)
        assertEquals("Bank", a.item(id)!!.fields.first { it.name == "title" }.value)

        // Wrong password and a mistyped Secret Key on phone B.
        val b = client()
        assertEquals(Problem.WrongPassword, problemOf(runCatching { b.signin(url, "dana@example.com", "wrong password!", kit.secretKey) }.exceptionOrNull()!!))
        assertEquals(VaultState.NoAccount, b.state.value)
        assertFailsWith<SecretKeyException> { b.signin(url, "dana@example.com", pw, "A1-NOT-A-KEY") }

        // A server whose key is not the pinned one is refused before anything is sent.
        val pinned = client(pins = mapOf(url to B64.encode(ByteArray(32))))
        assertFailsWith<ServerKeyException> { pinned.signin(url, "dana@example.com", pw, kit.secretKey) }

        // Phone B signs in and sees the login.
        b.signin(url, "dana@example.com", pw, kit.secretKey.lowercase())
        assertEquals(VaultState.Unlocked, b.state.value)
        assertEquals(listOf(id), b.items().map { it.id })
        assertEquals(id, b.loginsForPage("https://bank.com/signin").single().item.id)
        assertTrue(b.loginsForPage("https://bank.com.evil.io").isEmpty())
        assertEquals(listOf("bank.com"), b.lookalikes("https://bank-secure.com"))

        // Edits on both phones merge.
        b.update(id, listOf(f("password", "n3w-pass")))
        val card = a.create(ItemKind.CARD, listOf(f("title", "Visa"), f("card_number", "4111 1111 1111 1111")))
        a.sync()
        b.sync()
        assertEquals("n3w-pass", a.item(id)!!.fields.first { it.name == "password" }.value)
        assertEquals("4111111111111111", b.item(card)!!.fields.first { it.name == "card_number" }.value)

        // Trash.
        a.trash(card, true)
        b.sync()
        assertEquals(listOf(card), b.items(trashed = true).map { it.id })
        assertEquals(listOf(id), b.items().map { it.id })

        // An Android app login.
        val app = androidAppUrl("com.bank.mobile", ByteArray(32) { 9 })
        b.update(id, listOf(f("url.2", app)))
        assertEquals(id, b.loginsForApp(app).single().item.id)
        assertTrue(b.loginsForApp(androidAppUrl("com.bank.mobile", ByteArray(32) { 1 })).isEmpty())

        // Lock, then unlock with the password and with the device key.
        val setup = b.enableQuickUnlock()
        assertTrue(b.hasQuickUnlock)
        b.lock()
        assertEquals(VaultState.Locked, b.state.value)
        assertFailsWith<LockedException> { b.items() }
        assertEquals(Problem.WrongPassword, problemOf(runCatching { b.quickUnlock(ByteArray(32)) }.exceptionOrNull()!!))
        b.quickUnlock(setup.deviceKey)
        assertEquals(2, b.items(true).size + b.items().size)
        b.lock()
        assertEquals(Problem.WrongPassword, problemOf(runCatching { b.unlock("nope nope nope") }.exceptionOrNull()!!))
        b.unlock(pw)
        assertEquals(VaultState.Unlocked, b.state.value)

        // Offline edits are kept and uploaded later.
        b.signout()
        assertEquals(VaultState.NoAccount, b.state.value)
        assertNull(b.accountInfo())

        // Forgotten password: emailed code (off on this server) plus the kit.
        val c = client()
        c.recover(url, "dana@example.com", null, kit.recoveryWords, kit.secretKey, "a brand new master password")
        assertEquals(VaultState.Unlocked, c.state.value)
        assertEquals("n3w-pass", c.item(id)!!.fields.first { it.name == "password" }.value)

        // Phone A was signed out by the password change.
        val err = runCatching { a.sync() }.exceptionOrNull()
        assertNotNull(err)
        assertEquals(SyncProblem.SignedOut, a.syncStatus.value.problem)
    }

    @Test
    fun accountSurvivesRestartInEncryptedFile() = runBlocking {
        val key = ByteArray(16) { 7 }
        val cipher = object : BlobCipher {
            override fun seal(plain: ByteArray) = plain.mapIndexed { i, b -> (b.toInt() xor key[i % 16].toInt()).toByte() }.toByteArray()
            override fun open(sealed: ByteArray) = seal(sealed)
        }
        val file = File(dir, "account.bin")
        val a = client(FileStore(file, cipher))
        a.signup(url, "sam@example.com", pw, null)
        val id = a.create(ItemKind.SECURE_NOTE, listOf(f("title", "Wifi"), f("notes", "pass: abc")))
        a.lock()

        // "Restart": a new client over the same file.
        val again = client(FileStore(file, cipher))
        assertEquals(VaultState.Locked, again.state.value)
        assertEquals("sam@example.com", again.accountInfo()!!.first)
        again.unlock(pw)
        assertEquals("pass: abc", again.item(id)!!.fields.first { it.name == "notes" }.value)
    }

    @Test
    fun serverAddresses() {
        assertEquals("https://api.example.com", checkServerUrl(" https://api.example.com/ ", false))
        assertEquals("https://x.org/amanfold", checkServerUrl("https://x.org/amanfold//", false))
        assertEquals("http://10.0.2.2:8080", checkServerUrl("http://10.0.2.2:8080", true))
        for (bad in listOf("http://api.example.com", "http://10.0.2.2:8080", "ftp://x.org", "https://u:p@x.org", "https://x.org/?a=1", "nonsense")) {
            assertFailsWith<BadServerException>(bad) { checkServerUrl(bad, false) }
        }
        assertFailsWith<BadServerException> { checkServerUrl("http://evil.example", true) }
    }
}
