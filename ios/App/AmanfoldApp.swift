import AmanfoldKit
import SwiftUI

@main
struct AmanfoldApp: App {
    @StateObject private var session = Shared.makeSession()
    @Environment(\.scenePhase) private var phase
    @AppStorage("autoLock", store: Shared.defaults) private var autoLockMinutes = 5
    @State private var backgroundSince: Date?

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .tint(Brand.primary)
                // Nothing readable in the app switcher.
                .overlay {
                    if phase != .active {
                        ZStack {
                            Brand.background
                            Mark().frame(width: 80)
                        }
                        .ignoresSafeArea()
                    }
                }
        }
        .onChange(of: phase) { _, now in
            switch now {
            case .background:
                backgroundSince = Date()
                if autoLockMinutes == 0 { session.lock() }
            case .active:
                if let since = backgroundSince, Date().timeIntervalSince(since) >= Double(autoLockMinutes * 60) {
                    session.lock()
                }
                backgroundSince = nil
            default:
                break
            }
        }
    }
}

struct RootView: View {
    @EnvironmentObject var session: AmanfoldSession
    @State private var kit: RecoveryKit?
    @State private var offerBiometrics = false
    @AppStorage("biometricsOffered", store: Shared.defaults) private var offered = false

    var body: some View {
        Group {
            if let kit {
                RecoveryKitView(kit: kit) {
                    self.kit = nil
                    offerBiometrics = Keychain.biometricsAvailable
                }
            } else {
                switch session.state {
                case .noAccount: OnboardingView(onCreated: { kit = $0 })
                case .locked: UnlockView(session: session, onSignOut: { Task { await signOut() } })
                case .unlocked: VaultView()
                }
            }
        }
        .background(Brand.background.ignoresSafeArea())
        .onChange(of: session.state) { _, s in
            if s == .unlocked, kit == nil, !offered, !session.biometricsOn, Keychain.biometricsAvailable { offerBiometrics = true }
        }
        .alert(L("biometric_enable_title"), isPresented: $offerBiometrics) {
            Button(L("turn_on")) {
                offered = true
                Task { try? await session.turnOnBiometrics() }
            }
            Button(L("not_now"), role: .cancel) { offered = true }
        } message: {
            Text(L("biometric_offer"))
        }
    }

    private func signOut() async {
        session.turnOffBiometrics()
        offered = false
        await session.signout()
    }
}
