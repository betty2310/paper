import AppKit
import Combine
import PaperEngine

/// The source list: smart lists, the collection tree, and tags. Items can be
/// dropped on a collection to add them to it.
@MainActor
final class SidebarViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate, NSTextFieldDelegate {
    /// One row of the outline. Groups are headers; everything else selects a scope.
    final class Node {
        enum Kind {
            case group(String)
            case smartList(Scope, title: String, symbol: String)
            case collection(CollectionNode)
            case tag(Tag)
        }

        let id: String
        let kind: Kind
        var children: [Node] = []

        init(id: String, kind: Kind) {
            self.id = id
            self.kind = kind
        }

        var scope: Scope? {
            switch kind {
            case .group: nil
            case .smartList(let scope, _, _): scope
            case .collection(let node): .collection(id: node.id)
            case .tag(let tag): .tag(id: tag.id)
            }
        }
    }

    static let itemPasteboardType = NSPasteboard.PasteboardType("com.betty2310.paper.item-ids")
    private static let cellIdentifier = NSUserInterfaceItemIdentifier("sidebarCell")
    private static let groupIdentifier = NSUserInterfaceItemIdentifier("sidebarGroup")

    let store: LibraryStore
    private let outlineView = NSOutlineView()
    private let scrollView = NSScrollView()
    private let footer = NSView()
    private let libraryLabel = NSTextField(labelWithString: "")
    private var roots: [Node] = []
    private var nodesById: [String: Node] = [:]
    private var collapsedIds: Set<String> = []
    private var subscriptions: Set<AnyCancellable> = []
    private var suppressSelectionChange = false
    /// The context menu's color items, shown for a tag.
    private var colorItems: [NSMenuItem] = []

    init(store: LibraryStore) {
        self.store = store
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    var selectedCollectionId: String? {
        if case .collection(let node)? = (outlineView.item(atRow: outlineView.selectedRow) as? Node)?.kind {
            return node.id
        }
        return nil
    }

    override func loadView() {
        let root = NSView()

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("main"))
        column.resizingMask = .autoresizingMask
        outlineView.addTableColumn(column)
        outlineView.outlineTableColumn = column
        outlineView.headerView = nil
        outlineView.style = .sourceList
        outlineView.floatsGroupRows = false
        outlineView.rowSizeStyle = .default
        outlineView.indentationPerLevel = Theme.Size.padding
        // On, expanding the groups at launch widens the column by one indentation
        // level past the clip view, which cuts off the selection and the badges
        // until the sidebar is resized. The column follows the sidebar width instead.
        outlineView.autoresizesOutlineColumn = false
        outlineView.allowsEmptySelection = false
        outlineView.dataSource = self
        outlineView.delegate = self
        outlineView.registerForDraggedTypes([Self.itemPasteboardType])
        outlineView.menu = makeContextMenu()
        outlineView.target = self
        outlineView.doubleAction = #selector(doubleClicked(_:))

        scrollView.documentView = outlineView
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false
        scrollView.automaticallyAdjustsContentInsets = true

        // "+ New Collection" like the bottom of the Reminders sidebar; the menu
        // adds the subcollection choice.
        let plus = NSPopUpButton(frame: .zero, pullsDown: true)
        plus.isBordered = false
        plus.font = Theme.Font.secondary
        plus.imagePosition = .imageLeading
        (plus.cell as? NSPopUpButtonCell)?.arrowPosition = .noArrow
        plus.setContentHuggingPriority(.required, for: .horizontal)
        let plusMenu = NSMenu()
        let iconItem = NSMenuItem()
        iconItem.title = "New Collection"
        iconItem.image = NSImage(systemSymbolName: Theme.Symbol.addPaper, accessibilityDescription: "New")
        plusMenu.addItem(iconItem)
        plusMenu.addItem(withTitle: "New Collection", action: #selector(MainWindowController.newCollection(_:)), keyEquivalent: "")
        plusMenu.addItem(withTitle: "New Subcollection", action: #selector(MainWindowController.newSubcollection(_:)), keyEquivalent: "")
        plus.menu = plusMenu
        plus.toolTip = "New collection"
        plus.setAccessibilityLabel("New collection")

        libraryLabel.font = Theme.Font.secondary
        libraryLabel.textColor = Theme.Color.secondaryText
        libraryLabel.lineBreakMode = .byTruncatingMiddle
        libraryLabel.stringValue = store.libraryName
        libraryLabel.toolTip = store.libraryPath

        for subview in [plus, libraryLabel] as [NSView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            footer.addSubview(subview)
        }
        for subview in [scrollView, footer] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: root.topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: footer.topAnchor),
            footer.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            footer.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            footer.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            footer.heightAnchor.constraint(equalToConstant: Theme.Size.sidebarFooterHeight),
            plus.leadingAnchor.constraint(equalTo: footer.leadingAnchor, constant: Theme.Size.gap),
            plus.centerYAnchor.constraint(equalTo: footer.centerYAnchor),
            libraryLabel.leadingAnchor.constraint(equalTo: plus.trailingAnchor, constant: Theme.Size.gap),
            libraryLabel.trailingAnchor.constraint(equalTo: footer.trailingAnchor, constant: -Theme.Size.padding),
            libraryLabel.centerYAnchor.constraint(equalTo: footer.centerYAnchor),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        store.$collections.combineLatest(store.$tags, store.$smartListCounts)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _, _, _ in self?.rebuild() }
            .store(in: &subscriptions)
        store.$scope
            .receive(on: DispatchQueue.main)
            .sink { [weak self] scope in self?.selectRow(for: scope) }
            .store(in: &subscriptions)
    }

    // MARK: Tree

    private func rebuild() {
        let library = Node(id: "group.library", kind: .group("Library"))
        library.children = [
            Node(id: "smart.all", kind: .smartList(.all, title: "All Papers", symbol: Theme.Symbol.allPapers)),
            Node(id: "smart.recent", kind: .smartList(.recentlyAdded, title: "Recently Added", symbol: Theme.Symbol.recentlyAdded)),
            Node(id: "smart.unread", kind: .smartList(.unread, title: "Unread", symbol: Theme.Symbol.unread)),
            Node(id: "smart.nopdf", kind: .smartList(.noPdf, title: "No PDF", symbol: Theme.Symbol.noPdf)),
        ]

        let collections = Node(id: "group.collections", kind: .group("Collections"))
        var byId: [String: Node] = [:]
        for collection in store.collections {
            let node = Node(id: "collection.\(collection.id)", kind: .collection(collection))
            byId[collection.id] = node
            if let parent = collection.parentId, let parentNode = byId[parent] {
                parentNode.children.append(node)
            } else {
                collections.children.append(node)
            }
        }

        let tags = Node(id: "group.tags", kind: .group("Tags"))
        tags.children = store.tags.map { Node(id: "tag.\($0.id)", kind: .tag($0)) }

        roots = [library, collections, tags]
        nodesById = [:]
        func index(_ node: Node) {
            nodesById[node.id] = node
            node.children.forEach(index)
        }
        roots.forEach(index)

        suppressSelectionChange = true
        outlineView.reloadData()
        // Parents first: expanding a node whose parent is still collapsed
        // does nothing, so dictionary order would leave nested collections
        // closed at random.
        func expand(_ node: Node) {
            guard !collapsedIds.contains(node.id) else { return }
            outlineView.expandItem(node)
            node.children.forEach(expand)
        }
        roots.forEach(expand)
        suppressSelectionChange = false
        selectRow(for: store.scope)
    }

    private func nodeId(for scope: Scope) -> String {
        switch scope {
        case .all: "smart.all"
        case .recentlyAdded: "smart.recent"
        case .unread: "smart.unread"
        case .noPdf: "smart.nopdf"
        case .collection(let id): "collection.\(id)"
        case .tag(let id): "tag.\(id)"
        }
    }

    private func selectRow(for scope: Scope) {
        guard let node = nodesById[nodeId(for: scope)] else { return }
        let row = outlineView.row(forItem: node)
        guard row >= 0, outlineView.selectedRow != row else { return }
        suppressSelectionChange = true
        outlineView.selectRowIndexes([row], byExtendingSelection: false)
        suppressSelectionChange = false
    }

    // MARK: Actions

    func newCollection(parentId: String?) {
        guard let id = store.createCollection(name: "New Collection", parentId: parentId) else { return }
        if let parentId, let parent = nodesById["collection.\(parentId)"] {
            collapsedIds.remove(parent.id)
            outlineView.expandItem(parent)
        }
        store.scope = .collection(id: id)
        let row = outlineView.row(forItem: nodesById["collection.\(id)"] as Any)
        if row >= 0 { outlineView.editColumn(0, row: row, with: nil, select: true) }
    }

    @objc private func doubleClicked(_ sender: Any?) {
        let row = outlineView.clickedRow
        guard row >= 0, case .collection? = (outlineView.item(atRow: row) as? Node)?.kind else { return }
        outlineView.editColumn(0, row: row, with: nil, select: true)
    }

    @objc private func renameClicked(_ sender: Any?) {
        let row = outlineView.clickedRow
        guard row >= 0 else { return }
        outlineView.editColumn(0, row: row, with: nil, select: true)
    }

    @objc private func deleteCollectionClicked(_ sender: Any?) {
        guard let node = outlineView.item(atRow: outlineView.clickedRow) as? Node,
              case .collection(let collection) = node.kind
        else { return }
        let alert = NSAlert()
        alert.messageText = "Delete “\(collection.name)”?"
        alert.informativeText = "The papers in it stay in your library."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.buttons.first?.hasDestructiveAction = true
        guard let window = view.window else { return }
        alert.beginSheetModal(for: window) { [store] response in
            guard response == .alertFirstButtonReturn else { return }
            store.deleteCollection(id: collection.id)
        }
    }

    @objc private func newSubcollectionClicked(_ sender: Any?) {
        guard let node = outlineView.item(atRow: outlineView.clickedRow) as? Node,
              case .collection(let collection) = node.kind
        else { return }
        newCollection(parentId: collection.id)
    }

    /// Writes the papers directly in the clicked collection to a `.bib` file
    /// named after it.
    @objc private func exportCollectionClicked(_ sender: Any?) {
        guard let node = outlineView.item(atRow: outlineView.clickedRow) as? Node,
              case .collection(let collection) = node.kind
        else { return }
        let ids = store.itemIds(inCollection: collection.id)
        guard !ids.isEmpty else {
            store.show(Toast(id: "copied", style: .warning, message: "“\(collection.name)” has no papers to export"))
            return
        }
        store.exportBibtex(itemIds: ids, name: collection.name)
    }

    private func makeContextMenu() -> NSMenu {
        let menu = NSMenu()
        menu.addItem(withTitle: "New Collection", action: #selector(MainWindowController.newCollection(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "New Subcollection", action: #selector(newSubcollectionClicked(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Export BibTeX…", action: #selector(exportCollectionClicked(_:)), keyEquivalent: "")
        menu.addItem(.separator())
        menu.addItem(withTitle: "Rename", action: #selector(renameClicked(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Delete Collection", action: #selector(deleteCollectionClicked(_:)), keyEquivalent: "")
        colorItems = [NSMenuItem.sectionHeader(title: "Color")]
        for swatch in Theme.TagColor.swatches {
            let item = NSMenuItem(title: swatch.name, action: #selector(tagColorClicked(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = swatch.hex
            item.image = NSImage(size: NSSize(width: Theme.Size.iconSize, height: Theme.Size.iconSize), flipped: false) { rect in
                swatch.color.setFill()
                let inset = (Theme.Size.iconSize - Theme.Size.tagDotSize) / 2
                NSBezierPath(ovalIn: rect.insetBy(dx: inset, dy: inset)).fill()
                return true
            }
            colorItems.append(item)
        }
        colorItems.forEach(menu.addItem)
        menu.delegate = self
        return menu
    }

    @objc private func tagColorClicked(_ sender: NSMenuItem) {
        guard let node = outlineView.item(atRow: outlineView.clickedRow) as? Node,
              case .tag(let tag) = node.kind,
              let hex = sender.representedObject as? String
        else { return }
        store.setTagColor(tagId: tag.id, color: hex)
    }

    func controlTextDidEndEditing(_ notification: Notification) {
        guard let field = notification.object as? NSTextField else { return }
        let row = outlineView.row(for: field)
        guard row >= 0, let node = outlineView.item(atRow: row) as? Node,
              case .collection(let collection) = node.kind
        else { return }
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty {
            field.stringValue = collection.name
        } else if name != collection.name {
            store.renameCollection(id: collection.id, name: name)
        }
    }

    // MARK: Data source

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        (item as? Node)?.children.count ?? roots.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        (item as? Node)?.children[index] ?? roots[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        !((item as? Node)?.children.isEmpty ?? true)
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        if case .group? = (item as? Node)?.kind { return true }
        return false
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        !self.outlineView(outlineView, isGroupItem: item)
    }

    func outlineView(_ outlineView: NSOutlineView, shouldShowOutlineCellForItem item: Any) -> Bool {
        if case .collection? = (item as? Node)?.kind { return true }
        return false
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        self.outlineView(outlineView, isGroupItem: item) ? Theme.Size.sidebarGroupHeight : Theme.Size.sidebarRowHeight
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let node = item as? Node else { return nil }
        switch node.kind {
        case .group(let title):
            let cell = outlineView.makeView(withIdentifier: Self.groupIdentifier, owner: self) as? NSTableCellView
                ?? Self.makeGroupCell(identifier: Self.groupIdentifier)
            cell.textField?.stringValue = title
            return cell
        case .smartList(let scope, let title, let symbol):
            let cell = dequeueCell()
            cell.configure(title: title, symbol: symbol, color: nil, badge: store.smartListCounts[scope], editable: false)
            return cell
        case .collection(let collection):
            let cell = dequeueCell()
            cell.configure(title: collection.name, symbol: Theme.Symbol.collection, color: nil,
                           badge: collection.itemCount > 0 ? Int(collection.itemCount) : nil, editable: true)
            cell.label.delegate = self
            return cell
        case .tag(let tag):
            let cell = dequeueCell()
            cell.configure(title: tag.name, symbol: nil, color: NSColor(hexString: tag.color) ?? Theme.Color.defaultTag,
                           badge: Int(tag.itemCount), editable: false)
            return cell
        }
    }

    private func dequeueCell() -> SidebarCellView {
        outlineView.makeView(withIdentifier: Self.cellIdentifier, owner: self) as? SidebarCellView
            ?? SidebarCellView(identifier: Self.cellIdentifier)
    }

    /// A group label row; the reader's table of contents uses it too.
    static func makeGroupCell(identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
        let cell = NSTableCellView()
        cell.identifier = identifier
        let label = NSTextField(labelWithString: "")
        label.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(label)
        cell.textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor),
            label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionChange,
              let node = outlineView.item(atRow: outlineView.selectedRow) as? Node,
              let scope = node.scope, scope != store.scope
        else { return }
        store.scope = scope
    }

    func outlineViewItemDidCollapse(_ notification: Notification) {
        if let node = notification.userInfo?["NSObject"] as? Node { collapsedIds.insert(node.id) }
    }

    func outlineViewItemDidExpand(_ notification: Notification) {
        if let node = notification.userInfo?["NSObject"] as? Node { collapsedIds.remove(node.id) }
    }

    // MARK: Drop

    func outlineView(_ outlineView: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?, proposedChildIndex index: Int) -> NSDragOperation {
        guard case .collection? = (item as? Node)?.kind else { return [] }
        if index != NSOutlineViewDropOnItemIndex {
            outlineView.setDropItem(item, dropChildIndex: NSOutlineViewDropOnItemIndex)
        }
        return .link
    }

    func outlineView(_ outlineView: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
        guard case .collection(let collection)? = (item as? Node)?.kind,
              let data = info.draggingPasteboard.data(forType: Self.itemPasteboardType),
              let ids = try? JSONDecoder().decode([String].self, from: data)
        else { return false }
        store.addToCollection(collectionId: collection.id, itemIds: ids)
        return true
    }
}

extension SidebarViewController: NSMenuDelegate {
    func menuNeedsUpdate(_ menu: NSMenu) {
        let row = outlineView.clickedRow
        let kind = row >= 0 ? (outlineView.item(atRow: row) as? Node)?.kind : nil
        var isCollection = false
        var tag: Tag?
        switch kind {
        case .collection?: isCollection = true
        case .tag(let clicked)?: tag = clicked
        default: break
        }
        for item in menu.items {
            if colorItems.contains(item) {
                item.isHidden = tag == nil
                let hex = item.representedObject as? String
                item.state = hex?.caseInsensitiveCompare(tag?.color ?? "") == .orderedSame ? .on : .off
            } else if item.action == #selector(MainWindowController.newCollection(_:)) {
                item.isHidden = tag != nil
            } else {
                item.isHidden = !isCollection
            }
        }
    }
}

/// A source list row: symbol or color dot, name, and a count badge.
@MainActor
final class SidebarCellView: NSTableCellView {
    private let icon = NSImageView()
    private let dot = NSView()
    let label = NSTextField(labelWithString: "")
    private let badge = NSTextField(labelWithString: "")

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier

        icon.symbolConfiguration = .init(pointSize: Theme.Size.iconSize, weight: .regular)
        icon.contentTintColor = Theme.Color.accent
        dot.wantsLayer = true
        dot.layer?.cornerRadius = Theme.Size.tagDotSize / 2
        label.font = Theme.Font.sidebar
        label.lineBreakMode = .byTruncatingTail
        label.isEditable = false
        label.isBordered = false
        label.drawsBackground = false
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        badge.font = Theme.Font.sidebarBadge
        badge.textColor = Theme.Color.secondaryText
        badge.alignment = .right

        for subview in [icon, dot, label, badge] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            addSubview(subview)
        }
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: leadingAnchor),
            icon.centerYAnchor.constraint(equalTo: centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: Theme.Size.iconSize + Theme.Size.smallGap),
            dot.centerXAnchor.constraint(equalTo: icon.centerXAnchor),
            dot.centerYAnchor.constraint(equalTo: centerYAnchor),
            dot.widthAnchor.constraint(equalToConstant: Theme.Size.tagDotSize),
            dot.heightAnchor.constraint(equalToConstant: Theme.Size.tagDotSize),
            label.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: Theme.Size.gap),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.leadingAnchor.constraint(greaterThanOrEqualTo: label.trailingAnchor, constant: Theme.Size.gap),
            badge.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Size.smallGap),
            badge.centerYAnchor.constraint(equalTo: centerYAnchor),
            badge.widthAnchor.constraint(greaterThanOrEqualToConstant: Theme.Size.sidebarBadgeMinWidth),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func configure(title: String, symbol: String?, color: NSColor?, badge count: Int?, editable: Bool) {
        label.stringValue = title
        label.isEditable = editable
        if let symbol {
            icon.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            icon.isHidden = false
            dot.isHidden = true
        } else {
            icon.image = nil
            icon.isHidden = true
            dot.isHidden = false
            dot.layer?.backgroundColor = color?.cgColor
        }
        badge.stringValue = count.map(String.init) ?? ""
        badge.isHidden = count == nil
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet {
            badge.textColor = backgroundStyle == .emphasized ? Theme.Color.selectedText : Theme.Color.secondaryText
        }
    }
}

extension NSColor {
    /// `#RRGGBB` as stored by the engine for tag colors.
    convenience init?(hexString: String?) {
        guard let hexString, hexString.hasPrefix("#"), hexString.count == 7,
              let value = UInt32(hexString.dropFirst(), radix: 16)
        else { return nil }
        self.init(
            srgbRed: CGFloat((value >> 16) & 0xFF) / 255,
            green: CGFloat((value >> 8) & 0xFF) / 255,
            blue: CGFloat(value & 0xFF) / 255,
            alpha: 1
        )
    }
}
