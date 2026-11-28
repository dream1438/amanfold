import Foundation

// Client for the Amanfold sync server (docs/SERVER.md). JSON over HTTPS,
// byte strings as unpadded base64url. The server only ever sees ciphertext.

public enum B64 {
    public static func encode(_ d: Data) -> String {
        d.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    public static func decode(_ s: String) throws -> Data {
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while t.count % 4 != 0 { t += "=" }
        guard let d = Data(base64Encoded: t) else { throw ApiError(status: 200, kind: "bad_response", message: "bad base64") }
        return d
    }
}

/// The server refused a call, or could not be reached (`status` 0).
public struct ApiError: Error, CustomStringConvertible {
    public let status: Int
    public let kind: String
    public let message: String
    public var seq: Int64? = nil
    public var description: String { "\(kind): \(message)" }
}

public struct BadServerError: Error {}

/// Checks a server address. HTTPS is required, except for this computer
/// (the simulator reaches a local amanfold-server) when `allowLocalHttp`.
public func checkServerUrl(_ raw: String, allowLocalHttp: Bool) throws -> String {
    guard let u = URLComponents(string: raw.trimmingCharacters(in: .whitespacesAndNewlines)),
          let host = u.host, !host.isEmpty
    else { throw BadServerError() }
    let local = ["localhost", "127.0.0.1", "::1"].contains(host)
    guard u.scheme == "https" || (u.scheme == "http" && local && allowLocalHttp) else { throw BadServerError() }
    guard u.user == nil, u.password == nil, u.query == nil, u.fragment == nil else { throw BadServerError() }
    var path = u.path
    while path.hasSuffix("/") { path.removeLast() }
    let port = u.port.map { ":\($0)" } ?? ""
    return "\(u.scheme!)://\(host)\(port)\(path)"
}

/// Refuses redirects: a redirected API call could carry the token elsewhere.
private final class NoRedirects: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

private let session: URLSession = {
    let c = URLSessionConfiguration.ephemeral
    c.requestCachePolicy = .reloadIgnoringLocalCacheData
    c.urlCache = nil
    c.httpCookieStorage = nil
    c.timeoutIntervalForRequest = 30
    return URLSession(configuration: c, delegate: NoRedirects(), delegateQueue: nil)
}()

public typealias JSON = [String: Any]

public final class Api {
    public let base: String
    public var token: String?

    public init(base: String, token: String? = nil) {
        self.base = base
        self.token = token
    }

    public func call(_ method: String, _ path: String, _ body: JSON? = nil) async throws -> JSON {
        guard let url = URL(string: base + path) else { throw BadServerError() }
        var req = URLRequest(url: url)
        req.httpMethod = method
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        if let token { req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let body {
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
            req.httpBody = try JSONSerialization.data(withJSONObject: body)
        }
        let data: Data
        let resp: URLResponse
        do {
            (data, resp) = try await session.data(for: req)
        } catch {
            throw ApiError(status: 0, kind: "offline", message: "could not reach the server")
        }
        let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
        var obj: JSON = [:]
        if !data.isEmpty {
            guard let o = try? JSONSerialization.jsonObject(with: data) as? JSON else {
                throw ApiError(status: status, kind: "bad_response", message: "the server sent something unexpected")
            }
            obj = o
        }
        guard (200..<300).contains(status) else {
            var e = ApiError(status: status, kind: obj["error"] as? String ?? "http", message: obj["message"] as? String ?? "HTTP \(status)")
            e.seq = (obj["seq"] as? NSNumber)?.int64Value
            throw e
        }
        return obj
    }

    public func get(_ path: String) async throws -> JSON { try await call("GET", path) }
    public func post(_ path: String, _ body: JSON = [:]) async throws -> JSON { try await call("POST", path, body) }
    public func put(_ path: String, _ body: JSON = [:]) async throws -> JSON { try await call("PUT", path, body) }
    public func delete(_ path: String) async throws -> JSON { try await call("DELETE", path) }
}

extension Dictionary where Key == String, Value == Any {
    func need(_ k: String) throws -> String {
        guard let v = self[k] as? String else { throw ApiError(status: 200, kind: "bad_response", message: "missing \(k)") }
        return v
    }

    func int64(_ k: String) -> Int64? { (self[k] as? NSNumber)?.int64Value }

    func bool(_ k: String) -> Bool { self[k] as? Bool ?? false }

    func str(_ k: String) -> String? { self[k] as? String }
}
