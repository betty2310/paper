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
        static let paletteRadius: CGFloat = 16
        /// Where the palette's top edge sits, as a fraction of the window height.
        static let paletteTopFraction: CGFloat = 0.16
        /// Room for the selection bar before a row's text.
        static let paletteRowInset: CGFloat = 24
        static let paletteSelectionBar: CGFloat = 3
        /// Key hints and the action badge in rows.
        static let paletteBadgeSize: CGFloat = 20
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
        /// Tabs in the toolbar, one per open PDF after the Library tab.
        static let tabHeight: CGFloat = 28
        static let tabMinWidth: CGFloat = 96
        static let tabMaxWidth: CGFloat = 240
        static let tabPadding: CGFloat = 10
        static let tabSpacing: CGFloat = 2
        static let tabCloseSize: CGFloat = 16
        /// The page indicator over the bottom of the reader.
        static let pageIndicatorInset: CGFloat = 16
        static let pageIndicatorPadding: CGFloat = 10
        static let pageIndicatorHeight: CGFloat = 28
        /// Reader zoom limits and step, as PDFKit scale factors.
        static let minZoom: CGFloat = 0.5
        static let maxZoom: CGFloat = 4
        static let zoomStep: CGFloat = 1.25

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
        /// The canvas around the pages in the reader.
        static let readerBackground = NSColor.underPageBackgroundColor
        /// Tab fills: hovered, pressed, and the active tab's accent tint.
        static let tabHover = NSColor.labelColor.withAlphaComponent(0.06)
        static let tabPressed = NSColor.labelColor.withAlphaComponent(0.1)
        static let swiftUIReaderBackground = SwiftUI.Color(nsColor: readerBackground)
        /// Toasts: an opaque capsule, white in Light Mode, with a gray close button.
        static let swiftUIToastBackground = SwiftUI.Color(nsColor: .controlBackgroundColor)
        static let swiftUIToastTrack = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.1))
        static let swiftUIToastClose = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.06))
        static let swiftUIToastCloseHover = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.1))
        static let swiftUIToastClosePressed = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.16))
        /// Keeps text on the palette's glass readable over busy windows.
        static let paletteTint = NSColor.windowBackgroundColor.withAlphaComponent(Opacity.glassTint)
    }

    /// Accent fills behind text: light enough that the text keeps its color.
    enum Opacity {
        static let tint: Double = 0.12
        static let hoverTint: Double = 0.18
        static let pressedTint: Double = 0.24
        static let glassTint: Double = 0.6
    }

    @MainActor enum Font {
        /// Paper titles in the list.
        static let rowTitle = serif(size: 15, weight: .regular)
        /// Paper titles in tabs.
        static let tabTitle = serif(size: 13, weight: .regular)
        static let pageIndicator = NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .medium)
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
        static let swiftUIPaletteIcon = SwiftUI.Font.system(size: 18, weight: .medium)
        static let swiftUIBody = SwiftUI.Font.body
        static let swiftUISecondary = SwiftUI.Font.callout
        static let swiftUICaption = SwiftUI.Font.caption
        static let swiftUISectionLabel = SwiftUI.Font.caption.weight(.semibold)
        static let swiftUIToastMessage = SwiftUI.Font.system(size: 13, weight: .medium)
        static let swiftUIToastDetail = SwiftUI.Font.system(size: 12).monospacedDigit()
        static let swiftUIToastSubject = SwiftUI.Font.system(size: 12, weight: .regular, design: .serif)
        static let swiftUIToastAction = SwiftUI.Font.system(size: 12, weight: .semibold)
        static let swiftUIToastClose = SwiftUI.Font.system(size: 10, weight: .bold)
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
        /// How long a finished toast stays before it leaves.
        static let toast: TimeInterval = 5
        /// One turn of the toast spinner.
        static let spinnerTurn: TimeInterval = 0.9
        /// The progress ring catching up with a download event.
        static let progressStep: TimeInterval = 0.12
        /// Hover fills fading in and out.
        static let hover: TimeInterval = 0.1
        /// Keystrokes to wait before the note is saved.
        static let noteAutosave: TimeInterval = 0.8
        /// A row flying into its new tab when a PDF opens.
        static let tabFlight: TimeInterval = 0.5
        /// A tab growing in or shrinking out of the tab strip.
        static let tabResize: TimeInterval = 0.25
        /// Cross fade between the library and a reader, or two readers.
        static let tabSwitch: TimeInterval = 0.2
        /// Pages fading in once PDFKit has the document.
        static let documentFade: TimeInterval = 0.15
        /// Page turns to wait before the reading position is saved.
        static let readingPositionSave: TimeInterval = 1
        /// How long the page indicator stays after scrolling stops.
        static let pageIndicator: TimeInterval = 1.5
    }

    /// The floating toasts in the window's bottom right corner.
    enum Toast {
        static let height: CGFloat = 40
        static let iconSize: CGFloat = 18
        /// The action capsule's height and the close button's diameter.
        static let controlSize: CGFloat = 24
        static let leadingPadding: CGFloat = 12
        static let trailingPadding: CGFloat = 8
        static let verticalPadding: CGFloat = 8
        static let actionPadding: CGFloat = 10
        static let lineSpacing: CGFloat = 1
        static let textMaxWidth: CGFloat = 280
        /// Every toast about a paper is this wide, so it never jumps as it updates.
        static let width: CGFloat = 360
        /// How far below its place a toast starts as it fades in.
        static let enterOffset: CGFloat = 12
        static let ringWidth: CGFloat = 2
        /// A started download shows at least a sliver of ring.
        static let ringMinimum: Double = 0.03
        static let spinnerArc: Double = 0.28
        static let focusWidth: CGFloat = 2
        static let disabledOpacity: Double = 0.4
        static let removalScale: CGFloat = 0.92
        static let iconPopScale: CGFloat = 0.5
        static let shadowColor = SwiftUI.Color.black.opacity(0.1)
        static let shadowRadius: CGFloat = 12
        static let shadowY: CGFloat = 4
        static let contactShadowColor = SwiftUI.Color.black.opacity(0.06)
        static let contactShadowRadius: CGFloat = 1
        static let contactShadowY: CGFloat = 0.5
        /// Toasts arriving, leaving, and moving up.
        static let spring = Animation.spring(duration: 0.4, bounce: 0.18)
        /// A toast's own text and icon changing.
        static let contentAnimation = Animation.smooth(duration: 0.25)
        /// At most this many toasts show; older ones leave first.
        static let maxCount = 4
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
        static let zoomIn = "plus.magnifyingglass"
        static let zoomOut = "minus.magnifyingglass"
        static let attach = "paperclip"
        static let findPdf = "arrow.down.doc"
        static let reveal = "folder"
    }
}
