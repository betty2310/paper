import AppKit
import Combine
import PaperEngine
import SwiftUI

/// One window per open library: a unified toolbar over a three column split
/// view (sidebar, paper list, inspector). Commands are selectors on the
/// responder chain so menu items, toolbar buttons, and SwiftUI buttons share
/// one code path.
@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSSearchFieldDelegate, NSMenuItemValidation {
    let store: LibraryStore
    let splitViewController = NSSplitViewController()
    let sidebar: SidebarViewController
    let list: PaperListViewController
    let inspector: InspectorViewController
    private var inspectorItem: NSSplitViewItem!
    private let searchField = NSSearchField()
    private let addPopover = NSPopover()
    private var subscriptions: Set<AnyCancellable> = []

    private enum ToolbarID {
        static let add = NSToolbarItem.Identifier("add")
        static let sort = NSToolbarItem.Identifier("sort")
        static let search = NSToolbarItem.Identifier("search")
        static let inspector = NSToolbarItem.Identifier("inspector")
    }

    init(store: LibraryStore) {
        self.store = store
        sidebar = SidebarViewController(store: store)
        list = PaperListViewController(store: store)
        inspector = InspectorViewController(store: store)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: Theme.Size.windowWidth, height: Theme.Size.windowHeight),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init(window: window)

        window.delegate = self
        window.minSize = NSSize(width: Theme.Size.windowMinWidth, height: Theme.Size.windowMinHeight)
        window.toolbarStyle = .unified
        window.titlebarSeparatorStyle = .automatic
        window.tabbingMode = .disallowed
        window.isReleasedWhenClosed = false

        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sidebarItem.minimumThickness = Theme.Size.sidebarMinWidth
        sidebarItem.maximumThickness = Theme.Size.sidebarMaxWidth
        sidebarItem.canCollapse = true
        sidebarItem.titlebarSeparatorStyle = .none

        let listItem = NSSplitViewItem(viewController: list)
        listItem.minimumThickness = Theme.Size.listMinWidth

        inspectorItem = NSSplitViewItem(inspectorWithViewController: inspector)
        inspectorItem.minimumThickness = Theme.Size.inspectorMinWidth
        inspectorItem.maximumThickness = Theme.Size.inspectorMaxWidth

        // The list, with the lowest holding priority, absorbs window resizing.
        let settings = store.settings
        sidebarItem.holdingPriority = .defaultLow + 2
        inspectorItem.holdingPriority = .defaultLow + 2
        splitViewController.addSplitViewItem(sidebarItem)
        splitViewController.addSplitViewItem(listItem)
        splitViewController.addSplitViewItem(inspectorItem)
        window.contentViewController = splitViewController

        // Setting the content view controller shrank the window to the split view's
        // fitting size, so the frame comes after it.
        if let saved = settings.window {
            window.setFrame(NSRect(x: saved.x, y: saved.y, width: saved.width, height: saved.height), display: false)
        } else {
            window.setContentSize(NSSize(width: Theme.Size.windowWidth, height: Theme.Size.windowHeight))
            window.center()
        }

        let toolbar = NSToolbar(identifier: "PaperLibraryToolbar")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar

        searchField.placeholderString = "Search"
        searchField.delegate = self
        searchField.sendsSearchStringImmediately = true
        searchField.target = self
        searchField.action = #selector(searchChanged(_:))

        addPopover.behavior = .transient
        addPopover.contentViewController = NSHostingController(
            rootView: AddPaperView { [weak self] identifier in
                self?.addPopover.performClose(nil)
                self?.store.add(identifier: identifier)
            }
        )

        store.$scope.combineLatest(store.$items)
            .sink { [weak self] scope, items in self?.updateTitle(scope: scope, count: items.count) }
            .store(in: &subscriptions)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    private var hasShown = false

    override func showWindow(_ sender: Any?) {
        super.showWindow(sender)
        // Divider positions only stick once the split view has its final size.
        guard !hasShown, let window else { return }
        hasShown = true
        window.layoutIfNeeded()
        let settings = store.settings
        let splitView = splitViewController.splitView
        splitView.setPosition(settings.sidebarWidth ?? Theme.Size.sidebarWidth, ofDividerAt: 0)
        splitView.setPosition(splitView.bounds.width - (settings.detailWidth ?? Theme.Size.inspectorWidth), ofDividerAt: 1)
    }

    private func updateTitle(scope: Scope, count: Int) {
        guard let window else { return }
        window.title = scopeTitle(scope)
        window.subtitle = count == 1 ? "1 paper" : "\(count) papers"
    }

    private func scopeTitle(_ scope: Scope) -> String {
        switch scope {
        case .all: "All Papers"
        case .unread: "Unread"
        case .recentlyAdded: "Recently Added"
        case .noPdf: "No PDF"
        case .collection(let id): store.collection(id: id)?.name ?? "Collection"
        case .tag(let id): store.tag(id: id)?.name ?? "Tag"
        }
    }

    // MARK: Commands

    @objc func addPaper(_ sender: Any?) {
        // An image toolbar item has no `view` to anchor to; the toolbar item
        // overload positions the popover under the button, or at the overflow
        // chevron when the item is clipped.
        guard let item = window?.toolbar?.items.first(where: { $0.itemIdentifier == ToolbarID.add }) else { return }
        if addPopover.isShown {
            addPopover.performClose(nil)
        } else {
            addPopover.show(relativeTo: item)
        }
    }

    @objc func focusSearch(_ sender: Any?) {
        window?.makeFirstResponder(searchField)
    }

    @objc func toggleSidebar(_ sender: Any?) {
        splitViewController.toggleSidebar(sender)
    }

    @objc func toggleInspector(_ sender: Any?) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Theme.Duration.panel
            inspectorItem.animator().isCollapsed.toggle()
        }
    }

    @objc func sortBy(_ sender: NSMenuItem) {
        guard let column = sender.representedObject as? SortColumn else { return }
        if store.sort == column {
            store.ascending.toggle()
        } else {
            store.sort = column
            store.ascending = column == .title || column == .firstAuthor || column == .journal
        }
    }

    @objc func newCollection(_ sender: Any?) {
        sidebar.newCollection(parentId: nil)
    }

    @objc func newSubcollection(_ sender: Any?) {
        sidebar.newCollection(parentId: sidebar.selectedCollectionId)
    }

    @objc func revealLibraryInFinder(_ sender: Any?) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: store.libraryPath)])
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        switch menuItem.action {
        case #selector(sortBy(_:)):
            let column = menuItem.representedObject as? SortColumn
            menuItem.state = column == store.sort ? .on : .off
            return true
        case #selector(toggleSidebar(_:)):
            menuItem.title = splitViewController.splitViewItems[0].isCollapsed ? "Show Sidebar" : "Hide Sidebar"
            return true
        case #selector(toggleInspector(_:)):
            menuItem.title = inspectorItem.isCollapsed ? "Show Inspector" : "Hide Inspector"
            return true
        case #selector(newSubcollection(_:)):
            return sidebar.selectedCollectionId != nil
        default:
            return true
        }
    }

    // MARK: Search

    @objc private func searchChanged(_ sender: NSSearchField) {
        store.searchText = sender.stringValue
    }

    /// Enter in the search field with an identifier adds the paper instead of searching.
    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        guard commandSelector == #selector(NSResponder.insertNewline(_:)),
              let identifier = parseIdentifier(text: searchField.stringValue)
        else { return false }
        searchField.stringValue = ""
        store.searchText = ""
        store.add(identifier: identifier)
        return true
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .sidebarTrackingSeparator, .flexibleSpace, ToolbarID.sort, ToolbarID.add,
         .inspectorTrackingSeparator, ToolbarID.search, ToolbarID.inspector]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier identifier: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        switch identifier {
        case ToolbarID.add:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Add Paper"
            item.toolTip = "Add a paper by DOI or arXiv ID (⌘N)"
            item.image = NSImage(systemSymbolName: Theme.Symbol.addPaper, accessibilityDescription: "Add Paper")
            item.target = self
            item.action = #selector(addPaper(_:))
            item.isBordered = true
            return item
        case ToolbarID.sort:
            let item = NSMenuToolbarItem(itemIdentifier: identifier)
            item.label = "Sort"
            item.toolTip = "Sort papers"
            item.image = NSImage(systemSymbolName: Theme.Symbol.sort, accessibilityDescription: "Sort")
            item.menu = makeSortMenu()
            item.showsIndicator = true
            item.isBordered = true
            return item
        case ToolbarID.search:
            let item = NSSearchToolbarItem(itemIdentifier: identifier)
            item.searchField = searchField
            item.label = "Search"
            item.preferredWidthForSearchField = Theme.Size.searchFieldWidth
            return item
        case ToolbarID.inspector:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Inspector"
            item.toolTip = "Show or hide the inspector (⌥⌘I)"
            item.image = NSImage(systemSymbolName: Theme.Symbol.inspector, accessibilityDescription: "Inspector")
            item.target = self
            item.action = #selector(toggleInspector(_:))
            item.isBordered = true
            return item
        default:
            return nil
        }
    }

    func makeSortMenu() -> NSMenu {
        let menu = NSMenu()
        for column in SortColumn.allCases {
            let item = NSMenuItem(title: column.label, action: #selector(sortBy(_:)), keyEquivalent: "")
            item.representedObject = column
            item.target = self
            menu.addItem(item)
        }
        return menu
    }

    // MARK: Window

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? {
        store.undoManager
    }

    func windowWillClose(_ notification: Notification) {
        var settings = store.settings
        settings.sidebarWidth = splitViewController.splitViewItems[0].isCollapsed ? settings.sidebarWidth : sidebar.view.frame.width
        settings.detailWidth = inspectorItem.isCollapsed ? settings.detailWidth : inspector.view.frame.width
        if let frame = window?.frame {
            settings.window = WindowState(x: frame.minX, y: frame.minY, width: frame.width, height: frame.height)
        }
        store.updateSettings(settings)
        store.close()
        (NSApp.delegate as? AppDelegate)?.windowClosed(self)
    }
}
