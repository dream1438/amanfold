import AmanfoldKit
import AuthenticationServices
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject var session: AmanfoldSession
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @AppStorage("autoLock", store: Shared.defaults) private var autoLockMinutes = 5
    @AppStorage("biometricsOffered", store: Shared.defaults) private var offered = false
    @State private var autofillOn: Bool?
    @State private var biometrics = false
    @State private var confirmSignOut = false
    @State private var error: String?
    @State private var syncing = false

    private let lockChoices = [0, 1, 5, 15, 60]

    var body: some View {
        NavigationStack {
            Form {
                Section(L("autofill")) {
                    HStack {
                        Text(L(autofillOn == true ? "autofill_on" : "autofill_off"))
                        Spacer()
                        Image(systemName: autofillOn == true ? "checkmark.circle.fill" : "exclamationmark.circle")
                            .foregroundStyle(autofillOn == true ? Brand.primary : Brand.amber)
                    }
                    if autofillOn != true {
                        Text(L("ios_autofill_steps")).font(.footnote)
                        Button(L("ios_open_settings")) { ASSettingsHelper.openCredentialProviderAppSettings { _ in } }
                    }
                }
                Section {
                    Picker(L("auto_lock"), selection: $autoLockMinutes) {
                        ForEach(lockChoices, id: \.self) { m in
                            Text(m == 0 ? L("immediately") : (m >= 60 ? L("hours", m / 60) : L("minutes", m))).tag(m)
                        }
                    }
                    if Keychain.biometricsAvailable {
                        Toggle(L("quick_unlock"), isOn: $biometrics)
                            .onChange(of: biometrics) { _, on in setBiometrics(on) }
                    }
                }
                Section(L("language")) {
                    Text(L("ios_language_hint")).font(.footnote)
                    Button(L("ios_open_settings")) {
                        if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
                    }
                }
                Section(L("account")) {
                    if let info = session.accountInfo {
                        LTR(info.email)
                        LTR(info.server).font(.footnote).foregroundStyle(Brand.muted)
                    }
                    Text(lastSync).font(.footnote).foregroundStyle(Brand.muted)
                    Button(syncing ? L("working") : L("sync_now")) {
                        syncing = true
                        Task {
                            do { try await session.sync(); error = nil } catch { self.error = problem(of: error).text }
                            syncing = false
                        }
                    }
                    .disabled(syncing)
                    if let error { Banner(error, danger: true) }
                    Button(L("sign_out"), role: .destructive) { confirmSignOut = true }
                }
                Section {
                    NavigationLink(L("generator")) { GeneratorView(onUse: nil) }
                }
                Section {
                    HStack {
                        Text(L("version", "\(appVersion) · core \(coreVersion())"))
                        Spacer()
                        Text(L("beta")).font(.caption.weight(.semibold))
                            .padding(.horizontal, 8).padding(.vertical, 2)
                            .background(Brand.amber.opacity(0.3), in: Capsule())
                    }
                    .font(.footnote).foregroundStyle(Brand.muted)
                }
            }
            .navigationTitle(L("settings"))
            .toolbar { Button(L("back")) { dismiss() } }
            .confirmationDialog(L("ios_sign_out_confirm"), isPresented: $confirmSignOut, titleVisibility: .visible) {
                Button(L("sign_out"), role: .destructive) {
                    session.turnOffBiometrics()
                    offered = false
                    dismiss()
                    Task { await session.signout() }
                }
            }
            .task { await refresh() }
            .onChange(of: phase) { _, p in if p == .active { Task { await refresh() } } }
        }
    }

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "?"
    }

    private var lastSync: String {
        guard let ms = session.lastSyncMs else { return L("never_synced") }
        let d = Date(timeIntervalSince1970: Double(ms) / 1000)
        return L("last_sync", d.formatted(.relative(presentation: .named)))
    }

    private func refresh() async {
        biometrics = session.biometricsOn
        autofillOn = await ASCredentialIdentityStore.shared.state().isEnabled
    }

    private func setBiometrics(_ on: Bool) {
        guard on != session.biometricsOn else { return }
        if on {
            Task {
                do { try await session.turnOnBiometrics() } catch {
                    self.error = problem(of: error).text
                    biometrics = false
                }
            }
        } else {
            session.turnOffBiometrics()
        }
    }
}

struct GeneratorView: View {
    let onUse: ((String) -> Void)?
    @Environment(\.dismiss) private var dismiss
    private let defaults = defaultPasswordRules()
    @State private var length = 20.0
    @State private var lower = true
    @State private var upper = true
    @State private var digits = true
    @State private var symbols = true
    @State private var passphrase = false
    @State private var words = 5.0
    @State private var value = ""
    @State private var copied = false

    var body: some View {
        Form {
            Section {
                LTR(value, secret: true).font(.title3.monospaced())
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 6)
                HStack {
                    Button(L("generate"), action: make)
                    Spacer()
                    Button(L("copy")) { Clipboard.copy(value); copied = true }
                    if let onUse {
                        Spacer()
                        Button(L("use")) { onUse(value) }.buttonStyle(.borderedProminent)
                    }
                }
                .buttonStyle(.borderless)
                if copied { Text(L("copied")).font(.footnote).foregroundStyle(Brand.muted) }
            }
            Section {
                Toggle(L("passphrase"), isOn: $passphrase)
                if passphrase {
                    VStack(alignment: .leading) {
                        Text(L("words", Int(words)))
                        Slider(value: $words, in: 3...10, step: 1)
                    }
                } else {
                    VStack(alignment: .leading) {
                        Text(L("length", Int(length)))
                        Slider(value: $length, in: 8...64, step: 1)
                    }
                    Toggle(L("lowercase"), isOn: $lower)
                    Toggle(L("uppercase"), isOn: $upper)
                    Toggle(L("digits"), isOn: $digits)
                    Toggle(L("symbols"), isOn: $symbols)
                }
            }
        }
        .navigationTitle(L("generator"))
        .toolbar {
            if onUse != nil { ToolbarItem(placement: .cancellationAction) { Button(L("cancel")) { dismiss() } } }
        }
        .onAppear {
            length = Double(defaults.length)
            make()
        }
        .onChange(of: [length, words]) { _, _ in make() }
        .onChange(of: [lower, upper, digits, symbols, passphrase]) { _, _ in make() }
    }

    private func make() {
        copied = false
        if passphrase {
            value = (try? generatePassphrase(words: UInt32(words), separator: "-")) ?? ""
        } else {
            let rules = PasswordRules(length: UInt32(length), lowercase: lower, uppercase: upper, digits: digits,
                                      symbols: symbols, avoidAmbiguous: defaults.avoidAmbiguous)
            value = (try? generatePassword(rules: rules)) ?? ""
        }
    }
}
