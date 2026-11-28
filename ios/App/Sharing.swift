import AmanfoldKit
import SwiftUI

// Family and team sharing: the same screens and rules as the Android app.

extension Role {
    var label: String {
        switch self {
        case .owner: return L("role_owner")
        case .manager: return L("role_manager")
        case .writer: return L("role_writer")
        case .reader: return L("role_reader")
        }
    }

    var hint: String {
        switch self {
        case .owner: return L("role_owner_hint")
        case .manager: return L("role_manager_hint")
        case .writer: return L("role_writer_hint")
        case .reader: return L("role_reader_hint")
        }
    }
}

/// A safety number: Latin digits, always left to right.
struct SafetyNumber: View {
    let value: String
    var body: some View {
        LTR(value)
            .font(.title3.monospacedDigit())
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Brand.infoBg, in: RoundedRectangle(cornerRadius: 10))
    }
}

/// What to compare, and with whom, before trusting them.
struct Compare: Identifiable {
    let id = UUID()
    let email: String
    let fingerprint: String
    let confirm: String
    let action: () -> Void
}

/// Asks the person to compare a safety number with the other person, in
/// person or on a call.
struct CompareSheet: View {
    let compare: Compare
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text(L("compare_body", compare.email))
                    SafetyNumber(value: compare.fingerprint).listRowInsets(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                } footer: { Text(L("compare_warning")) }
                Section {
                    Button(compare.confirm) {
                        dismiss()
                        compare.action()
                    }
                    Button(L("they_dont_match"), role: .destructive) { dismiss() }
                }
            }
            .navigationTitle(L("compare_title"))
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.medium, .large])
    }
}

// MARK: Overview

struct SharingScreen: View {
    @EnvironmentObject var session: AmanfoldSession
    @Environment(\.dismiss) private var close
    @State private var view: SharingView?
    @State private var error: String?
    @State private var notice: String?
    @State private var compare: Compare?
    @State private var declining: InviteView?
    @State private var creating = false
    @State private var newName = ""
    @State private var newKind = VaultKind.family
    @State private var opened: String?

    var body: some View {
        List {
            if let error { Section { Banner(error, danger: true) } }
            if let notice { Section { Banner(notice) } }
            if let v = view {
                if !v.invites.isEmpty {
                    Section(L("invitations")) {
                        ForEach(v.invites) { inv in
                            VStack(alignment: .leading, spacing: 8) {
                                Text(L("invite_from", inv.fromEmail)).font(.headline)
                                Text(L("invite_role", inv.role.label)).font(.subheadline)
                                HStack {
                                    Button(L("compare_and_accept")) { accept(inv) }.buttonStyle(.borderedProminent)
                                    Button(L("decline")) { declining = inv }.buttonStyle(.borderless)
                                }
                            }
                            .padding(.vertical, 4)
                        }
                    }
                }
                if !v.waiting.isEmpty {
                    Section(L("waiting_title")) {
                        ForEach(v.waiting) { w in
                            VStack(alignment: .leading, spacing: 8) {
                                Text(L("waiting_body", w.email))
                                Button(L("compare_numbers")) { check(w) }.buttonStyle(.borderedProminent)
                            }
                            .padding(.vertical, 4)
                        }
                    }
                }
                Section(L("shared_vaults")) {
                    if v.vaults.isEmpty { Text(L("no_shared_vaults")).foregroundStyle(Brand.muted) }
                    ForEach(v.vaults) { sv in
                        NavigationLink(value: SharedVaultRoute(id: sv.id)) {
                            VStack(alignment: .leading, spacing: 4) {
                                Label(vaultName(sv.name), systemImage: sv.kind.symbol).font(.headline)
                                Text(L("vault_summary", sv.role.label, sv.items)).font(.footnote).foregroundStyle(Brand.muted)
                                if sv.needsVerification { Banner(L("needs_verification"), danger: true) }
                            }
                        }
                    }
                }
                Section {
                    SafetyNumber(value: v.myFingerprint)
                } header: { Text(L("your_safety_number")) } footer: { Text(L("your_safety_number_hint")) }
            } else if error == nil {
                ProgressView().frame(maxWidth: .infinity)
            }
        }
        .navigationTitle(L("sharing"))
        .navigationDestination(for: SharedVaultRoute.self) { MembersScreen(vaultId: $0.id) }
        .navigationDestination(item: $opened) { MembersScreen(vaultId: $0) }
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(L("done")) { close() } }
            ToolbarItem(placement: .primaryAction) { Button(L("new_vault")) { creating = true } }
        }
        .refreshable { await load() }
        .task { await load() }
        .sheet(item: $compare) { CompareSheet(compare: $0) }
        .confirmationDialog(
            declining.map { L("decline_confirm", $0.fromEmail) } ?? "",
            isPresented: Binding(get: { declining != nil }, set: { if !$0 { declining = nil } }),
            titleVisibility: .visible
        ) {
            Button(L("decline"), role: .destructive) {
                if let inv = declining { act { try await session.cancelInvite(inv.inviteId) } }
            }
        }
        .alert(L("new_vault"), isPresented: $creating) {
            TextField(L("vault_name"), text: $newName)
            Button(L("kind_family")) { create(.family) }
            Button(L("kind_team")) { create(.team) }
            Button(L("cancel"), role: .cancel) {}
        } message: { Text(L("new_vault_hint")) }
    }

    private func load() async {
        do {
            try? await session.sync()
            view = try await session.sharing()
            error = nil
        } catch {
            self.error = problem(of: error).text
        }
    }

    private func act(_ body: @escaping () async throws -> Void) {
        Task {
            error = nil
            do { try await body() } catch { self.error = problem(of: error).text }
            await load()
        }
    }

    private func accept(_ inv: InviteView) {
        compare = Compare(email: inv.fromEmail, fingerprint: inv.fromFingerprint, confirm: L("they_match_accept")) {
            act {
                try await session.acceptInvite(inv.inviteId, fingerprint: inv.fromFingerprint)
                notice = L("invite_accepted")
            }
        }
    }

    private func check(_ w: WaitingView) {
        compare = Compare(email: w.email, fingerprint: w.fingerprint, confirm: L("they_match")) {
            act { try await session.verify(w.vaultId, accountId: w.accountId, fingerprint: w.fingerprint) }
        }
    }

    private func create(_ kind: VaultKind) {
        let name = newName
        newName = ""
        act { opened = try await session.createVault(name: name, kind: kind) }
    }
}

struct SharedVaultRoute: Hashable { let id: String }

// MARK: One shared vault

struct MembersScreen: View {
    @EnvironmentObject var session: AmanfoldSession
    @Environment(\.dismiss) private var dismiss
    let vaultId: String
    @State private var view: MembersView?
    @State private var needsVerification = false
    @State private var error: String?
    @State private var busy = false
    @State private var email = ""
    @State private var inviteRole = Role.writer
    @State private var sent: String?
    @State private var selected: MemberView?
    @State private var leaving = false

    private var tag: VaultTag? {
        _ = session.revision
        return session.vaults().first { $0.id == vaultId }
    }

    var body: some View {
        if let tag {
            content(tag)
        } else {
            Text(L("no_results")).foregroundStyle(Brand.muted).onAppear { dismiss() }
        }
    }

    private func content(_ tag: VaultTag) -> some View {
        let manage = tag.role.canManage
        return List {
            if let error { Section { Banner(error, danger: true) } }
            if needsVerification { Section { Banner(L("needs_verification"), danger: true) } }
            Section {
                Text(L("your_role", tag.role.label))
            } footer: { Text(tag.role.hint) }
            if let v = view {
                Section(L("members")) {
                    ForEach(v.members.sorted(by: order)) { m in
                        Button { if !m.me { selected = m } } label: { memberRow(m, manage: manage) }
                            .foregroundStyle(Brand.text)
                            .disabled(m.me)
                    }
                }
                if manage {
                    Section {
                        TextField(L("email"), text: $email)
                            .keyboardType(.emailAddress)
                            .textContentType(.emailAddress)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        Picker(L("change_role"), selection: $inviteRole) {
                            ForEach(tag.role == .owner ? [Role.manager, .writer, .reader] : [Role.writer, .reader], id: \.self) {
                                Text($0.label).tag($0)
                            }
                        }
                        if let sent { Banner(sent) }
                        Button(busy ? L("working") : L("send_invite")) { invite() }
                            .disabled(busy || !email.contains("@"))
                    } header: { Text(L("invite_someone")) } footer: { Text(inviteRole.hint + " " + L("invite_hint")) }
                    if !v.invites.isEmpty {
                        Section(L("sent_invites")) {
                            ForEach(v.invites) { inv in
                                HStack {
                                    VStack(alignment: .leading) {
                                        Text(verbatim: inv.email).lineLimit(1)
                                        Text(inv.role.label).font(.footnote).foregroundStyle(Brand.muted)
                                    }
                                    Spacer()
                                    Button(L("cancel_invite")) { act { try await session.cancelInvite(inv.inviteId) } }
                                        .buttonStyle(.borderless)
                                }
                            }
                        }
                    }
                }
            } else if error == nil {
                ProgressView().frame(maxWidth: .infinity)
            }
            Section {
                Button(tag.role == .owner ? L("delete_vault") : L("leave_vault"), role: .destructive) { leaving = true }
            }
        }
        .navigationTitle(vaultName(tag.name))
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await load() }
        .task { await load() }
        .sheet(item: $selected) { m in
            MemberSheet(
                member: m,
                myRole: tag.role,
                onVerify: { act { try await session.verify(vaultId, accountId: m.accountId, fingerprint: m.fingerprint) } },
                onRole: { r in act { try await session.setRole(vaultId, accountId: m.accountId, role: r) } },
                onRemove: { act { try await session.removeMember(vaultId, accountId: m.accountId) } }
            )
        }
        .confirmationDialog(
            L(tag.role == .owner ? "delete_vault_confirm" : "leave_vault_confirm", vaultName(tag.name)),
            isPresented: $leaving,
            titleVisibility: .visible
        ) {
            Button(tag.role == .owner ? L("delete_vault") : L("leave_vault"), role: .destructive) {
                let owner = tag.role == .owner
                act {
                    if owner { try await session.deleteVault(vaultId) } else { try await session.leaveVault(vaultId) }
                    dismiss()
                }
            }
        }
    }

    private func order(_ a: MemberView, _ b: MemberView) -> Bool {
        if a.me != b.me { return a.me }
        let ra = Role.allCases.firstIndex(of: a.role)!, rb = Role.allCases.firstIndex(of: b.role)!
        return ra != rb ? ra < rb : a.email < b.email
    }

    private func memberRow(_ m: MemberView, manage: Bool) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(verbatim: m.me ? L("member_you", m.email) : m.email).font(.headline).lineLimit(1)
                Text(m.role.label).font(.footnote).foregroundStyle(Brand.muted)
                if !m.me && !m.active && manage { Text(L("pending_hint")).font(.footnote) }
            }
            Spacer()
            if m.me {
                EmptyView()
            } else if !m.active {
                Text(L("member_pending")).font(.caption.weight(.semibold)).foregroundStyle(Brand.danger)
            } else if m.verified {
                Image(systemName: "checkmark.seal.fill").foregroundStyle(Brand.primary).accessibilityLabel(L("member_verified"))
            } else {
                Text(L("member_unverified")).font(.caption.weight(.semibold)).foregroundStyle(Brand.danger)
            }
        }
    }

    private func load() async {
        do {
            view = try await session.members(vaultId)
            needsVerification = (try? await session.sharing().vaults.first { $0.id == vaultId }?.needsVerification) ?? false
            error = nil
        } catch {
            self.error = problem(of: error).text
        }
    }

    private func act(_ body: @escaping () async throws -> Void) {
        busy = true
        Task {
            error = nil
            do { try await body() } catch { self.error = problem(of: error).text }
            busy = false
            await load()
        }
    }

    private func invite() {
        let to = email
        let role = inviteRole
        act {
            try await session.invite(vaultId, email: to, role: role)
            email = ""
            sent = L("invite_sent")
        }
    }
}

/// One member: safety number, verify, role, remove. Dialogs are shown from
/// this sheet so they never race its dismissal.
struct MemberSheet: View {
    let member: MemberView
    let myRole: Role
    let onVerify: () -> Void
    let onRole: (Role) -> Void
    let onRemove: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var compare: Compare?
    @State private var removing = false

    var body: some View {
        let m = member
        // Owners change any role but their own; managers manage writers and readers.
        let canChange = m.role != .owner && (myRole == .owner || (myRole == .manager && m.role != .manager))
        let roles: [Role] = myRole == .owner ? [.manager, .writer, .reader] : [.writer, .reader]
        let giveAccess = !m.active && myRole.canManage
        NavigationStack {
            List {
                Section {
                    Text(m.role.label + " · " + m.role.hint).font(.footnote)
                }
                Section(L("safety_number")) {
                    SafetyNumber(value: m.fingerprint)
                    if !m.verified {
                        Button(L("compare_numbers")) {
                            compare = Compare(email: m.email, fingerprint: m.fingerprint,
                                              confirm: L(giveAccess ? "they_match_give_access" : "they_match")) {
                                dismiss()
                                onVerify()
                            }
                        }
                    }
                }
                if canChange && m.active {
                    Section(L("change_role")) {
                        ForEach(roles, id: \.self) { r in
                            Button {
                                dismiss()
                                if r != m.role { onRole(r) }
                            } label: {
                                HStack {
                                    Text(r.label).foregroundStyle(Brand.text)
                                    Spacer()
                                    if r == m.role { Image(systemName: "checkmark").foregroundStyle(Brand.primary) }
                                }
                            }
                        }
                    }
                }
                if canChange {
                    Section {
                        Button(L("remove"), role: .destructive) { removing = true }
                    }
                }
            }
            .navigationTitle(Text(verbatim: m.email))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button(L("done")) { dismiss() } } }
            .sheet(item: $compare) { CompareSheet(compare: $0) }
            .confirmationDialog(L("remove_member_title", m.email), isPresented: $removing, titleVisibility: .visible) {
                Button(L("remove"), role: .destructive) {
                    dismiss()
                    onRemove()
                }
            } message: { Text(L("remove_member_body")) }
        }
    }
}
