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

    init(store: LibraryStore) {
        self.store = store
        model = SearchPaletteModel(store: store)
        host = NSHostingView(rootView: SearchPaletteView(model: model))
        super.init()

        host.sizingOptions = []
        model.onDismiss = { [weak self] in self?.dismiss() }
        model.onResize = { [weak self] size in
            self?.contentSize = size
            self?.resize()
        }

        let background = NSVisualEffectView()
        background.material = .popover
        background.blendingMode = .behindWindow
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = Theme.Size.paletteRadius
        background.layer?.cornerCurve = .continuous
        background.layer?.masksToBounds = true
        background.addSubview(host)
        host.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            host.topAnchor.constraint(equalTo: background.topAnchor),
            host.leadingAnchor.constraint(equalTo: background.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: background.trailingAnchor),
            host.bottomAnchor.constraint(equalTo: background.bottomAnchor),
        ])
        panel.contentView = background
        panel.delegate = self
        panel.onCancel = { [weak self] in self?.dismiss() }
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
            store.select(item.id)
        case .showAll(_, let text):
            store.scope = .all
            store.searchText = text
        }
    }
}

struct SearchPaletteView: View {
    @ObservedObject var model: SearchPaletteModel
    @FocusState private var fieldFocused: Bool

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
        .onGeometryChange(for: CGSize.self) { $0.size } action: { model.onResize?($0) }
        .onAppear { fieldFocused = true }
        .onChange(of: model.rows) { fieldFocused = true }
    }

    private var field: some View {
        HStack(spacing: Theme.Size.gap) {
            Image(systemName: Theme.Symbol.search)
                .font(Theme.Font.swiftUIPaletteField)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
            TextField("Search papers, or paste a DOI", text: $model.query)
                .textFieldStyle(.plain)
                .font(Theme.Font.swiftUIPaletteField)
                .focused($fieldFocused)
                .onSubmit { model.activate(model.selection) }
                .onKeyPress(.upArrow) { model.moveSelection(by: -1); return .handled }
                .onKeyPress(.downArrow) { model.moveSelection(by: 1); return .handled }
                .onKeyPress(.escape) { model.onDismiss?(); return .handled }
                .accessibilityLabel("Search Library")
            if !model.query.isEmpty {
                Button { model.query = "" } label: {
                    Image(systemName: Theme.Symbol.clear)
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
                    Text(model.showsRecent ? "Recently Added" : "Papers")
                        .font(Theme.Font.swiftUISectionLabel)
                        .foregroundStyle(Theme.Color.swiftUITertiaryText)
                        .textCase(.uppercase)
                        .padding(.horizontal, Theme.Size.padding)
                        .frame(height: Theme.Size.paletteSectionHeight, alignment: .bottomLeading)
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
                Image(systemName: Theme.Symbol.returnKey)
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(secondary)
            }
        }
        .padding(.horizontal, Theme.Size.padding)
        .frame(height: Theme.Size.paletteRowHeight)
        .contentShape(Rectangle())
    }

    private func paper(_ item: ItemRow) -> some View {
        VStack(alignment: .leading, spacing: Theme.Size.smallGap / 2) {
            Text(item.title.isEmpty ? (item.doi ?? "Untitled") : item.title)
                .font(Theme.Font.swiftUIPaletteTitle)
                .foregroundStyle(primary)
                .lineLimit(1)
            Text(metadata(item))
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(secondary)
                .lineLimit(1)
        }
    }

    private func action(symbol: String, title: String, detail: String, hint: String) -> some View {
        HStack(spacing: Theme.Size.gap) {
            Image(systemName: symbol)
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(selected ? primary : Theme.Color.swiftUIAccent)
                .frame(width: Theme.Size.iconSize)
            (Text(title) + Text(detail).fontWeight(.semibold))
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(primary)
                .lineLimit(1)
            Text(hint)
                .font(Theme.Font.swiftUICaption)
                .foregroundStyle(secondary)
        }
    }

    private func metadata(_ item: ItemRow) -> String {
        [item.authors, item.journal, item.year.map(String.init)]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
            .joined(separator: " · ")
    }

    private var primary: SwiftUI.Color {
        selected ? SwiftUI.Color(nsColor: Theme.Color.selectedText) : Theme.Color.swiftUIText
    }

    private var secondary: SwiftUI.Color {
        selected ? SwiftUI.Color(nsColor: Theme.Color.selectedText).opacity(0.8) : Theme.Color.swiftUISecondaryText
    }
}

/// Accent fill for the selected (or hovered) row, darkened while pressed.
private struct PaletteRowButtonStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous)
                    .fill(selected ? Theme.Color.swiftUIAccent : SwiftUI.Color.clear)
                    .opacity(configuration.isPressed ? 0.8 : 1)
                    .padding(.horizontal, Theme.Size.gap)
            )
    }
}
