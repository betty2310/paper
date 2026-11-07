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
        /// The paper's preview at the start of its row: its whole first
        /// figure fitted inside, or the first letter of its title. Wider than
        /// tall, because most figures are.
        static let rowPreviewWidth: CGFloat = 56
        static let rowPreviewHeight: CGFloat = 40
        /// White space between a fitted figure and the tile's edge.
        static let rowPreviewInset: CGFloat = 4
        static let previewRadius: CGFloat = 6
        /// The larger preview at the top of the inspector.
        static let inspectorPreviewMaxHeight: CGFloat = 200
        static let inspectorLetterHeight: CGFloat = 96
        /// The longer side of a cached figure image, in pixels: enough for
        /// the inspector at 2x.
        static let previewPixels: CGFloat = 720
        /// A small figure is drawn at most this many pixels per point.
        static let previewMaxScale: CGFloat = 4
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
        static let settingsHeight: CGFloat = 360
        /// The sheet that edits a paper's fields or takes a new paper's.
        static let metadataEditorWidth: CGFloat = 520
        static let metadataEditorHeight: CGFloat = 640
        /// The title and subtitle at the start of the toolbar, placed where
        /// the system puts its own: baselines from the top and between lines.
        static let windowTitleHeight: CGFloat = 32
        static let windowTitleBaseline: CGFloat = 14
        static let windowSubtitleBaseline: CGFloat = 14
        static let windowTitleInset: CGFloat = 3.75
        /// The tabs sit in a track as tall as the title and subtitle, one
        /// per open PDF after the Library tab.
        static let tabTrackHeight: CGFloat = windowTitleHeight
        static let tabTrackInset: CGFloat = 2
        static let tabHeight: CGFloat = tabTrackHeight - tabTrackInset * 2
        /// The narrowest a crowded tab gets: its icon and padding.
        static let tabMinWidth: CGFloat = 36
        /// Narrower than this, a tab shows only its icon.
        static let tabCompactWidth: CGFloat = 100
        static let tabPadding: CGFloat = 10
        static let tabSpacing: CGFloat = 2
        static let tabCloseSize: CGFloat = 16
        /// A pinned tab's letter tile, the size of a tab icon.
        static let tabPinnedRadius: CGFloat = 4
        /// The star at the end of a paper's title line: a round target
        /// around a 12 pt symbol.
        static let favoriteButtonSize: CGFloat = 20
        static let favoriteSymbolSize: CGFloat = 12
        /// How far the sidebar's lists and a PDF's table of contents travel
        /// as one replaces the other.
        static let sidebarSlide: CGFloat = 24
        /// A table of contents entry counts as on screen when it starts this
        /// far below the top of the view, in PDF points.
        static let outlinePositionSlop: CGFloat = 8
        /// The sidebar's page strip: 88 px page images with the number under
        /// each, and a 2 px accent outline just outside the current page.
        static let thumbnailWidth: CGFloat = 88
        static let thumbnailPadding: CGFloat = 12
        static let thumbnailLabelHeight: CGFloat = 16
        static let thumbnailOutlineWidth: CGFloat = 2
        static let thumbnailOutlineGap: CGFloat = 2
        static let thumbnailOutlineRadius: CGFloat = 4
        /// The find bar's field, and room for "128 of 340" so it keeps its width.
        static let findFieldWidth: CGFloat = 180
        static let findCountWidth: CGFloat = 76
        /// The reader's floating controls, over the top of the page.
        static let readerBarInset: CGFloat = 16
        static let readerBarButton: CGFloat = 28
        /// Inside the capsule's ends and inside each button.
        static let readerBarPadding: CGFloat = 6
        static let readerBarSpacing: CGFloat = 2
        /// Room for "100%" and "12 of 340" so the bar keeps its width.
        static let readerBarZoomWidth: CGFloat = 48
        /// Reader zoom limits and step, as PDFKit scale factors.
        static let minZoom: CGFloat = 0.5
        static let maxZoom: CGFloat = 4
        static let zoomStep: CGFloat = 1.25
        /// The floating markup toolbar over a selection or a highlight.
        static let markupBarHeight: CGFloat = 36
        static let markupBarPadding: CGFloat = 8
        static let markupBarRadius: CGFloat = 8
        /// Space between the toolbar and the text it belongs to.
        static let markupBarOffset: CGFloat = 8
        static let markupDotSize: CGFloat = 16
        /// The dot's hit area, larger than the dot.
        static let markupDotTarget: CGFloat = 24
        static let markupRingWidth: CGFloat = 2
        static let markupButtonHeight: CGFloat = 24
        static let markupButtonPadding: CGFloat = 8
        /// A sticky note's icon on the page, in PDF points.
        static let noteIconSize: CGFloat = 20
        static let noteEditorWidth: CGFloat = 280
        static let noteEditorHeight: CGFloat = 120

        /// The outline around the list while PDFs are dragged over it.
        static let dropRingWidth: CGFloat = 2

        static let chipRadius: CGFloat = 6
        static let cardRadius: CGFloat = 8
        static let noteMinHeight: CGFloat = 120
        /// The reader's annotation cards: a color bar at the leading edge,
        /// the quote cut off after a few lines.
        static let annotationBarWidth: CGFloat = 3
        static let annotationCardPadding: CGFloat = 12
        static let annotationQuoteLines = 6
        /// TextEditor insets its text by the line fragment padding.
        static let textEditorInset: CGFloat = 5
        /// Room for a list's bullet or "10." in the rendered note.
        static let listMarkerWidth: CGFloat = 16
    }

    @MainActor enum Color {
        static let text = NSColor.labelColor
        static let secondaryText = NSColor.secondaryLabelColor
        static let tertiaryText = NSColor.tertiaryLabelColor
        static let selectedText = NSColor.alternateSelectedControlTextColor
        static let accent = NSColor.controlAccentColor
        static let separator = NSColor.separatorColor
        static let listBackground = NSColor.controlBackgroundColor
        /// The bar above the list that names the search filter.
        static let filterBarBackground = NSColor.windowBackgroundColor
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
        /// The tab track, the raised active tab, and the other tabs' fills.
        static let tabTrack = NSColor.labelColor.withAlphaComponent(0.05)
        static let tabSelected = NSColor.controlBackgroundColor
        static let tabHover = NSColor.labelColor.withAlphaComponent(0.06)
        static let tabPressed = NSColor.labelColor.withAlphaComponent(0.1)
        /// The page strip: a page's paper behind its image while it draws,
        /// and the page border under the pointer and while pressed.
        static let thumbnailPage = NSColor.white
        static let thumbnailHoverBorder = NSColor.labelColor.withAlphaComponent(0.3)
        static let thumbnailPressedBorder = NSColor.labelColor.withAlphaComponent(0.5)
        /// Find matches on the page; the current one is the selection.
        static let findMatch = NSColor.findHighlightColor
        static let swiftUIReaderBackground = SwiftUI.Color(nsColor: readerBackground)
        /// Toasts: an opaque capsule, white in Light Mode, with a gray close button.
        static let swiftUIToastBackground = SwiftUI.Color(nsColor: .controlBackgroundColor)
        static let swiftUIToastTrack = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.1))
        static let swiftUIToastClose = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.06))
        static let swiftUIToastCloseHover = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.1))
        static let swiftUIToastClosePressed = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.16))
        /// Hover and pressed fills of small icon buttons, such as the
        /// favorite star and the markup toolbar's buttons.
        static let controlHover = NSColor.labelColor.withAlphaComponent(0.06)
        static let controlPressed = NSColor.labelColor.withAlphaComponent(0.12)
        static let swiftUIControlHover = SwiftUI.Color(nsColor: controlHover)
        static let swiftUIControlPressed = SwiftUI.Color(nsColor: controlPressed)
        /// The ring around a color dot under the pointer.
        static let swiftUIDotHoverRing = SwiftUI.Color(nsColor: NSColor.labelColor.withAlphaComponent(0.25))
        static let swiftUIPanel = SwiftUI.Color(nsColor: .controlBackgroundColor)
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
        /// The letter on a pinned tab's tile.
        static let tabPinnedLetter = serif(size: 11, weight: .semibold)
        /// The selected paper's title in the inspector.
        static let inspectorTitle = serif(size: 20, weight: .semibold)
        /// The window title and subtitle in the toolbar, as the system draws them.
        static let windowTitle = NSFont.systemFont(ofSize: NSFont.systemFontSize, weight: .bold)
        static let windowSubtitle = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
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
        /// An annotation's comment, and quotes in the rendered note.
        static let swiftUIComment = SwiftUI.Font.body.italic()
        static let swiftUICode = SwiftUI.Font.system(size: 12, design: .monospaced)
        /// Headings in the rendered note.
        static let swiftUINoteHeading1 = SwiftUI.Font.system(size: 20, weight: .semibold)
        static let swiftUINoteHeading2 = SwiftUI.Font.system(size: 16, weight: .semibold)
        static let swiftUINoteHeading3 = SwiftUI.Font.system(size: 13, weight: .semibold)
        static let swiftUIToastMessage = SwiftUI.Font.system(size: 13, weight: .medium)
        static let swiftUIToastDetail = SwiftUI.Font.system(size: 12).monospacedDigit()
        static let swiftUIToastSubject = SwiftUI.Font.system(size: 12, weight: .regular, design: .serif)
        static let swiftUIToastAction = SwiftUI.Font.system(size: 12, weight: .semibold)
        static let swiftUIToastClose = SwiftUI.Font.system(size: 10, weight: .bold)
        static let swiftUIReaderBarIcon = SwiftUI.Font.system(size: 13, weight: .medium)
        static let swiftUIReaderBarLabel = SwiftUI.Font.system(size: 12, weight: .medium).monospacedDigit()
        static let swiftUIDropIcon = SwiftUI.Font.system(size: 28, weight: .light)
        static let swiftUIWelcomeTitle = SwiftUI.Font.system(size: 28, weight: .semibold, design: .serif)

        /// The letter on a preview tile of a paper without a figure.
        static let rowPreviewLetter = serif(size: 20, weight: .regular)
        static let swiftUIInspectorPreviewLetter = SwiftUI.Font.system(size: 48, weight: .regular, design: .serif)

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
        /// Keystrokes to wait before the find bar searches.
        static let findDelay: TimeInterval = 0.15
        /// Page turns to wait before the reading position is saved.
        static let readingPositionSave: TimeInterval = 1
    }

    /// Highlight and note colors, stored by hex in the library. Pastels that
    /// keep black text readable under PDFKit's multiply blending.
    enum Markup {
        struct Swatch: Identifiable, Hashable {
            let name: String
            let hex: String
            var id: String { hex }
            var color: NSColor { NSColor(hexString: hex)! }
        }

        static let swatches = [
            Swatch(name: "Yellow", hex: "#FFE58A"),
            Swatch(name: "Green", hex: "#CFE8B8"),
            Swatch(name: "Blue", hex: "#BFD9F2"),
            Swatch(name: "Pink", hex: "#F7C6D0"),
            Swatch(name: "Purple", hex: "#DCCBEE"),
        ]
        /// Notes are drawn in a stronger yellow so the icon stands out.
        static let noteColor = "#F5C04A"
        /// Every Paper annotation carries this `userName`, so annotations that
        /// came inside the file can be told apart.
        static let author = "Paper"
        static let shadowColor = SwiftUI.Color.black.opacity(0.08)
        static let shadowRadius: CGFloat = 8
        static let shadowY: CGFloat = 2
        static let pressedScale: CGFloat = 0.88
    }

    /// Paper previews: figures sit on white like the page they came from, and
    /// a paper without one gets its title's first letter on a soft color,
    /// picked from its title so it stays the same.
    enum Preview {
        struct Tile {
            let background: NSColor
            let letter: NSColor
            var swiftUIBackground: SwiftUI.Color { SwiftUI.Color(nsColor: background) }
            var swiftUILetter: SwiftUI.Color { SwiftUI.Color(nsColor: letter) }
        }

        static let figureBackground = NSColor.white
        /// The same white, for drawing figures off the main thread.
        nonisolated static let figureBackgroundCG = CGColor(gray: 1, alpha: 1)
        static let swiftUIFigureBackground = SwiftUI.Color(nsColor: figureBackground)
        static let border = NSColor.labelColor.withAlphaComponent(0.12)
        static let swiftUIBorder = SwiftUI.Color(nsColor: border)
        static let tiles = [
            ("#E3EAF5", "#3D5A8A"),
            ("#E4EFE0", "#4A6B3F"),
            ("#F4E9DA", "#8A5E2E"),
            ("#F3E3E8", "#8A4357"),
            ("#EAE4F3", "#5E4A86"),
            ("#DFEEEC", "#36706A"),
        ].map { Tile(background: NSColor(hexString: $0.0)!, letter: NSColor(hexString: $0.1)!) }
    }

    /// The raised active tab's soft shadow.
    enum Tab {
        static let shadowOpacity: Float = 0.08
        static let shadowRadius: CGFloat = 2
        static let shadowY: CGFloat = 1
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
        static let back = "chevron.left"
        static let forward = "chevron.right"
        static let note = "note.text"
        static let highlight = "highlighter"
        static let fitWidth = "arrow.left.and.right"
        static let findPrevious = "chevron.up"
        static let findNext = "chevron.down"
        static let importPdf = "arrow.down.doc"
        static let preview = "eye"
        static let task = "square"
        static let taskDone = "checkmark.square.fill"
        static let favorite = "star"
        static let favoriteOn = "star.fill"
    }
}
