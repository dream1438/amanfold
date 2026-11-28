import Foundation

/// What went wrong, in terms the interface has a sentence for.
public enum Problem: Equatable {
    case wrongPassword, secretKey, recoveryWords, offline, exists, badServer, serverKey
    case rollback, signedOut, verification, code, rateLimited, locked
    case readOnly, safetyNumber, unverified, notFound
    case server(String), other(String)

    /// The sentence to show.
    public var text: String {
        switch self {
        case .wrongPassword: return L("err_wrong_password")
        case .secretKey: return L("err_secret_key")
        case .recoveryWords: return L("err_recovery_words")
        case .offline: return L("err_offline")
        case .exists: return L("err_exists")
        case .badServer: return L("err_bad_server")
        case .serverKey: return L("err_server_key")
        case .rollback: return L("err_rollback")
        case .signedOut: return L("err_signed_out")
        case .verification: return L("err_verification")
        case .code: return L("err_code")
        case .rateLimited: return L("err_rate_limited")
        case .locked: return L("err_locked")
        case .readOnly: return L("err_read_only")
        case .safetyNumber: return L("err_safety_number")
        case .unverified: return L("err_unverified")
        case .notFound: return L("err_not_found")
        case .server(let m): return L("err_server", m)
        case .other(let m): return L("err_generic", m)
        }
    }
}

/// Maps any failure to a [Problem]. `hadCode`: the call carried an emailed
/// code, so a refusal most likely means the code was wrong.
public func problem(of e: Error, hadCode: Bool = false) -> Problem {
    switch e {
    case is LockedError: return .locked
    case is ServerKeyError: return .serverKey
    case is SecretKeyError: return .secretKey
    case is BadServerError: return .badServer
    case is ReadOnlyError: return .readOnly
    case is FingerprintChangedError: return .safetyNumber
    case is UnverifiedMembersError: return .unverified
    case let a as AmanfoldError:
        switch a {
        case .Decrypt: return .wrongPassword
        case .InvalidCode(let reason): return reason.lowercased().contains("secret") ? .secretKey : .recoveryWords
        case .Rollback, .BadSignature: return .rollback
        case .NotFound: return .notFound
        default: return .other(String(describing: a))
        }
    case let a as ApiError:
        if a.status == 0 { return .offline }
        if a.kind == "exists" { return .exists }
        if a.kind == "rate_limited" { return .rateLimited }
        if a.kind == "verification_failed" { return hadCode ? .code : .verification }
        if a.status == 401 { return .signedOut }
        if a.status == 404 { return .notFound }
        return .server(a.description)
    default:
        return .other(String(describing: e))
    }
}

/// The page address AutoFill matches for an iOS service identifier. Domains
/// come from Safari or from an app's verified associated domains; iOS checks
/// those, so a domain is taken as `https`.
public func pageURL(serviceIdentifier: String, isURL: Bool) -> String? {
    let s = serviceIdentifier.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !s.isEmpty else { return nil }
    let url = isURL ? s : "https://\(s)"
    guard let scheme = URLComponents(string: url)?.scheme?.lowercased(), scheme == "https" || scheme == "http" else { return nil }
    return pageSite(pageUrl: url) == nil ? nil : url
}
