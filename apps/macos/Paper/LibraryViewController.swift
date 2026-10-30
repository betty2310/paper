import AppKit
import PaperEngine

/// The library tab: a search field, an "Add DOI" button, the item table, and a
/// status line. Reads come from `LibraryStore`; nothing here calls the engine.
final class LibraryViewController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private enum Column {
        static let title = NSUserInterfaceItemIdentifier("title")
        static let authors = NSUserInterfaceItemIdentifier("authors")
        static let year = NSUserInterfaceItemIdentifier("year")
    }

    private let store: LibraryStore
    private let searchField = NSSearchField()
    private let addButton = NSButton(title: "Add DOI", target: nil, action: nil)
    private let tableView = NSTableView()
    private let statusLabel = NSTextField(labelWithString: "")

    init(store: LibraryStore) {
        self.store = store
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not used")
    }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.Color.canvas.cgColor

        searchField.placeholderString = "Search papers, or paste a DOI and press Add DOI"
        searchField.font = Theme.Font.body
        searchField.target = self
        searchField.action = #selector(searchChanged(_:))

        addButton.bezelStyle = .rounded
        addButton.keyEquivalent = "\r"
        addButton.target = self
        addButton.action = #selector(addDOI(_:))

        tableView.addTableColumn(makeColumn(Column.title, "Title", minWidth: Theme.Size.titleColumnMinWidth))
        tableView.addTableColumn(makeColumn(Column.authors, "Authors", width: Theme.Size.authorsColumnWidth))
        tableView.addTableColumn(makeColumn(Column.year, "Year", width: Theme.Size.yearColumnWidth))
        tableView.rowHeight = Theme.Size.rowHeight
        tableView.gridStyleMask = .solidHorizontalGridLineMask
        tableView.gridColor = Theme.Color.border
        tableView.backgroundColor = Theme.Color.panel
        tableView.columnAutoresizingStyle = .firstColumnOnlyAutoresizingStyle
        tableView.dataSource = self
        tableView.delegate = self

        let scrollView = NSScrollView()
        scrollView.documentView = tableView
        scrollView.hasVerticalScroller = true

        statusLabel.font = Theme.Font.muted
        statusLabel.textColor = Theme.Color.textMuted
        statusLabel.lineBreakMode = .byTruncatingMiddle

        for subview in [searchField, addButton, scrollView, statusLabel] as [NSView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        let padding = Theme.Size.padding
        NSLayoutConstraint.activate([
            searchField.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: padding),
            searchField.centerYAnchor.constraint(equalTo: root.topAnchor, constant: Theme.Size.toolbarHeight / 2),
            addButton.leadingAnchor.constraint(equalTo: searchField.trailingAnchor, constant: Theme.Size.gap),
            addButton.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -padding),
            addButton.centerYAnchor.constraint(equalTo: searchField.centerYAnchor),

            scrollView.topAnchor.constraint(equalTo: root.topAnchor, constant: Theme.Size.toolbarHeight),
            scrollView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -Theme.Size.statusBarHeight),

            statusLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: padding),
            statusLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -padding),
            statusLabel.centerYAnchor.constraint(equalTo: root.bottomAnchor, constant: -Theme.Size.statusBarHeight / 2),
        ])
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        store.onItemsChanged = { [weak self] in
            self?.tableView.reloadData()
            self?.showItemCount()
        }
        showItemCount()
    }

    // MARK: Actions

    @objc private func searchChanged(_ sender: NSSearchField) {
        store.search(sender.stringValue)
    }

    @objc private func addDOI(_ sender: Any?) {
        let text = searchField.stringValue
        addButton.isEnabled = false
        statusLabel.stringValue = "Looking up \(text)…"
        // The Task inherits the main actor, so the UI code after `await` is safe.
        Task {
            defer { addButton.isEnabled = true }
            do {
                let row = try await store.add(identifier: text)
                searchField.stringValue = ""
                store.search("")
                statusLabel.stringValue = "Added “\(row.title)”"
            } catch {
                statusLabel.stringValue = Self.message(for: error)
            }
        }
    }

    private func showItemCount() {
        statusLabel.stringValue = "\(store.items.count) papers · \(store.libraryPath)"
    }

    /// Rust's `PaperError` arrives as a Swift enum, so the app can switch on it.
    /// UniFFI keeps the Rust variant names, which is why the cases are capitalized.
    private static func message(for error: Error) -> String {
        switch error as? PaperError {
        case .InvalidIdentifier(let text): "Not a DOI or arXiv ID: \(text)"
        case .Duplicate(let doi, _): "Already in the library: \(doi)"
        case .MetadataNotFound(let doi, _): "No metadata found for \(doi)"
        case .Lookup(let doi, _, let message): "Could not look up \(doi): \(message)"
        case .some(let other): String(describing: other)
        case nil: error.localizedDescription
        }
    }

    // MARK: Table

    func numberOfRows(in tableView: NSTableView) -> Int {
        store.items.count
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let identifier = tableColumn?.identifier else { return nil }
        let item = store.items[row]
        let cell = tableView.makeView(withIdentifier: identifier, owner: self) as? NSTableCellView
            ?? makeCell(identifier)
        let label = cell.textField!
        switch identifier {
        case Column.title:
            label.stringValue = item.title
            label.font = Theme.Font.title
            label.textColor = Theme.Color.textPrimary
        case Column.authors:
            label.stringValue = item.authors
            label.font = Theme.Font.body
            label.textColor = Theme.Color.textSecondary
        default:
            label.stringValue = item.year.map(String.init) ?? ""
            label.font = Theme.Font.body
            label.textColor = Theme.Color.textSecondary
        }
        return cell
    }

    private func makeColumn(
        _ identifier: NSUserInterfaceItemIdentifier, _ title: String,
        width: CGFloat? = nil, minWidth: CGFloat? = nil
    ) -> NSTableColumn {
        let column = NSTableColumn(identifier: identifier)
        column.title = title
        if let width { column.width = width }
        if let minWidth { column.minWidth = minWidth }
        return column
    }

    private func makeCell(_ identifier: NSUserInterfaceItemIdentifier) -> NSTableCellView {
        let cell = NSTableCellView()
        cell.identifier = identifier
        let label = NSTextField(labelWithString: "")
        label.lineBreakMode = .byTruncatingTail
        label.translatesAutoresizingMaskIntoConstraints = false
        cell.addSubview(label)
        cell.textField = label
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: Theme.Size.gap),
            label.trailingAnchor.constraint(equalTo: cell.trailingAnchor, constant: -Theme.Size.gap),
            label.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
        ])
        return cell
    }
}
