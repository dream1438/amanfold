package com.amanfold.client

import java.io.File
import kotlinx.serialization.Serializable

/**
 * What this device remembers about the signed-in account. Everything here is
 * either public, already encrypted by the core (bundle, grant, cache, pending
 * records), or needed to sign in again with the master password (Secret Key,
 * token). The platform encrypts the whole blob at rest ([BlobCipher]).
 */
@Serializable
data class StoredAccount(
    val server: String,
    val email: String,
    val accountId: String,
    val bundle: String,
    val secretKey: String,
    val serverKey: String,
    val token: String,
    /** The account's own vault; the others in [vaults] are shared. */
    val personalId: String,
    val vaults: Map<String, StoredVault> = emptyMap(),
    /** People whose safety numbers were compared on this device, by account id. */
    val contacts: Map<String, Contact> = emptyMap(),
    val lastSyncMs: Long? = null,
    /** Quick unlock: the account keys wrapped under the device key. */
    val quickWrapped: String? = null,
)

/** One vault on this device. */
@Serializable
data class StoredVault(
    val id: String,
    /** `owner`, `manager`, `writer` or `reader`. */
    val role: String,
    /** The newest grant this device opened the vault with. */
    val grant: String,
    /** Public keys of whoever signed [grant]; empty for this account. */
    val granter: String = "",
    val cache: String? = null,
    val seq: Long = 0,
    /** Encrypted records not yet accepted by the server, by item id. */
    val pending: Map<String, String> = emptyMap(),
)

/** Someone whose safety number was compared: their email and public keys. */
@Serializable
data class Contact(val email: String, val keys: String)

/** Where [StoredAccount] lives. */
interface AccountStore {
    fun load(): StoredAccount?
    fun save(account: StoredAccount)
    fun clear()
}

/** Encrypts the stored account at rest (Android Keystore, iOS Keychain). */
interface BlobCipher {
    fun seal(plain: ByteArray): ByteArray
    fun open(sealed: ByteArray): ByteArray
}

/** For tests. */
class MemoryStore : AccountStore {
    @Volatile private var value: StoredAccount? = null
    override fun load() = value
    override fun save(account: StoredAccount) { value = account }
    override fun clear() { value = null }
}

/** One encrypted file, replaced atomically. */
class FileStore(private val file: File, private val cipher: BlobCipher) : AccountStore {
    private val lock = Any()

    override fun load(): StoredAccount? = synchronized(lock) {
        if (!file.exists()) return null
        val plain = cipher.open(file.readBytes())
        json.decodeFromString(StoredAccount.serializer(), plain.decodeToString())
    }

    override fun save(account: StoredAccount) = synchronized(lock) {
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeBytes(cipher.seal(json.encodeToString(StoredAccount.serializer(), account).toByteArray()))
        if (!tmp.renameTo(file)) {
            file.delete()
            check(tmp.renameTo(file)) { "could not save the account" }
        }
    }

    override fun clear() = synchronized(lock) {
        file.delete()
        File(file.parentFile, file.name + ".tmp").delete()
        Unit
    }
}
