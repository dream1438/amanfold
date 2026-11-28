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
}
