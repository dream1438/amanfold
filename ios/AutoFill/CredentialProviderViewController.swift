import AmanfoldKit
import AuthenticationServices
import SwiftUI

/// The AutoFill sheet iOS shows over Safari or an app. It offers only the
/// logins the core matches to the page (same rules as the browser
/// extension); anything else needs a search and a clear confirmation.
///
/// iOS's QuickType bar suggestions (the credential identity store) are left
/// empty on purpose: they would list site names and usernames outside the
/// encrypted vault.
final class CredentialProviderViewController: ASCredentialProviderViewController {
    private lazy var session = Shared.makeSession()

    override func prepareCredentialList(for serviceIdentifiers: [ASCredentialServiceIdentifier]) {
        let pages = serviceIdentifiers.compactMap { pageURL(serviceIdentifier: $0.identifier, isURL: $0.type == .URL) }
        let root = PickerRoot(
            session: session,
            page: pages.first,
            onPick: { [weak self] item in self?.fill(item) },
            onCancel: { [weak self] in self?.cancel(.userCanceled) }
        )
        let host = UIHostingController(rootView: root.tint(Brand.primary))
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view)
        host.didMove(toParent: self)
    }

    override func provideCredentialWithoutUserInteraction(for credentialIdentity: ASPasswordCredentialIdentity) {
        cancel(.userInteractionRequired)
    }

    private func fill(_ item: ItemView) {
        let credential = ASPasswordCredential(user: item.value("username") ?? "", password: item.value("password") ?? "")
        extensionContext.completeRequest(withSelectedCredential: credential) { _ in }
        session.lock()
    }

    private func cancel(_ code: ASExtensionError.Code) {
        session.lock()
        extensionContext.cancelRequest(withError: NSError(domain: ASExtensionErrorDomain, code: code.rawValue))
    }
}

struct PickerRoot: View {
    @ObservedObject var session: AmanfoldSession
    let page: String?
    let onPick: (ItemView) -> Void
    let onCancel: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                switch session.state {
                case .unlocked: LoginPicker(session: session, page: page, onPick: onPick)
                case .locked: UnlockView(session: session)
                case .noAccount:
                    Centered { Text(L("fill_locked")).multilineTextAlignment(.center) }
                }
            }
            .navigationTitle(L("fill_with"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button(L("cancel"), action: onCancel) } }
        }
    }
}

struct Centered<Content: View>: View {
    @ViewBuilder let content: Content
    var body: some View {
        VStack { Spacer(); content.padding(24); Spacer() }.frame(maxWidth: .infinity)
    }
}

struct LoginPicker: View {
    @ObservedObject var session: AmanfoldSession
    let page: String?
    let onPick: (ItemView) -> Void
    @State private var query = ""
    @State private var searching = false
    @State private var confirm: ItemView?

    private var site: SiteView? { page.flatMap { pageSite(pageUrl: $0) } }

    var body: some View {
        let matches = page.map { session.logins(forPage: $0).map(\.item) } ?? []
        let lookalikes = page.map { session.lookalikes(forPage: $0) } ?? []
        List {
            if let site {
                Section {
                    LTR(site.displayHost).font(.headline)
                    if site.displayHost != site.host { Banner(L("punycode", site.host), danger: true) }
                    if !site.secure { Banner(L("insecure"), danger: true) }
                    ForEach(lookalikes, id: \.self) { Banner(L("lookalike", $0), danger: true) }
                }
            }
            Section(site.map { L("for_this_site", $0.site) } ?? L("logins")) {
                if matches.isEmpty {
                    Text(L("none_for_site")).foregroundStyle(Brand.muted)
                }
                ForEach(matches, id: \.id) { item in
                    Button { onPick(item) } label: { ItemRow(item) }.foregroundStyle(Brand.text)
                }
            }
            Section {
                if searching {
                    TextField(L("search"), text: $query)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    ForEach(others(excluding: Set(matches.map(\.id))), id: \.id) { item in
                        Button { confirm = item } label: { ItemRow(item) }.foregroundStyle(Brand.text)
                    }
                } else {
                    Button(L("search_all")) { searching = true }
                }
            }
        }
        .alert(L("confirm_any_site", site?.displayHost ?? "?"), isPresented: Binding(
            get: { confirm != nil }, set: { if !$0 { confirm = nil } }
        )) {
            Button(L("fill_anyway"), role: .destructive) { if let c = confirm { onPick(c) } }
            Button(L("cancel"), role: .cancel) {}
        }
    }

    private func others(excluding: Set<String>) -> [ItemView] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return session.items()
            .filter { $0.kind == .login && !excluding.contains($0.id) }
            .filter { q.isEmpty || $0.titleText.lowercased().contains(q) || $0.subtitle.lowercased().contains(q) }
            .sorted { $0.titleText.localizedCaseInsensitiveCompare($1.titleText) == .orderedAscending }
    }
}
