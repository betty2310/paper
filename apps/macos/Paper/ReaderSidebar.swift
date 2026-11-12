import AppKit
import PDFKit

/// The sidebar while a PDF's tab is in front: a switch at the top between
/// the PDF's table of contents and its pages. The choice is shared by every
/// PDF and remembered across launches.
@MainActor
final class ReaderSidebarViewController: NSViewController {
    enum Mode: Int {
        case contents
        case pages
    }

    private static let modeKey = "readerSidebarMode"

    let outline = ReaderOutlineViewController()
    let pages = ReaderPagesViewController()
    private let modeControl = NSSegmentedControl(labels: ["Contents", "Pages"], trackingMode: .selectOne, target: nil, action: nil)

    private static var storedMode: Mode {
        get { Mode(rawValue: UserDefaults.standard.integer(forKey: modeKey)) ?? .contents }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: modeKey) }
    }

    override func loadView() {
        let root = NSView()
        addChild(outline)
        addChild(pages)

        modeControl.segmentDistribution = .fillEqually
        modeControl.target = self
        modeControl.action = #selector(modeChanged(_:))
        modeControl.setAccessibilityLabel("Sidebar content")

        for subview in [modeControl, outline.view, pages.view] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        NSLayoutConstraint.activate([
            modeControl.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: Theme.Size.gap),
            modeControl.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: Theme.Size.padding),
            modeControl.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -Theme.Size.padding),
        ])
        for content in [outline.view, pages.view] {
            NSLayoutConstraint.activate([
                content.topAnchor.constraint(equalTo: modeControl.bottomAnchor, constant: Theme.Size.gap),
                content.leadingAnchor.constraint(equalTo: root.leadingAnchor),
                content.trailingAnchor.constraint(equalTo: root.trailingAnchor),
                content.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            ])
        }
        view = root
        show(Self.storedMode)
    }

    /// Another PDF's sidebar may have switched while this one was away.
    override func viewWillAppear() {
        super.viewWillAppear()
        show(Self.storedMode)
    }

    func load(_ document: PDFDocument) {
        _ = view
        outline.load(document)
        pages.load(document)
    }

    /// Follows the part of the document at the top of the reader.
    func reveal(_ position: PDFDestination, pageIndex: Int) {
        outline.reveal(position, pageIndex: pageIndex)
        pages.reveal(pageIndex)
    }

    var showsPages: Bool { Self.storedMode == .pages }

    /// ⌘2: between the table of contents and the pages.
    func togglePages() {
        _ = view
        set(showsPages ? .contents : .pages)
    }

    @objc private func modeChanged(_ sender: NSSegmentedControl) {
        set(Mode(rawValue: sender.selectedSegment) ?? .contents)
    }

    private func set(_ mode: Mode) {
        Self.storedMode = mode
        show(mode)
    }

    private func show(_ mode: Mode) {
        modeControl.selectedSegment = mode.rawValue
        outline.view.isHidden = mode != .contents
        pages.view.isHidden = mode != .pages
        if mode == .pages { pages.scrollToCurrent() }
    }
}

/// Every page of a PDF as a thumbnail with its number under it, the
/// current page outlined in the accent color. Clicking a page goes there,
/// and Back returns, as after a link. Thumbnails are drawn off the main
/// actor the first time their row shows, and kept.
@MainActor
final class ReaderPagesViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private static let cellIdentifier = NSUserInterfaceItemIdentifier("pageCell")

    /// Called with the 0 based index of the page the user picked.
    var onSelect: (Int) -> Void = { _ in }

    private let tableView = PagesTableView()
    private let scrollView = NSScrollView()
    private var document: PDFDocument?
    /// Each page's thumbnail size in points, from its crop box and rotation.
    private var sizes: [NSSize] = []
    private var thumbnails: [Int: NSImage] = [:]
    private var rendering: Set<Int> = []
    private var current = 0
    private var suppressSelectionChange = false

    override func loadView() {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("main"))
        column.resizingMask = .autoresizingMask
        tableView.addTableColumn(column)
        tableView.headerView = nil
        tableView.style = .plain
        tableView.backgroundColor = .clear
        tableView.selectionHighlightStyle = .none
        tableView.intercellSpacing = .zero
        tableView.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        tableView.focusRingType = .none
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.action = #selector(clicked(_:))
        tableView.setAccessibilityLabel("Pages")

        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = false
        scrollView.automaticallyAdjustsContentInsets = true
        view = scrollView
    }

    func load(_ document: PDFDocument) {
        _ = view
        self.document = document
        thumbnails = [:]
        rendering = []
        current = 0
        sizes = (0..<document.pageCount).map { index in
            guard let page = document.page(at: index) else { return .zero }
            var box = page.bounds(for: .cropBox).size
            if page.rotation % 180 != 0 { box = NSSize(width: box.height, height: box.width) }
            guard box.width > 0 else { return .zero }
            let width = Theme.Size.thumbnailWidth
            return NSSize(width: width, height: (width * box.height / box.width).rounded())
        }
        tableView.reloadData()
        select(current)
    }

    /// Outlines the page at the top of the reader and keeps it in view.
    func reveal(_ index: Int) {
        guard index != current, index < sizes.count else { return }
        let old = current
        current = index
        tableView.reloadData(forRowIndexes: [old, index], columnIndexes: [0])
        select(index)
        scrollToCurrent()
    }

    func scrollToCurrent() {
        guard current < tableView.numberOfRows else { return }
        tableView.scrollRowToVisible(current)
    }

    private func select(_ row: Int) {
        guard row < tableView.numberOfRows, tableView.selectedRow != row else { return }
        suppressSelectionChange = true
        tableView.selectRowIndexes([row], byExtendingSelection: false)
        suppressSelectionChange = false
    }

    /// A click goes to the page, even the current one when it is scrolled
    /// partly out of view.
    @objc private func clicked(_ sender: Any?) {
        let row = tableView.clickedRow
        guard row >= 0 else { return }
        onSelect(row)
    }

    // MARK: Thumbnails

    private func thumbnail(for index: Int) -> NSImage? {
        if let image = thumbnails[index] { return image }
        guard !rendering.contains(index), let document, let page = document.page(at: index) else { return nil }
        rendering.insert(index)
        // Drawn at 2x so they stay sharp on Retina displays.
        let pixels = NSSize(width: sizes[index].width * 2, height: sizes[index].height * 2)
        let box = PDFKitBox(value: page)
        Task { [weak self] in
            let image = await Task.detached(priority: .utility) {
                PDFKitBox(value: box.value.thumbnail(of: pixels, for: .cropBox))
            }.value
            guard let self, self.document === document else { return }
            self.rendering.remove(index)
            self.thumbnails[index] = image.value
            self.tableView.reloadData(forRowIndexes: [index], columnIndexes: [0])
        }
        return nil
    }

    // MARK: Table

    func numberOfRows(in tableView: NSTableView) -> Int {
        sizes.count
    }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        PageCellView.height(forThumbnail: sizes[row])
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = tableView.makeView(withIdentifier: Self.cellIdentifier, owner: self) as? PageCellView
            ?? PageCellView(identifier: Self.cellIdentifier)
        cell.configure(
            number: row + 1,
            size: sizes[row],
            image: thumbnail(for: row),
            isCurrent: row == current,
            isPressed: row == self.tableView.pressedRow
        )
        return cell
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        PageRowView()
    }

    /// The arrow keys move between pages; clicks go through `clicked`.
    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !suppressSelectionChange, NSApp.currentEvent?.type == .keyDown, tableView.selectedRow >= 0 else { return }
        onSelect(tableView.selectedRow)
    }
}

/// Knows which row is under a mouse press, so its page can look pressed.
@MainActor
private final class PagesTableView: NSTableView {
    private(set) var pressedRow = -1

    override func mouseDown(with event: NSEvent) {
        let row = row(at: convert(event.locationInWindow, from: nil))
        setPressed(row)
        // The table tracks the click until the mouse goes up.
        super.mouseDown(with: event)
        setPressed(-1)
    }

    private func setPressed(_ row: Int) {
        let rows = IndexSet([pressedRow, row].filter { $0 >= 0 })
        pressedRow = row
        reloadData(forRowIndexes: rows, columnIndexes: [0])
    }
}

/// Shows its page as hovered while the pointer is over the row.
@MainActor
private final class PageRowView: NSTableRowView {
    private var hoverArea: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hoverArea { removeTrackingArea(hoverArea) }
        let area = NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area)
        hoverArea = area
    }

    override func mouseEntered(with event: NSEvent) { setHovered(true) }
    override func mouseExited(with event: NSEvent) { setHovered(false) }

    override func didAddSubview(_ subview: NSView) {
        super.didAddSubview(subview)
        (subview as? PageCellView)?.isHovered = false
    }

    private func setHovered(_ hovered: Bool) {
        for case let cell as PageCellView in subviews { cell.isHovered = hovered }
    }
}

/// One page: its thumbnail, centered, and its number under it.
@MainActor
private final class PageCellView: NSTableCellView {
    private let thumbnail = PageThumbnailView()
    private let number = NSTextField(labelWithString: "")
    private let thumbnailWidth: NSLayoutConstraint
    private let thumbnailHeight: NSLayoutConstraint
    private var isCurrent = false

    var isHovered = false {
        didSet { thumbnail.isHovered = isHovered }
    }

    static func height(forThumbnail size: NSSize) -> CGFloat {
        Theme.Size.thumbnailPadding * 2 + size.height + Theme.Size.smallGap + Theme.Size.thumbnailLabelHeight
    }

    init(identifier: NSUserInterfaceItemIdentifier) {
        thumbnailWidth = thumbnail.widthAnchor.constraint(equalToConstant: Theme.Size.thumbnailWidth)
        thumbnailHeight = thumbnail.heightAnchor.constraint(equalToConstant: 0)
        super.init(frame: .zero)
        self.identifier = identifier
        applyFonts()
        NotificationCenter.default.addObserver(self, selector: #selector(applyFonts), name: Theme.Font.didChange, object: nil)
        number.alignment = .center
        for subview in [thumbnail, number] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            addSubview(subview)
        }
        NSLayoutConstraint.activate([
            thumbnailWidth,
            thumbnailHeight,
            thumbnail.topAnchor.constraint(equalTo: topAnchor, constant: Theme.Size.thumbnailPadding),
            thumbnail.centerXAnchor.constraint(equalTo: centerXAnchor),
            number.topAnchor.constraint(equalTo: thumbnail.bottomAnchor, constant: Theme.Size.smallGap),
            number.heightAnchor.constraint(equalToConstant: Theme.Size.thumbnailLabelHeight),
            number.centerXAnchor.constraint(equalTo: centerXAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    @objc private func applyFonts() {
        number.font = Theme.Font.sidebarBadge
    }

    func configure(number page: Int, size: NSSize, image: NSImage?, isCurrent: Bool, isPressed: Bool) {
        self.isCurrent = isCurrent
        thumbnailWidth.constant = size.width
        thumbnailHeight.constant = size.height
        thumbnail.image = image
        thumbnail.isCurrent = isCurrent
        thumbnail.isPressed = isPressed
        number.stringValue = String(page)
        number.textColor = isCurrent ? Theme.Color.accent : Theme.Color.secondaryText
        setAccessibilityLabel("Page \(page)")
        setAccessibilityValue(isCurrent ? "Current page" : nil)
    }
}

/// A page image with a hairline border, and the accent outline around it
/// when it is the current page. Hover and press darken the border.
@MainActor
private final class PageThumbnailView: NSView {
    var image: NSImage? { didSet { needsDisplay = true } }
    var isCurrent = false { didSet { needsDisplay = true } }
    var isHovered = false { didSet { needsDisplay = true } }
    var isPressed = false { didSet { needsDisplay = true } }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        let hairline = 1 / (window?.backingScaleFactor ?? 2)
        Theme.Color.thumbnailPage.setFill()
        bounds.fill()
        image?.draw(in: bounds)

        let borderColor = isPressed ? Theme.Color.thumbnailPressedBorder
            : isHovered ? Theme.Color.thumbnailHoverBorder
            : Theme.Color.separator
        borderColor.setStroke()
        let border = NSBezierPath(rect: bounds.insetBy(dx: hairline / 2, dy: hairline / 2))
        border.lineWidth = hairline
        border.stroke()

        guard isCurrent else { return }
        // Outside the page, so the outline never covers its content.
        let width = Theme.Size.thumbnailOutlineWidth
        let inset = -(Theme.Size.thumbnailOutlineGap + width / 2)
        let outline = NSBezierPath(
            roundedRect: bounds.insetBy(dx: inset, dy: inset),
            xRadius: Theme.Size.thumbnailOutlineRadius,
            yRadius: Theme.Size.thumbnailOutlineRadius
        )
        outline.lineWidth = width
        Theme.Color.accent.setStroke()
        outline.stroke()
    }

    /// The outline is drawn outside the page, so the view must not clip it.
    override var wantsDefaultClipping: Bool { false }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        needsDisplay = true
    }
}
