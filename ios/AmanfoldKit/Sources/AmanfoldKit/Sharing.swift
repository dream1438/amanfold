import Foundation

// Family and team sharing, as the apps show it. The rules match the browser
// extension: a vault key is accepted only from this account or a contact (a
// person whose safety number was compared on this device), and changes to a
// shared vault only when signed by one of them.

/// Field of the hidden note that holds a shared vault's name and kind.
public let vaultInfoField = "custom:amanfold.vault"

public enum VaultKind: Equatable, Hashable { case personal, family, team }

public enum Role: String, CaseIterable, Hashable {
    case owner, manager, writer, reader

    public var canWrite: Bool { self != .reader }
    public var canManage: Bool { self == .owner || self == .manager }

    static func of(_ wire: String?) -> Role { Role(rawValue: wire ?? "") ?? .reader }
}

/// Which vault an item is in.
public struct VaultTag: Equatable, Hashable, Identifiable {
    public let id: String
    public let name: String
    public let kind: VaultKind
    public let role: Role

    public var shared: Bool { kind != .personal }
    public var readOnly: Bool { !role.canWrite }
}

public struct SharedVaultView: Equatable, Identifiable {
    public let id: String
    public let name: String
    public let kind: VaultKind
    public let role: Role
    public let items: Int
    /// Changes are waiting from a member whose safety number was not compared here.
    public let needsVerification: Bool
}

/// An invitation to someone else's vault. Compare `fromFingerprint` before accepting.
public struct InviteView: Equatable, Identifiable {
    public let inviteId: String
    public let vaultId: String
    public let role: Role
    public let fromEmail: String
    public let fromFingerprint: String
    public var id: String { inviteId }
}

/// A vault shared with this account by someone not yet verified here.
public struct WaitingView: Equatable, Identifiable {
    public let vaultId: String
    public let accountId: String
    public let email: String
    public let fingerprint: String
    public var id: String { vaultId + "/" + accountId }
}

public struct SharingView: Equatable {
    public let vaults: [SharedVaultView]
    public let invites: [InviteView]
    public let waiting: [WaitingView]
    public let myFingerprint: String
}

public struct MemberView: Equatable, Identifiable {
    public let accountId: String
    public let email: String
    public let role: Role
    /// Has the vault key (pending members have accepted but wait for a manager).
    public let active: Bool
    public let fingerprint: String
    public let verified: Bool
    public let me: Bool
    public var id: String { accountId }
}

public struct SentInvite: Equatable, Identifiable {
    public let inviteId: String
    public let email: String
    public let role: Role
    public var id: String { inviteId }
}

public struct MembersView: Equatable {
    public let members: [MemberView]
    public let invites: [SentInvite]
}

/// This shared vault is read-only for this account.
public struct ReadOnlyError: Error {}

/// The safety number is not the one the person compared.
public struct FingerprintChangedError: Error {}

/// Rotation needs every remaining member's safety number compared first.
public struct UnverifiedMembersError: Error {}
