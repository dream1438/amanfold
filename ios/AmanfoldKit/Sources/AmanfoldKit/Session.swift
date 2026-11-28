import Amanfold
import Foundation

/// Settings that come from the build, not from the person.
public struct ClientConfig {
    public var deviceName: String
    /// Pinned OPAQUE public keys (base64url) of servers this build trusts without asking.
    public var pinnedServerKeys: [String: String]
    /// Allow `http://` for this computer (simulator and tests only).
    public var allowLocalHttp: Bool

    public init(deviceName: String, pinnedServerKeys: [String: String] = [:], allowLocalHttp: Bool = false) {
        self.deviceName = deviceName
        self.pinnedServerKeys = pinnedServerKeys
        self.allowLocalHttp = allowLocalHttp
    }
}

public struct RecoveryKit: Equatable {
    public let email: String
    public let secretKey: String
    public let recoveryWords: String
}

public enum VaultState { case noAccount, locked, unlocked }

public enum SyncProblem { case offline, signedOut, rollback, conflict, other }

public struct LockedError: Error {}
public struct ServerKeyError: Error {}
public struct SecretKeyError: Error {}

/// Runs async blocks one at a time.
actor SerialQueue {
    private var tail: Task<Void, Never>?

    func run<T>(_ body: @escaping () async throws -> T) async throws -> T {
        let previous = tail
        let task = Task<T, Error> {
            await previous?.value
            return try await body()
        }
        tail = Task { _ = try? await task.value }
        return try await task.value
    }
}

/// Runs CPU-heavy core work (Argon2id) off the main thread.
func background<T>(_ body: @escaping () throws -> T) async throws -> T {
    try await Task.detached(priority: .userInitiated) { try body() }.value
}

/// The account and vault on this device. Keys stay inside the Rust core; this
/// object holds the core's objects while unlocked and drops them on `lock()`.
/// Every vault operation runs through one serial queue, so a sync never
/// interleaves with an edit or a lock.
@MainActor
public final class AmanfoldSession: ObservableObject {
    private struct Open {
        let account: Account
        let vault: LocalVault
        let me: Data
    }

    private let store: AccountStore
    private let config: ClientConfig
    private let queue = SerialQueue()
    private var open: Open?

    @Published public private(set) var state: VaultState = .noAccount
    @Published public private(set) var lastSyncMs: Int64?
    @Published public private(set) var syncProblem: SyncProblem?
    /// Bumped after every change to the items, so lists refresh.
    @Published public private(set) var revision = 0

    public init(store: AccountStore, config: ClientConfig) {
        self.store = store
        self.config = config
        lastSyncMs = store.load()?.lastSyncMs
        publish()
    }

    private func publish() {
        state = open != nil ? .unlocked : (store.load() != nil ? .locked : .noAccount)
        revision += 1
    }

    private func stored() throws -> StoredAccount {
        guard let s = store.load() else { throw LockedError() }
        return s
    }

    private func need() throws -> Open {
        guard let o = open else { throw LockedError() }
        return o
    }

    public var accountInfo: (email: String, server: String)? {
        store.load().map { ($0.email, $0.server) }
    }

    public var hasQuickUnlock: Bool { store.load()?.quickWrapped != nil }
    public var isUnlocked: Bool { open != nil }

    // MARK: Locking

    public func unlock(password: String) async throws {
        try await queue.run { @MainActor in
            let s = try self.stored()
            let bundle = try B64.decode(s.bundle)
            let account = try await background { try unlockAccount(bundle: bundle, password: password, secretKey: s.secretKey) }
            self.replaceOpen(try self.openVault(account, s))
        }
    }

    /// Unlock with the device key released by Face ID / Touch ID.
    public func quickUnlock(deviceKey: Data) async throws {
        try await queue.run { @MainActor in
            let s = try self.stored()
            guard let wrapped = s.quickWrapped else { throw LockedError() }
            let account = try coreQuickUnlock(bundle: try B64.decode(s.bundle), deviceKey: deviceKey, wrappedKeys: try B64.decode(wrapped))
            self.replaceOpen(try self.openVault(account, s))
        }
    }

    /// Turn quick unlock on: returns the device key for the Keychain (behind
    /// biometrics); the wrapped account keys stay in the account file.
    public func enableQuickUnlock() async throws -> Data {
        try await queue.run { @MainActor in
            let q = try self.need().account.enableQuickUnlock()
            var s = try self.stored()
            s.quickWrapped = B64.encode(q.wrappedKeys)
            try self.store.save(s)
            return q.deviceKey
        }
    }

    public func disableQuickUnlock() {
        if var s = store.load() {
            s.quickWrapped = nil
            try? store.save(s)
        }
    }

    public func lock() {
        open = nil
        publish()
    }

    private func replaceOpen(_ o: Open) {
        open = o
        publish()
    }

    private func openVault(_ account: Account, _ s: StoredAccount) throws -> Open {
        let me = account.publicKeys()
        let cache = try s.cache.map { try B64.decode($0) }
        let vault = try account.openVault(grant: try B64.decode(s.grant), granterPublicKeys: me, cache: cache)
        return Open(account: account, vault: vault, me: me)
    }

    // MARK: Sign-up, sign-in, recovery

    private func api(_ server: String) throws -> Api {
        Api(base: try checkServerUrl(server, allowLocalHttp: config.allowLocalHttp))
    }

    private func pinnedKey(_ api: Api) async throws -> String {
        let key = try await api.get("/v1/server").need("opaque_public_key")
        if let pinned = config.pinnedServerKeys[api.base] {
            guard pinned == key else { throw ServerKeyError() }
            return pinned
        }
        return key
    }

    public struct ServerInfo { public let emailVerification: Bool; public let version: String }

    public func serverInfo(_ server: String) async throws -> ServerInfo {
        let r = try await api(server).get("/v1/server")
        return ServerInfo(emailVerification: r["email_verification"] as? Bool ?? true, version: r["version"] as? String ?? "")
    }

    public func requestSignupCode(server: String, email: String) async throws {
        _ = try await api(server).post("/v1/signup/email", ["email": email])
    }

    /// Create the account and its personal vault. Show the returned kit once.
    public func signup(server: String, email: String, password: String, code: String?) async throws -> RecoveryKit {
        try await queue.run { @MainActor in
            guard self.store.load() == nil else { throw LockedError() }
            let api = try self.api(server)
            let serverKey = try await self.pinnedKey(api)
            let created = try await background { try createAccount(password: password) }
            let account = created.account
            let reg = created.registration
            var start: JSON = ["email": email, "account_id": account.accountId(), "request": B64.encode(try reg.request())]
            if let code { start["code"] = code }
            let r = try await api.post("/v1/signup/start", start)
            let upload = try reg.finish(response: try B64.decode(try r.need("response")))
            var finish: JSON = ["email": email, "upload": B64.encode(upload), "bundle": B64.encode(created.bundle), "device_name": self.config.deviceName]
            if let code { finish["code"] = code }
            let fin = try await api.post("/v1/signup/finish", finish)
            api.token = try fin.need("api_token")
            let nv = try account.createVault()
            _ = try await api.post("/v1/vaults", ["grant": B64.encode(nv.grant)])
            let s = StoredAccount(
                server: api.base, email: email, accountId: try fin.need("account_id"),
                bundle: B64.encode(created.bundle), secretKey: created.secretKey, serverKey: serverKey,
                token: api.token!, vaultId: nv.vault.id(), grant: B64.encode(nv.grant),
                cache: B64.encode(nv.vault.exportCache()), lastSyncMs: Self.nowMs()
            )
            try self.store.save(s)
            self.lastSyncMs = s.lastSyncMs
            self.replaceOpen(Open(account: account, vault: nv.vault, me: account.publicKeys()))
            return RecoveryKit(email: email, secretKey: created.secretKey, recoveryWords: created.recoveryWords)
        }
    }

    /// Sign in on this device with the master password and the Secret Key.
    public func signin(server: String, email: String, password: String, secretKey secretKeyText: String) async throws {
        try await queue.run { @MainActor in
            guard self.store.load() == nil else { throw LockedError() }
            let secretKey: String
            do { secretKey = try checkSecretKey(text: secretKeyText) } catch { throw SecretKeyError() }
            let api = try self.api(server)
            let serverKey = try await self.pinnedKey(api)
            let pre = try B64.decode(try await api.post("/v1/prelogin", ["email": email]).need("prelogin"))
            let session = try await background { try LoginSession(prelogin: pre, password: password, secretKey: secretKey) }
            let start = try await api.post("/v1/login/start", ["email": email, "request": B64.encode(try session.start())])
            let response = try B64.decode(try start.need("response"))
            let pin = try B64.decode(serverKey)
            let out = try await background { try session.finish(response: response, pinnedServerKey: pin) }
            let fin = try await api.post("/v1/login/finish", [
                "login_id": try start.need("login_id"),
                "finalization": B64.encode(out.finalization),
                "device_name": self.config.deviceName,
            ])
            let bundle = try fin.need("bundle")
            let account = try session.unlock(bundle: try B64.decode(bundle))
            api.token = B64.encode(out.apiToken)
            let vaults = (try await api.get("/v1/vaults")["vaults"] as? [JSON]) ?? []
            guard let mine = vaults.first(where: { $0["role"] as? String == "owner" }) else {
                throw ApiError(status: 200, kind: "no_vault", message: "this account has no vault yet")
            }
            let vaultId = try mine.need("vault_id")
            var s = StoredAccount(
                server: api.base, email: email, accountId: try fin.need("account_id"), bundle: bundle,
                secretKey: secretKey, serverKey: serverKey, token: api.token!, vaultId: vaultId,
                grant: try await self.newestGrant(api, vaultId)
            )
            let o = try self.openVault(account, s)
            s.cache = B64.encode(o.vault.exportCache())
            try self.store.save(s)
            self.replaceOpen(o)
        }
        try? await sync()
    }

    private func newestGrant(_ api: Api, _ vaultId: String) async throws -> String {
        let grants = (try await api.get("/v1/vaults/\(vaultId)")["grants"] as? [JSON]) ?? []
        guard let last = grants.last else { throw ApiError(status: 200, kind: "no_grant", message: "no vault key") }
        return try last.need("grant")
    }

    public func requestRecoveryCode(server: String, email: String) async throws {
        _ = try await api(server).post("/v1/recovery/email", ["email": email])
    }

    /// Forgotten master password: the emailed code plus the whole recovery
    /// kit. Every device is signed out; this one then signs in with the new password.
    public func recover(server: String, email: String, code: String?, recoveryWords: String,
                        secretKey secretKeyText: String, newPassword: String) async throws {
        guard store.load() == nil else { throw LockedError() }
        let secretKey: String
        do { secretKey = try checkSecretKey(text: secretKeyText) } catch { throw SecretKeyError() }
        let api = try self.api(server)
        _ = try await pinnedKey(api)
        var startBody: JSON = ["email": email]
        if let code { startBody["code"] = code }
        let start = try await api.post("/v1/recovery/start", startBody)
        let challenge = try B64.decode(try start.need("challenge"))
        let bundle = try B64.decode(try start.need("bundle"))
        let acct = try recoverAccount(bundle: bundle, recoveryWords: recoveryWords)
        let change = try await background { try acct.changePassword(newPassword: newPassword, secretKey: secretKey) }
        let signature = try acct.signChallenge(purpose: "recover", challenge: challenge)
        var reg: JSON = ["email": email, "request": B64.encode(try change.registration.request())]
        if let code { reg["code"] = code }
        let r = try await api.post("/v1/recovery/register", reg)
        var fin: JSON = [
            "email": email,
            "upload": B64.encode(try change.registration.finish(response: try B64.decode(try r.need("response")))),
            "bundle": B64.encode(change.bundle),
            "challenge": B64.encode(challenge),
            "signature": B64.encode(signature),
        ]
        if let code { fin["code"] = code }
        _ = try await api.post("/v1/recovery/finish", fin)
        try await signin(server: server, email: email, password: newPassword, secretKey: secretKeyText)
    }

    /// Sign this device out and forget the account here.
    public func signout() async {
        _ = try? await queue.run { @MainActor in
            if let s = self.store.load() {
                _ = try? await Api(base: s.server, token: s.token).delete("/v1/sessions/current")
            }
            self.open = nil
            self.store.clear()
            self.lastSyncMs = nil
            self.syncProblem = nil
            self.publish()
        }
    }

    // MARK: Items

    public func items(trashed: Bool = false) -> [ItemView] {
        guard let o = open else { return [] }
        return o.vault.items(includeTrashed: trashed).filter { $0.trashed == trashed }
    }

    public func item(_ id: String) -> ItemView? { try? open?.vault.item(id: id) }

    /// The only logins AutoFill may offer on this page, best match first.
    public func logins(forPage url: String) -> [LoginMatch] { open?.vault.loginsForUrl(pageUrl: url) ?? [] }

    /// Saved sites this page imitates, for a warning.
    public func lookalikes(forPage url: String) -> [String] { open?.vault.lookalikeSites(pageUrl: url) ?? [] }

    @discardableResult
    public func create(_ kind: ItemKind, _ fields: [FieldInput]) async throws -> String {
        let id = try await queue.run { @MainActor in
            let saved = try self.need().vault.createItem(kind: kind, fields: fields)
            try self.saveLocal(saved)
            return saved.id
        }
        try? await sync()
        return id
    }

    public func update(_ id: String, set: [FieldInput], remove: [String] = []) async throws {
        try await queue.run { @MainActor in
            try self.saveLocal(try self.need().vault.updateItem(id: id, set: set, remove: remove))
        }
        try? await sync()
    }

    public func trash(_ id: String, _ trashed: Bool) async throws {
        try await queue.run { @MainActor in
            try self.saveLocal(try self.need().vault.setTrashed(id: id, trashed: trashed))
        }
        try? await sync()
    }

    private func saveLocal(_ saved: SavedItem) throws {
        let o = try need()
        var s = try stored()
        s.pending[saved.id] = B64.encode(saved.record)
        s.cache = B64.encode(o.vault.exportCache())
        try store.save(s)
        revision += 1
    }

    // MARK: Sync

    /// One round of the sync loop in docs/SERVER.md, retried on a conflict.
    public func sync() async throws {
        try await queue.run { @MainActor in
            guard self.open != nil else { return }
            for _ in 0..<4 {
                do {
                    try await self.syncOnce()
                    self.syncProblem = nil
                    self.lastSyncMs = self.store.load()?.lastSyncMs
                    self.revision += 1
                    return
                } catch let e as ApiError {
                    if e.status == 409 { continue }
                    self.syncProblem = e.status == 401 ? .signedOut : (e.status == 0 ? .offline : .other)
                    throw e
                } catch let e as AmanfoldError {
                    switch e {
                    case .Rollback, .BadSignature: self.syncProblem = .rollback
                    default: self.syncProblem = .other
                    }
                    throw e
                }
            }
            self.syncProblem = .conflict
        }
    }

    private func syncOnce() async throws {
        let o = try need()
        var s = try stored()
        let api = Api(base: s.server, token: s.token)
        let r = try await api.get("/v1/vaults/\(s.vaultId)/sync?since=\(s.seq)")
        if (r.int64("epoch") ?? 0) > Int64(o.vault.epoch()) {
            let grant = try await newestGrant(api, s.vaultId)
            try o.account.acceptRotation(vault: o.vault, grant: try B64.decode(grant), granterPublicKeys: o.me)
            s.grant = grant
        }
        if let manifest = r["manifest"] as? String {
            let records = try ((r["records"] as? [String]) ?? []).map { try B64.decode($0) }
            for up in try o.vault.sync(manifest: try B64.decode(manifest), changed: records, trustedWriters: [o.me]) {
                s.pending[up.id] = B64.encode(up.record)
            }
        }
        s.cache = B64.encode(o.vault.exportCache())
        if s.pending.isEmpty {
            s.seq = r.int64("seq") ?? s.seq
            s.lastSyncMs = Self.nowMs()
            try store.save(s)
            return
        }
        try store.save(s)
        let signed = o.vault.signManifest(author: o.account)
        let done = try await api.post("/v1/vaults/\(s.vaultId)/commit", [
            "manifest": B64.encode(signed),
            "records": Array(s.pending.values),
        ])
        try o.vault.commitAccepted()
        s.seq = done.int64("seq") ?? s.seq
        s.pending = [:]
        s.cache = B64.encode(o.vault.exportCache())
        s.lastSyncMs = Self.nowMs()
        try store.save(s)
    }

    static func nowMs() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }
}

/// The core's quick unlock, outside the class so its method does not shadow it.
private func coreQuickUnlock(bundle: Data, deviceKey: Data, wrappedKeys: Data) throws -> Account {
    try quickUnlock(bundle: bundle, deviceKey: deviceKey, wrappedKeys: wrappedKeys)
}
