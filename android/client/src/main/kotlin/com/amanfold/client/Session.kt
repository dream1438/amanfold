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
import com.amanfold.core.fingerprint
import com.amanfold.core.grantGranter
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
    private class Open(val account: Account, val me: ByteArray, val personalId: String) {
        val vaults = LinkedHashMap<String, LocalVault>()
        val meB64: String = B64.encode(me)

        fun close() {
            vaults.values.forEach { it.destroy() }
            vaults.clear()
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
        replaceOpen(openAll(account, s))
    }

    /** Unlock with the device key the platform released after a biometric check. */
    suspend fun quickUnlock(deviceKey: ByteArray) = mutex.withLock {
        val s = stored()
        val wrapped = s.quickWrapped ?: throw IllegalStateException("quick unlock is off")
        val account = quickUnlock(B64.decode(s.bundle), deviceKey, B64.decode(wrapped))
        replaceOpen(openAll(account, s))
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

    private fun granterOf(o: Open, v: StoredVault): ByteArray = if (v.granter.isEmpty()) o.me else B64.decode(v.granter)

    /**
     * Open every vault this device has. The personal vault must open; a
     * shared one that fails is dropped and opened again from the server on
     * the next sync.
     */
    private fun openAll(account: Account, s: StoredAccount): Open {
        val o = Open(account, account.publicKeys(), s.personalId)
        val dropped = mutableListOf<String>()
        try {
            for (v in s.vaults.values) {
                try {
                    o.vaults[v.id] = account.openVault(B64.decode(v.grant), granterOf(o, v), v.cache?.let(B64::decode))
                } catch (e: AmanfoldException) {
                    if (v.id == s.personalId) throw e
                    dropped += v.id
                }
            }
            if (s.personalId !in o.vaults) throw IllegalStateException("personal vault missing")
        } catch (e: Throwable) {
            o.close()
            throw e
        }
        if (dropped.isNotEmpty()) store.save(s.copy(vaults = s.vaults - dropped.toSet()))
        return o
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
                        personalId = nv.vault.id(),
                        vaults = mapOf(
                            nv.vault.id() to StoredVault(
                                id = nv.vault.id(),
                                role = "owner",
                                grant = B64.encode(nv.grant),
                                cache = B64.encode(nv.vault.exportCache()),
                            ),
                        ),
                        lastSyncMs = now(),
                    )
                    store.save(s)
                    val o = Open(account, account.publicKeys(), s.personalId)
                    o.vaults[s.personalId] = nv.vault
                    replaceOpen(o)
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
                            personalId = vaultId,
                            vaults = mapOf(vaultId to StoredVault(id = vaultId, role = "owner", grant = newestGrant(api, vaultId))),
                        )
                        val o = openAll(account, s)
                        store.save(s)
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
    // Items, across the personal vault and every shared vault
    // -----------------------------------------------------------------------

    private fun isInfo(i: ItemView) = i.fields.any { it.name == VAULT_INFO }

    private fun info(v: LocalVault): Pair<String, VaultKind> {
        val note = v.items(false).firstOrNull(::isInfo)
        val kind = if (note?.fields?.firstOrNull { it.name == VAULT_INFO }?.value == "team") VaultKind.Team else VaultKind.Family
        return (note?.fields?.firstOrNull { it.name == "title" }?.value ?: "") to kind
    }

    private fun tag(o: Open, s: StoredAccount, id: String): VaultTag {
        val role = Role.of(s.vaults[id]?.role ?: "reader")
        if (id == o.personalId) return VaultTag(id, "", VaultKind.Personal, role)
        val (name, kind) = o.vaults[id]?.let(::info) ?: ("" to VaultKind.Family)
        return VaultTag(id, name, kind, role)
    }

    /** The vaults items can be shown from, personal first. */
    fun vaults(): List<VaultTag> {
        val o = need()
        val s = stored()
        return o.vaults.keys.map { tag(o, s, it) }
    }

    fun items(trashed: Boolean = false): List<ItemView> =
        need().vaults.values.flatMap { v -> v.items(trashed).filter { it.trashed == trashed && !isInfo(it) } }

    private fun locate(id: String): Pair<String, ItemView>? {
        for ((vid, v) in need().vaults) {
            val i = v.item(id) ?: continue
            if (!isInfo(i)) return vid to i
        }
        return null
    }

    fun item(id: String): ItemView? = locate(id)?.second

    /** The vault an item is in. */
    fun vaultOf(itemId: String): VaultTag? = locate(itemId)?.let { tag(need(), stored(), it.first) }

    private fun <T> merged(f: (LocalVault) -> List<T>): List<T> = need().vaults.values.flatMap(f)

    /** The only logins autofill may offer on this web page, from every vault, best match first. */
    fun loginsForPage(pageUrl: String): List<LoginMatch> =
        merged { it.loginsForUrl(pageUrl) }.filter { !isInfo(it.item) }.sortedByDescending { it.strength.ordinal }

    /** The only logins autofill may offer inside this Android app. */
    fun loginsForApp(appUrl: String): List<LoginMatch> =
        merged { it.loginsForApp(appUrl) }.filter { !isInfo(it.item) }.sortedByDescending { it.strength.ordinal }

    /** Saved sites (in any vault) this page imitates, for a warning. */
    fun lookalikes(pageUrl: String): List<String> = merged { it.lookalikeSites(pageUrl) }.distinct()

    private fun writable(vaultId: String) {
        val sv = stored().vaults[vaultId] ?: throw IllegalArgumentException("no such vault")
        if (!Role.of(sv.role).canWrite) throw ReadOnlyException()
    }

    /** New item, in the personal vault unless [vaultId] names a shared one. */
    suspend fun create(kind: ItemKind, fields: List<FieldInput>, vaultId: String? = null): String = mutex.withLock {
        val o = need()
        val target = vaultId ?: o.personalId
        writable(target)
        val v = o.vaults[target] ?: throw IllegalArgumentException("no such vault")
        val saved = v.createItem(kind, fields)
        saveLocal(target, saved.id, saved.record)
        saved.id
    }.also { syncQuietly() }

    suspend fun update(id: String, set: List<FieldInput>, remove: List<String> = emptyList()) {
        mutex.withLock {
            val (vid, _) = locate(id) ?: throw IllegalArgumentException("no such item")
            writable(vid)
            val saved = need().vaults[vid]!!.updateItem(id, set, remove)
            saveLocal(vid, saved.id, saved.record)
        }
        syncQuietly()
    }

    suspend fun trash(id: String, trashed: Boolean) {
        mutex.withLock {
            val (vid, _) = locate(id) ?: throw IllegalArgumentException("no such item")
            writable(vid)
            val saved = need().vaults[vid]!!.setTrashed(id, trashed)
            saveLocal(vid, saved.id, saved.record)
        }
        syncQuietly()
    }

    /** Copy an item into another vault and move the original to the trash. */
    suspend fun move(id: String, toVault: String): String {
        val newId = mutex.withLock {
            val (from, item) = locate(id) ?: throw IllegalArgumentException("no such item")
            if (from == toVault) return id
            writable(from)
            writable(toVault)
            val o = need()
            val fields = item.fields.map { FieldInput(it.name, it.value) }
            val copy = o.vaults[toVault]!!.createItem(item.kind, fields)
            saveLocal(toVault, copy.id, copy.record)
            val gone = o.vaults[from]!!.setTrashed(id, true)
            saveLocal(from, gone.id, gone.record)
            copy.id
        }
        syncQuietly()
        return newId
    }

    private fun saveLocal(vaultId: String, id: String, record: ByteArray) {
        val o = need()
        val s = stored()
        val sv = s.vaults[vaultId] ?: throw IllegalArgumentException("no such vault")
        val next = sv.copy(pending = sv.pending + (id to B64.encode(record)), cache = B64.encode(o.vaults[vaultId]!!.exportCache()))
        store.save(s.copy(vaults = s.vaults + (vaultId to next)))
        _revision.value++
    }

    private fun putVault(v: StoredVault) {
        val s = stored()
        store.save(s.copy(vaults = s.vaults + (v.id to v)))
    }

    private fun dropVault(o: Open, id: String) {
        o.vaults.remove(id)?.destroy()
        val s = stored()
        store.save(s.copy(vaults = s.vaults - id))
        _revision.value++
    }

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------

    private suspend fun syncQuietly() {
        runCatching { sync() }
    }

    @Volatile private var waiting: List<WaitingView> = emptyList()

    /**
     * Shared vaults whose last sync carried changes or a new key signed by
     * someone not verified on this device. They stay as they were until the
     * person compares that member's safety number.
     */
    @Volatile private var unverifiedChanges: Set<String> = emptySet()

    /**
     * One round of the sync loop in docs/SERVER.md for every vault: pick up
     * vaults shared with this account, sync each (retrying on a conflict),
     * and rotate the key of a vault someone was removed from. A shared vault
     * that fails does not stop the others; the first failure is reported.
     */
    suspend fun sync() = mutex.withLock {
        val o = open ?: return@withLock
        try {
            withContext(io) {
                val api = Api(stored().server, stored().token)
                val list = discover(o, api)
                var first: Throwable? = null
                val blocked = mutableSetOf<String>()
                for (id in o.vaults.keys.toList()) {
                    try {
                        syncVault(o, id, api)
                        val row = list.firstOrNull { it.str("vault_id") == id }
                        if (row?.bool("needs_rotation") == true && Role.of(stored().vaults[id]?.role ?: "reader").canManage) {
                            rotate(o, id, api)
                        }
                    } catch (e: AmanfoldException.BadSignature) {
                        if (id == o.personalId) throw e
                        blocked += id
                    } catch (e: Throwable) {
                        if (id == o.personalId) throw e
                        if (first == null) first = e
                    }
                }
                unverifiedChanges = blocked
                first?.let { throw it }
            }
            store.save(stored().copy(lastSyncMs = now()))
            _sync.value = SyncStatus(stored().lastSyncMs)
            _revision.value++
        } catch (e: ApiException) {
            _sync.value = _sync.value.copy(
                problem = when {
                    e.status == 409 -> SyncProblem.Conflict
                    e.status == 401 -> SyncProblem.SignedOut
                    e.status == 0 -> SyncProblem.Offline
                    else -> SyncProblem.Other
                },
            )
            _revision.value++
            throw e
        } catch (e: AmanfoldException) {
            _sync.value = _sync.value.copy(
                problem = when (e) {
                    is AmanfoldException.Rollback, is AmanfoldException.BadSignature -> SyncProblem.Rollback
                    else -> SyncProblem.Other
                },
            )
            _revision.value++
            throw e
        }
    }

    /** Is this public key this account, or someone whose safety number was compared? */
    private fun trusted(o: Open, s: StoredAccount, keys: String) = keys == o.meB64 || s.contacts.values.any { it.keys == keys }

    private fun writers(o: Open, s: StoredAccount): List<ByteArray> = listOf(o.me) + s.contacts.values.map { B64.decode(it.keys) }

    private fun members(api: Api, vaultId: String) = api.get("/v1/vaults/$vaultId/members")["members"]!!.jsonArray.map { it.jsonObject }

    /** Open vaults newly shared with this account; drop ones it left. */
    private fun discover(o: Open, api: Api): List<JsonObject> {
        val rows = api.get("/v1/vaults")["vaults"]!!.jsonArray.map { it.jsonObject }
        val nextWaiting = mutableListOf<WaitingView>()
        for (row in rows) {
            val id = row.need("vault_id")
            val have = stored().vaults[id]
            if (have != null) {
                val role = row.need("role")
                if (role != have.role) putVault(have.copy(role = role))
                continue
            }
            if (!row.bool("active")) continue
            val grant = newestGrant(api, id)
            val granter = B64.encode(grantGranter(B64.decode(grant)))
            if (!trusted(o, stored(), granter)) {
                // Opened only after the person compares the sharer's safety number.
                members(api, id).firstOrNull { it.str("public_keys") == granter }?.let { m ->
                    nextWaiting += WaitingView(id, m.need("account_id"), m.need("email"), fingerprint(B64.decode(granter)))
                }
                continue
            }
            val v = o.account.openVault(B64.decode(grant), B64.decode(granter), null)
            o.vaults[id] = v
            putVault(
                StoredVault(
                    id = id,
                    role = row.need("role"),
                    grant = grant,
                    granter = if (granter == o.meB64) "" else granter,
                    cache = B64.encode(v.exportCache()),
                ),
            )
        }
        for (id in stored().vaults.keys) {
            // Removed from it, or it was deleted.
            if (id != o.personalId && rows.none { it.str("vault_id") == id }) dropVault(o, id)
        }
        waiting = nextWaiting
        return rows
    }

    private fun syncVault(o: Open, id: String, api: Api) {
        repeat(4) {
            try {
                syncOnce(o, id, api)
                return
            } catch (e: ApiException) {
                if (e.status != 409) throw e
            }
        }
        throw ApiException(409, "conflict", "another device keeps writing; try again")
    }

    private fun syncOnce(o: Open, id: String, api: Api) {
        val s = stored()
        var sv = s.vaults[id] ?: return
        val v = o.vaults[id] ?: return
        val r = api.get("/v1/vaults/$id/sync?since=${sv.seq}")
        if ((r.long("epoch") ?: 0) > v.epoch().toLong()) {
            // Someone removed a member and moved the vault to a new key.
            val grant = newestGrant(api, id)
            val granter = B64.encode(grantGranter(B64.decode(grant)))
            if (!trusted(o, s, granter)) throw AmanfoldException.BadSignature()
            o.account.acceptRotation(v, B64.decode(grant), B64.decode(granter))
            sv = sv.copy(grant = grant, granter = if (granter == o.meB64) "" else granter)
        }
        val canWrite = Role.of(sv.role).canWrite
        val pending = sv.pending.toMutableMap()
        val manifest = r.str("manifest")
        if (manifest != null) {
            val records = (r["records"] as? JsonArray ?: JsonArray(emptyList())).map { B64.decode(it.jsonPrimitive.content) }
            for (up in v.sync(B64.decode(manifest), records, writers(o, s))) {
                if (canWrite) pending[up.id] = B64.encode(up.record)
            }
        }
        sv = sv.copy(cache = B64.encode(v.exportCache()))
        if (pending.isEmpty() || !canWrite) {
            // A reader's edits would be refused; they never leave this device.
            putVault(sv.copy(pending = emptyMap(), seq = r.long("seq") ?: sv.seq))
            return
        }
        putVault(sv.copy(pending = pending))
        val signed = v.signManifest(o.account)
        val done = api.post("/v1/vaults/$id/commit", JsonObject(mapOf(
            "manifest" to JsonPrimitive(B64.encode(signed)),
            "records" to JsonArray(pending.values.map(::JsonPrimitive)),
        )))
        v.commitAccepted()
        putVault(sv.copy(seq = done.long("seq") ?: sv.seq, pending = emptyMap(), cache = B64.encode(v.exportCache())))
    }

    /**
     * New vault key after a member left or was removed, so they cannot read
     * anything written from now on. Every remaining member must have been
     * verified on this device, since each gets the new key.
     */
    private fun rotate(o: Open, id: String, api: Api) {
        val s = stored()
        val sv = s.vaults[id] ?: throw IllegalArgumentException("no such vault")
        val v = o.vaults[id] ?: throw IllegalArgumentException("no such vault")
        val keys = members(api, id).filter { it.bool("active") }.map { m ->
            when {
                m.str("account_id") == s.accountId -> o.meB64
                s.contacts[m.str("account_id")]?.keys == m.str("public_keys") -> m.need("public_keys")
                else -> throw UnverifiedMembersException()
            }
        }
        val records = v.rotateKey()
        try {
            val grants = keys.map { B64.encode(o.account.grantVault(v, B64.decode(it))) }
            val manifest = v.signManifest(o.account)
            val r = api.post("/v1/vaults/$id/rotate", JsonObject(mapOf(
                "manifest" to JsonPrimitive(B64.encode(manifest)),
                "records" to JsonArray(records.map { JsonPrimitive(B64.encode(it.record)) }),
                "grants" to JsonArray(grants.map(::JsonPrimitive)),
            )))
            v.commitAccepted()
            putVault(
                sv.copy(
                    grant = grants[keys.indexOf(o.meB64)],
                    granter = "",
                    seq = r.long("seq") ?: sv.seq,
                    pending = emptyMap(),
                    cache = B64.encode(v.exportCache()),
                ),
            )
        } catch (e: Throwable) {
            // Not accepted: go back to the old key, which the server still has.
            o.vaults.remove(id)?.destroy()
            try {
                o.vaults[id] = o.account.openVault(B64.decode(sv.grant), granterOf(o, sv), sv.cache?.let(B64::decode))
            } catch (_: AmanfoldException) {
                // Opened again from the server on the next sync.
                val now = stored()
                store.save(now.copy(vaults = now.vaults - id))
            }
            throw e
        }
    }

    // -----------------------------------------------------------------------
    // Sharing
    // -----------------------------------------------------------------------

    private fun apiNow(): Api = stored().let { Api(it.server, it.token) }

    /** Shared vaults, invitations, vaults waiting for a safety-number check, and this account's safety number. */
    suspend fun sharing(): SharingView = mutex.withLock {
        val o = need()
        withContext(io) {
            val invites = apiNow().get("/v1/invites")["invites"]!!.jsonArray.map { it.jsonObject }
            val s = stored()
            SharingView(
                vaults = o.vaults.filterKeys { it != o.personalId }.map { (id, v) ->
                    val (name, kind) = info(v)
                    SharedVaultView(
                        id, name, kind, Role.of(s.vaults[id]?.role ?: "reader"),
                        items = v.items(false).count { !isInfo(it) },
                        needsVerification = id in unverifiedChanges,
                    )
                },
                invites = invites.map { i ->
                    val from = i["from"]!!.jsonObject
                    InviteView(
                        inviteId = i.need("invite_id"),
                        vaultId = i.need("vault_id"),
                        role = Role.of(i.need("role")),
                        fromEmail = from.need("email"),
                        fromFingerprint = fingerprint(B64.decode(from.need("public_keys"))),
                    )
                },
                waiting = waiting,
                myFingerprint = o.account.fingerprint(),
            )
        }
    }

    /** A new family or team vault, owned by this account. */
    suspend fun createVault(name: String, kind: VaultKind): String {
        val id = mutex.withLock {
            val o = need()
            withContext(io) {
                val nv = o.account.createVault()
                val grant = B64.encode(nv.grant)
                apiNow().post("/v1/vaults", buildJsonObject { put("grant", grant) })
                val v = nv.vault
                val id = v.id()
                o.vaults[id] = v
                val title = name.trim().ifEmpty { if (kind == VaultKind.Team) "Team" else "Family" }
                val info = v.createItem(
                    ItemKind.SECURE_NOTE,
                    listOf(FieldInput("title", title), FieldInput(VAULT_INFO, if (kind == VaultKind.Team) "team" else "family")),
                )
                putVault(
                    StoredVault(
                        id = id,
                        role = "owner",
                        grant = grant,
                        cache = B64.encode(v.exportCache()),
                        pending = mapOf(info.id to B64.encode(info.record)),
                    ),
                )
                _revision.value++
                id
            }
        }
        syncQuietly()
        return id
    }

    suspend fun members(vaultId: String): MembersView = mutex.withLock {
        val o = need()
        withContext(io) {
            val s = stored()
            val sv = s.vaults[vaultId] ?: throw IllegalArgumentException("no such vault")
            val api = apiNow()
            val sent = if (Role.of(sv.role).canManage) {
                api.get("/v1/vaults/$vaultId/invites")["invites"]!!.jsonArray.map { it.jsonObject }
                    .map { SentInvite(it.need("invite_id"), it.need("email"), Role.of(it.need("role"))) }
            } else {
                emptyList()
            }
            MembersView(
                members = members(api, vaultId).map { m ->
                    val keys = m.need("public_keys")
                    MemberView(
                        accountId = m.need("account_id"),
                        email = m.need("email"),
                        role = Role.of(m.need("role")),
                        active = m.bool("active"),
                        fingerprint = fingerprint(B64.decode(keys)),
                        verified = trusted(o, s, keys),
                        me = m.str("account_id") == s.accountId,
                    )
                },
                invites = sent,
            )
        }
    }

    suspend fun invite(vaultId: String, email: String, role: Role) {
        require(role != Role.Owner) { "invite as manager, writer or reader" }
        withContext(io) {
            apiNow().post("/v1/vaults/$vaultId/invites", buildJsonObject {
                put("email", email.trim())
                put("role", role.wire)
            })
        }
    }

    /** Cancel an invitation (managers), or decline one (the invitee). */
    suspend fun cancelInvite(inviteId: String) {
        withContext(io) { apiNow().delete("/v1/invites/$inviteId") }
    }

    /**
     * Join a shared vault. [fingerprint] is the safety number the person saw
     * and compared; the inviter is trusted only if it still matches.
     */
    suspend fun acceptInvite(inviteId: String, fingerprint: String) {
        mutex.withLock {
            withContext(io) {
                val api = apiNow()
                val inv = api.get("/v1/invites")["invites"]!!.jsonArray.map { it.jsonObject }
                    .firstOrNull { it.str("invite_id") == inviteId } ?: throw ApiException(404, "not_found", "invitation not found")
                val from = inv["from"]!!.jsonObject
                if (fingerprint(B64.decode(from.need("public_keys"))) != fingerprint) throw FingerprintChangedException()
                addContact(from.need("account_id"), from.need("email"), from.need("public_keys"))
                api.post("/v1/invites/$inviteId/accept")
            }
        }
    }

    private fun addContact(accountId: String, email: String, keys: String) {
        val s = stored()
        store.save(s.copy(contacts = s.contacts + (accountId to Contact(email, keys))))
    }

    /**
     * Mark someone as verified after comparing safety numbers, and give them
     * the vault key if they are waiting for it (owners and managers). A vault
     * waiting for this person's key opens on the sync that follows.
     */
    suspend fun verify(vaultId: String, accountId: String, fingerprint: String) {
        mutex.withLock {
            val o = need()
            withContext(io) {
                val api = apiNow()
                val m = members(api, vaultId).firstOrNull { it.str("account_id") == accountId }
                    ?: throw ApiException(404, "not_found", "member not found")
                val keys = m.need("public_keys")
                if (fingerprint(B64.decode(keys)) != fingerprint) throw FingerprintChangedException()
                addContact(accountId, m.need("email"), keys)
                val sv = stored().vaults[vaultId]
                val v = o.vaults[vaultId]
                if (sv != null && v != null && !m.bool("active") && Role.of(sv.role).canManage) {
                    val grant = o.account.grantVault(v, B64.decode(keys))
                    api.put("/v1/vaults/$vaultId/members/$accountId/grant", buildJsonObject { put("grant", B64.encode(grant)) })
                }
            }
        }
        syncQuietly()
    }

    suspend fun setRole(vaultId: String, accountId: String, role: Role) {
        withContext(io) {
            apiNow().put("/v1/vaults/$vaultId/members/$accountId/role", buildJsonObject { put("role", role.wire) })
        }
    }

    /** Remove someone, then move the vault to a new key at once. */
    suspend fun removeMember(vaultId: String, accountId: String) {
        mutex.withLock {
            val o = need()
            withContext(io) {
                val api = apiNow()
                api.delete("/v1/vaults/$vaultId/members/$accountId")
                syncVault(o, vaultId, api)
                rotate(o, vaultId, api)
            }
            _revision.value++
        }
    }

    suspend fun leaveVault(vaultId: String) {
        mutex.withLock {
            val o = need()
            require(vaultId != o.personalId) { "the personal vault cannot be left" }
            withContext(io) { apiNow().delete("/v1/vaults/$vaultId/members/${stored().accountId}") }
            dropVault(o, vaultId)
        }
    }

    /** Delete a shared vault and everything in it, for every member (owner only). */
    suspend fun deleteVault(vaultId: String) {
        mutex.withLock {
            val o = need()
            require(vaultId != o.personalId) { "the personal vault cannot be deleted here" }
            withContext(io) { apiNow().delete("/v1/vaults/$vaultId") }
            dropVault(o, vaultId)
        }
    }
}


/** The vault is locked; unlock it first. */
class LockedException : IllegalStateException("the vault is locked")

/** The server's key is not the one this build trusts. */
class ServerKeyException : SecurityException("the server's key is not the one this app trusts")

/** The typed Secret Key failed its checksum. */
class SecretKeyException : IllegalArgumentException("secret key")
