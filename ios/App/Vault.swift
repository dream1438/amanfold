import AmanfoldKit
import SwiftUI

enum Filter: String, CaseIterable, Identifiable {
    case all, logins, cards, notes, trash
    var id: String { rawValue }
    var label: String {
        switch self {
        case .all: return L("all_items")
        case .logins: return L("logins")
        case .cards: return L("cards")
        case .notes: return L("notes_filter")
        case .trash: return L("trash")
        }
    }

    func keeps(_ i: ItemView) -> Bool {
        switch self {
        case .all, .trash: return true
        case .logins: return i.kind == .login
        case .cards: return i.kind == .card
        case .notes: return i.kind == .secureNote
        }
    }
}

struct VaultView: View {
    @EnvironmentObject var session: AmanfoldSession
    @State private var query = ""
    @State private var filter = Filter.all
    @State private var adding: ItemKind?
    @State private var settings = false
    @State private var sharing = false
    /// Invitations and vaults waiting for a safety-number check.
    @State private var asks = 0

    private var shown: [ItemView] {
        _ = session.revision
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return session.items(trashed: filter == .trash)
            .filter(self.filter.keeps)
            .filter { q.isEmpty || $0.titleText.lowercased().contains(q) || $0.subtitle.lowercased().contains(q) }
            .sorted { $0.titleText.localizedCaseInsensitiveCompare($1.titleText) == .orderedAscending }
    }

    var body: some View {
        NavigationStack {
            List {
                if let p = session.syncProblem {
                    Section { Banner(syncText(p), danger: p != .offline) }
                }
                if asks > 0 {
                    Section { Button(L("sharing_asks", asks)) { sharing = true } }
                }
                Section {
                    Picker(L("all_items"), selection: $filter) {
                        ForEach(Filter.allCases) { Text($0.label).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(Color.clear)
                }
                let items = shown
                if items.isEmpty {
                    Text(query.isEmpty ? (filter == .trash ? L("trash_empty") : L("empty")) : L("no_results"))
                        .foregroundStyle(Brand.muted)
                } else {
                    let anyShared = session.vaults().contains { $0.shared }
                    ForEach(items, id: \.id) { item in
                        NavigationLink(value: item.id) { ItemRow(item, vault: anyShared ? session.vaultOf(item.id) : nil) }
                    }
                }
            }
            .searchable(text: $query, prompt: L("search"))
            .refreshable { try? await session.sync() }
            .navigationTitle(L("app_name"))
            .navigationDestination(for: String.self) { ItemDetailView(id: $0) }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { settings = true } label: { Image(systemName: "gearshape") }
                        .accessibilityLabel(L("settings"))
                }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button { sharing = true } label: { Image(systemName: "person.2") }
                        .accessibilityLabel(L("sharing"))
                    Button { session.lock() } label: { Image(systemName: "lock") }
                        .accessibilityLabel(L("lock"))
                    Menu {
                        Button(L("new_login"), systemImage: "key") { adding = .login }
                        Button(L("new_card"), systemImage: "creditcard") { adding = .card }
                        Button(L("new_note"), systemImage: "note.text") { adding = .secureNote }
                    } label: { Image(systemName: "plus") }
                        .accessibilityLabel(L("add"))
                }
            }
            .sheet(item: $adding) { kind in
                NavigationStack { EditItemView(kind: kind, existing: nil) }
            }
            .sheet(isPresented: $settings) { SettingsView() }
            .sheet(isPresented: $sharing, onDismiss: { Task { await countAsks() } }) {
                NavigationStack { SharingScreen() }
            }
            .task {
                try? await session.sync()
                await countAsks()
            }
        }
    }

    private func countAsks() async {
        if let v = try? await session.sharing() { asks = v.invites.count + v.waiting.count }
    }

    private func syncText(_ p: SyncProblem) -> String {
        switch p {
        case .offline: return L("sync_offline")
        case .signedOut: return L("err_signed_out")
        case .rollback: return L("err_rollback")
        case .conflict, .other: return L("err_server", "sync")
        }
    }
}

extension ItemKind: @retroactive Identifiable {
    public var id: Self { self }
}

// MARK: Detail

struct ItemDetailView: View {
    @EnvironmentObject var session: AmanfoldSession
    @Environment(\.dismiss) private var dismiss
    let id: String
    @State private var editing = false
    @State private var confirmTrash = false
    @State private var toast: String?
    @State private var error: String?

    var body: some View {
        let _ = session.revision
        if let item = session.item(id) {
            let vault = session.vaultOf(id)
            let writable = vault?.readOnly != true
            let targets = session.vaults().filter { $0.id != vault?.id && !$0.readOnly }
            List {
                Section {
                    HStack(spacing: 14) {
                        Image(systemName: item.symbol).font(.title2).foregroundStyle(Brand.primary)
                        Text(item.titleText).font(.title3.weight(.semibold))
                    }
                    if let vault, vault.shared {
                        HStack {
                            VaultChip(vault)
                            if vault.readOnly { Text(L("read_only")).font(.footnote).foregroundStyle(Brand.muted) }
                        }
                    }
                }
                if let error { Section { Banner(error, danger: true) } }
                Section {
                    ForEach(rows(item), id: \.name) { f in
                        if f.name == "totp" {
                            TotpRow(secret: f.value, onCopy: copied)
                        } else {
                            FieldRow(label: label(f.name), value: f.value, secret: f.concealed || secretNames.contains(f.name),
                                     ltr: f.name != "notes" && f.name != "cardholder", onCopy: copied)
                        }
                    }
                }
                if let toast { Section { Text(toast).font(.footnote).foregroundStyle(Brand.muted) } }
                if writable {
                    Section {
                        if !item.trashed && !targets.isEmpty {
                            Menu(L("move_to")) {
                                ForEach(targets) { t in
                                    Button(t.shared ? vaultName(t.name) : L("personal_vault")) { move(to: t.id) }
                                }
                            }
                        }
                        if item.trashed {
                            Button(L("restore")) { Task { try? await session.trash(id, false); dismiss() } }
                        } else {
                            Button(L("delete"), role: .destructive) { confirmTrash = true }
                        }
                    }
                }
            }
            .navigationTitle(item.titleText)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if !item.trashed && writable { Button(L("edit")) { editing = true } }
            }
            .sheet(isPresented: $editing) {
                NavigationStack { EditItemView(kind: item.kind, existing: item) }
            }
            .confirmationDialog(L("delete_confirm"), isPresented: $confirmTrash, titleVisibility: .visible) {
                Button(L("delete"), role: .destructive) { Task { try? await session.trash(id, true); dismiss() } }
            }
        } else {
            Text(L("no_results")).foregroundStyle(Brand.muted)
        }
    }

    private func move(to vaultId: String) {
        Task {
            do {
                try await session.move(id, to: vaultId)
                dismiss()
            } catch {
                self.error = problem(of: error).text
            }
        }
    }

    private let secretNames: Set<String> = ["password", "card_number", "cvv", "pin"]

    private func rows(_ item: ItemView) -> [FieldView] {
        let order: [String]
        switch item.kind {
        case .login: order = ["username", "password", "totp", "url", "notes"]
        case .card: order = ["cardholder", "card_number", "exp_month", "exp_year", "cvv", "pin", "notes"]
        default: order = ["notes"]
        }
        return order.compactMap { n in item.fields.first { $0.name == n && !$0.value.isEmpty } }
    }

    private func label(_ name: String) -> String {
        switch name {
        case "url": return shortURLLabel
        case "totp": return L("one_time_code")
        default: return L(name)
        }
    }

    private var shortURLLabel: String { L("website") }

    private func copied() {
        toast = L("copied")
        Task {
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            toast = nil
        }
    }
}

struct FieldRow: View {
    let label: String
    let value: String
    let secret: Bool
    let ltr: Bool
    let onCopy: () -> Void
    @State private var shown = false

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 3) {
                Text(label).font(.caption).foregroundStyle(Brand.muted)
                if secret && !shown {
                    Text(verbatim: "••••••••").font(secretFont)
                } else if ltr {
                    LTR(androidAppPackage(savedUrl: value) ?? value, secret: secret)
                } else {
                    Text(verbatim: value).textSelection(.enabled)
                }
            }
            Spacer()
            if secret {
                Button { shown.toggle() } label: { Image(systemName: shown ? "eye.slash" : "eye") }
                    .buttonStyle(.borderless)
                    .accessibilityLabel(L(shown ? "hide" : "show"))
            }
            Button { Clipboard.copy(value); onCopy() } label: { Image(systemName: "doc.on.doc") }
                .buttonStyle(.borderless)
                .accessibilityLabel(L("copy"))
        }
    }
}

struct TotpRow: View {
    let secret: String
    let onCopy: () -> Void

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { _ in
            if let code = try? totpNow(secret: secret) {
                HStack {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(L("one_time_code")).font(.caption).foregroundStyle(Brand.muted)
                        LTR(String(code.code.prefix(3)) + " " + String(code.code.dropFirst(3)))
                            .font(.title3.monospacedDigit())
                    }
                    Spacer()
                    Text(L("seconds_left", Int(code.secondsRemaining))).font(.footnote.monospacedDigit())
                        .foregroundStyle(code.secondsRemaining <= 5 ? Brand.danger : Brand.muted)
                    Button { Clipboard.copy(code.code); onCopy() } label: { Image(systemName: "doc.on.doc") }
                        .buttonStyle(.borderless)
                        .accessibilityLabel(L("copy"))
                }
            } else {
                Text(L("totp_invalid")).foregroundStyle(Brand.danger)
            }
        }
    }
}

// MARK: Edit

struct EditItemView: View {
    @EnvironmentObject var session: AmanfoldSession
    @Environment(\.dismiss) private var dismiss
    let kind: ItemKind
    let existing: ItemView?

    @State private var values: [String: String] = [:]
    @State private var loaded = false
    @State private var generator = false
    @State private var busy = false
    @State private var error: String?
    /// Where a new item goes; the personal vault unless changed.
    @State private var target: String?

    private var names: [String] {
        switch kind {
        case .login: return ["title", "username", "password", "url", "totp", "notes"]
        case .card: return ["title", "cardholder", "card_number", "exp_month", "exp_year", "cvv", "pin", "notes"]
        default: return ["title", "notes"]
        }
    }

    private func binding(_ n: String) -> Binding<String> {
        Binding(get: { values[n] ?? "" }, set: { values[n] = $0 })
    }

    var body: some View {
        Form {
            let choices = existing == nil ? session.vaults().filter { !$0.readOnly } : []
            if choices.count > 1 {
                Section {
                    Picker(L("vault_label"), selection: $target) {
                        ForEach(choices) { c in
                            Text(c.shared ? vaultName(c.name) : L("personal_vault")).tag(Optional(c.id))
                        }
                    }
                }
            }
            Section {
                TextField(L("title"), text: binding("title"))
            }
            switch kind {
            case .login:
                Section {
                    plain("username", content: .username)
                    HStack {
                        SecretField(L("password"), text: binding("password"))
                        Button(L("generate")) { generator = true }.buttonStyle(.borderless)
                    }
                    plain("url", content: .URL, keyboard: .URL, label: L("website"))
                }
                Section {
                    plain("totp", content: .oneTimeCode, label: L("totp"))
                } footer: { Text(L("totp_hint")) }
            case .card:
                Section {
                    TextField(L("cardholder"), text: binding("cardholder")).textContentType(.name)
                    plain("card_number", content: .creditCardNumber, keyboard: .numberPad)
                    HStack {
                        plain("exp_month", keyboard: .numberPad)
                        plain("exp_year", keyboard: .numberPad)
                    }
                    SecretField(L("cvv"), text: binding("cvv")).keyboardType(.numberPad)
                    SecretField(L("pin"), text: binding("pin")).keyboardType(.numberPad)
                }
            default:
                EmptyView()
            }
            Section(L("notes")) {
                TextField(L("notes"), text: binding("notes"), axis: .vertical).lineLimit(3...12)
            }
            if let error { Section { Banner(error, danger: true) } }
        }
        .navigationTitle(existing == nil ? newTitle : L("edit"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(L("cancel")) { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button(busy ? L("working") : L("save"), action: save).disabled(busy)
            }
        }
        .sheet(isPresented: $generator) {
            NavigationStack {
                GeneratorView { values["password"] = $0; generator = false }
            }
        }
        .onAppear {
            guard !loaded else { return }
            loaded = true
            for f in existing?.fields ?? [] { values[f.name] = f.value }
            target = session.vaults().first { !$0.shared }?.id
        }
    }

    private var newTitle: String {
        switch kind {
        case .login: return L("new_login")
        case .card: return L("new_card")
        default: return L("new_note")
        }
    }

    private func plain(_ n: String, content: UITextContentType? = nil, keyboard: UIKeyboardType = .default,
                       label: String? = nil) -> some View {
        TextField(label ?? L(n), text: binding(n))
            .textContentType(content)
            .keyboardType(keyboard)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .environment(\.layoutDirection, .leftToRight)
    }

    private func save() {
        error = nil
        var v = values.mapValues { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        if let n = v["card_number"], !n.isEmpty {
            guard let info = try? cardInfo(number: n), info.luhnValid else { error = L("card_invalid"); return }
            v["card_number"] = info.digits
        }
        if let t = v["totp"], !t.isEmpty, (try? totpNow(secret: t)) == nil {
            error = L("totp_invalid"); return
        }
        if kind == .login, let u = v["url"], !u.isEmpty, !u.contains("://") { v["url"] = "https://" + u }
        if (v["title"] ?? "").isEmpty {
            v["title"] = kind == .login ? (v["url"].map(shortURL) ?? "") : ""
        }
        let set = names.compactMap { n in v[n].flatMap { $0.isEmpty ? nil : FieldInput(name: n, value: $0) } }
        let remove = names.filter { n in (v[n] ?? "").isEmpty && existing?.value(n) != nil }
        busy = true
        Task {
            defer { busy = false }
            do {
                if let existing {
                    try await session.update(existing.id, set: set, remove: remove)
                } else {
                    try await session.create(kind, set, in: target)
                }
                dismiss()
            } catch {
                self.error = problem(of: error).text
            }
        }
    }
}
