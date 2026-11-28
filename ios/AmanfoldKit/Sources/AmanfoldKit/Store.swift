import CryptoKit
import Foundation
import LocalAuthentication
import Security

/// What this device remembers about the signed-in account. Everything here is
/// public, already encrypted by the core (bundle, grant, cache, pending
/// records), or needed to sign in again with the master password (Secret
/// Key, token). The whole file is encrypted at rest ([FileStore]).
public struct StoredAccount: Codable, Equatable {
    public var server: String
    public var email: String
    public var accountId: String
    public var bundle: String
    public var secretKey: String
    public var serverKey: String
    public var token: String
    /// The account's own vault; the others in `vaults` are shared.
    public var personalId: String
    public var vaults: [String: StoredVault] = [:]
    /// People whose safety numbers were compared on this device, by account id.
    public var contacts: [String: Contact] = [:]
    public var lastSyncMs: Int64?
    /// Quick unlock: the account keys wrapped under the device key.
    public var quickWrapped: String?
}

/// One vault on this device.
public struct StoredVault: Codable, Equatable {
    public var id: String
    /// `owner`, `manager`, `writer` or `reader`.
    public var role: String
    /// The newest grant this device opened the vault with.
    public var grant: String
    /// Public keys of whoever signed `grant`; empty for this account.
    public var granter: String = ""
    public var cache: String?
    public var seq: Int64 = 0
    /// Encrypted records not yet accepted by the server, by item id.
    public var pending: [String: String] = [:]
}

/// Someone whose safety number was compared: their email and public keys.
public struct Contact: Codable, Equatable {
    public var email: String
    public var keys: String
}

public protocol AccountStore: AnyObject {
    func load() -> StoredAccount?
    func save(_ account: StoredAccount) throws
    func clear()
}

public final class MemoryStore: AccountStore {
    private var value: StoredAccount?
    public init() {}
    public func load() -> StoredAccount? { value }
    public func save(_ account: StoredAccount) throws { value = account }
    public func clear() { value = nil }
}

/// One AES-GCM encrypted file. The key lives in the Keychain (this device
/// only, shared with the AutoFill extension through the access group).
public final class FileStore: AccountStore {
    private let url: URL
    private let key: SymmetricKey

    public init(url: URL, key: SymmetricKey) {
        self.url = url
        self.key = key
    }

    public func load() -> StoredAccount? {
        guard let sealed = try? Data(contentsOf: url),
              let box = try? AES.GCM.SealedBox(combined: sealed),
              let plain = try? AES.GCM.open(box, using: key)
        else { return nil }
        return try? JSONDecoder().decode(StoredAccount.self, from: plain)
    }

    public func save(_ account: StoredAccount) throws {
        let plain = try JSONEncoder().encode(account)
        let sealed = try AES.GCM.seal(plain, using: key).combined!
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        #if os(iOS)
        try sealed.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        #else
        try sealed.write(to: url, options: [.atomic])
        #endif
    }

    public func clear() {
        try? FileManager.default.removeItem(at: url)
    }
}

/// Small Keychain helpers. Items are this-device-only and never synced to iCloud.
public enum Keychain {
    static func query(_ account: String, group: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "com.amanfold",
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: false,
        ]
        if let group { q[kSecAttrAccessGroup as String] = group }
        #if os(macOS)
        q[kSecUseDataProtectionKeychain as String] = true
        #endif
        return q
    }

    /// The key of the account file, created on first use.
    public static func fileKey(group: String?) throws -> SymmetricKey {
        var q = query("file-key", group: group)
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        if SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data {
            return SymmetricKey(data: d)
        }
        let key = SymmetricKey(size: .bits256)
        var add = query("file-key", group: group)
        add[kSecValueData as String] = key.withUnsafeBytes { Data($0) }
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainError(status: status) }
        return key
    }

    /// Store the quick-unlock device key behind Face ID / Touch ID. Adding a
    /// fingerprint or face invalidates it (`biometryCurrentSet`), and it is
    /// gone if the passcode is removed.
    public static func saveDeviceKey(_ key: Data, group: String?) throws {
        deleteDeviceKey(group: group)
        var error: Unmanaged<CFError>?
        guard let access = SecAccessControlCreateWithFlags(
            nil, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, .biometryCurrentSet, &error
        ) else { throw KeychainError(status: errSecParam) }
        var add = query("device-key", group: group)
        add[kSecValueData as String] = key
        add[kSecAttrAccessControl as String] = access
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainError(status: status) }
    }

    /// Asks for Face ID / Touch ID. `nil` if cancelled, or the key is gone
    /// (biometrics changed).
    public static func readDeviceKey(reason: String, group: String?) -> Data? {
        let ctx = LAContext()
        ctx.localizedReason = reason
        var q = query("device-key", group: group)
        q[kSecReturnData as String] = true
        q[kSecUseAuthenticationContext as String] = ctx
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess else { return nil }
        return out as? Data
    }

    public static func hasDeviceKey(group: String?) -> Bool {
        var q = query("device-key", group: group)
        let ctx = LAContext()
        ctx.interactionNotAllowed = true
        q[kSecUseAuthenticationContext as String] = ctx
        let status = SecItemCopyMatching(q as CFDictionary, nil)
        // Present but needs authentication counts as present.
        return status == errSecSuccess || status == errSecInteractionNotAllowed
    }

    public static func deleteDeviceKey(group: String?) {
        SecItemDelete(query("device-key", group: group) as CFDictionary)
    }

    public static func deleteAll(group: String?) {
        deleteDeviceKey(group: group)
        SecItemDelete(query("file-key", group: group) as CFDictionary)
    }

    public static var biometricsAvailable: Bool {
        LAContext().canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil)
    }
}

public struct KeychainError: Error {
    public let status: OSStatus
}
