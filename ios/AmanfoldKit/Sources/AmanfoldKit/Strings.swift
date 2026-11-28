@_exported import Amanfold
import Foundation

/// Interface text, shared by the app and the AutoFill extension. The
/// .strings files are generated from the Android app's strings
/// (scripts/strings.py), so both apps say the same thing in English and Arabic.
public func L(_ key: String) -> String {
    NSLocalizedString(key, bundle: .module, comment: "")
}

public func L(_ key: String, _ args: CVarArg...) -> String {
    String(format: L(key), locale: Locale.current, arguments: args)
}

/// `true` when the interface is right to left (Arabic).
public var isRTL: Bool {
    Locale.Language(identifier: Bundle.module.preferredLocalizations.first ?? "en").characterDirection == .rightToLeft
}
