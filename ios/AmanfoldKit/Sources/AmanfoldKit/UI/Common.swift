#if os(iOS)
import SwiftUI
import UIKit
import UniformTypeIdentifiers

extension ItemView {
    public func value(_ name: String) -> String? {
        guard let v = fields.first(where: { $0.name == name })?.value, !v.isEmpty else { return nil }
        return v
    }

    public var titleText: String { value("title") ?? L("untitled") }

    public var subtitle: String {
        switch kind {
        case .login: return [value("username"), value("url").map(shortURL)].compactMap { $0 }.joined(separator: " · ")
        case .card: return value("card_number").map { "•••• " + $0.suffix(4) } ?? ""
        default: return value("notes").map { String($0.split(separator: "\n").first?.prefix(60) ?? "") } ?? ""
        }
    }

    public var symbol: String {
        switch kind {
        case .login: return "key.fill"
        case .card: return "creditcard.fill"
        case .passkey: return "person.badge.key.fill"
        default: return "note.text"
        }
    }
}

public func shortURL(_ u: String) -> String {
    if let pkg = androidAppPackage(savedUrl: u) { return pkg }
    var s = u
    for p in ["https://", "http://"] where s.hasPrefix(p) { s.removeFirst(p.count) }
    return String(s.split(separator: "/").first ?? Substring(s))
}

/// Copies a value for 60 seconds, on this device only (not Universal Clipboard).
public enum Clipboard {
    public static func copy(_ value: String) {
        UIPasteboard.general.setItems(
            [[UTType.utf8PlainText.identifier: value]],
            options: [.localOnly: true, .expirationDate: Date().addingTimeInterval(60)]
        )
    }
}

public struct ItemRow: View {
    let item: ItemView
    public init(_ item: ItemView) { self.item = item }
    public var body: some View {
        HStack(spacing: 14) {
            Image(systemName: item.symbol)
                .foregroundStyle(Brand.primary)
                .frame(width: 36, height: 36)
                .background(Brand.infoBg, in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 2) {
                Text(item.titleText).font(.headline).lineLimit(1)
                if !item.subtitle.isEmpty {
                    Text(item.subtitle).font(.footnote).foregroundStyle(Brand.muted).lineLimit(1)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

/// A hidden-until-shown input that the keyboard never learns from.
public struct SecretField: View {
    let label: String
    @Binding var text: String
    @State private var shown = false
    public init(_ label: String, text: Binding<String>) {
        self.label = label
        _text = text
    }

    public var body: some View {
        HStack {
            Group {
                if shown { TextField(label, text: $text) } else { SecureField(label, text: $text) }
            }
            .font(secretFont)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .textContentType(.password)
            Button { shown.toggle() } label: {
                Image(systemName: shown ? "eye.slash" : "eye")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(L(shown ? "hide" : "show"))
        }
    }
}

/// Unlock with the master password or Face ID / Touch ID. Used by the app
/// and by the AutoFill extension.
public struct UnlockView: View {
    @ObservedObject var session: AmanfoldSession
    let onSignOut: (() -> Void)?
    @State private var password = ""
    @State private var busy = false
    @State private var error: String?
    @State private var notice: String?
    @State private var triedBiometrics = false

    public init(session: AmanfoldSession, onSignOut: (() -> Void)? = nil) {
        self.session = session
        self.onSignOut = onSignOut
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Logo()
                Text(L("locked")).font(.title2.weight(.semibold))
                if let email = session.accountInfo?.email {
                    Text(verbatim: email).foregroundStyle(Brand.muted)
                }
                if let notice { Banner(notice) }
                SecretField(L("master_password"), text: $password)
                    .padding(12)
                    .background(Brand.surface, in: RoundedRectangle(cornerRadius: 12))
                    .onSubmit(unlock)
                if let error { Banner(error, danger: true) }
                Button(action: unlock) {
                    Text(busy ? L("working") : L("unlock")).frame(maxWidth: .infinity, minHeight: 34)
                }
                .buttonStyle(.borderedProminent)
                .tint(Brand.primary)
                .disabled(password.isEmpty || busy)
                if session.biometricsOn {
                    Button {
                        Task { await biometrics() }
                    } label: {
                        Label(L("unlock_biometric"), systemImage: "faceid").frame(maxWidth: .infinity, minHeight: 34)
                    }
                    .buttonStyle(.bordered)
                }
                if let onSignOut {
                    Button(L("sign_out"), role: .destructive, action: onSignOut).buttonStyle(.borderless)
                }
            }
            .padding(24)
        }
        .background(Brand.background.ignoresSafeArea())
        .task {
            guard !triedBiometrics, session.biometricsOn else { return }
            triedBiometrics = true
            await biometrics()
        }
    }

    private func biometrics() async {
        if await session.unlockWithBiometrics() == .changed { notice = L("biometric_changed") }
    }

    private func unlock() {
        guard !password.isEmpty else { return }
        busy = true
        error = nil
        Task {
            do {
                try await session.unlock(password: password)
                password = ""
            } catch {
                self.error = problem(of: error).text
            }
            busy = false
        }
    }
}
#endif
