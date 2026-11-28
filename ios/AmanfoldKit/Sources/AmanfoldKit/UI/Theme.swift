#if os(iOS)
import SwiftUI
import UIKit

/// Brand colours (brand/tokens/tokens.json), light and dark. Amber is the one
/// highlight per screen.
public enum Brand {
    static func dyn(_ light: UInt32, _ dark: UInt32) -> Color {
        Color(UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: dark) : UIColor(hex: light) })
    }

    public static let primary = dyn(0x0B6E4F, 0x2CB183)
    public static let onPrimary = dyn(0xFFFFFF, 0x0C1F1A)
    public static let mint = Color(UIColor(hex: 0x9BE4C4))
    public static let background = dyn(0xF4F6F3, 0x081410)
    public static let surface = dyn(0xFFFFFF, 0x0C1F1A)
    public static let text = dyn(0x0C1F1A, 0xEAEEEA)
    public static let muted = dyn(0x55655F, 0x93A29B)
    public static let border = dyn(0xDDE3DE, 0x1F302A)
    public static let danger = dyn(0xB3261E, 0xF2877B)
    public static let dangerBg = dyn(0xFBE9E7, 0x4A1410)
    public static let infoBg = dyn(0xEAF7F1, 0x0A3326)
    public static let amber = Color(UIColor(hex: 0xF5B544))
}

extension UIColor {
    convenience init(hex: UInt32) {
        self.init(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                  blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}

/// Passwords, codes and the Secret Key: monospaced, so 0/O and 1/l differ.
public let secretFont = Font.system(.body, design: .monospaced)

public struct Banner: View {
    let text: String
    let danger: Bool
    public init(_ text: String, danger: Bool = false) {
        self.text = text
        self.danger = danger
    }

    public var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(danger ? Brand.danger : Brand.text)
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(danger ? Brand.dangerBg : Brand.infoBg, in: RoundedRectangle(cornerRadius: 12))
            .accessibilityAddTraits(.isStaticText)
    }
}

/// The Keyhole a (brand/logo/amanfold-mark.svg), drawn natively.
public struct Mark: View {
    public init() {}
    public var body: some View {
        Canvas { ctx, size in
            let s = min(size.width / 50, size.height / 48)
            var t = CGAffineTransform(scaleX: s, y: s).translatedBy(x: -6, y: -8)
            ctx.fill(Path(Self.body(&t)), with: .color(Color(UIColor(hex: 0x0B6E4F))), style: FillStyle(eoFill: true))
            ctx.fill(Path(Self.fold(&t)), with: .color(Brand.mint))
        }
        .aspectRatio(50.0 / 48.0, contentMode: .fit)
        .accessibilityHidden(true)
    }

    static func body(_ t: inout CGAffineTransform) -> CGPath {
        let p = CGMutablePath()
        // Outer: the folded tile edge and the bowl of the a.
        p.move(to: CGPoint(x: 44, y: 8.8), transform: t)
        p.addLine(to: CGPoint(x: 56, y: 20.8), transform: t)
        p.addLine(to: CGPoint(x: 56, y: 53.2), transform: t)
        p.addQuadCurve(to: CGPoint(x: 54, y: 55.2), control: CGPoint(x: 56, y: 55.2), transform: t)
        p.addLine(to: CGPoint(x: 44, y: 55.2), transform: t)
        p.addLine(to: CGPoint(x: 44, y: 51.49), transform: t)
        // The bowl: a circle of radius 24 around (30, 32), open on the right.
        p.addArc(center: CGPoint(x: 30, y: 32), radius: 24, startAngle: .pi * 0.3016, endAngle: -.pi * 0.3016,
                 clockwise: false, transform: t)
        p.closeSubpath()
        // The keyhole: a circle of radius 8 around (30, 27) and its stem, one outline.
        p.move(to: CGPoint(x: 26.4, y: 34.14), transform: t)
        p.addArc(center: CGPoint(x: 30, y: 27), radius: 8, startAngle: .pi * 0.6489, endAngle: .pi * 2.3511,
                 clockwise: false, transform: t)
        p.addLine(to: CGPoint(x: 35.99, y: 45.53), transform: t)
        p.addQuadCurve(to: CGPoint(x: 34.8, y: 47), control: CGPoint(x: 36.3, y: 47), transform: t)
        p.addLine(to: CGPoint(x: 25.2, y: 47), transform: t)
        p.addQuadCurve(to: CGPoint(x: 24.01, y: 45.53), control: CGPoint(x: 23.7, y: 47), transform: t)
        p.closeSubpath()
        return p
    }

    static func fold(_ t: inout CGAffineTransform) -> CGPath {
        let p = CGMutablePath()
        p.move(to: CGPoint(x: 44, y: 8.8), transform: t)
        p.addLine(to: CGPoint(x: 44, y: 18.8), transform: t)
        p.addQuadCurve(to: CGPoint(x: 46, y: 20.8), control: CGPoint(x: 44, y: 20.8), transform: t)
        p.addLine(to: CGPoint(x: 56, y: 20.8), transform: t)
        p.closeSubpath()
        return p
    }
}

public struct Logo: View {
    public init() {}
    public var body: some View {
        HStack(spacing: 10) {
            Mark().frame(height: 40)
            // The English logo everywhere for now, also on Arabic screens.
            Text(verbatim: "Amanfold").font(.largeTitle.weight(.semibold)).foregroundStyle(Brand.text)
            Text(L("beta"))
                .font(.caption2.weight(.semibold))
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(Brand.infoBg, in: RoundedRectangle(cornerRadius: 6))
                .foregroundStyle(Brand.primary)
        }
        .environment(\.layoutDirection, .leftToRight)
    }
}

/// A Latin value (password, code, address): always left to right.
public struct LTR: View {
    let text: String
    let secret: Bool
    public init(_ text: String, secret: Bool = false) {
        self.text = text
        self.secret = secret
    }

    public var body: some View {
        Text(verbatim: text)
            .font(secret ? secretFont : .body)
            .environment(\.layoutDirection, .leftToRight)
            .textSelection(.disabled)
    }
}
#endif
