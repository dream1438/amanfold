package com.amanfold.client

import com.amanfold.core.Account
import com.amanfold.core.AmanfoldException
import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import com.amanfold.core.ItemView
import com.amanfold.core.LocalVault
import com.amanfold.core.LoginMatch
import com.amanfold.core.LoginSession
import com.amanfold.core.checkSecretKey
import com.amanfold.core.createAccount
import com.amanfold.core.quickUnlock
import com.amanfold.core.recoverAccount
import com.amanfold.core.unlockAccount
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** Settings that come from the build, not from the person. */
data class ClientConfig(
    /** Shown in the server's list of signed-in devices. */
    val deviceName: String,
    /** Pinned OPAQUE public keys (base64url) of servers this build trusts without asking. */
    val pinnedServerKeys: Map<String, String> = emptyMap(),
    /** Allow `http://` for this device or the emulator's host (debug builds only). */
    val allowLocalHttp: Boolean = false,
)

/** Shown once after sign-up, for the recovery kit. */
data class RecoveryKit(val email: String, val secretKey: String, val recoveryWords: String)

/** Where the app stands. */
enum class VaultState { NoAccount, Locked, Unlocked }

/** Why the last sync failed, for a banner. `null` while all is well. */
enum class SyncProblem { Offline, SignedOut, Rollback, Conflict, Other }

data class SyncStatus(val lastSyncMs: Long? = null, val problem: SyncProblem? = null)

/** The device key for quick unlock: the platform must protect it with biometrics. */
class QuickUnlockSetup(val deviceKey: ByteArray)

/**
 * The account and vault on this device. Keys stay inside the Rust core; this
 * class holds the core's objects while unlocked and drops them on [lock].
 *
 * Every operation that touches the vault runs under one mutex, so a sync never
 * interleaves with an edit or a lock. Network calls run on [io].
 */
class Amanfold(
    private val store: AccountStore,
    private val config: ClientConfig,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val now: () -> Long = System::currentTimeMillis,
) {
    private class Open(val account: Account, val vault: LocalVault, val me: ByteArray) {
        fun close() {
            vault.destroy()
            account.destroy()
        }
    }

    private val mutex = Mutex()
    @Volatile private var open: Open? = null

    private val _state = MutableStateFlow(initialState())
    val state: StateFlow<VaultState> = _state.asStateFlow()

    private val _sync = MutableStateFlow(SyncStatus(store.load()?.lastSyncMs))
    val syncStatus: StateFlow<SyncStatus> = _sync.asStateFlow()

    /** Bumped after every change to the items, so lists can refresh. */
    private val _revision = MutableStateFlow(0L)
    val revision: StateFlow<Long> = _revision.asStateFlow()

    private fun initialState() = if (store.load() == null) VaultState.NoAccount else VaultState.Locked

    private fun publish() {
        _state.value = when {
            open != null -> VaultState.Unlocked
            store.load() != null -> VaultState.Locked
            else -> VaultState.NoAccount
        }
        _revision.value++
    }

    /** The signed-in account's email and server, if any. */
    fun accountInfo(): Pair<String, String>? = store.load()?.let { it.email to it.server }

    val hasQuickUnlock: Boolean get() = store.load()?.quickWrapped != null

    private fun stored(): StoredAccount = store.load() ?: throw IllegalStateException("no account")

    private fun need(): Open = open ?: throw LockedException()

    // -----------------------------------------------------------------------
    // Locking
    // -----------------------------------------------------------------------

    suspend fun unlock(password: String) = mutex.withLock {
        val s = stored()
        val account = withContext(io) { unlockAccount(B64.decode(s.bundle), password, s.secretKey) }
        replaceOpen(openVault(account, s))
    }

    /** Unlock with the device key the platform released after a biometric check. */
    suspend fun quickUnlock(deviceKey: ByteArray) = mutex.withLock {
        val s = stored()
        val wrapped = s.quickWrapped ?: throw IllegalStateException("quick unlock is off")
        val account = quickUnlock(B64.decode(s.bundle), deviceKey, B64.decode(wrapped))
        replaceOpen(openVault(account, s))
    }

    /**
     * Turn on quick unlock. Returns the device key for the platform to keep
     * behind biometrics; the wrapped account keys stay in ordinary storage.
     */
    suspend fun enableQuickUnlock(): QuickUnlockSetup = mutex.withLock {
        val q = need().account.enableQuickUnlock()
        store.save(stored().copy(quickWrapped = B64.encode(q.wrappedKeys)))
        QuickUnlockSetup(q.deviceKey)
    }

    fun disableQuickUnlock() {
        store.load()?.let { store.save(it.copy(quickWrapped = null)) }
    }

    /** Forget the keys in memory. The vault stays on the device, encrypted. */
    fun lock() {
        val o = open
        open = null
        o?.close()
        publish()
    }

    val isUnlocked: Boolean get() = open != null

    private fun replaceOpen(o: Open) {
        val old = open
        open = o
        old?.close()
        publish()
    }

    private fun openVault(account: Account, s: StoredAccount): Open {
        val me = account.publicKeys()
        val cache = s.cache?.let(B64::decode)
        val vault = try {
            account.openVault(B64.decode(s.grant), me, cache)
        } catch (e: AmanfoldException) {
            account.destroy()
            throw e
        }
        return Open(account, vault, me)
    }

    // -----------------------------------------------------------------------
    // Sign-up, sign-in, recovery
    // -----------------------------------------------------------------------

    private fun api(server: String) = Api(checkServerUrl(server, config.allowLocalHttp))

    private fun pinnedKey(api: Api): String {
        val pinned = config.pinnedServerKeys[api.base]
        val key = api.get("/v1/server").need("opaque_public_key")
        if (pinned != null && key != pinned) throw ServerKeyException()
        return pinned ?: key
    }

    data class ServerInfo(val emailVerification: Boolean, val version: String)

    suspend fun serverInfo(server: String): ServerInfo = withContext(io) {
        val r = api(server).get("/v1/server")
        ServerInfo(r["email_verification"]?.jsonPrimitive?.content == "true", r.str("version") ?: "")
    }

    suspend fun requestSignupCode(server: String, email: String) = withContext(io) {
        api(server).post("/v1/signup/email", buildJsonObject { put("email", email) })
        Unit
    }

    /** Create the account and its personal vault. Show the returned kit once. */
    suspend fun signup(server: String, email: String, password: String, code: String?): RecoveryKit =
        mutex.withLock {
            check(store.load() == null) { "already signed in" }
            withContext(io) {
                val api = api(server)
                val serverKey = pinnedKey(api)
                val created = createAccount(password)
                val account = created.account
                val reg = created.registration
                try {
                    val start = api.post("/v1/signup/start", buildJsonObject {
                        put("email", email)
                        code?.let { put("code", it) }
                        put("account_id", account.accountId())
                        put("request", B64.encode(reg.request()))
                    })
                    val upload = reg.finish(B64.decode(start.need("response")))
                    val fin = api.post("/v1/signup/finish", buildJsonObject {
                        put("email", email)
                        code?.let { put("code", it) }
                        put("upload", B64.encode(upload))
                        put("bundle", B64.encode(created.bundle))
                        put("device_name", config.deviceName)
                    })
                    api.token = fin.need("api_token")
                    val nv = account.createVault()
                    api.post("/v1/vaults", buildJsonObject { put("grant", B64.encode(nv.grant)) })
                    val s = StoredAccount(
                        server = api.base,
                        email = email,
                        accountId = fin.need("account_id"),
                        bundle = B64.encode(created.bundle),
                        secretKey = created.secretKey,
                        serverKey = serverKey,
                        token = api.token!!,
                        vaultId = nv.vault.id(),
                        grant = B64.encode(nv.grant),
                        cache = B64.encode(nv.vault.exportCache()),
                        lastSyncMs = now(),
                    )
                    store.save(s)
                    replaceOpen(Open(account, nv.vault, account.publicKeys()))
                    _sync.value = SyncStatus(s.lastSyncMs)
                    RecoveryKit(email, created.secretKey, created.recoveryWords)
                } catch (e: Throwable) {
                    if (open?.account !== account) account.destroy()
                    throw e
                } finally {
                    reg.destroy()
                }
            }
        }

    /** Sign in on this device with the master password and the Secret Key. */
    suspend fun signin(server: String, email: String, password: String, secretKeyText: String) {
        mutex.withLock {
            check(store.load() == null) { "already signed in" }
            withContext(io) {
                val secretKey = try {
                    checkSecretKey(secretKeyText)
                } catch (e: AmanfoldException) {
                    throw SecretKeyException()
                }
                val api = api(server)
                val serverKey = pinnedKey(api)
                val pre = api.post("/v1/prelogin", buildJsonObject { put("email", email) })
                val session = LoginSession(B64.decode(pre.need("prelogin")), password, secretKey)
                try {
                    val start = api.post("/v1/login/start", buildJsonObject {
                        put("email", email)
                        put("request", B64.encode(session.start()))
                    })
                    val out = session.finish(B64.decode(start.need("response")), B64.decode(serverKey))
                    val fin = api.post("/v1/login/finish", buildJsonObject {
                        put("login_id", start.need("login_id"))
                        put("finalization", B64.encode(out.finalization))
                        put("device_name", config.deviceName)
                    })
                    val bundle = fin.need("bundle")
                    val account = session.unlock(B64.decode(bundle))
                    api.token = B64.encode(out.apiToken)
                    try {
                        val vaults = api.get("/v1/vaults")["vaults"]!!.jsonArray.map { it.jsonObject }
                        val mine = vaults.firstOrNull { it.str("role") == "owner" }
                            ?: throw ApiException(200, "no_vault", "this account has no vault yet")
                        val vaultId = mine.need("vault_id")
                        val s = StoredAccount(
                            server = api.base,
                            email = email,
                            accountId = fin.need("account_id"),
                            bundle = bundle,
                            secretKey = secretKey,
                            serverKey = serverKey,
                            token = api.token!!,
                            vaultId = vaultId,
                            grant = newestGrant(api, vaultId),
                        )
                        val o = openVault(account, s)
                        store.save(s.copy(cache = B64.encode(o.vault.exportCache())))
                        replaceOpen(o)
                    } catch (e: Throwable) {
                        if (open?.account !== account) account.destroy()
                        throw e
                    }
                } finally {
                    session.destroy()
                }
            }
        }
        sync()
    }

    private fun newestGrant(api: Api, vaultId: String): String {
        val grants = api.get("/v1/vaults/$vaultId")["grants"]!!.jsonArray
        val last = grants.lastOrNull()?.jsonObject ?: throw ApiException(200, "no_grant", "no vault key")
        return last.need("grant")
    }

    suspend fun requestRecoveryCode(server: String, email: String) = withContext(io) {
        api(server).post("/v1/recovery/email", buildJsonObject { put("email", email) })
        Unit
    }

    /**
     * Forgotten master password: needs the emailed code and the whole recovery
     * kit. Sets the new password; every device is signed out, and this one
     * signs in again with it.
     */
    suspend fun recover(
        server: String,
        email: String,
        code: String?,
        recoveryWords: String,
        secretKeyText: String,
        newPassword: String,
    ) {
        check(store.load() == null) { "already signed in" }
        withContext(io) {
            val secretKey = try {
                checkSecretKey(secretKeyText)
            } catch (e: AmanfoldException) {
                throw SecretKeyException()
            }
            val api = api(server)
            pinnedKey(api)
            val start = api.post("/v1/recovery/start", buildJsonObject {
                put("email", email)
                code?.let { put("code", it) }
            })
            val challenge = B64.decode(start.need("challenge"))
            val acct = recoverAccount(B64.decode(start.need("bundle")), recoveryWords)
            try {
                val change = acct.changePassword(newPassword, secretKey)
                val signature = acct.signChallenge("recover", challenge)
                val reg = change.registration
                try {
                    val r = api.post("/v1/recovery/register", buildJsonObject {
                        put("email", email)
                        code?.let { put("code", it) }
                        put("request", B64.encode(reg.request()))
                    })
                    api.post("/v1/recovery/finish", buildJsonObject {
                        put("email", email)
                        code?.let { put("code", it) }
                        put("upload", B64.encode(reg.finish(B64.decode(r.need("response")))))
                        put("bundle", B64.encode(change.bundle))
                        put("challenge", B64.encode(challenge))
                        put("signature", B64.encode(signature))
                    })
                } finally {
                    reg.destroy()
                }
            } finally {
                acct.destroy()
            }
        }
        signin(server, email, newPassword, secretKeyText)
    }

    /** Sign this device out and forget everything about the account here. */
    suspend fun signout() = mutex.withLock {
        val s = store.load()
        if (s != null) {
            withContext(io) {
                runCatching { Api(s.server, s.token).delete("/v1/sessions/current") }
            }
        }
        val o = open
        open = null
        o?.close()
        store.clear()
        _sync.value = SyncStatus()
        publish()
    }

    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    fun items(trashed: Boolean = false): List<ItemView> = need().vault.items(trashed).filter { it.trashed == trashed }

    fun item(id: String): ItemView? = need().vault.item(id)

    /** The only logins autofill may offer on this web page, best match first. */
    fun loginsForPage(pageUrl: String): List<LoginMatch> = need().vault.loginsForUrl(pageUrl)

    /** The only logins autofill may offer inside this Android app. */
    fun loginsForApp(appUrl: String): List<LoginMatch> = need().vault.loginsForApp(appUrl)

    /** Saved sites this page imitates, for a warning. */
    fun lookalikes(pageUrl: String): List<String> = need().vault.lookalikeSites(pageUrl)

    suspend fun create(kind: ItemKind, fields: List<FieldInput>): String = mutex.withLock {
        val saved = need().vault.createItem(kind, fields)
        saveLocal(saved.id, saved.record)
        saved.id
    }.also { syncQuietly() }

    suspend fun update(id: String, set: List<FieldInput>, remove: List<String> = emptyList()) {
        mutex.withLock {
            val saved = need().vault.updateItem(id, set, remove)
            saveLocal(saved.id, saved.record)
        }
        syncQuietly()
    }

    suspend fun trash(id: String, trashed: Boolean) {
        mutex.withLock {
            val saved = need().vault.setTrashed(id, trashed)
            saveLocal(saved.id, saved.record)
        }
        syncQuietly()
    }

    private fun saveLocal(id: String, record: ByteArray) {
        val o = need()
        val s = stored()
        store.save(s.copy(pending = s.pending + (id to B64.encode(record)), cache = B64.encode(o.vault.exportCache())))
        _revision.value++
    }

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------

    private suspend fun syncQuietly() {
        runCatching { sync() }
    }

    /** One round of the sync loop in docs/SERVER.md, retried on a conflict. */
    suspend fun sync() = mutex.withLock {
        if (open == null) return@withLock
        repeat(4) {
            try {
                withContext(io) { syncOnce() }
                _sync.value = SyncStatus(store.load()?.lastSyncMs)
                _revision.value++
                return@withLock
            } catch (e: ApiException) {
                if (e.status == 409) return@repeat
                _sync.value = _sync.value.copy(
                    problem = when {
                        e.status == 401 -> SyncProblem.SignedOut
                        e.status == 0 -> SyncProblem.Offline
                        else -> SyncProblem.Other
                    },
                )
                throw e
            } catch (e: AmanfoldException) {
                _sync.value = _sync.value.copy(
                    problem = when (e) {
                        is AmanfoldException.Rollback, is AmanfoldException.BadSignature -> SyncProblem.Rollback
                        else -> SyncProblem.Other
                    },
                )
                throw e
            }
        }
        _sync.value = _sync.value.copy(problem = SyncProblem.Conflict)
    }

    private fun syncOnce() {
        val o = need()
        var s = stored()
        val api = Api(s.server, s.token)
        val r = api.get("/v1/vaults/${s.vaultId}/sync?since=${s.seq}")
        val epoch = r.long("epoch") ?: 0
        if (epoch > o.vault.epoch().toLong()) {
            val grant = newestGrant(api, s.vaultId)
            o.account.acceptRotation(o.vault, B64.decode(grant), o.me)
            s = s.copy(grant = grant)
        }
        val pending = s.pending.toMutableMap()
        val manifest = r.str("manifest")
        if (manifest != null) {
            val records = (r["records"] as? JsonArray ?: JsonArray(emptyList())).map { B64.decode(it.jsonPrimitive.content) }
            for (up in o.vault.sync(B64.decode(manifest), records, listOf(o.me))) {
                pending[up.id] = B64.encode(up.record)
            }
        }
        s = s.copy(pending = pending, cache = B64.encode(o.vault.exportCache()))
        if (pending.isEmpty()) {
            store.save(s.copy(seq = r.long("seq") ?: s.seq, lastSyncMs = now()))
            return
        }
        store.save(s)
        val signed = o.vault.signManifest(o.account)
        val done = api.post("/v1/vaults/${s.vaultId}/commit", JsonObject(mapOf(
            "manifest" to JsonPrimitive(B64.encode(signed)),
            "records" to JsonArray(pending.values.map(::JsonPrimitive)),
        )))
        o.vault.commitAccepted()
        store.save(
            s.copy(
                seq = done.long("seq") ?: s.seq,
                pending = emptyMap(),
                cache = B64.encode(o.vault.exportCache()),
                lastSyncMs = now(),
            ),
        )
    }
}

/** The vault is locked; unlock it first. */
class LockedException : IllegalStateException("the vault is locked")

/** The server's key is not the one this build trusts. */
class ServerKeyException : SecurityException("the server's key is not the one this app trusts")

/** The typed Secret Key failed its checksum. */
class SecretKeyException : IllegalArgumentException("secret key")
