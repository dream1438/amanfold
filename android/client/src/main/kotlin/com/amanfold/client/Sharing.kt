package com.amanfold.client

/**
 * Family and team sharing, as the apps show it. The rules match the browser
 * extension: a vault key is accepted only from this account or a contact (a
 * person whose safety number was compared on this device), and changes to a
 * shared vault only when signed by one of them.
 */

/** Field of the hidden note that holds a shared vault's name and kind. */
const val VAULT_INFO = "custom:amanfold.vault"

enum class VaultKind { Personal, Family, Team }

enum class Role(val wire: String) {
    Owner("owner"), Manager("manager"), Writer("writer"), Reader("reader");

    val canWrite: Boolean get() = this != Reader
    val canManage: Boolean get() = this == Owner || this == Manager

    companion object {
        fun of(wire: String): Role = entries.firstOrNull { it.wire == wire } ?: Reader
    }
}

/** Which vault an item is in. */
data class VaultTag(
    val id: String,
    val name: String,
    val kind: VaultKind,
    val role: Role,
) {
    val shared: Boolean get() = kind != VaultKind.Personal
    val readOnly: Boolean get() = !role.canWrite
}

data class SharedVaultView(
    val id: String,
    val name: String,
    val kind: VaultKind,
    val role: Role,
    val items: Int,
    /** Changes are waiting from a member whose safety number was not compared here. */
    val needsVerification: Boolean = false,
)

/** An invitation to someone else's vault. Compare [fromFingerprint] before accepting. */
data class InviteView(
    val inviteId: String,
    val vaultId: String,
    val role: Role,
    val fromEmail: String,
    val fromFingerprint: String,
)

/** A vault shared with this account by someone not yet verified here. */
data class WaitingView(val vaultId: String, val accountId: String, val email: String, val fingerprint: String)

data class SharingView(
    val vaults: List<SharedVaultView>,
    val invites: List<InviteView>,
    val waiting: List<WaitingView>,
    val myFingerprint: String,
)

data class MemberView(
    val accountId: String,
    val email: String,
    val role: Role,
    /** Has the vault key (pending members have accepted but wait for a manager). */
    val active: Boolean,
    val fingerprint: String,
    val verified: Boolean,
    val me: Boolean,
)

data class SentInvite(val inviteId: String, val email: String, val role: Role)

data class MembersView(val members: List<MemberView>, val invites: List<SentInvite>)

/** This shared vault is read-only for this account. */
class ReadOnlyException : IllegalStateException("this shared vault is read-only for you")

/** The safety number is not the one the person compared. */
class FingerprintChangedException : SecurityException("the safety number changed")

/** Rotation needs every remaining member's safety number compared first. */
class UnverifiedMembersException : IllegalStateException("compare safety numbers with every member first")
