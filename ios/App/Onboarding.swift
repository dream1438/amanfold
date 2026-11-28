import AmanfoldKit
import SwiftUI
import UIKit

struct OnboardingView: View {
    let onCreated: (RecoveryKit) -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    Logo()
                    Text(L("tagline")).font(.title3)
                    Banner(L("beta_note"))
                    NavigationLink {
                        CreateAccountView(onCreated: onCreated)
                    } label: {
                        Text(L("create_account")).frame(maxWidth: .infinity, minHeight: 34)
                    }
                    .buttonStyle(.borderedProminent)
                    NavigationLink {
                        SignInView()
                    } label: {
                        Text(L("sign_in")).frame(maxWidth: .infinity, minHeight: 34)
                    }
                    .buttonStyle(.bordered)
                }
                .padding(24)
                .padding(.top, 60)
            }
            .background(Brand.background.ignoresSafeArea())
        }
    }
}

/// The server address, folded away unless the person wants another one.
struct ServerRow: View {
    @Binding var server: String
    @State private var editing = false
    var body: some View {
        if editing {
            TextField(L("server"), text: $server)
                .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
        } else {
            HStack {
                Text(L("server")).foregroundStyle(Brand.muted)
                LTR(server).font(.footnote).foregroundStyle(Brand.muted)
                Spacer()
                Button(L("edit")) { editing = true }.buttonStyle(.borderless)
            }
        }
    }
}

struct EmailField: View {
    @Binding var email: String
    var body: some View {
        TextField(L("email"), text: $email)
            .keyboardType(.emailAddress).textContentType(.username)
            .textInputAutocapitalization(.never).autocorrectionDisabled()
    }
}

struct CreateAccountView: View {
    @EnvironmentObject var session: AmanfoldSession
    let onCreated: (RecoveryKit) -> Void
    @AppStorage("server", store: Shared.defaults) private var savedServer = Shared.defaultServer
    @State private var server = ""
    @State private var email = ""
    @State private var code = ""
    @State private var needsCode: Bool?
    @State private var password = ""
    @State private var password2 = ""
    @State private var understood = false
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            Section { Banner(L("beta_note")) }
            Section {
                ServerRow(server: $server)
                EmailField(email: $email)
            }
            if let needsCode {
                Section {
                    if needsCode {
                        Text(L("code_sent", email)).font(.footnote)
                        TextField(L("email_code"), text: $code).keyboardType(.numberPad).textContentType(.oneTimeCode)
                    }
                    SecretField(L("master_password"), text: $password)
                    SecretField(L("confirm_password"), text: $password2)
                } footer: {
                    Text(L("password_hint"))
                }
                Section {
                    Toggle(L("no_reset"), isOn: $understood)
                }
            }
            if let error { Section { Banner(error, danger: true) } }
            if busy, needsCode != nil { Text(L("creating_keys")).font(.footnote) }
            Section {
                Button(busy ? L("working") : (needsCode == nil ? L("continue_") : L("create_account")), action: next)
                    .disabled(busy || !email.contains("@") || (needsCode != nil && (!understood || password.isEmpty)))
            }
        }
        .navigationTitle(L("create_account"))
        .onAppear { if server.isEmpty { server = savedServer } }
    }

    private func next() {
        error = nil
        if needsCode != nil {
            if password.count < 10 { error = L("password_short"); return }
            if password != password2 { error = L("passwords_differ"); return }
        }
        busy = true
        Task {
            defer { busy = false }
            do {
                if needsCode == nil {
                    let info = try await session.serverInfo(server)
                    if info.emailVerification { try await session.requestSignupCode(server: server, email: email) }
                    needsCode = info.emailVerification
                    savedServer = server
                } else {
                    let kit = try await session.signup(server: server, email: email, password: password,
                                                       code: needsCode == true ? code : nil)
                    savedServer = server
                    onCreated(kit)
                }
            } catch {
                self.error = problem(of: error, hadCode: needsCode == true).text
            }
        }
    }
}

struct SignInView: View {
    @EnvironmentObject var session: AmanfoldSession
    @AppStorage("server", store: Shared.defaults) private var savedServer = Shared.defaultServer
    @State private var server = ""
    @State private var email = ""
    @State private var password = ""
    @State private var secretKey = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            Section {
                ServerRow(server: $server)
                EmailField(email: $email)
                SecretField(L("master_password"), text: $password)
            }
            Section {
                SecretField(L("secret_key"), text: $secretKey)
            } footer: {
                Text(L("secret_key_hint"))
            }
            if let error { Section { Banner(error, danger: true) } }
            Section {
                Button(busy ? L("working") : L("sign_in")) {
                    busy = true
                    error = nil
                    Task {
                        defer { busy = false }
                        do {
                            try await session.signin(server: server, email: email, password: password, secretKey: secretKey)
                            savedServer = server
                        } catch {
                            self.error = problem(of: error).text
                        }
                    }
                }
                .disabled(busy || !email.contains("@") || password.isEmpty || secretKey.isEmpty)
                NavigationLink(L("forgot_password")) { RecoverView() }
            }
        }
        .navigationTitle(L("sign_in"))
        .onAppear { if server.isEmpty { server = savedServer } }
    }
}

struct RecoverView: View {
    @EnvironmentObject var session: AmanfoldSession
    @AppStorage("server", store: Shared.defaults) private var savedServer = Shared.defaultServer
    @State private var server = ""
    @State private var email = ""
    @State private var needsCode: Bool?
    @State private var code = ""
    @State private var words = ""
    @State private var secretKey = ""
    @State private var password = ""
    @State private var password2 = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            Section { Text(L("recover_intro")).font(.callout) }
            Section {
                ServerRow(server: $server)
                EmailField(email: $email)
            }
            if let needsCode {
                Section {
                    if needsCode {
                        Text(L("code_sent", email)).font(.footnote)
                        TextField(L("email_code"), text: $code).keyboardType(.numberPad).textContentType(.oneTimeCode)
                    }
                    TextField(L("recovery_words"), text: $words, axis: .vertical)
                        .font(secretFont).lineLimit(3...6)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    SecretField(L("secret_key"), text: $secretKey)
                    SecretField(L("new_master_password"), text: $password)
                    SecretField(L("confirm_password"), text: $password2)
                }
            }
            if let error { Section { Banner(error, danger: true) } }
            Section {
                Button(busy ? L("working") : L("continue_"), action: next)
                    .disabled(busy || !email.contains("@"))
            }
        }
        .navigationTitle(L("recover_title"))
        .onAppear { if server.isEmpty { server = savedServer } }
    }

    private func next() {
        error = nil
        if needsCode != nil {
            if password.count < 10 { error = L("password_short"); return }
            if password != password2 { error = L("passwords_differ"); return }
        }
        busy = true
        Task {
            defer { busy = false }
            do {
                if needsCode == nil {
                    let info = try await session.serverInfo(server)
                    if info.emailVerification { try await session.requestRecoveryCode(server: server, email: email) }
                    needsCode = info.emailVerification
                } else {
                    try await session.recover(server: server, email: email, code: needsCode == true ? code : nil,
                                              recoveryWords: words.trimmingCharacters(in: .whitespacesAndNewlines),
                                              secretKey: secretKey, newPassword: password)
                    savedServer = server
                }
            } catch {
                self.error = problem(of: error, hadCode: needsCode == true).text
            }
        }
    }
}

struct RecoveryKitView: View {
    let kit: RecoveryKit
    let onDone: () -> Void
    @State private var saved = false

    var body: some View {
        NavigationStack {
            Form {
                Section { Text(L("kit_intro")).font(.callout) }
                Section(L("email")) { LTR(kit.email) }
                Section(L("secret_key")) { LTR(kit.secretKey, secret: true) }
                Section(L("recovery_words")) { LTR(numbered(kit.recoveryWords), secret: true) }
                Section {
                    Button(L("print_kit"), action: printKit)
                    Toggle(L("kit_saved_check"), isOn: $saved)
                    Button(L("continue_"), action: onDone).disabled(!saved)
                }
            }
            .navigationTitle(L("kit_title"))
        }
    }

    private func numbered(_ words: String) -> String {
        let w = words.split(separator: " ")
        return stride(from: 0, to: w.count, by: 3).map { i in
            (i..<min(i + 3, w.count)).map { "\($0 + 1). \(w[$0])" }.joined(separator: "   ")
        }.joined(separator: "\n")
    }

    /// The system print sheet (which also saves a PDF). Nothing is written to app storage.
    private func printKit() {
        func esc(_ s: String) -> String {
            s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
        }
        let words = kit.recoveryWords.split(separator: " ").map { "<li>\(esc(String($0)))</li>" }.joined()
        let html = """
        <html><head><meta charset="utf-8"><style>
        body{font-family:-apple-system,sans-serif;color:#0C1F1A}
        h1{color:#0B6E4F;font-size:22px} .warn{border:2px solid #B3261E;padding:10px;border-radius:8px}
        .v{font-family:Menlo,monospace;font-size:17px;direction:ltr} ol{columns:3;font-family:Menlo,monospace;direction:ltr}
        </style></head><body>
        <h1>\(esc(L("kit_file_title")))</h1><p class="warn">\(esc(L("kit_file_warn")))</p>
        <h3>\(esc(L("email")))</h3><p class="v">\(esc(kit.email))</p>
        <h3>\(esc(L("secret_key")))</h3><p class="v">\(esc(kit.secretKey))</p>
        <h3>\(esc(L("recovery_words")))</h3><ol>\(words)</ol>
        </body></html>
        """
        let info = UIPrintInfo(dictionary: nil)
        info.jobName = L("kit_file_title")
        info.outputType = .general
        let pc = UIPrintInteractionController.shared
        pc.printInfo = info
        pc.printFormatter = UIMarkupTextPrintFormatter(markupText: html)
        pc.present(animated: true)
    }
}
