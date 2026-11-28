import CryptoKit
import Foundation
import XCTest
@testable import AmanfoldKit

/// The session against a real amanfold-server, the way the iPhone talks to
/// it. Set AMANFOLD_SERVER_BIN to the server binary (CI does).
@MainActor
final class SessionTests: XCTestCase {
    private var server: Process!
    private var url = ""
    private var dir: URL!
    private let pw = "تفاحة-حصان-بطارية-دبوس"

    override func setUp() async throws {
        guard let bin = ProcessInfo.processInfo.environment["AMANFOLD_SERVER_BIN"] else {
            throw XCTSkip("AMANFOLD_SERVER_BIN not set")
        }
        dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let keygen = Process()
        keygen.executableURL = URL(fileURLWithPath: bin)
        keygen.arguments = ["keygen"]
        let pipe = Pipe()
        keygen.standardOutput = pipe
        keygen.standardError = FileHandle.nullDevice
        try keygen.run()
        let secrets = pipe.fileHandleForReading.readDataToEndOfFile()
        keygen.waitUntilExit()
        try secrets.write(to: dir.appendingPathComponent("secrets.json"))

        let port = Int.random(in: 20000..<40000)
        url = "http://127.0.0.1:\(port)"
        server = Process()
        server.executableURL = URL(fileURLWithPath: bin)
        server.arguments = ["serve"]
        server.environment = [
            "AMANFOLD_SECRETS_FILE": dir.appendingPathComponent("secrets.json").path,
            "AMANFOLD_DB": dir.appendingPathComponent("db.sqlite").path,
            "AMANFOLD_LISTEN": "127.0.0.1:\(port)",
            "AMANFOLD_DEV": "1",
            "AMANFOLD_NO_EMAIL_VERIFICATION": "1",
        ]
        server.standardOutput = FileHandle.nullDevice
        server.standardError = FileHandle.nullDevice
        try server.run()
        for _ in 0..<200 {
            if (try? await Api(base: url).get("/v1/server")) != nil { return }
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        XCTFail("server did not start")
    }

    override func tearDown() async throws {
        server?.terminate()
        if let dir { try? FileManager.default.removeItem(at: dir) }
    }

    private func client(_ store: AccountStore = MemoryStore(), pins: [String: String] = [:]) -> AmanfoldSession {
        AmanfoldSession(store: store, config: ClientConfig(deviceName: "Test iPhone", pinnedServerKeys: pins, allowLocalHttp: true))
    }

    private func f(_ n: String, _ v: String) -> FieldInput { FieldInput(name: n, value: v) }

    private func value(_ s: AmanfoldSession, _ id: String, _ name: String) -> String? {
        s.item(id)?.fields.first { $0.name == name }?.value
    }

    func testFullFlow() async throws {
        let a = client()
        XCTAssertEqual(a.state, .noAccount)
        let info = try await a.serverInfo(url)
        XCTAssertFalse(info.emailVerification)
        let kit = try await a.signup(server: url, email: "dana@example.com", password: pw, code: nil)
        XCTAssertTrue(kit.secretKey.hasPrefix("A1-"))
        XCTAssertEqual(kit.recoveryWords.split(separator: " ").count, 24)
        XCTAssertEqual(a.state, .unlocked)

        let id = try await a.create(.login, [f("title", "Bank"), f("username", "dana"), f("password", "hunter2!"), f("url", "https://www.bank.com/login")])
        XCTAssertNil(a.syncProblem)

        // Wrong password, mistyped Secret Key, impostor server.
        let b = client()
        do {
            try await b.signin(server: url, email: "dana@example.com", password: "wrong password!", secretKey: kit.secretKey)
            XCTFail("signed in with a wrong password")
        } catch {
            XCTAssertEqual(problem(of: error), .wrongPassword)
        }
        do {
            try await b.signin(server: url, email: "dana@example.com", password: pw, secretKey: "A1-NOT-A-KEY")
            XCTFail("accepted a bad Secret Key")
        } catch {
            XCTAssertEqual(problem(of: error), .secretKey)
        }
        let pinned = client(pins: [url: B64.encode(Data(count: 32))])
        do {
            try await pinned.signin(server: url, email: "dana@example.com", password: pw, secretKey: kit.secretKey)
            XCTFail("trusted an impostor server")
        } catch {
            XCTAssertEqual(problem(of: error), .serverKey)
        }

        // Second device.
        try await b.signin(server: url, email: "dana@example.com", password: pw, secretKey: kit.secretKey.lowercased())
        XCTAssertEqual(b.items().map(\.id), [id])
        XCTAssertEqual(b.logins(forPage: "https://bank.com/signin").first?.item.id, id)
        XCTAssertTrue(b.logins(forPage: "https://bank.com.evil.io").isEmpty)
        XCTAssertEqual(b.lookalikes(forPage: "https://bank-secure.com"), ["bank.com"])

        // Edits on both merge.
        try await b.update(id, set: [f("password", "n3w-pass")])
        let card = try await a.create(.card, [f("title", "Visa"), f("card_number", "4111 1111 1111 1111")])
        try await a.sync()
        try await b.sync()
        XCTAssertEqual(value(a, id, "password"), "n3w-pass")
        XCTAssertEqual(value(b, card, "card_number"), "4111111111111111")

        try await a.trash(card, true)
        try await b.sync()
        XCTAssertEqual(b.items(trashed: true).map(\.id), [card])

        // Lock; quick unlock with the device key; password unlock.
        let deviceKey = try await b.enableQuickUnlock()
        XCTAssertTrue(b.hasQuickUnlock)
        b.lock()
        XCTAssertEqual(b.state, .locked)
        XCTAssertTrue(b.items().isEmpty)
        do {
            try await b.quickUnlock(deviceKey: Data(count: 32))
            XCTFail("wrong device key")
        } catch {
            XCTAssertEqual(problem(of: error), .wrongPassword)
        }
        try await b.quickUnlock(deviceKey: deviceKey)
        XCTAssertEqual(b.state, .unlocked)
        b.lock()
        try await b.unlock(password: pw)
        XCTAssertEqual(value(b, id, "password"), "n3w-pass")

        await b.signout()
        XCTAssertEqual(b.state, .noAccount)

        // Forgotten password with the recovery kit.
        let c = client()
        try await c.recover(server: url, email: "dana@example.com", code: nil, recoveryWords: kit.recoveryWords,
                            secretKey: kit.secretKey, newPassword: "a brand new master password")
        XCTAssertEqual(c.state, .unlocked)
        XCTAssertEqual(value(c, id, "password"), "n3w-pass")

        // Phone A was signed out by the password change.
        do {
            try await a.sync()
            XCTFail("old session still works")
        } catch {
            XCTAssertEqual(a.syncProblem, .signedOut)
        }
    }

    func testEncryptedFileSurvivesRestart() async throws {
        let key = SymmetricKey(size: .bits256)
        let file = dir.appendingPathComponent("account.bin")
        let a = client(FileStore(url: file, key: key))
        _ = try await a.signup(server: url, email: "sam@example.com", password: pw, code: nil)
        let id = try await a.create(.secureNote, [f("title", "Wifi"), f("notes", "pass: abc")])
        a.lock()
        XCTAssertNil(FileStore(url: file, key: SymmetricKey(size: .bits256)).load(), "wrong key opens nothing")
        let again = client(FileStore(url: file, key: key))
        XCTAssertEqual(again.state, .locked)
        try await again.unlock(password: pw)
        XCTAssertEqual(value(again, id, "notes"), "pass: abc")
    }

    func testServerAddresses() throws {
        XCTAssertEqual(try checkServerUrl(" https://api.example.com/ ", allowLocalHttp: false), "https://api.example.com")
        XCTAssertEqual(try checkServerUrl("http://localhost:8080", allowLocalHttp: true), "http://localhost:8080")
        for bad in ["http://api.example.com", "http://localhost:8080", "ftp://x.org", "https://u:p@x.org", "https://x.org/?a=1", "nonsense"] {
            XCTAssertThrowsError(try checkServerUrl(bad, allowLocalHttp: false), bad)
        }
    }

    func testPageAddresses() {
        XCTAssertEqual(pageURL(serviceIdentifier: "bank.com", isURL: false), "https://bank.com")
        XCTAssertEqual(pageURL(serviceIdentifier: "https://login.bank.com/x", isURL: true), "https://login.bank.com/x")
        XCTAssertNil(pageURL(serviceIdentifier: "javascript:alert(1)", isURL: true))
        XCTAssertNil(pageURL(serviceIdentifier: "", isURL: false))
    }

    // MARK: Sharing (same rules as the Android client and the extension)

    private func person(_ email: String) async throws -> AmanfoldSession {
        let c = client()
        _ = try await c.signup(server: url, email: email, password: "long master password for \(email)", code: nil)
        return c
    }

    private func titles(_ c: AmanfoldSession) -> [String] {
        c.items().map { i in i.fields.first { $0.name == "title" }?.value ?? "" }.sorted()
    }

    private func join(_ owner: AmanfoldSession, _ vault: String, _ who: AmanfoldSession, _ email: String, _ role: Role) async throws {
        try await owner.invite(vault, email: email, role: role)
        let invites = try await who.sharing().invites
        XCTAssertEqual(invites.count, 1)
        let inv = invites[0]
        let ownerNumber = try await owner.sharing().myFingerprint
        XCTAssertEqual(inv.fromFingerprint, ownerNumber)
        XCTAssertEqual(inv.role, role)
        // The app refuses if the safety number is not the one the person compared.
        do {
            try await who.acceptInvite(inv.inviteId, fingerprint: "00000 00000")
            XCTFail("accepted with the wrong safety number")
        } catch {
            XCTAssertEqual(problem(of: error), .safetyNumber)
        }
        try await who.acceptInvite(inv.inviteId, fingerprint: inv.fromFingerprint)
        let pending = try await owner.members(vault).members.first { $0.email == email }!
        XCTAssertFalse(pending.active)
        XCTAssertFalse(pending.verified)
        let whoNumber = try await who.sharing().myFingerprint
        XCTAssertEqual(pending.fingerprint, whoNumber)
        try await owner.verify(vault, accountId: pending.accountId, fingerprint: pending.fingerprint)
        let after = try await owner.members(vault).members.first { $0.email == email }!
        XCTAssertTrue(after.active && after.verified)
        try await who.sync()
    }

    /// Compare safety numbers with every other member (done in person or on a call).
    private func verifyAll(_ c: AmanfoldSession, _ vault: String) async throws {
        for m in try await c.members(vault).members where !m.me && !m.verified {
            try await c.verify(vault, accountId: m.accountId, fingerprint: m.fingerprint)
        }
    }

    func testFamilySharing() async throws {
        let mum = try await person("mum@example.com")
        let dad = try await person("dad@example.com")
        let kid = try await person("kid@example.com")

        let fam = try await mum.createVault(name: "Family", kind: .family)
        try await mum.create(.login, [f("title", "Netflix"), f("password", "p1")], in: fam)
        try await mum.create(.login, [f("title", "Mum's bank")])
        // The vault's name note is hidden from the item list.
        XCTAssertEqual(titles(mum), ["Mum's bank", "Netflix"])
        XCTAssertEqual(mum.vaults().first { $0.shared }?.name, "Family")

        try await join(mum, fam, dad, "dad@example.com", .writer)
        try await join(mum, fam, kid, "kid@example.com", .reader)
        try await verifyAll(dad, fam)

        XCTAssertEqual(titles(dad), ["Netflix"])
        let netflix = kid.items().first!.id
        XCTAssertEqual(kid.vaultOf(netflix), VaultTag(id: fam, name: "Family", kind: .family, role: .reader))
        XCTAssertTrue(kid.vaultOf(netflix)!.readOnly)

        // A writer's change reaches everyone who verified the writer.
        try await dad.create(.login, [f("title", "Electricity")], in: fam)
        try await mum.sync()
        try await kid.sync()
        XCTAssertEqual(titles(kid), ["Netflix"])
        let waitingKid = try await kid.sharing().vaults.first!.needsVerification
        XCTAssertTrue(waitingKid)
        try await verifyAll(kid, fam)
        try await kid.sync()
        let doneKid = try await kid.sharing().vaults.first!.needsVerification
        XCTAssertFalse(doneKid)
        XCTAssertEqual(titles(mum), ["Electricity", "Mum's bank", "Netflix"])
        XCTAssertEqual(titles(kid), ["Electricity", "Netflix"])
        do {
            try await kid.create(.login, [f("title", "Game")], in: fam)
            XCTFail("a reader added an item")
        } catch {
            XCTAssertEqual(problem(of: error), .readOnly)
        }
        do {
            try await kid.update(netflix, set: [f("password", "mine")])
            XCTFail("a reader changed an item")
        } catch {
            XCTAssertEqual(problem(of: error), .readOnly)
        }

        // Moving an item into the shared vault.
        let bank = mum.items().first { $0.fields.contains { $0.value == "Mum's bank" } }!.id
        let moved = try await mum.move(bank, to: fam)
        try await dad.sync()
        XCTAssertEqual(titles(dad), ["Electricity", "Mum's bank", "Netflix"])
        XCTAssertEqual(mum.vaultOf(moved)?.id, fam)

        // Removing dad rotates the key: he loses the vault, the kid keeps it.
        let dadId = try await mum.members(fam).members.first { $0.email == "dad@example.com" }!.accountId
        try await mum.removeMember(fam, accountId: dadId)
        try await dad.sync()
        XCTAssertTrue(dad.items().isEmpty)
        XCTAssertFalse(dad.vaults().contains { $0.shared })
        try await mum.create(.login, [f("title", "After")], in: fam)
        try await kid.sync()
        XCTAssertEqual(titles(kid), ["After", "Electricity", "Mum's bank", "Netflix"])

        // The kid leaves.
        try await kid.leaveVault(fam)
        XCTAssertTrue(kid.items().isEmpty)
        try await kid.sync()
        XCTAssertFalse(kid.vaults().contains { $0.shared })
        let left = try await mum.members(fam).members.count
        XCTAssertEqual(left, 1)
    }

    func testVaultFromSomeoneUnverifiedWaits() async throws {
        let mum = try await person("mum@example.com")
        let dad = try await person("dad@example.com")
        let pat = try await person("pat@example.com")
        let fam = try await mum.createVault(name: "Family", kind: .family)
        try await join(mum, fam, dad, "dad@example.com", .manager)

        // Pat compared mum's number only; dad, a manager, grants access.
        try await mum.invite(fam, email: "pat@example.com", role: .reader)
        let inv = try await pat.sharing().invites.first!
        try await pat.acceptInvite(inv.inviteId, fingerprint: inv.fromFingerprint)
        let p = try await dad.members(fam).members.first { $0.email == "pat@example.com" }!
        try await dad.verify(fam, accountId: p.accountId, fingerprint: p.fingerprint)
        try await pat.sync()
        let waiting = try await pat.sharing().waiting
        XCTAssertEqual(waiting.map(\.email), ["dad@example.com"])
        let dadNumber = try await dad.sharing().myFingerprint
        XCTAssertEqual(waiting.first?.fingerprint, dadNumber)
        XCTAssertFalse(pat.vaults().contains { $0.id == fam })
        try await pat.verify(fam, accountId: waiting[0].accountId, fingerprint: waiting[0].fingerprint)
        XCTAssertTrue(pat.vaults().contains { $0.id == fam })
        let none = try await pat.sharing().waiting
        XCTAssertTrue(none.isEmpty)
    }
}
