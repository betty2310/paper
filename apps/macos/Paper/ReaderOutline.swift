import AppKit
import PDFKit

/// A PDF's table of contents, from its outline (bookmarks), shown in the
/// sidebar while the PDF's tab is in front. Clicking an entry goes there,
/// and the entry for the part on screen stays selected as the reader scrolls.
@MainActor
final class ReaderOutlineViewController: NSViewController, NSOutlineViewDataSource, NSOutlineViewDelegate {
    /// One entry, copied out of `PDFOutline` once when the document opens.
    final class Entry {
        let title: String
        let destination: PDFDestination?
        /// 0 based, or nil when the entry goes nowhere in this document.
        let pageIndex: Int?
        weak var parent: Entry?
        var children: [Entry] = []

        init(title: String, destination: PDFDestination?, pageIndex: Int?) {
            self.title = title
            self.destination = destination
            self.pageIndex = pageIndex
        }
    }

    private static let cellIdentifier = NSUserInterfaceItemIdentifier("outlineCell")
    private static let groupIdentifier = NSUserInterfaceItemIdentifier("outlineGroup")
    /// The group row heading the entries, as in the library's sidebar.
    private let group = Entry(title: "Contents", destination: nil, pageIndex: nil)

    /// Called when the user picks an entry.
    var onSelect: (PDFDestination) -> Void = { _ in }

    private let outlineView = NSOutlineView()
    private let scrollView = NSScrollView()
    private let emptyLabel = NSTextField(wrappingLabelWithString: "This PDF has no table of contents.")
    /// Every entry with a place in the document, in reading order.
    private var ordered: [Entry] = []
    private var suppressSelectionChange = false

    override func loadView() {
        let root = NSView()

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("main"))
        column.resizingMask = .autoresizingMask
        outlineView.addTableColumn(column)
        outlineView.outlineTableColumn = column
        outlineView.headerView = nil
        outlineView.style = .sourceList
        outlineView.floatsGroupRows = false
        outlineView.indentationPerLevel = Theme.Size.padding
        // See the library sidebar: the column follows the sidebar's width.
        outlineView.autoresizesOutlineColumn = false
        outlineView.dataSource = self
        outlineView.delegate = self
        outlineView.setAccessibilityLabel("Table of Contents")

        scrollView.documentView = outlineView
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false
        scrollView.automaticallyAdjustsContentInsets = true

        emptyLabel.font = Theme.Font.body
        emptyLabel.textColor = Theme.Color.tertiaryText
        emptyLabel.alignment = .center
        emptyLabel.isHidden = true

        for subview in [scrollView, emptyLabel] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: root.topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            emptyLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: Theme.Size.padding),
            emptyLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -Theme.Size.padding),
            emptyLabel.centerYAnchor.constraint(equalTo: root.centerYAnchor),
        ])
        view = root
    }

    /// Reads the document's outline. Top level entries start expanded, so
    /// a paper's sections and subsections show at once.
    func load(_ document: PDFDocument) {
        _ = view
        group.children = []
        ordered = []
        if let root = document.outlineRoot {
            group.children = entries(under: root, in: document, parent: group)
        }
        emptyLabel.isHidden = !group.children.isEmpty
        scrollView.isHidden = group.children.isEmpty
        suppressSelectionChange = true
        outlineView.reloadData()
        outlineView.expandItem(group)
        group.children.forEach { outlineView.expandItem($0) }
        suppressSelectionChange = false
    }

    private func entries(under outline: PDFOutline, in document: PDFDocument, parent: Entry) -> [Entry] {
        (0..<outline.numberOfChildren).compactMap { index in
            guard let child = outline.child(at: index) else { return nil }
            let destination = child.destination ?? (child.action as? PDFActionGoTo)?.destination
            let entry = Entry(
                title: child.label?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "",
                destination: destination,
                pageIndex: destination?.page.map { document.index(for: $0) }
            )
            entry.parent = parent
            if entry.pageIndex != nil { ordered.append(entry) }
            entry.children = entries(under: child, in: document, parent: entry)
            return entry
        }
    }

    /// Selects the entry for the part of the document at the top of the
    /// reader: the last one that starts at or before it. When that entry is
    /// inside a collapsed one, the collapsed one is selected instead.
    func reveal(_ position: PDFDestination, pageIndex: Int) {
        let y = position.point.y
        let entry = ordered.last { entry in
            guard let page = entry.pageIndex else { return false }
            if page != pageIndex { return page < pageIndex }
            // Page space grows upward; an unspecified point is the page's top.
            guard let top = entry.destination?.point.y, top < kPDFDestinationUnspecifiedValue else { return true }
            return top >= y - Theme.Size.outlinePositionSlop
        }
        var shown = entry
        while let candidate = shown, outlineView.row(forItem: candidate) < 0 { shown = candidate.parent }
        guard let shown, shown !== group else {
            outlineView.deselectAll(nil)
            return
        }
        let row = outlineView.row(forItem: shown)
        guard outlineView.selectedRow != row else { return }
        suppressSelectionChange = true
        outlineView.selectRowIndexes([row], byExtendingSelection: false)
        outlineView.scrollRowToVisible(row)
        suppressSelectionChange = false
    }

    // MARK: Data source

    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        guard let entry = item as? Entry else { return group.children.isEmpty ? 0 : 1 }
        return entry.children.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        (item as? Entry)?.children[index] ?? group
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        !((item as? Entry)?.children.isEmpty ?? true)
    }

    func outlineView(_ outlineView: NSOutlineView, isGroupItem item: Any) -> Bool {
        item as? Entry === group
    }

    func outlineView(_ outlineView: NSOutlineView, shouldSelectItem item: Any) -> Bool {
        (item as? Entry)?.destination != nil
    }

    func outlineView(_ outlineView: NSOutlineView, heightOfRowByItem item: Any) -> CGFloat {
        item as? Entry === group ? Theme.Size.sidebarGroupHeight : Theme.Size.sidebarRowHeight
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        guard let entry = item as? Entry else { return nil }
        if entry === group {
            let cell = outlineView.makeView(withIdentifier: Self.groupIdentifier, owner: self) as? NSTableCellView
                ?? SidebarViewController.makeGroupCell(identifier: Self.groupIdentifier)
            cell.textField?.stringValue = entry.title
            return cell
        }
        let cell = outlineView.makeView(withIdentifier: Self.cellIdentifier, owner: self) as? OutlineCellView
            ?? OutlineCellView(identifier: Self.cellIdentifier)
        cell.configure(title: entry.title, page: entry.pageIndex.map { $0 + 1 })
        return cell
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionChange,
              let destination = (outlineView.item(atRow: outlineView.selectedRow) as? Entry)?.destination
        else { return }
        onSelect(destination)
    }
}

/// An entry's row: its title, and its page number at the trailing edge.
@MainActor
private final class OutlineCellView: NSTableCellView {
    private let label = NSTextField(labelWithString: "")
    private let page = NSTextField(labelWithString: "")

    init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        label.font = Theme.Font.sidebar
        label.lineBreakMode = .byTruncatingTail
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        page.font = Theme.Font.sidebarBadge
        page.textColor = Theme.Color.secondaryText
        page.alignment = .right
        page.setContentCompressionResistancePriority(.required, for: .horizontal)
        for subview in [label, page] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            addSubview(subview)
        }
        textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            page.leadingAnchor.constraint(greaterThanOrEqualTo: label.trailingAnchor, constant: Theme.Size.gap),
            page.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Size.smallGap),
            page.centerYAnchor.constraint(equalTo: centerYAnchor),
            page.widthAnchor.constraint(greaterThanOrEqualToConstant: Theme.Size.sidebarBadgeMinWidth),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func configure(title: String, page number: Int?) {
        label.stringValue = title
        // Long section titles are cut; the full one shows on hover.
        toolTip = title
        page.stringValue = number.map(String.init) ?? ""
        setAccessibilityLabel(number.map { "\(title), page \($0)" } ?? title)
    }

    override var backgroundStyle: NSView.BackgroundStyle {
        didSet {
            page.textColor = backgroundStyle == .emphasized ? Theme.Color.selectedText : Theme.Color.secondaryText
        }
    }
}
