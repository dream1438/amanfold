import Foundation
#if canImport(UIKit)
import UIKit
#endif

/// Set-up shared by the app and the AutoFill extension: both open the same
/// encrypted account file (App Group container) with the same Keychain key.
public enum Shared {
    public static let appGroup = "group.com.amanfold.app"

    /// `$(AppIdentifierPrefix)com.amanfold.shared`, from Info.plist. `nil` in
    /// unsigned builds (simulator CI), where the default group is used.
    public static var keychainGroup: String? {
        guard let g = Bundle.main.object(forInfoDictionaryKey: "AmanfoldKeychainGroup") as? String,
              !g.isEmpty, !g.contains("$(")
        else { return nil }
        return g
    }

    public static var defaultServer: String {
        Bundle.main.object(forInfoDictionaryKey: "AmanfoldServer") as? String ?? "https://api.amanfold.com"
    }

    public static var allowLocalHttp: Bool {
        #if DEBUG
        return true
        #else
        return false
        #endif
    }

    public static var defaults: UserDefaults { UserDefaults(suiteName: appGroup) ?? .standard }

    static var accountFile: URL {
        let base = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Amanfold/account.bin")
    }

    @MainActor
    public static func makeSession() -> AmanfoldSession {
        #if canImport(UIKit)
        let device = "Amanfold for \(UIDevice.current.model)"
        #else
        let device = "Amanfold for Mac"
        #endif
        let store: AccountStore
        if let key = try? Keychain.fileKey(group: keychainGroup) {
            store = FileStore(url: accountFile, key: key)
        } else {
            // No Keychain (should not happen on a phone): keep nothing on disk.
            store = MemoryStore()
        }
        let s = AmanfoldSession(store: store, config: ClientConfig(deviceName: device, allowLocalHttp: allowLocalHttp))
        if !Keychain.hasDeviceKey(group: keychainGroup) { s.disableQuickUnlock() }
        return s
    }
}

public enum QuickResult { case unlocked, cancelled, changed }

extension AmanfoldSession {
    /// Unlock with Face ID / Touch ID.
    public func unlockWithBiometrics() async -> QuickResult {
        let group = Shared.keychainGroup
        let reason = L("ios_face_id_reason")
        guard hasQuickUnlock, Keychain.hasDeviceKey(group: group) else {
            disableQuickUnlock()
            return .changed
        }
        let key = await Task.detached { Keychain.readDeviceKey(reason: reason, group: group) }.value
        guard let key else { return .cancelled }
        do {
            try await quickUnlock(deviceKey: key)
            return .unlocked
        } catch {
            Keychain.deleteDeviceKey(group: group)
            disableQuickUnlock()
            return .changed
        }
    }

    /// Turn on Face ID / Touch ID unlock (the vault must be unlocked).
    public func turnOnBiometrics() async throws {
        let key = try await enableQuickUnlock()
        do {
            try Keychain.saveDeviceKey(key, group: Shared.keychainGroup)
        } catch {
            disableQuickUnlock()
            throw error
        }
    }

    public func turnOffBiometrics() {
        Keychain.deleteDeviceKey(group: Shared.keychainGroup)
        disableQuickUnlock()
    }

    public var biometricsOn: Bool { hasQuickUnlock && Keychain.hasDeviceKey(group: Shared.keychainGroup) }
}
