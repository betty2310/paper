import SwiftUI

/// What the floating markup toolbar is about: fresh selected text, or a
/// highlight the user clicked.
@MainActor
final class MarkupBarModel: ObservableObject {
    enum Mode: Equatable {
        /// Dots highlight the selection; Copy copies it.
        case selection
        /// Dots recolor the highlight; the trash removes it.
        case highlight(color: String)
    }

    @Published var mode: Mode = .selection
    var pick: (Theme.Swatch) -> Void = { _ in }
    var copy: () -> Void = {}
    var delete: () -> Void = {}
}

/// Five color dots and one action, in a small card 8 pt above the text.
struct MarkupBar: View {
    @ObservedObject var model: MarkupBarModel

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Theme.Markup.swatches) { swatch in
                Button { model.pick(swatch) } label: { EmptyView() }
                    .buttonStyle(ColorDotStyle(color: swatch.swiftUIColor, selected: isCurrent(swatch)))
                    .help(model.mode == .selection ? "Highlight in \(swatch.name.lowercased())" : "Make \(swatch.name.lowercased())")
                    .accessibilityLabel(model.mode == .selection ? "Highlight \(swatch.name)" : swatch.name)
            }
            Divider()
                .frame(height: Theme.Size.markupDotSize)
                .padding(.horizontal, Theme.Size.smallGap)
            switch model.mode {
            case .selection:
                Button("Copy") { model.copy() }
                    .buttonStyle(MarkupTextButtonStyle())
                    .help("Copy the selected text")
            case .highlight:
                Button { model.delete() } label: {
                    Image(systemName: Theme.Symbol.delete)
                }
                .buttonStyle(MarkupTextButtonStyle())
                .help("Remove the highlight")
                .accessibilityLabel("Remove Highlight")
            }
        }
        .padding(.horizontal, Theme.Size.markupBarPadding)
        .frame(height: Theme.Size.markupBarHeight)
        .modifier(FloatingGlass(shape: RoundedRectangle(cornerRadius: Theme.Size.markupBarRadius, style: .continuous)) { content, shape in
            content
                .background(Theme.Color.swiftUIPanel, in: shape)
                .overlay(shape.strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1))
                .shadow(color: Theme.Markup.shadowColor, radius: Theme.Markup.shadowRadius, y: Theme.Markup.shadowY)
        })
        // Room for the shadow inside the hosting view.
        .padding(Theme.Markup.shadowRadius)
        .fixedSize()
    }

    private func isCurrent(_ swatch: Theme.Swatch) -> Bool {
        if case .highlight(let color) = model.mode { return color.caseInsensitiveCompare(swatch.hex) == .orderedSame }
        return false
    }
}

/// A sticky note's text, edited in a popover next to its icon.
@MainActor
final class NoteEditorModel: ObservableObject {
    @Published var text: String
    var delete: () -> Void = {}
    var done: () -> Void = {}

    init(text: String) {
        self.text = text
    }
}

struct NoteEditor: View {
    @ObservedObject var model: NoteEditorModel
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $model.text)
                    .font(Theme.Font.swiftUIBody)
                    .scrollContentBackground(.hidden)
                    .focused($focused)
                if model.text.isEmpty {
                    Text("Add a note…")
                        .font(Theme.Font.swiftUIBody)
                        .foregroundStyle(Theme.Color.swiftUITertiaryText)
                        // TextEditor insets its text by the line fragment padding.
                        .padding(.leading, 5)
                        .allowsHitTesting(false)
                }
            }
            .frame(height: Theme.Size.noteEditorHeight)
            HStack {
                Button(role: .destructive) { model.delete() } label: {
                    Image(systemName: Theme.Symbol.delete)
                }
                .buttonStyle(MarkupTextButtonStyle())
                .help("Delete the note")
                .accessibilityLabel("Delete Note")
                Spacer()
                Button { model.done() } label: {
                    Text("Done").foregroundStyle(Theme.Color.swiftUIAccent)
                }
                .buttonStyle(MarkupTextButtonStyle())
                    .keyboardShortcut(.return, modifiers: .command)
            }
        }
        .padding(Theme.Size.padding)
        .frame(width: Theme.Size.noteEditorWidth)
        .font(Theme.Font.swiftUIDefault)
        .onAppear { focused = true }
    }
}

/// The reader's state the floating controls show, and what they do.
@MainActor
final class ReaderBarModel: ObservableObject {
    /// 1 based; 0 until the document is open.
    @Published var page = 0
    @Published var pageCount = 0
    /// Where Back and Forward go, as 1 based page numbers, or nil.
    @Published var backPage: Int?
    @Published var forwardPage: Int?
    @Published var zoomPercent = 100
    @Published var fitsWidth = true
    @Published var canZoomIn = false
    @Published var canZoomOut = false
    @Published var canHighlight = false
    @Published var isPlacingNote = false
    /// The zoom controls step aside while the search is open in a reader
    /// too narrow for both.
    @Published var hidesZoom = false
    var back: () -> Void = {}
    var forward: () -> Void = {}
    var zoomIn: () -> Void = {}
    var zoomOut: () -> Void = {}
    var fitWidth: () -> Void = {}
    var highlight: () -> Void = {}
    var addNote: () -> Void = {}
}

/// The reader's controls in one capsule over the top of the page: Back and
/// Forward after following a link, the page, zoom, the two kinds of markup,
/// and Find, which grows the capsule to hold the search.
struct ReaderBar: View {
    @ObservedObject var model: ReaderBarModel
    @ObservedObject var find: ReaderFindModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: Theme.Size.readerBarSpacing) {
            Button(action: model.back) {
                HStack(spacing: Theme.Size.smallGap) {
                    Image(systemName: Theme.Symbol.back)
                    // Where Back goes, so a citation is one click from the
                    // sentence that cited it.
                    if let page = model.backPage {
                        Text("p. \(page)").font(Theme.Font.swiftUIReaderBarLabel)
                    }
                }
            }
            .buttonStyle(ReaderBarButtonStyle())
            .disabled(model.backPage == nil)
            .help(model.backPage.map { "Back to page \($0) (⌘[)" } ?? "Back (⌘[)")
            .accessibilityLabel(model.backPage.map { "Back to page \($0)" } ?? "Back")
            icon(Theme.Symbol.forward, "Forward", enabled: model.forwardPage != nil, action: model.forward)
                .help(model.forwardPage.map { "Forward to page \($0) (⌘])" } ?? "Forward (⌘])")
            separator
            Text(model.isPlacingNote ? "Click where the note goes" : "\(model.page) of \(model.pageCount)")
                .font(Theme.Font.swiftUIReaderBarLabel)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .padding(.horizontal, Theme.Size.smallGap)
                .contentTransition(.numericText())
                .accessibilityLabel(model.isPlacingNote ? "Click where the note goes" : "Page \(model.page) of \(model.pageCount)")
            if !model.hidesZoom {
                separator
                icon(Theme.Symbol.zoomOut, "Zoom Out", enabled: model.canZoomOut, action: model.zoomOut)
                    .help("Zoom out (⌘−)")
                Text("\(model.zoomPercent)%")
                    .font(Theme.Font.swiftUIReaderBarLabel)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .frame(width: Theme.Size.readerBarZoomWidth)
                    .contentTransition(.numericText())
                    .accessibilityLabel("Zoom \(model.zoomPercent) percent")
                icon(Theme.Symbol.zoomIn, "Zoom In", enabled: model.canZoomIn, action: model.zoomIn)
                    .help("Zoom in (⌘+)")
                icon(Theme.Symbol.fitWidth, "Fit Width", selected: model.fitsWidth, action: model.fitWidth)
                    .help("Fit width (⌘0)")
            }
            separator
            icon(Theme.Symbol.highlight, "Highlight", enabled: model.canHighlight, action: model.highlight)
                .help("Highlight the selected text (⇧⌘H)")
            icon(Theme.Symbol.note, "Add Note", selected: model.isPlacingNote, action: model.addNote)
                .help(model.isPlacingNote ? "Cancel (Esc)" : "Add a note, then click where it goes (⌃⌘N)")
            separator
            if find.isOpen {
                ReaderFindControls(model: find)
                    .transition(.opacity.combined(with: .move(edge: .trailing)))
            } else {
                icon(Theme.Symbol.search, "Find", action: find.open)
                    .help("Find in the PDF (⌘F)")
                    .transition(.opacity)
            }
        }
        // The end buttons sit as far inside the capsule's ends as above and
        // below, so their round hover fills are concentric with the ends.
        .padding(.horizontal, Theme.Size.readerBarPadding)
        .modifier(ReaderCapsule())
        .animation(reduceMotion ? nil : Theme.Toast.contentAnimation, value: model.backPage)
        .animation(reduceMotion ? nil : Theme.Toast.contentAnimation, value: model.isPlacingNote)
        .animation(reduceMotion ? nil : Theme.Toast.contentAnimation, value: model.zoomPercent)
        .animation(reduceMotion ? nil : Theme.Toast.contentAnimation, value: find.isOpen)
        .animation(reduceMotion ? nil : Theme.Toast.contentAnimation, value: model.hidesZoom)
        // Room for the shadow inside the hosting view.
        .padding(Theme.Toast.shadowRadius + Theme.Toast.shadowY)
        .fixedSize()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Reader Controls")
    }

    private var separator: some View {
        Divider()
            .frame(height: Theme.Size.iconSize)
            .padding(.horizontal, Theme.Size.smallGap)
    }

    private func icon(_ symbol: String, _ label: String, enabled: Bool = true, selected: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
        }
        .buttonStyle(ReaderBarButtonStyle(selected: selected))
        .disabled(!enabled)
        .accessibilityLabel(label)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A PDF's search, which opens in the reader's controls, and what PDFKit
/// found for it. Each tab keeps its own.
@MainActor
final class ReaderFindModel: ObservableObject {
    @Published var query = "" {
        didSet { if query != oldValue { changed() } }
    }
    @Published private(set) var isOpen = false
    @Published var matchCount = 0
    /// 1 based; 0 while no match is selected.
    @Published var current = 0
    @Published var isSearching = false
    /// Bumped to put the keyboard in the field.
    @Published var focusRequest = 0
    var changed: () -> Void = {}
    var open: () -> Void = {}
    var next: () -> Void = {}
    var previous: () -> Void = {}
    var close: () -> Void = {}

    func setOpen(_ open: Bool) { isOpen = open }

    /// Whether the search is open with text to find.
    var isActive: Bool { isOpen && !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    /// "3 of 97", "97 matches", or "No matches"; empty while the first
    /// matches are still coming.
    var countLabel: String {
        if !isActive { return "" }
        if matchCount == 0 { return isSearching ? "" : "No matches" }
        if current == 0 { return matchCount == 1 ? "1 match" : "\(matchCount) matches" }
        return "\(current) of \(matchCount)"
    }
}

/// The search inside the reader's controls: the field with the match count
/// in it, Previous and Next, and Done. Return goes to the next match and
/// Shift Return to the previous one; Escape closes it.
private struct ReaderFindControls: View {
    @ObservedObject var model: ReaderFindModel
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: Theme.Size.readerBarSpacing) {
            HStack(spacing: Theme.Size.smallGap) {
                Image(systemName: Theme.Symbol.search)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .accessibilityHidden(true)
                TextField("Find in PDF", text: $model.query)
                    .textFieldStyle(.plain)
                    .font(Theme.Font.swiftUIBody)
                    .focused($focused)
                    .onSubmit { model.next() }
                    .onKeyPress(.return, phases: .down) { press in
                        guard press.modifiers.contains(.shift) else { return .ignored }
                        model.previous()
                        return .handled
                    }
                    .onExitCommand { model.close() }
                    .accessibilityLabel("Find in PDF")
                Text(model.countLabel)
                    .font(Theme.Font.swiftUIReaderBarLabel)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .lineLimit(1)
                    .fixedSize()
                    .accessibilityLabel(model.countLabel)
            }
            .padding(.horizontal, Theme.Size.readerBarPadding + Theme.Size.smallGap)
            .frame(width: Theme.Size.findFieldWidth, height: Theme.Size.readerBarButton)
            // A quiet well, so the field reads as one on the glass.
            .background(Theme.Color.swiftUIControlHover, in: Capsule())
            .contentShape(Capsule())
            .onTapGesture { focused = true }
            Button(action: model.previous) { Image(systemName: Theme.Symbol.findPrevious) }
                .buttonStyle(ReaderBarButtonStyle())
                .disabled(model.matchCount == 0)
                .help("Previous match (⇧⌘G)")
                .accessibilityLabel("Previous Match")
            Button(action: model.next) { Image(systemName: Theme.Symbol.findNext) }
                .buttonStyle(ReaderBarButtonStyle())
                .disabled(model.matchCount == 0)
                .help("Next match (⌘G)")
                .accessibilityLabel("Next Match")
            Button(action: model.close) { Image(systemName: Theme.Symbol.close) }
                .buttonStyle(ReaderBarButtonStyle())
                .help("Done (Esc)")
                .accessibilityLabel("Done")
        }
        .onChange(of: model.focusRequest) { focused = true }
        .onAppear { focused = true }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Find")
    }
}

/// The capsule the reader's floating controls sit in: Liquid Glass where the
/// system has it, and before that an opaque capsule styled like the toasts,
/// with a hairline border and a soft shadow.
private struct ReaderCapsule: ViewModifier {
    @Environment(\.displayScale) private var displayScale

    func body(content: Content) -> some View {
        content
            .frame(height: Theme.Toast.height)
            .modifier(FloatingGlass(shape: Capsule()) { content, shape in
                content
                    .background(Theme.Color.swiftUIToastBackground, in: shape)
                    .overlay(shape.strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1 / displayScale))
                    .shadow(color: Theme.Toast.shadowColor, radius: Theme.Toast.shadowRadius, y: Theme.Toast.shadowY)
                    .shadow(color: Theme.Toast.contactShadowColor, radius: Theme.Toast.contactShadowRadius, y: Theme.Toast.contactShadowY)
            })
    }
}

/// Puts controls that float over the pages on Liquid Glass on macOS 26 and
/// later, so the page shows through them as through the toolbar. Earlier
/// systems draw the opaque surface `fallback` gives.
struct FloatingGlass<S: InsettableShape, Fallback: View>: ViewModifier {
    let shape: S
    let fallback: (AnyView, S) -> Fallback

    init(shape: S, @ViewBuilder fallback: @escaping (AnyView, S) -> Fallback) {
        self.shape = shape
        self.fallback = fallback
    }

    func body(content: Content) -> some View {
        if #available(macOS 26, *) {
            // Frosted, so the controls stay readable over dense text.
            content
                .background(SwiftUI.Color(nsColor: Theme.Color.floatingGlassTint), in: shape)
                .glassEffect(.regular.interactive(), in: shape)
        } else {
            fallback(AnyView(content), shape)
        }
    }
}

// MARK: Styles

/// A color dot: a ring on hover, an accent ring for the current color or
/// keyboard focus, and a small press. The markup toolbar's and the tag
/// colors' dots sit in a larger hit area; a tag chip's dot fits its line.
struct ColorDotStyle: ButtonStyle {
    let color: SwiftUI.Color
    let selected: Bool
    var size = Theme.Size.markupDotSize
    var target = Theme.Size.markupDotTarget

    func makeBody(configuration: Configuration) -> some View {
        DotBody(configuration: configuration, color: color, selected: selected, size: size, target: target)
    }

    private struct DotBody: View {
        let configuration: ButtonStyleConfiguration
        let color: SwiftUI.Color
        let selected: Bool
        let size: CGFloat
        let target: CGFloat
        @State private var hovering = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            Circle()
                .fill(color)
                .overlay(Circle().strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1))
                .frame(width: size, height: size)
                .padding(Theme.Size.markupRingWidth)
                .overlay(Circle().strokeBorder(ring, lineWidth: Theme.Size.markupRingWidth))
                .scaleEffect(configuration.isPressed ? Theme.Markup.pressedScale : 1)
                .frame(width: target, height: target)
                .contentShape(Rectangle())
                .opacity(isEnabled ? 1 : Theme.Toast.disabledOpacity)
                .onHover { hovering = $0 }
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
                .animation(.easeOut(duration: Theme.Duration.hover), value: configuration.isPressed)
        }

        private var ring: SwiftUI.Color {
            if selected || isFocused { return Theme.Color.swiftUIAccent }
            return hovering ? Theme.Color.swiftUIDotHoverRing : .clear
        }
    }
}

/// Text and icon buttons in the toolbar and the note editor: a quiet fill on
/// hover, a darker one while pressed, an accent ring for keyboard focus.
struct MarkupTextButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        TextButtonBody(configuration: configuration)
    }

    private struct TextButtonBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var hovering = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            configuration.label
                .font(Theme.Font.swiftUIToastMessage)
                .foregroundStyle(configuration.role == .destructive ? Theme.Color.swiftUIDanger : Theme.Color.swiftUIText)
                .padding(.horizontal, Theme.Size.markupButtonPadding)
                .frame(height: Theme.Size.markupButtonHeight)
                .background(fill, in: Capsule())
                .overlay(Capsule().strokeBorder(Theme.Color.swiftUIAccent, lineWidth: isFocused ? Theme.Size.markupRingWidth : 0))
                .contentShape(Capsule())
                .opacity(isEnabled ? 1 : Theme.Toast.disabledOpacity)
                .onHover { hovering = $0 }
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
        }

        private var fill: SwiftUI.Color {
            configuration.isPressed ? Theme.Color.swiftUIControlPressed : hovering ? Theme.Color.swiftUIControlHover : .clear
        }
    }
}

/// A button in the reader's controls: a round 28 pt target with a quiet fill
/// on hover, a darker one while pressed, an accent tint while its mode is on,
/// and an accent ring for keyboard focus.
struct ReaderBarButtonStyle: ButtonStyle {
    var selected = false

    func makeBody(configuration: Configuration) -> some View {
        ReaderBarButtonBody(configuration: configuration, selected: selected)
    }

    private struct ReaderBarButtonBody: View {
        let configuration: ButtonStyleConfiguration
        let selected: Bool
        @State private var hovering = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            configuration.label
                .font(Theme.Font.swiftUIReaderBarIcon)
                .foregroundStyle(selected ? Theme.Color.swiftUIAccent : Theme.Color.swiftUIText)
                .padding(.horizontal, Theme.Size.readerBarPadding)
                .frame(minWidth: Theme.Size.readerBarButton, minHeight: Theme.Size.readerBarButton)
                .background(fill, in: Capsule())
                .overlay(Capsule().strokeBorder(Theme.Color.swiftUIAccent, lineWidth: isFocused ? Theme.Size.markupRingWidth : 0))
                .contentShape(Capsule())
                .opacity(isEnabled ? 1 : Theme.Toast.disabledOpacity)
                .onHover { hovering = $0 && isEnabled }
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
                .animation(.easeOut(duration: Theme.Duration.hover), value: configuration.isPressed)
        }

        private var fill: SwiftUI.Color {
            if configuration.isPressed { return Theme.Color.swiftUIControlPressed }
            if selected { return Theme.Color.swiftUIAccent.opacity(Theme.Opacity.tint) }
            return hovering ? Theme.Color.swiftUIControlHover : .clear
        }
    }
}
