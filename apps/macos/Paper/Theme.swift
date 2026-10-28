import AppKit

/// Every size, color, and font the views use, from docs/UI.md.
@MainActor
enum Theme {
    enum Color {
        static let canvas = NSColor(hex: 0xFBFAF7)
        static let panel = NSColor(hex: 0xFFFFFF)
        static let border = NSColor(hex: 0xE6E2DB)
        static let textPrimary = NSColor(hex: 0x1E1D1A)
        static let textSecondary = NSColor(hex: 0x5F5B54)
        static let textMuted = NSColor(hex: 0x8E8A82)
        static let accent = NSColor(hex: 0x2F4A7A)
        static let danger = NSColor(hex: 0xB4473A)
    }

    enum Size {
        static let padding: CGFloat = 16
        static let gap: CGFloat = 8
        static let toolbarHeight: CGFloat = 52
        static let statusBarHeight: CGFloat = 28
        static let rowHeight: CGFloat = 44
        static let titleColumnMinWidth: CGFloat = 320
        static let authorsColumnWidth: CGFloat = 200
        static let yearColumnWidth: CGFloat = 64
        static let windowWidth: CGFloat = 1100
        static let windowHeight: CGFloat = 720
    }

    /// Source Serif 4 and Inter are not bundled yet, so these fall back to the system font.
    enum Font {
        static let title = NSFont(name: "Source Serif 4", size: 15) ?? .systemFont(ofSize: 15)
        static let body = NSFont(name: "Inter", size: 13) ?? .systemFont(ofSize: 13)
        static let muted = NSFont(name: "Inter", size: 12) ?? .systemFont(ofSize: 12)
    }
}

private extension NSColor {
    convenience init(hex: UInt32) {
        self.init(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: 1
        )
    }
}
