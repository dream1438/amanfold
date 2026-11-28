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
    private final class Open {
        let account: Account
        let me: Data
        let meB64: String
        let personalId: String
        var vaults: [String: LocalVault] = [:]

        init(account: Account, personalId: String) {
            self.account = account
            me = account.publicKeys()
            meB64 = B64.encode(me)
            self.personalId = personalId
        }

        /// Personal first, then shared vaults by id.
        var ids: [String] { [personalId] + vaults.keys.filter { $0 != personalId }.sorted() }
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
            self.replaceOpen(try self.openAll(account, s))
        }
    }

    /// Unlock with the device key released by Face ID / Touch ID.
    public func quickUnlock(deviceKey: Data) async throws {
        try await queue.run { @MainActor in
            let s = try self.stored()
            guard let wrapped = s.quickWrapped else { throw LockedError() }
            let account = try coreQuickUnlock(bundle: try B64.decode(s.bundle), deviceKey: deviceKey, wrappedKeys: try B64.decode(wrapped))
            self.replaceOpen(try self.openAll(account, s))
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

    private func granterOf(_ o: Open, _ v: StoredVault) throws -> Data {
        v.granter.isEmpty ? o.me : try B64.decode(v.granter)
    }

    private func reopen(_ o: Open, _ v: StoredVault) throws -> LocalVault {
        let cache = try v.cache.map { try B64.decode($0) }
        return try o.account.openVault(grant: try B64.decode(v.grant), granterPublicKeys: try granterOf(o, v), cache: cache)
    }

    /// Open every vault this device has. The personal vault must open; a
    /// shared one that fails is dropped and opened again from the server on
    /// the next sync.
    private func openAll(_ account: Account, _ s: StoredAccount) throws -> Open {
        let o = Open(account: account, personalId: s.personalId)
        var dropped: [String] = []
        for v in s.vaults.values {
            do {
                o.vaults[v.id] = try reopen(o, v)
            } catch {
                if v.id == s.personalId { throw error }
                dropped.append(v.id)
            }
        }
        guard o.vaults[s.personalId] != nil else { throw LockedError() }
        if !dropped.isEmpty {
            var n = s
            for d in dropped { n.vaults[d] = nil }
            try? store.save(n)
        }
        return o
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
            let id = nv.vault.id()
            let s = StoredAccount(
                server: api.base, email: email, accountId: try fin.need("account_id"),
                bundle: B64.encode(created.bundle), secretKey: created.secretKey, serverKey: serverKey,
                token: api.token!, personalId: id,
                vaults: [id: StoredVault(id: id, role: "owner", grant: B64.encode(nv.grant), cache: B64.encode(nv.vault.exportCache()))],
                lastSyncMs: Self.nowMs()
            )
            try self.store.save(s)
            self.lastSyncMs = s.lastSyncMs
            let o = Open(account: account, personalId: id)
            o.vaults[id] = nv.vault
            self.replaceOpen(o)
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
            // The personal vault: the first one this account owns. Shared
            // vaults are picked up by the sync below.
            let vaults = (try await api.get("/v1/vaults")["vaults"] as? [JSON]) ?? []
            guard let mine = vaults.first(where: { $0["role"] as? String == "owner" }) else {
                throw ApiError(status: 200, kind: "no_vault", message: "this account has no vault yet")
            }
            let vaultId = try mine.need("vault_id")
            var s = StoredAccount(
                server: api.base, email: email, accountId: try fin.need("account_id"), bundle: bundle,
                secretKey: secretKey, serverKey: serverKey, token: api.token!, personalId: vaultId,
                vaults: [vaultId: StoredVault(id: vaultId, role: "owner", grant: try await self.newestGrant(api, vaultId))]
            )
            let o = try self.openAll(account, s)
            s.vaults[vaultId]?.cache = B64.encode(o.vaults[vaultId]!.exportCache())
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

    // MARK: Items, across the personal vault and every shared vault

    private func info(_ v: LocalVault) -> (name: String, kind: VaultKind) {
        let note = v.items(includeTrashed: false).first(where: isVaultInfo)
        let kind: VaultKind = note?.fields.first { $0.name == vaultInfoField }?.value == "team" ? .team : .family
        return (note?.fields.first { $0.name == "title" }?.value ?? "", kind)
    }

    private func tag(_ o: Open, _ s: StoredAccount, _ id: String) -> VaultTag {
        let role = Role.of(s.vaults[id]?.role)
        if id == o.personalId { return VaultTag(id: id, name: "", kind: .personal, role: role) }
        let i: (name: String, kind: VaultKind) = o.vaults[id].map { info($0) } ?? (name: "", kind: .family)
        return VaultTag(id: id, name: i.name, kind: i.kind, role: role)
    }

    /// The vaults items can be shown from, personal first.
    public func vaults() -> [VaultTag] {
        guard let o = open, let s = store.load() else { return [] }
        return o.ids.map { tag(o, s, $0) }
    }

    public func items(trashed: Bool = false) -> [ItemView] {
        guard let o = open else { return [] }
        return o.ids.compactMap { o.vaults[$0] }.flatMap { v in
            v.items(includeTrashed: trashed).filter { $0.trashed == trashed && !isVaultInfo($0) }
        }
    }

    private func locate(_ id: String) -> (vault: String, item: ItemView)? {
        guard let o = open else { return nil }
        for vid in o.ids {
            if let i = try? o.vaults[vid]?.item(id: id), !isVaultInfo(i) { return (vid, i) }
        }
        return nil
    }

    public func item(_ id: String) -> ItemView? { locate(id)?.item }

    /// The vault an item is in.
    public func vaultOf(_ itemId: String) -> VaultTag? {
        guard let o = open, let s = store.load(), let l = locate(itemId) else { return nil }
        return tag(o, s, l.vault)
    }

    private static func rank(_ m: SiteMatch) -> Int {
        switch m {
        case .none: return 0
        case .site: return 1
        case .host: return 2
        case .origin: return 3
        }
    }

    /// The only logins AutoFill may offer on this page, from every vault, best match first.
    public func logins(forPage url: String) -> [LoginMatch] {
        guard let o = open else { return [] }
        let all = o.ids.compactMap { o.vaults[$0] }.flatMap { $0.loginsForUrl(pageUrl: url) }.filter { !isVaultInfo($0.item) }
        // A stable sort: vault order is kept within the same strength.
        return all.enumerated().sorted { a, b in
            let ra = Self.rank(a.element.strength), rb = Self.rank(b.element.strength)
            return ra != rb ? ra > rb : a.offset < b.offset
        }.map(\.element)
    }

    /// Saved sites this page imitates, for a warning.
    public func lookalikes(forPage url: String) -> [String] {
        guard let o = open else { return [] }
        var seen = Set<String>()
        return o.ids.compactMap { o.vaults[$0] }.flatMap { $0.lookalikeSites(pageUrl: url) }.filter { seen.insert($0).inserted }
    }

    private func writable(_ vaultId: String) throws {
        guard let sv = try stored().vaults[vaultId] else { throw AmanfoldError.NotFound }
        if !Role.of(sv.role).canWrite { throw ReadOnlyError() }
    }

    private func vault(_ id: String) throws -> LocalVault {
        guard let v = try need().vaults[id] else { throw AmanfoldError.NotFound }
        return v
    }

    /// A new item, in the personal vault unless `vaultId` names a shared one.
    @discardableResult
    public func create(_ kind: ItemKind, _ fields: [FieldInput], in vaultId: String? = nil) async throws -> String {
        let id = try await queue.run { @MainActor in
            let target = try vaultId ?? self.need().personalId
            try self.writable(target)
            let saved = try self.vault(target).createItem(kind: kind, fields: fields)
            try self.saveLocal(target, saved)
            return saved.id
        }
        try? await sync()
        return id
    }

    public func update(_ id: String, set: [FieldInput], remove: [String] = []) async throws {
        try await queue.run { @MainActor in
            guard let l = self.locate(id) else { throw AmanfoldError.NotFound }
            try self.writable(l.vault)
            try self.saveLocal(l.vault, try self.vault(l.vault).updateItem(id: id, set: set, remove: remove))
        }
        try? await sync()
    }

    public func trash(_ id: String, _ trashed: Bool) async throws {
        try await queue.run { @MainActor in
            guard let l = self.locate(id) else { throw AmanfoldError.NotFound }
            try self.writable(l.vault)
            try self.saveLocal(l.vault, try self.vault(l.vault).setTrashed(id: id, trashed: trashed))
        }
        try? await sync()
    }

    /// Copy an item into another vault and move the original to the trash.
    @discardableResult
    public func move(_ id: String, to vaultId: String) async throws -> String {
        let newId = try await queue.run { @MainActor in
            guard let l = self.locate(id) else { throw AmanfoldError.NotFound }
            if l.vault == vaultId { return id }
            try self.writable(l.vault)
            try self.writable(vaultId)
            let fields = l.item.fields.map { FieldInput(name: $0.name, value: $0.value) }
            let copy = try self.vault(vaultId).createItem(kind: l.item.kind, fields: fields)
            try self.saveLocal(vaultId, copy)
            try self.saveLocal(l.vault, try self.vault(l.vault).setTrashed(id: id, trashed: true))
            return copy.id
        }
        try? await sync()
        return newId
    }

    private func saveLocal(_ vaultId: String, _ saved: SavedItem) throws {
        let v = try vault(vaultId)
        var s = try stored()
        guard var sv = s.vaults[vaultId] else { throw AmanfoldError.NotFound }
        sv.pending[saved.id] = B64.encode(saved.record)
        sv.cache = B64.encode(v.exportCache())
        s.vaults[vaultId] = sv
        try store.save(s)
        revision += 1
    }

    private func putVault(_ v: StoredVault) throws {
        var s = try stored()
        s.vaults[v.id] = v
        try store.save(s)
    }

    private func dropVault(_ o: Open, _ id: String) throws {
        o.vaults[id] = nil
        var s = try stored()
        s.vaults[id] = nil
        try store.save(s)
        revision += 1
    }

    // MARK: Sync

    /// Vaults shared with this account by someone not verified on this device.
    private var waiting: [WaitingView] = []

    /// Shared vaults whose last sync carried changes or a new key signed by
    /// someone not verified on this device. They stay as they were until the
    /// person compares that member's safety number.
    private var unverifiedChanges: Set<String> = []

    /// One round of the sync loop in docs/SERVER.md for every vault: pick up
    /// vaults shared with this account, sync each (retrying on a conflict),
    /// and rotate the key of a vault someone was removed from. A shared vault
    /// that fails does not stop the others; the first failure is reported.
    public func sync() async throws {
        try await queue.run { @MainActor in
            guard let o = self.open else { return }
            do {
                let s = try self.stored()
                let api = Api(base: s.server, token: s.token)
                let rows = try await self.discover(o, api)
                var first: Error?
                var blocked = Set<String>()
                for id in o.ids {
                    do {
                        try await self.syncVault(o, id, api)
                        let row = rows.first { $0.str("vault_id") == id }
                        if row?.bool("needs_rotation") == true, Role.of(try self.stored().vaults[id]?.role).canManage {
                            try await self.rotate(o, id, api)
                        }
                    } catch AmanfoldError.BadSignature where id != o.personalId {
                        blocked.insert(id)
                    } catch {
                        if id == o.personalId { throw error }
                        if first == nil { first = error }
                    }
                }
                self.unverifiedChanges = blocked
                if let first { throw first }
                var done = try self.stored()
                done.lastSyncMs = Self.nowMs()
                try self.store.save(done)
                self.syncProblem = nil
                self.lastSyncMs = done.lastSyncMs
                self.revision += 1
            } catch let e as ApiError {
                switch e.status {
                case 409: self.syncProblem = .conflict
                case 401: self.syncProblem = .signedOut
                case 0: self.syncProblem = .offline
                default: self.syncProblem = .other
                }
                self.revision += 1
                throw e
            } catch let e as AmanfoldError {
                switch e {
                case .Rollback, .BadSignature: self.syncProblem = .rollback
                default: self.syncProblem = .other
                }
                self.revision += 1
                throw e
            }
        }
    }

    /// Is this public key this account, or someone whose safety number was compared?
    private func trusted(_ o: Open, _ s: StoredAccount, _ keys: String) -> Bool {
        keys == o.meB64 || s.contacts.values.contains { $0.keys == keys }
    }

    private func writers(_ o: Open, _ s: StoredAccount) throws -> [Data] {
        [o.me] + (try s.contacts.values.map { try B64.decode($0.keys) })
    }

    private func memberRows(_ api: Api, _ vaultId: String) async throws -> [JSON] {
        (try await api.get("/v1/vaults/\(vaultId)/members")["members"] as? [JSON]) ?? []
    }

    /// Open vaults newly shared with this account; drop ones it left.
    private func discover(_ o: Open, _ api: Api) async throws -> [JSON] {
        let rows = (try await api.get("/v1/vaults")["vaults"] as? [JSON]) ?? []
        var nextWaiting: [WaitingView] = []
        for row in rows {
            let id = try row.need("vault_id")
            if var have = try stored().vaults[id] {
                let role = try row.need("role")
                if role != have.role {
                    have.role = role
                    try putVault(have)
                }
                continue
            }
            if !row.bool("active") { continue }
            let grant = try await newestGrant(api, id)
            let granterKeys = try grantGranter(grant: try B64.decode(grant))
            let granter = B64.encode(granterKeys)
            if !trusted(o, try stored(), granter) {
                // Opened only after the person compares the sharer's safety number.
                if let m = try await memberRows(api, id).first(where: { $0.str("public_keys") == granter }) {
                    nextWaiting.append(WaitingView(
                        vaultId: id, accountId: try m.need("account_id"), email: try m.need("email"),
                        fingerprint: try safetyNumber(granterKeys)
                    ))
                }
                continue
            }
            let v = try o.account.openVault(grant: try B64.decode(grant), granterPublicKeys: granterKeys, cache: nil)
            o.vaults[id] = v
            try putVault(StoredVault(
                id: id, role: try row.need("role"), grant: grant,
                granter: granter == o.meB64 ? "" : granter, cache: B64.encode(v.exportCache())
            ))
        }
        for id in try stored().vaults.keys where id != o.personalId && !rows.contains(where: { $0.str("vault_id") == id }) {
            // Removed from it, or it was deleted.
            try dropVault(o, id)
        }
        waiting = nextWaiting
        return rows
    }

    private func syncVault(_ o: Open, _ id: String, _ api: Api) async throws {
        for _ in 0..<4 {
            do {
                try await syncOnce(o, id, api)
                return
            } catch let e as ApiError where e.status == 409 {
                continue
            }
        }
        throw ApiError(status: 409, kind: "conflict", message: "another device keeps writing; try again")
    }

    private func syncOnce(_ o: Open, _ id: String, _ api: Api) async throws {
        let s = try stored()
        guard var sv = s.vaults[id], let v = o.vaults[id] else { return }
        let r = try await api.get("/v1/vaults/\(id)/sync?since=\(sv.seq)")
        if (r.int64("epoch") ?? 0) > Int64(v.epoch()) {
            // Someone removed a member and moved the vault to a new key.
            let grant = try await newestGrant(api, id)
            let granterKeys = try grantGranter(grant: try B64.decode(grant))
            let granter = B64.encode(granterKeys)
            if !trusted(o, s, granter) { throw AmanfoldError.BadSignature }
            try o.account.acceptRotation(vault: v, grant: try B64.decode(grant), granterPublicKeys: granterKeys)
            sv.grant = grant
            sv.granter = granter == o.meB64 ? "" : granter
        }
        let canWrite = Role.of(sv.role).canWrite
        var pending = sv.pending
        if let manifest = r["manifest"] as? String {
            let records = try ((r["records"] as? [String]) ?? []).map { try B64.decode($0) }
            for up in try v.sync(manifest: try B64.decode(manifest), changed: records, trustedWriters: try writers(o, s)) where canWrite {
                pending[up.id] = B64.encode(up.record)
            }
        }
        sv.cache = B64.encode(v.exportCache())
        if pending.isEmpty || !canWrite {
            // A reader's edits would be refused; they never leave this device.
            sv.pending = [:]
            sv.seq = r.int64("seq") ?? sv.seq
            try putVault(sv)
            return
        }
        sv.pending = pending
        try putVault(sv)
        let signed = v.signManifest(author: o.account)
        let done = try await api.post("/v1/vaults/\(id)/commit", [
            "manifest": B64.encode(signed),
            "records": Array(pending.values),
        ])
        try v.commitAccepted()
        sv.seq = done.int64("seq") ?? sv.seq
        sv.pending = [:]
        sv.cache = B64.encode(v.exportCache())
        try putVault(sv)
    }

    /// New vault key after a member left or was removed, so they cannot read
    /// anything written from now on. Every remaining member must have been
    /// verified on this device, since each gets the new key.
    private func rotate(_ o: Open, _ id: String, _ api: Api) async throws {
        let s = try stored()
        guard var sv = s.vaults[id], let v = o.vaults[id] else { throw AmanfoldError.NotFound }
        var keys: [String] = []
        for m in try await memberRows(api, id) where m.bool("active") {
            if m.str("account_id") == s.accountId {
                keys.append(o.meB64)
            } else if let k = m.str("public_keys"), let acct = m.str("account_id"), s.contacts[acct]?.keys == k {
                keys.append(k)
            } else {
                throw UnverifiedMembersError()
            }
        }
        let records = try v.rotateKey()
        do {
            let grants = try keys.map { B64.encode(try o.account.grantVault(vault: v, recipientPublicKeys: try B64.decode($0))) }
            let manifest = v.signManifest(author: o.account)
            let r = try await api.post("/v1/vaults/\(id)/rotate", [
                "manifest": B64.encode(manifest),
                "records": records.map { B64.encode($0.record) },
                "grants": grants,
            ])
            try v.commitAccepted()
            sv.grant = grants[keys.firstIndex(of: o.meB64)!]
            sv.granter = ""
            sv.seq = r.int64("seq") ?? sv.seq
            sv.pending = [:]
            sv.cache = B64.encode(v.exportCache())
            try putVault(sv)
        } catch {
            // Not accepted: go back to the old key, which the server still has.
            o.vaults[id] = nil
            if let back = try? reopen(o, sv) {
                o.vaults[id] = back
            } else {
                // Opened again from the server on the next sync.
                var now = try stored()
                now.vaults[id] = nil
                try store.save(now)
            }
            throw error
        }
    }

    // MARK: Sharing

    private func apiNow() throws -> Api {
        let s = try stored()
        return Api(base: s.server, token: s.token)
    }

    /// Shared vaults, invitations, vaults waiting for a safety-number check,
    /// and this account's safety number.
    public func sharing() async throws -> SharingView {
        try await queue.run { @MainActor in
            let o = try self.need()
            let invites = (try await self.apiNow().get("/v1/invites")["invites"] as? [JSON]) ?? []
            let s = try self.stored()
            return SharingView(
                vaults: o.ids.filter { $0 != o.personalId }.compactMap { id in
                    guard let v = o.vaults[id] else { return nil }
                    let i = self.info(v)
                    return SharedVaultView(
                        id: id, name: i.name, kind: i.kind, role: Role.of(s.vaults[id]?.role),
                        items: v.items(includeTrashed: false).filter { !isVaultInfo($0) }.count,
                        needsVerification: self.unverifiedChanges.contains(id)
                    )
                },
                invites: try invites.map { i in
                    let from = i["from"] as? JSON ?? [:]
                    return InviteView(
                        inviteId: try i.need("invite_id"), vaultId: try i.need("vault_id"), role: Role.of(i.str("role")),
                        fromEmail: try from.need("email"), fromFingerprint: try safetyNumber(try B64.decode(try from.need("public_keys")))
                    )
                },
                waiting: self.waiting,
                myFingerprint: o.account.fingerprint()
            )
        }
    }

    /// A new family or team vault, owned by this account.
    @discardableResult
    public func createVault(name: String, kind: VaultKind) async throws -> String {
        let id = try await queue.run { @MainActor in
            let o = try self.need()
            let nv = try o.account.createVault()
            let grant = B64.encode(nv.grant)
            _ = try await self.apiNow().post("/v1/vaults", ["grant": grant])
            let v = nv.vault
            let id = v.id()
            o.vaults[id] = v
            let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
            let title = trimmed.isEmpty ? (kind == .team ? "Team" : "Family") : trimmed
            let note = try v.createItem(kind: .secureNote, fields: [
                FieldInput(name: "title", value: title),
                FieldInput(name: vaultInfoField, value: kind == .team ? "team" : "family"),
            ])
            try self.putVault(StoredVault(
                id: id, role: "owner", grant: grant, cache: B64.encode(v.exportCache()),
                pending: [note.id: B64.encode(note.record)]
            ))
            self.revision += 1
            return id
        }
        try? await sync()
        return id
    }

    public func members(_ vaultId: String) async throws -> MembersView {
        try await queue.run { @MainActor in
            let o = try self.need()
            let s = try self.stored()
            guard let sv = s.vaults[vaultId] else { throw AmanfoldError.NotFound }
            let api = try self.apiNow()
            var sent: [SentInvite] = []
            if Role.of(sv.role).canManage {
                for i in (try await api.get("/v1/vaults/\(vaultId)/invites")["invites"] as? [JSON]) ?? [] {
                    sent.append(SentInvite(inviteId: try i.need("invite_id"), email: try i.need("email"), role: Role.of(i.str("role"))))
                }
            }
            var members: [MemberView] = []
            for m in try await self.memberRows(api, vaultId) {
                let keys = try m.need("public_keys")
                members.append(MemberView(
                    accountId: try m.need("account_id"), email: try m.need("email"), role: Role.of(m.str("role")),
                    active: m.bool("active"), fingerprint: try safetyNumber(try B64.decode(keys)),
                    verified: self.trusted(o, s, keys), me: m.str("account_id") == s.accountId
                ))
            }
            return MembersView(members: members, invites: sent)
        }
    }

    public func invite(_ vaultId: String, email: String, role: Role) async throws {
        guard role != .owner else { throw AmanfoldError.InvalidArgument(reason: "invite as manager, writer or reader") }
        _ = try await apiNow().post("/v1/vaults/\(vaultId)/invites", [
            "email": email.trimmingCharacters(in: .whitespacesAndNewlines), "role": role.rawValue,
        ])
    }

    /// Cancel an invitation (managers), or decline one (the invitee).
    public func cancelInvite(_ inviteId: String) async throws {
        _ = try await apiNow().delete("/v1/invites/\(inviteId)")
    }

    /// Join a shared vault. `fingerprint` is the safety number the person saw
    /// and compared; the inviter is trusted only if it still matches.
    public func acceptInvite(_ inviteId: String, fingerprint: String) async throws {
        try await queue.run { @MainActor in
            let api = try self.apiNow()
            let invites = (try await api.get("/v1/invites")["invites"] as? [JSON]) ?? []
            guard let inv = invites.first(where: { $0.str("invite_id") == inviteId }) else {
                throw ApiError(status: 404, kind: "not_found", message: "invitation not found")
            }
            let from = inv["from"] as? JSON ?? [:]
            let keys = try from.need("public_keys")
            guard try safetyNumber(try B64.decode(keys)) == fingerprint else { throw FingerprintChangedError() }
            try self.addContact(try from.need("account_id"), try from.need("email"), keys)
            _ = try await api.post("/v1/invites/\(inviteId)/accept")
        }
    }

    private func addContact(_ accountId: String, _ email: String, _ keys: String) throws {
        var s = try stored()
        s.contacts[accountId] = Contact(email: email, keys: keys)
        try store.save(s)
    }

    /// Mark someone as verified after comparing safety numbers, and give them
    /// the vault key if they are waiting for it (owners and managers). A vault
    /// waiting for this person's key opens on the sync that follows.
    public func verify(_ vaultId: String, accountId: String, fingerprint: String) async throws {
        try await queue.run { @MainActor in
            let o = try self.need()
            let api = try self.apiNow()
            guard let m = try await self.memberRows(api, vaultId).first(where: { $0.str("account_id") == accountId }) else {
                throw ApiError(status: 404, kind: "not_found", message: "member not found")
            }
            let keys = try m.need("public_keys")
            guard try safetyNumber(try B64.decode(keys)) == fingerprint else { throw FingerprintChangedError() }
            try self.addContact(accountId, try m.need("email"), keys)
            if let sv = try self.stored().vaults[vaultId], let v = o.vaults[vaultId], !m.bool("active"), Role.of(sv.role).canManage {
                let grant = try o.account.grantVault(vault: v, recipientPublicKeys: try B64.decode(keys))
                _ = try await api.put("/v1/vaults/\(vaultId)/members/\(accountId)/grant", ["grant": B64.encode(grant)])
            }
        }
        try? await sync()
    }

    public func setRole(_ vaultId: String, accountId: String, role: Role) async throws {
        _ = try await apiNow().put("/v1/vaults/\(vaultId)/members/\(accountId)/role", ["role": role.rawValue])
    }

    /// Remove someone, then move the vault to a new key at once.
    public func removeMember(_ vaultId: String, accountId: String) async throws {
        try await queue.run { @MainActor in
            let o = try self.need()
            let api = try self.apiNow()
            _ = try await api.delete("/v1/vaults/\(vaultId)/members/\(accountId)")
            try await self.syncVault(o, vaultId, api)
            try await self.rotate(o, vaultId, api)
            self.revision += 1
        }
    }

    public func leaveVault(_ vaultId: String) async throws {
        try await queue.run { @MainActor in
            let o = try self.need()
            guard vaultId != o.personalId else { throw AmanfoldError.InvalidArgument(reason: "the personal vault cannot be left") }
            _ = try await self.apiNow().delete("/v1/vaults/\(vaultId)/members/\(try self.stored().accountId)")
            try self.dropVault(o, vaultId)
        }
    }

    /// Delete a shared vault and everything in it, for every member (owner only).
    public func deleteVault(_ vaultId: String) async throws {
        try await queue.run { @MainActor in
            let o = try self.need()
            guard vaultId != o.personalId else { throw AmanfoldError.InvalidArgument(reason: "the personal vault cannot be deleted here") }
            _ = try await self.apiNow().delete("/v1/vaults/\(vaultId)")
            try self.dropVault(o, vaultId)
        }
    }

    static func nowMs() -> Int64 { Int64(Date().timeIntervalSince1970 * 1000) }
}

/// The hidden note that names a shared vault.
func isVaultInfo(_ i: ItemView) -> Bool { i.fields.contains { $0.name == vaultInfoField } }

/// A public key's safety number, outside the class so nothing shadows the core's function.
func safetyNumber(_ keys: Data) throws -> String {
    try fingerprint(publicKeysBytes: keys)
}

/// The core's quick unlock, outside the class so its method does not shadow it.
private func coreQuickUnlock(bundle: Data, deviceKey: Data, wrappedKeys: Data) throws -> Account {
    try quickUnlock(bundle: bundle, deviceKey: deviceKey, wrappedKeys: wrappedKeys)
}
