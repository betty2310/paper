import AppKit
import SwiftUI

/// Every size, color, font, and duration the views use, defined once.
///
/// Colors are the system's semantic colors, so the app follows Light and Dark
/// Mode and the user's accent color like any first party Mac app. Titles use
/// the system serif design (New York); everything else is San Francisco.
@MainActor
enum Theme {
    enum Size {
        static let windowWidth: CGFloat = 1180
        static let windowHeight: CGFloat = 760
        static let windowMinWidth: CGFloat = 760
        static let windowMinHeight: CGFloat = 440

        static let sidebarWidth: CGFloat = 220
        static let sidebarMinWidth: CGFloat = 180
        static let sidebarMaxWidth: CGFloat = 320
        static let inspectorWidth: CGFloat = 340
        static let inspectorMinWidth: CGFloat = 280
        static let inspectorMaxWidth: CGFloat = 520
        static let listMinWidth: CGFloat = 320

        /// The ⌘P search palette.
        static let paletteWidth: CGFloat = 600
        static let paletteFieldHeight: CGFloat = 56
        static let paletteRowHeight: CGFloat = 48
        static let paletteSectionHeight: CGFloat = 28
        static let paletteVisibleRows = 8
        static let paletteMaxResults = 30
        static let paletteRadius: CGFloat = 12
        /// Where the palette's top edge sits, as a fraction of the window height.
        static let paletteTopFraction: CGFloat = 0.16
        /// The strip above the list that names the active search filter.
        static let filterBarHeight: CGFloat = 32

        static let padding: CGFloat = 16
        static let gap: CGFloat = 8
        static let smallGap: CGFloat = 4
        static let iconSize: CGFloat = 16

        /// Two line paper rows: title and metadata.
        static let rowHeight: CGFloat = 52
        static let rowInset: CGFloat = 12
        static let rowTrailingWidth: CGFloat = 56
        static let statusIndicatorSize: CGFloat = 14
        static let sidebarRowHeight: CGFloat = 28
        static let sidebarGroupHeight: CGFloat = 24
        static let sidebarBadgeMinWidth: CGFloat = 20
        static let sidebarFooterHeight: CGFloat = 36
        static let tagDotSize: CGFloat = 10

        static let popoverWidth: CGFloat = 360
        static let welcomeWidth: CGFloat = 520
        static let welcomeHeight: CGFloat = 420
        static let settingsWidth: CGFloat = 460
        static let chipRadius: CGFloat = 6
        static let cardRadius: CGFloat = 8
        static let noteMinHeight: CGFloat = 120
    }

    @MainActor enum Color {
        static let text = NSColor.labelColor
        static let secondaryText = NSColor.secondaryLabelColor
        static let tertiaryText = NSColor.tertiaryLabelColor
        static let selectedText = NSColor.alternateSelectedControlTextColor
        static let accent = NSColor.controlAccentColor
        static let separator = NSColor.separatorColor
        static let listBackground = NSColor.controlBackgroundColor
        static let inspectorBackground = NSColor.windowBackgroundColor
        static let warning = NSColor.systemOrange
        static let danger = NSColor.systemRed
        /// The tag color when the user never picked one.
        static let defaultTag = NSColor.systemGray

        static let swiftUIText = SwiftUI.Color(nsColor: text)
        static let swiftUISecondaryText = SwiftUI.Color(nsColor: secondaryText)
        static let swiftUITertiaryText = SwiftUI.Color(nsColor: tertiaryText)
        static let swiftUIAccent = SwiftUI.Color.accentColor
        static let swiftUIWarning = SwiftUI.Color(nsColor: warning)
        static let swiftUIDanger = SwiftUI.Color(nsColor: danger)
        static let swiftUICard = SwiftUI.Color(nsColor: .quaternarySystemFill)
        static let swiftUISeparator = SwiftUI.Color(nsColor: separator)
    }

    @MainActor enum Font {
        /// Paper titles in the list.
        static let rowTitle = serif(size: 15, weight: .regular)
        /// The selected paper's title in the inspector.
        static let inspectorTitle = serif(size: 20, weight: .semibold)
        static let body = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        static let secondary = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        static let caption = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize - 1, weight: .medium)
        static let sidebar = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        static let sidebarBadge = NSFont.monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .medium)
        static let monospaced = NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)

        static let swiftUIInspectorTitle = SwiftUI.Font.system(size: 20, weight: .semibold, design: .serif)
        static let swiftUIPaletteTitle = SwiftUI.Font.system(size: 15, weight: .regular, design: .serif)
        static let swiftUIPaletteField = SwiftUI.Font.system(size: 20, weight: .regular)
        static let swiftUIBody = SwiftUI.Font.body
        static let swiftUISecondary = SwiftUI.Font.callout
        static let swiftUICaption = SwiftUI.Font.caption
        static let swiftUISectionLabel = SwiftUI.Font.caption.weight(.semibold)
        static let swiftUIWelcomeTitle = SwiftUI.Font.system(size: 28, weight: .semibold, design: .serif)

        private static func serif(size: CGFloat, weight: NSFont.Weight) -> NSFont {
            let base = NSFont.systemFont(ofSize: size, weight: weight)
            guard let descriptor = base.fontDescriptor.withDesign(.serif),
                  let font = NSFont(descriptor: descriptor, size: size)
            else { return base }
            return font
        }
    }

    enum Duration {
        /// Panels opening and closing.
        static let panel: TimeInterval = 0.15
        /// How long an informational notice stays before it fades.
        static let notice: TimeInterval = 6
        /// Keystrokes to wait before the note is saved.
        static let noteAutosave: TimeInterval = 0.8
    }

    enum Symbol {
        static let allPapers = "books.vertical"
        static let unread = "circle"
        static let recentlyAdded = "clock"
        static let noPdf = "doc.questionmark"
        static let collection = "folder"
        static let tag = "tag"
        static let addPaper = "plus"
        static let sort = "arrow.up.arrow.down"
        static let inspector = "sidebar.trailing"
        static let newCollection = "folder.badge.plus"
        static let pdf = "doc.richtext"
        static let failed = "exclamationmark.triangle.fill"
        static let more = "ellipsis.circle"
        static let link = "arrow.up.right.square"
        static let copy = "doc.on.doc"
        static let retry = "arrow.clockwise"
        static let library = "building.columns"
        static let openLibrary = "folder"
        static let newLibrary = "plus.rectangle.on.folder"
        static let close = "xmark"
        static let paper = "doc.text"
        static let edit = "pencil"
        static let delete = "trash"
        static let search = "magnifyingglass"
        static let showInList = "list.bullet"
        static let returnKey = "return"
        static let clear = "xmark.circle.fill"
    }
}
