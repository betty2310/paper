import AppKit
import Combine
import PaperEngine
import SwiftUI

/// `NSTableView` handles arrow keys and type select itself but drops Delete
/// and Return, so this table hands Delete and Forward Delete to the key
/// binding system, which calls `deleteBackward`/`deleteForward` on the
/// responder chain, and opens the selected paper on Return.
final class PaperTableView: NSTableView {
    var onReturn: () -> Void = {}

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask).subtracting(.function)
        let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first
        if modifiers.isEmpty, scalar == UnicodeScalar(NSDeleteCharacter) || scalar == UnicodeScalar(NSDeleteFunctionKey) {
            interpretKeyEvents([event])
        } else if modifiers.isEmpty, scalar == UnicodeScalar(NSCarriageReturnCharacter) || scalar == UnicodeScalar(NSEnterCharacter) {
            onReturn()
        } else {
            super.keyDown(with: event)
        }
    }
}

/// The paper list: a view based table with two line rows and an empty state.
/// Reads come from `LibraryStore`.
@MainActor
final class PaperListViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate, NSMenuItemValidation {
    private static let cellIdentifier = NSUserInterfaceItemIdentifier("paperRow")

    let store: LibraryStore
    let tableView = PaperTableView()
    /// Opens a paper's PDF: double click or Return on a row.
    var onOpen: (String) -> Void = { _ in }
    private let scrollView = NSScrollView()
    private var emptyStateHost: NSHostingView<EmptyStateView>!
    private var filterHost: NSHostingView<FilterBar>!
    private var subscriptions: Set<AnyCancellable> = []
    private var suppressSelectionChange = false

    init(store: LibraryStore) {
        self.store = store
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let root = NSView()

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("paper"))
        column.resizingMask = .autoresizingMask
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.style = .inset
        tableView.rowHeight = Theme.Size.rowHeight
        tableView.intercellSpacing = NSSize(width: 0, height: 0)
        tableView.gridStyleMask = []
        tableView.usesAlternatingRowBackgroundColors = false
        tableView.allowsMultipleSelection = true
        tableView.allowsEmptySelection = true
        tableView.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        tableView.backgroundColor = Theme.Color.listBackground
        tableView.dataSource = self
        tableView.delegate = self
        tableView.menu = makeContextMenu()
        tableView.setDraggingSourceOperationMask(.link, forLocal: true)
        tableView.setAccessibilityLabel("Papers")
        tableView.target = self
        tableView.doubleAction = #selector(rowDoubleClicked(_:))
        tableView.onReturn = { [weak self] in self?.openSelection() }

        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.automaticallyAdjustsContentInsets = true

        emptyStateHost = NSHostingView(rootView: EmptyStateView(store: store))
        emptyStateHost.isHidden = true
        filterHost = NSHostingView(rootView: FilterBar(store: store))
        // These hosts take their size from their content, but must never push
        // the list pane or the window around: the split view decides widths.
        for host in [emptyStateHost, filterHost] as [NSView] {
            (host as? NSHostingView<EmptyStateView>)?.sizingOptions = .intrinsicContentSize
            (host as? NSHostingView<FilterBar>)?.sizingOptions = .intrinsicContentSize
            host.setContentHuggingPriority(.init(1), for: .horizontal)
            host.setContentCompressionResistancePriority(.init(1), for: .horizontal)
        }

        for subview in [filterHost, scrollView, emptyStateHost] as [NSView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        NSLayoutConstraint.activate([
            filterHost.topAnchor.constraint(equalTo: root.topAnchor),
            filterHost.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            filterHost.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.topAnchor.constraint(equalTo: filterHost.bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            emptyStateHost.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            emptyStateHost.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            emptyStateHost.widthAnchor.constraint(lessThanOrEqualTo: root.widthAnchor, constant: -Theme.Size.padding * 2),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        store.$items
            .receive(on: DispatchQueue.main)
            .sink { [weak self] items in self?.reload(items) }
            .store(in: &subscriptions)
        // PDF searches start and end without changing the rows themselves.
        store.$pdfStatus
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in self?.refreshVisibleRows() }
            .store(in: &subscriptions)
        store.$selectedItemIds
            .receive(on: DispatchQueue.main)
            .sink { [weak self] ids in
                guard let self else { return }
                selectRows(ids)
                // When the app picks the rows (a finished add, a toast's
                // Show button), keyboard focus follows so arrow keys and
                // Delete act on them at once.
                if !ids.isEmpty { view.window?.makeFirstResponder(tableView) }
            }
            .store(in: &subscriptions)
    }

    private var shownItems: [ItemRow] = []

    private func reload(_ items: [ItemRow]) {
        let oldIds = shownItems.map(\.id)
        let newIds = items.map(\.id)
        shownItems = items
        suppressSelectionChange = true
        if oldIds == newIds {
            // Same rows, changed contents (a lookup finished): refresh in place without losing selection.
            tableView.reloadData(forRowIndexes: IndexSet(0..<items.count), columnIndexes: [0])
        } else {
            tableView.reloadData()
            selectRows(store.selectedItemIds)
        }
        suppressSelectionChange = false
        emptyStateHost.isHidden = !items.isEmpty
        scrollView.isHidden = items.isEmpty
    }

    private func refreshVisibleRows() {
        let visible = tableView.rows(in: tableView.visibleRect)
        guard visible.length > 0 else { return }
        for row in visible.location..<min(visible.location + visible.length, shownItems.count) {
            (tableView.view(atColumn: 0, row: row, makeIfNecessary: false) as? PaperRowView)?
                .configure(with: shownItems[row], pdfStatus: store.pdfStatus[shownItems[row].id])
        }
    }

    // MARK: Opening

    @objc private func rowDoubleClicked(_ sender: Any?) {
        let row = tableView.clickedRow
        guard row >= 0, row < shownItems.count else { return }
        onOpen(shownItems[row].id)
    }

    private func openSelection() {
        guard tableView.selectedRowIndexes.count == 1, let row = tableView.selectedRowIndexes.first else { return }
        onOpen(shownItems[row].id)
    }

    /// The paper's row as it looks now, for the flight into its new tab.
    func flightSource(itemId: String) -> TabFlight.Source? {
        guard let row = shownItems.firstIndex(where: { $0.id == itemId }),
              let rowView = tableView.rowView(atRow: row, makeIfNecessary: false),
              let cell = tableView.view(atColumn: 0, row: row, makeIfNecessary: false) as? PaperRowView,
              tableView.visibleRect.intersects(tableView.rect(ofRow: row))
        else { return nil }
        return TabFlight.capture(row: rowView, title: cell.titleField, titleFont: Theme.Font.rowTitle)
    }

    private func selectRows(_ ids: [String]) {
        let wanted = IndexSet(shownItems.enumerated().filter { ids.contains($0.element.id) }.map(\.offset))
        guard wanted != tableView.selectedRowIndexes else { return }
        suppressSelectionChange = true
        tableView.selectRowIndexes(wanted, byExtendingSelection: false)
        if let first = wanted.first { tableView.scrollRowToVisible(first) }
        suppressSelectionChange = false
    }

    // MARK: Commands

    @objc func delete(_ sender: Any?) {
        store.delete(itemIds: store.selectedItemIds)
    }

    /// The table forwards Delete and Forward Delete here through the responder chain.
    override func deleteBackward(_ sender: Any?) { delete(sender) }
    override func deleteForward(_ sender: Any?) { delete(sender) }

    @objc func copyDOI(_ sender: Any?) {
        let dois = store.selectedItems.compactMap(\.doi)
        guard !dois.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(dois.joined(separator: "\n"), forType: .string)
        store.show(Toast(id: "copied", style: .done, message: dois.count == 1 ? "DOI copied" : "\(dois.count) DOIs copied"))
    }

    @objc func openPublisherPage(_ sender: Any?) {
        for doi in store.selectedItems.compactMap(\.doi) {
            if let url = URL(string: "https://doi.org/\(doi)") { NSWorkspace.shared.open(url) }
        }
    }

    @objc func retryLookup(_ sender: Any?) {
        for item in store.selectedItems where item.lookupStatus == .failed {
            store.retryLookup(itemId: item.id)
        }
    }

    @objc private func addToCollectionClicked(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String else { return }
        store.addToCollection(collectionId: id, itemIds: store.selectedItemIds)
    }

    @objc private func removeFromCollectionClicked(_ sender: Any?) {
        guard case .collection(let id) = store.scope else { return }
        store.removeFromCollection(collectionId: id, itemIds: store.selectedItemIds)
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        let selected = store.selectedItems
        switch menuItem.action {
        case #selector(delete(_:)):
            menuItem.title = selected.count > 1 ? "Delete \(selected.count) Papers" : "Delete"
            return !selected.isEmpty
        case #selector(copyDOI(_:)), #selector(openPublisherPage(_:)):
            return selected.contains { $0.doi != nil }
        case #selector(retryLookup(_:)):
            return selected.contains { $0.lookupStatus == .failed }
        case #selector(removeFromCollectionClicked(_:)):
            if case .collection = store.scope { return !selected.isEmpty }
            return false
        default:
            return true
        }
    }

    private func makeContextMenu() -> NSMenu {
        let menu = NSMenu()
        menu.addItem(withTitle: "Open PDF", action: #selector(MainWindowController.openSelectedPaper(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Find PDF", action: #selector(MainWindowController.findPdf(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Attach PDF…", action: #selector(MainWindowController.attachPdf(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Reveal PDF in Finder", action: #selector(MainWindowController.revealPdf(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Copy DOI", action: #selector(copyDOI(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Open Publisher Page", action: #selector(openPublisherPage(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Retry Lookup", action: #selector(retryLookup(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        let addTo = NSMenuItem(title: "Add to Collection", action: nil, keyEquivalent: "")
        addTo.submenu = NSMenu()
        menu.addItem(addTo)
        menu.addItem(withTitle: "Remove from Collection", action: #selector(removeFromCollectionClicked(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Delete", action: #selector(delete(_:)), keyEquivalent: "")
        menu.delegate = self
        return menu
    }

    // MARK: Table

    func numberOfRows(in tableView: NSTableView) -> Int {
        shownItems.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: Self.cellIdentifier, owner: self) as? PaperRowView
            ?? PaperRowView(identifier: Self.cellIdentifier)
        cell.configure(with: shownItems[row], pdfStatus: store.pdfStatus[shownItems[row].id])
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionChange else { return }
        let ids = tableView.selectedRowIndexes.map { shownItems[$0].id }
        if ids != store.selectedItemIds { store.selectedItemIds = ids }
    }

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        let selected = tableView.selectedRowIndexes.contains(row) ? tableView.selectedRowIndexes : IndexSet([row])
        let ids = selected.map { shownItems[$0].id }
        let item = NSPasteboardItem()
        item.setData((try? JSONEncoder().encode(ids)) ?? Data(), forType: SidebarViewController.itemPasteboardType)
        item.setString(shownItems[row].title, forType: .string)
        return item
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession, willBeginAt screenPoint: NSPoint, forRowIndexes rowIndexes: IndexSet) {
        session.draggingFormation = .stack
    }
}

extension PaperListViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        // Right clicking a row outside the selection acts on that row alone, like Finder.
        let clicked = tableView.clickedRow
        if clicked >= 0, !tableView.selectedRowIndexes.contains(clicked) {
            tableView.selectRowIndexes([clicked], byExtendingSelection: false)
        }
        guard let submenu = menu.items.first(where: { $0.title == "Add to Collection" })?.submenu else { return }
        submenu.removeAllItems()
        for collection in store.collections {
            let item = NSMenuItem(title: collection.name, action: #selector(addToCollectionClicked(_:)), keyEquivalent: "")
            item.indentationLevel = Int(collection.depth)
            item.representedObject = collection.id
            item.target = self
            submenu.addItem(item)
        }
        if store.collections.isEmpty {
            submenu.addItem(withTitle: "No Collections", action: nil, keyEquivalent: "")
        }
    }
}

/// A two line paper row: serif title, metadata line, and a status at the right.
@MainActor
final class PaperRowView: NSTableCellView {
    private let title = NSTextField(labelWithString: "")
    private let subtitle = NSTextField(labelWithString: "")
    private let added = NSTextField(labelWithString: "")
    private let status = NSImageView()
    private let spinner = NSProgressIndicator()

    private static let addedFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        formatter.doesRelativeDateFormatting = true
        return formatter
    }()

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier

        title.font = Theme.Font.rowTitle
        title.textColor = Theme.Color.text
        title.lineBreakMode = .byTruncatingTail
        title.maximumNumberOfLines = 1
        subtitle.font = Theme.Font.secondary
        subtitle.textColor = Theme.Color.secondaryText
        subtitle.lineBreakMode = .byTruncatingTail
        subtitle.maximumNumberOfLines = 1
        added.font = Theme.Font.caption
        added.textColor = Theme.Color.tertiaryText
        added.alignment = .right
        status.symbolConfiguration = .init(pointSize: Theme.Size.statusIndicatorSize, weight: .regular)
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false

        for subview in [title, subtitle, added, status, spinner] as [NSView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            addSubview(subview)
        }
        textField = title
        title.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        subtitle.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        NSLayoutConstraint.activate([
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Size.rowInset),
            title.trailingAnchor.constraint(lessThanOrEqualTo: added.leadingAnchor, constant: -Theme.Size.gap),
            title.topAnchor.constraint(equalTo: topAnchor, constant: Theme.Size.gap),
            subtitle.leadingAnchor.constraint(equalTo: title.leadingAnchor),
            subtitle.trailingAnchor.constraint(lessThanOrEqualTo: status.leadingAnchor, constant: -Theme.Size.gap),
            subtitle.topAnchor.constraint(equalTo: title.bottomAnchor, constant: Theme.Size.smallGap / 2),
            added.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Size.rowInset),
            added.firstBaselineAnchor.constraint(equalTo: title.firstBaselineAnchor),
            status.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Size.rowInset),
            status.centerYAnchor.constraint(equalTo: subtitle.centerYAnchor),
            spinner.centerXAnchor.constraint(equalTo: status.centerXAnchor),
            spinner.centerYAnchor.constraint(equalTo: status.centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    private var lookupStatus: LookupStatus = .done

    /// The serif title, which flies into the tab when the PDF opens.
    var titleField: NSTextField { title }

    func configure(with item: ItemRow, pdfStatus: PdfStatus? = nil) {
        lookupStatus = item.lookupStatus
        added.stringValue = Self.addedFormatter.string(from: item.addedAt)
        switch item.lookupStatus {
        case .pending:
            title.stringValue = item.title
            title.font = Theme.Font.monospaced
            subtitle.stringValue = "Looking up metadata…"
            status.image = nil
            spinner.startAnimation(nil)
        case .failed:
            title.stringValue = item.title
            title.font = Theme.Font.monospaced
            subtitle.stringValue = "Lookup failed"
            status.image = NSImage(systemSymbolName: Theme.Symbol.failed, accessibilityDescription: "Lookup failed")
            status.contentTintColor = Theme.Color.warning
            spinner.stopAnimation(nil)
        case .done:
            title.stringValue = item.title
            title.font = Theme.Font.rowTitle
            subtitle.stringValue = item.subtitle
            status.contentTintColor = Theme.Color.secondaryText
            if !item.hasPdf, pdfStatus == .searching {
                status.image = nil
                status.toolTip = "Downloading the PDF"
                spinner.startAnimation(nil)
            } else {
                status.image = item.hasPdf ? NSImage(systemSymbolName: Theme.Symbol.pdf, accessibilityDescription: "Has PDF") : nil
                status.toolTip = item.hasPdf ? "Double click to read" : nil
                spinner.stopAnimation(nil)
            }
        }
        applyColors()
        setAccessibilityLabel("\(item.title). \(subtitle.stringValue)")
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet { applyColors() }
    }

    private func applyColors() {
        let emphasized = backgroundStyle == .emphasized
        title.textColor = emphasized ? Theme.Color.selectedText : (lookupStatus == .done ? Theme.Color.text : Theme.Color.secondaryText)
        subtitle.textColor = emphasized ? Theme.Color.selectedText : Theme.Color.secondaryText
        added.textColor = emphasized ? Theme.Color.selectedText : Theme.Color.tertiaryText
        if lookupStatus == .done {
            status.contentTintColor = emphasized ? Theme.Color.selectedText : Theme.Color.secondaryText
        }
    }
}

/// Shown instead of the table when there is nothing to list.
struct EmptyStateView: View {
    @ObservedObject var store: LibraryStore

    var body: some View {
        VStack(spacing: Theme.Size.gap) {
            Image(systemName: store.searchText.isEmpty ? Theme.Symbol.paper : "magnifyingglass")
                .font(.system(size: 36, weight: .light))
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
            Text(headline)
                .font(Theme.Font.swiftUIInspectorTitle)
                .foregroundStyle(Theme.Color.swiftUIText)
            Text(message)
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .multilineTextAlignment(.center)
            if store.searchText.isEmpty, store.scope == .all {
                Button("Add Paper…") {
                    NSApp.sendAction(#selector(MainWindowController.addPaper(_:)), to: nil, from: nil)
                }
                .keyboardShortcut("n", modifiers: .command)
                .padding(.top, Theme.Size.gap)
            }
        }
        .padding(Theme.Size.padding)
        .frame(maxWidth: 360)
    }

    private var headline: String {
        if !store.searchText.isEmpty { return "No Results" }
        switch store.scope {
        case .all: return "No Papers Yet"
        case .unread: return "Nothing Unread"
        case .recentlyAdded: return "Nothing Recent"
        case .noPdf: return "Every Paper Has a PDF"
        case .collection: return "Empty Collection"
        case .tag: return "No Papers With This Tag"
        }
    }

    private var message: String {
        if !store.searchText.isEmpty { return "Nothing matches “\(store.searchText)”." }
        switch store.scope {
        case .all: return "Paste a DOI, a doi.org link, or an arXiv ID to add your first paper."
        case .unread: return "Papers whose PDF you have not opened show up here."
        case .recentlyAdded: return "Papers added in the last \(recentlyAddedDays()) days show up here."
        case .noPdf: return "Papers without a PDF show up here."
        case .collection: return "Drag papers from the list onto this collection to add them."
        case .tag: return "Tag a paper in the inspector to see it here."
        }
    }
}

/// Names the search filter the palette applied to the list, with a way out.
/// Renders nothing, and so takes no height, when the list is unfiltered.
struct FilterBar: View {
    @ObservedObject var store: LibraryStore

    var body: some View {
        if !store.searchText.isEmpty {
            HStack(spacing: Theme.Size.gap) {
                Image(systemName: Theme.Symbol.search)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                (Text("Results for ") + Text("“\(store.searchText)”").fontWeight(.semibold))
                    .font(Theme.Font.swiftUISecondary)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Button {
                    store.searchText = ""
                } label: {
                    Image(systemName: Theme.Symbol.clear)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
                .buttonStyle(.plain)
                .help("Clear the search filter")
                .accessibilityLabel("Clear Search")
            }
            .padding(.horizontal, Theme.Size.padding)
            .frame(height: Theme.Size.filterBarHeight)
            .frame(maxWidth: .infinity)
            .background(Color(nsColor: Theme.Color.filterBarBackground))
            .overlay(alignment: .bottom) { Divider() }
        }
    }
}
