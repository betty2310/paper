import AppKit
import Combine
import PaperEngine
import SwiftUI

/// ⌘P: a floating palette over the library window that searches the whole
/// library no matter what the sidebar shows, in the spirit of Open Quickly.
/// Enter reveals the chosen paper in the list, a pasted DOI or arXiv ID offers
/// to add it, and the last row applies the text as a filter on the list.
@MainActor
final class SearchPaletteController: NSObject, NSWindowDelegate {
    private let store: LibraryStore
    private let model: SearchPaletteModel
    private let panel = PalettePanel()
    private let host: NSHostingView<SearchPaletteView>
    private weak var parent: NSWindow?
    /// The content's last measured size; it only reports changes, so reopening
    /// with the same results must reuse it.
    private var contentSize = NSSize(width: Theme.Size.paletteWidth, height: Theme.Size.paletteFieldHeight)
    /// Brings the library forward, for a result shown in the list while a
    /// PDF is in front.
    var onShowLibrary: (() -> Void)?

    init(store: LibraryStore) {
        self.store = store
        model = SearchPaletteModel(store: store)
        host = NSHostingView(rootView: SearchPaletteView(model: model))
        super.init()

        host.sizingOptions = []
        // The panel has no title bar; any safe area the window reports would
        // push the content down and leave the window background showing.
        host.safeAreaRegions = []
        model.onDismiss = { [weak self] in self?.dismiss() }
        model.onShowLibrary = { [weak self] in self?.onShowLibrary?() }
        model.onResize = { [weak self] size in
            self?.contentSize = size
            // SwiftUI reports the size from inside a layout pass. Resizing the
            // window there lets the layout engine overwrite the content view's
            // frame with a stale solution, so the content no longer fills the
            // window. Resize once the pass is over.
            DispatchQueue.main.async { self?.resize() }
        }

        // The window's own content view is a transparent, rounded clip. The
        // material and the SwiftUI content sit inside it, so nothing the window
        // draws itself can show around the panel's shape.
        let container = NSView()
        container.wantsLayer = true
        container.layer?.backgroundColor = NSColor.clear.cgColor
        container.layer?.cornerRadius = Theme.Size.paletteRadius
        container.layer?.cornerCurve = .continuous
        container.layer?.masksToBounds = true
        // Frames, not constraints: the glass view has an intrinsic size, and
        // pinning it with constraints lets Auto Layout resize the window's
        // content away from the frame the controller set.
        for view in [Self.makeBackground(), host] {
            view.frame = container.bounds
            view.autoresizingMask = [.width, .height]
            container.addSubview(view)
        }
        panel.contentView = container
        panel.delegate = self
        panel.onCancel = { [weak self] in self?.dismiss() }
    }

    /// Liquid Glass where the system has it, the popover material before.
    private static func makeBackground() -> NSView {
        if #available(macOS 26, *) {
            let glass = NSGlassEffectView()
            glass.cornerRadius = Theme.Size.paletteRadius
            glass.tintColor = Theme.Color.paletteTint
            return glass
        }
        let effect = NSVisualEffectView()
        effect.material = .popover
        effect.blendingMode = .behindWindow
        effect.state = .active
        effect.maskImage = roundedMask(radius: Theme.Size.paletteRadius)
        return effect
    }

    /// A stretchable rounded rectangle, the supported way to shape a
    /// behind-window material.
    private static func roundedMask(radius: CGFloat) -> NSImage {
        let side = radius * 2 + 1
        let image = NSImage(size: NSSize(width: side, height: side), flipped: false) { rect in
            NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
            return true
        }
        image.capInsets = NSEdgeInsets(top: radius, left: radius, bottom: radius, right: radius)
        image.resizingMode = .stretch
        return image
    }

    var isShown: Bool { panel.isVisible }

    func toggle(over window: NSWindow) {
        if isShown { dismiss() } else { show(over: window) }
    }

    func show(over window: NSWindow) {
        parent = window
        model.reset()
        resize()
        window.addChildWindow(panel, ordered: .above)
        panel.makeKeyAndOrderFront(nil)
    }

    func dismiss() {
        guard panel.isVisible else { return }
        parent?.removeChildWindow(panel)
        panel.orderOut(nil)
        parent?.makeKey()
    }

    /// Keeps the top edge where it is while the result list grows and shrinks.
    private func resize() {
        guard let parent else { return }
        let size = contentSize
        let top = parent.frame.maxY - parent.frame.height * Theme.Size.paletteTopFraction
        let frame = NSRect(x: parent.frame.midX - size.width / 2, y: top - size.height, width: size.width, height: size.height)
        panel.setFrame(frame, display: true)
        panel.invalidateShadow()
    }

    func windowDidResignKey(_ notification: Notification) {
        dismiss()
    }
}

/// Borderless panels refuse key status by default, which would leave the
/// search field unable to take typing.
final class PalettePanel: NSPanel {
    var onCancel: (() -> Void)?

    init() {
        super.init(contentRect: .zero, styleMask: [.borderless], backing: .buffered, defer: false)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        isMovable = false
        animationBehavior = .utilityWindow
        isReleasedWhenClosed = false
    }

    override var canBecomeKey: Bool { true }

    override func cancelOperation(_ sender: Any?) {
        onCancel?()
    }
}

/// What the palette offers for the current text, and which row is selected.
@MainActor
final class SearchPaletteModel: ObservableObject {
    enum Row: Equatable {
        case add(identifier: String)
        case paper(ItemRow)
        case showAll(count: Int, text: String)
    }

    @Published var query = "" { didSet { if query != oldValue { refresh() } } }
    @Published private(set) var rows: [Row] = []
    @Published var selection = 0
    /// Whether `rows` are recently added papers shown for an empty query.
    @Published private(set) var showsRecent = false

    var onDismiss: (() -> Void)?
    var onShowLibrary: (() -> Void)?
    var onResize: ((CGSize) -> Void)?

    private let store: LibraryStore

    init(store: LibraryStore) {
        self.store = store
    }

    func reset() {
        query = ""
        selection = 0
        refresh()
    }

    private func refresh() {
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let papers = store.search(text: text, limit: Theme.Size.paletteMaxResults)
        var rows: [Row] = []
        if let identifier = parseIdentifier(text: text) {
            rows.append(.add(identifier: identifier))
        }
        rows.append(contentsOf: papers.map(Row.paper))
        if !text.isEmpty, !papers.isEmpty {
            rows.append(.showAll(count: store.searchCount(text: text), text: text))
        }
        showsRecent = text.isEmpty
        self.rows = rows
        selection = 0
    }

    func moveSelection(by delta: Int) {
        guard !rows.isEmpty else { return }
        selection = (selection + delta + rows.count) % rows.count
    }

    func activate(_ index: Int) {
        guard rows.indices.contains(index) else { return }
        let row = rows[index]
        onDismiss?()
        switch row {
        case .add(let identifier):
            store.add(identifier: identifier)
        case .paper(let item):
            onShowLibrary?()
            store.select(item.id)
        case .showAll(_, let text):
            onShowLibrary?()
            store.scope = .all
            store.searchText = text
        }
    }
}

struct SearchPaletteView: View {
    @ObservedObject var model: SearchPaletteModel
    @FocusState private var fieldFocused: Bool
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        VStack(spacing: 0) {
            field
            if !model.rows.isEmpty {
                Divider()
                results
            }
        }
        .frame(width: Theme.Size.paletteWidth)
        // The panel takes its height from the content, never the other way around.
        .fixedSize(horizontal: false, vertical: true)
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Size.paletteRadius, style: .continuous)
                .strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1 / displayScale)
        )
        .ignoresSafeArea()
        .onGeometryChange(for: CGSize.self) { $0.size } action: { model.onResize?($0) }
        .onAppear { fieldFocused = true }
        .onChange(of: model.rows) { fieldFocused = true }
    }

    private var field: some View {
        HStack(spacing: Theme.Size.gap + Theme.Size.smallGap) {
            Image(systemName: Theme.Symbol.search)
                .font(Theme.Font.swiftUIPaletteIcon)
                .foregroundStyle(Theme.Color.swiftUIAccent)
            TextField("Search papers, or paste a DOI", text: $model.query)
                .textFieldStyle(.plain)
                .font(Theme.Font.swiftUIPaletteField)
                .focused($fieldFocused)
                .onSubmit { model.activate(model.selection) }
                .onKeyPress(.upArrow) { model.moveSelection(by: -1); return .handled }
                .onKeyPress(.downArrow) { model.moveSelection(by: 1); return .handled }
                .onKeyPress(.escape) { model.onDismiss?(); return .handled }
                .accessibilityLabel("Search Library")
            if model.query.isEmpty {
                KeyCap(text: "esc")
            } else {
                Button { model.query = "" } label: {
                    Image(systemName: Theme.Symbol.clear)
                        .font(Theme.Font.swiftUIBody)
                        .foregroundStyle(Theme.Color.swiftUITertiaryText)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear")
            }
        }
        .padding(.horizontal, Theme.Size.padding)
        .frame(height: Theme.Size.paletteFieldHeight)
    }

    private var results: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    HStack {
                        Text(model.showsRecent ? "Recently Added" : "Papers")
                            .foregroundStyle(Theme.Color.swiftUIAccent)
                            .textCase(.uppercase)
                        Spacer()
                        Text("↑↓ navigate   ↩ open")
                            .foregroundStyle(Theme.Color.swiftUITertiaryText)
                    }
                    .font(Theme.Font.swiftUISectionLabel)
                    .padding(.horizontal, Theme.Size.padding)
                    .frame(height: Theme.Size.paletteSectionHeight, alignment: .bottom)
                    ForEach(Array(model.rows.enumerated()), id: \.offset) { index, row in
                        Button { model.activate(index) } label: {
                            PaletteRowLabel(row: row, selected: index == model.selection)
                        }
                        .buttonStyle(PaletteRowButtonStyle(selected: index == model.selection))
                        .onHover { inside in if inside { model.selection = index } }
                        .id(index)
                    }
                }
                .padding(.bottom, Theme.Size.gap)
            }
            .frame(height: resultsHeight)
            .onChange(of: model.selection) { proxy.scrollTo(model.selection) }
        }
    }

    /// The list is as tall as its rows up to a cap, after which it scrolls.
    private var resultsHeight: CGFloat {
        let rows = min(model.rows.count, Theme.Size.paletteVisibleRows)
        let bottom = model.rows.count > Theme.Size.paletteVisibleRows ? 0 : Theme.Size.gap
        return Theme.Size.paletteSectionHeight + Theme.Size.paletteRowHeight * CGFloat(rows) + bottom
    }
}

/// One result row: a paper with its title and metadata, or an action.
private struct PaletteRowLabel: View {
    let row: SearchPaletteModel.Row
    let selected: Bool

    var body: some View {
        HStack(spacing: Theme.Size.gap) {
            switch row {
            case .add(let identifier):
                action(symbol: Theme.Symbol.addPaper, title: "Add ", detail: identifier, hint: "Looks up the metadata")
            case .paper(let item):
                paper(item)
            case .showAll(let count, let text):
                action(symbol: Theme.Symbol.showInList, title: "Show all \(count) results for ", detail: "“\(text)”", hint: "Filters the list")
            }
            Spacer(minLength: 0)
            if selected {
                KeyCap(symbol: Theme.Symbol.returnKey)
            }
        }
        .padding(.leading, Theme.Size.paletteRowInset)
        .padding(.trailing, Theme.Size.padding)
        .frame(height: Theme.Size.paletteRowHeight)
        .contentShape(Rectangle())
    }

    private func paper(_ item: ItemRow) -> some View {
        VStack(alignment: .leading, spacing: Theme.Size.smallGap / 2) {
            Text(item.title.isEmpty ? (item.doi ?? "Untitled") : item.title)
                .font(Theme.Font.swiftUIPaletteTitle)
                .foregroundStyle(Theme.Color.swiftUIText)
                .lineLimit(1)
            Text(metadata(item))
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .lineLimit(1)
        }
    }

    private func action(symbol: String, title: String, detail: String, hint: String) -> some View {
        HStack(spacing: Theme.Size.gap + Theme.Size.smallGap) {
            Image(systemName: symbol)
                .font(Theme.Font.swiftUICaption.weight(.bold))
                .foregroundStyle(SwiftUI.Color(nsColor: Theme.Color.selectedText))
                .frame(width: Theme.Size.paletteBadgeSize, height: Theme.Size.paletteBadgeSize)
                .background(Circle().fill(Theme.Color.swiftUIAccent))
            (Text(title) + Text(detail).fontWeight(.semibold))
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(Theme.Color.swiftUIText)
                .lineLimit(1)
            Text(hint)
                .font(Theme.Font.swiftUICaption)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
        }
    }

    private func metadata(_ item: ItemRow) -> String {
        [item.authors, item.journal, item.year.map(String.init)]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }
}

/// A small key label, like the hints in Spotlight: "esc" or the return arrow.
private struct KeyCap: View {
    var text: String?
    var symbol: String?

    var body: some View {
        Group {
            if let text { Text(text) } else if let symbol { Image(systemName: symbol) }
        }
        .font(Theme.Font.swiftUICaption.weight(.semibold))
        .foregroundStyle(Theme.Color.swiftUIAccent)
        .frame(minWidth: Theme.Size.paletteBadgeSize, minHeight: Theme.Size.paletteBadgeSize)
        .padding(.horizontal, Theme.Size.smallGap)
        .background(
            RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous)
                .fill(Theme.Color.swiftUIAccent.opacity(Theme.Opacity.tint))
        )
    }
}

/// The selected (or hovered) row: a soft accent tint with an accent bar on
/// its leading edge, which deepens while pressed.
private struct PaletteRowButtonStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous)
                    .fill(Theme.Color.swiftUIAccent.opacity(fillOpacity(pressed: configuration.isPressed)))
                    .padding(.horizontal, Theme.Size.gap)
            )
            .overlay(alignment: .leading) {
                if selected {
                    Capsule()
                        .fill(Theme.Color.swiftUIAccent)
                        .frame(width: Theme.Size.paletteSelectionBar, height: Theme.Size.paletteRowHeight - Theme.Size.padding)
                        .padding(.leading, Theme.Size.gap + Theme.Size.smallGap)
                }
            }
    }

    private func fillOpacity(pressed: Bool) -> Double {
        guard selected else { return 0 }
        return pressed ? Theme.Opacity.pressedTint : Theme.Opacity.tint
    }
}
