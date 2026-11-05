import AppKit
import Combine
import PaperEngine
import SwiftUI

/// One window per open library: a unified toolbar over a three column split
/// view (sidebar, center, inspector). The center shows the paper list or one
/// of the open PDFs, chosen by the tabs in the toolbar. Commands are
/// selectors on the responder chain so menu items, toolbar buttons, and
/// SwiftUI buttons share one code path.
@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSMenuItemValidation {
    let store: LibraryStore
    let splitViewController = NSSplitViewController()
    let sidebar: SidebarViewController
    let list: PaperListViewController
    let inspector: InspectorViewController
    private let center: TabContentViewController
    private let tabStrip = TabStripView()
    /// Open PDFs in tab order.
    private var readers: [ReaderViewController] = []
    /// The reader in front, or nil for the library.
    private var activeReader: ReaderViewController?
    /// A paper the user asked to open while its PDF was still downloading.
    private var pendingOpen: String?
    private var inspectorItem: NSSplitViewItem!
    private let searchPalette: SearchPaletteController
    private let addPopover = NSPopover()
    private var subscriptions: Set<AnyCancellable> = []

    private enum ToolbarID {
        static let add = NSToolbarItem.Identifier("add")
        static let sort = NSToolbarItem.Identifier("sort")
        static let search = NSToolbarItem.Identifier("search")
        static let inspector = NSToolbarItem.Identifier("inspector")
        static let tabs = NSToolbarItem.Identifier("tabs")
        static let zoom = NSToolbarItem.Identifier("zoom")

        /// What the toolbar shows between the tabs and the inspector button.
        static let library = [search, sort, add]
        static let reader = [zoom]
    }

    init(store: LibraryStore) {
        self.store = store
        sidebar = SidebarViewController(store: store)
        list = PaperListViewController(store: store)
        inspector = InspectorViewController(store: store)
        center = TabContentViewController(showing: list)
        searchPalette = SearchPaletteController(store: store)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: Theme.Size.windowWidth, height: Theme.Size.windowHeight),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init(window: window)
        // Set before the controller loads its view.
        let splitView = ToastSplitView()
        splitView.isVertical = true
        splitView.dividerStyle = .thin
        splitViewController.splitView = splitView

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

        let listItem = NSSplitViewItem(viewController: center)
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
        addToasts(over: splitView)

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

        tabStrip.onSelect = { [weak self] id in self?.selectTab(id) }
        tabStrip.onClose = { [weak self] id in self?.closeReader(itemId: id) }
        list.onOpen = { [weak self] itemId in self?.openPaper(itemId: itemId) }
        store.openPaper = { [weak self] itemId in self?.openPaper(itemId: itemId) }
        // Picking a list in the sidebar is about the library, so it comes forward.
        store.$scope.dropFirst()
            .sink { [weak self] _ in self?.showLibrary() }
            .store(in: &subscriptions)
        store.pdfFound
            .sink { [weak self] itemId in self?.pdfFound(itemId: itemId) }
            .store(in: &subscriptions)
        store.$pdfStatus
            .sink { [weak self] statuses in self?.pdfSearchEnded(statuses) }
            .store(in: &subscriptions)
        store.$items
            .sink { [weak self] _ in self?.closeReadersOfDeletedItems() }
            .store(in: &subscriptions)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// The toasts float over all three columns, below the toolbar. The split
    /// view lays out only its arranged subviews, so it leaves this one alone.
    private func addToasts(over splitView: ToastSplitView) {
        splitView.arrangesAllSubviews = false
        let toasts = ToastHostingView(store: store)
        splitView.toasts = toasts
        toasts.translatesAutoresizingMaskIntoConstraints = false
        splitView.addSubview(toasts)
        NSLayoutConstraint.activate([
            toasts.topAnchor.constraint(equalTo: splitView.safeAreaLayoutGuide.topAnchor),
            toasts.leadingAnchor.constraint(equalTo: splitView.leadingAnchor),
            toasts.trailingAnchor.constraint(equalTo: splitView.trailingAnchor),
            toasts.bottomAnchor.constraint(equalTo: splitView.bottomAnchor),
        ])
    }

    private var hasShown = false

    override func showWindow(_ sender: Any?) {
        super.showWindow(sender)
        // Divider positions only stick once the split view has its final size.
        guard !hasShown, let window else { return }
        hasShown = true
        window.layoutIfNeeded()
        let settings = store.settings
        let splitView = splitViewController.splitView
        // The inspector first: moving its divider afterwards also took a few
        // points from the sidebar. A divider's position is its leading edge, so
        // the inspector's width excludes the divider's thickness.
        splitView.setPosition(splitView.bounds.width - (settings.detailWidth ?? Theme.Size.inspectorWidth) - splitView.dividerThickness, ofDividerAt: 1)
        splitView.setPosition(settings.sidebarWidth ?? Theme.Size.sidebarWidth, ofDividerAt: 0)
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

    @objc func showSearch(_ sender: Any?) {
        guard let window else { return }
        showLibrary()
        addPopover.performClose(nil)
        searchPalette.toggle(over: window)
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
        case #selector(openSelectedPaper(_:)):
            return activeReader == nil && store.selectedItemIds.count == 1
        case #selector(zoomIn(_:)):
            return activeReader?.canZoomIn ?? false
        case #selector(zoomOut(_:)):
            return activeReader?.canZoomOut ?? false
        case #selector(zoomToFitWidth(_:)):
            return activeReader != nil
        case #selector(showNextTab(_:)), #selector(showPreviousTab(_:)):
            return !readers.isEmpty
        case #selector(findPdf(_:)):
            guard let id = store.selectedItemIds.first, store.selectedItemIds.count == 1 else { return false }
            return store.pdfStatus[id] != .searching && store.pdfAttachment(itemId: id) == nil
        case #selector(attachPdf(_:)):
            return store.selectedItemIds.count == 1
        case #selector(revealPdf(_:)):
            return store.selectedItemIds.first.flatMap(store.pdfAttachment(itemId:)) != nil
        default:
            return true
        }
    }

    // MARK: Tabs

    /// Opens the selected paper's PDF in a tab (Return, double click, ⌘↓).
    @objc func openSelectedPaper(_ sender: Any?) {
        guard store.selectedItemIds.count == 1, let itemId = store.selectedItemIds.first else { return }
        openPaper(itemId: itemId)
    }

    /// Brings the paper's tab forward, opening it if needed. A paper whose PDF
    /// is still downloading opens once it arrives.
    func openPaper(itemId: String) {
        if let reader = readers.first(where: { $0.itemId == itemId }) {
            activate(reader)
            return
        }
        guard let attachment = store.pdfAttachment(itemId: itemId) else {
            openWhenReady(itemId: itemId)
            return
        }
        let file: AttachmentFile
        do {
            file = try store.attachmentFile(id: attachment.id)
        } catch {
            store.report(error)
            return
        }
        let title = (try? store.engine.itemDetail(id: itemId))?.fields.title ?? URL(fileURLWithPath: file.path).deletingPathExtension().lastPathComponent
        let reader = ReaderViewController(store: store, itemId: itemId, file: file, paperTitle: title)
        // The row's picture is taken now, before anything moves.
        let flight = activeReader == nil ? list.flightSource(itemId: itemId) : nil
        readers.append(reader)
        Task { [weak self] in
            await reader.open()
            guard let self, readers.contains(where: { $0 === reader }) else { return }
            activate(reader, landing: flight != nil)
            guard let flight, let window else { return }
            // The toolbar lays out the new tab on the next pass; aim after it.
            DispatchQueue.main.async { [weak self] in
                guard let self, let tab = tabStrip.button(for: itemId) else { return }
                TabFlight.fly(flight, to: tab, over: window) { [weak self] in
                    self?.tabStrip.land(itemId)
                }
            }
        }
    }

    /// The paper's toast shows the search; the tab opens when the PDF arrives.
    private func openWhenReady(itemId: String) {
        pendingOpen = itemId
        if store.pdfStatus[itemId] != .searching {
            store.findPdf(itemId: itemId)
        }
    }

    private func pdfFound(itemId: String) {
        guard pendingOpen == itemId else { return }
        pendingOpen = nil
        // The tab opening says it already.
        store.dismissToast(id: Toast.itemId(itemId))
        openPaper(itemId: itemId)
    }

    /// A search that found nothing cancels the open; its toast says why.
    private func pdfSearchEnded(_ statuses: [String: PdfStatus]) {
        guard let itemId = pendingOpen, let status = statuses[itemId], status == .notFound || status == .failed else { return }
        pendingOpen = nil
    }

    private func selectTab(_ id: String) {
        if id == TabStripView.Tab.libraryId {
            showLibrary()
        } else if let reader = readers.first(where: { $0.itemId == id }) {
            activate(reader)
        }
    }

    /// Brings the library forward. Tabs stay open.
    func showLibrary() {
        guard let reader = activeReader else { return }
        reader.savePosition()
        activeReader = nil
        center.show(list, animated: true)
        updateTabs()
        setToolbarItems(ToolbarID.library, replacing: ToolbarID.reader)
        window?.makeFirstResponder(list.tableView)
    }

    private func activate(_ reader: ReaderViewController, landing: Bool = false) {
        guard activeReader !== reader else { return }
        activeReader?.savePosition()
        activeReader = reader
        center.show(reader, animated: true)
        updateTabs(landing: landing ? reader.itemId : nil)
        setToolbarItems(ToolbarID.reader, replacing: ToolbarID.library)
        // The inspector follows the paper in front, when the list shows it.
        if store.selectedItemIds != [reader.itemId], store.items.contains(where: { $0.id == reader.itemId }) {
            store.selectedItemIds = [reader.itemId]
        }
        reader.focus()
    }

    private func closeReader(itemId: String) {
        guard let index = readers.firstIndex(where: { $0.itemId == itemId }) else { return }
        let reader = readers.remove(at: index)
        reader.close()
        if activeReader === reader {
            activeReader = nil
            if readers.isEmpty {
                showLibraryAfterClosing()
            } else {
                activate(readers[max(0, index - 1)])
            }
        } else {
            updateTabs()
        }
        center.remove(reader)
    }

    /// Like `showLibrary`, for when the reader in front has just closed.
    private func showLibraryAfterClosing() {
        center.show(list, animated: true)
        updateTabs()
        setToolbarItems(ToolbarID.library, replacing: ToolbarID.reader)
        window?.makeFirstResponder(list.tableView)
    }

    private func closeReadersOfDeletedItems() {
        for reader in readers where (try? store.engine.itemDetail(id: reader.itemId)) == nil {
            closeReader(itemId: reader.itemId)
        }
    }

    /// The strip is in the toolbar only while a PDF is open, so an empty
    /// toolbar item never takes space or draws.
    private func updateTabs(landing: String? = nil) {
        let tabs = readers.isEmpty ? [] : [TabStripView.Tab(id: TabStripView.Tab.libraryId, title: "Library")]
            + readers.map { TabStripView.Tab(id: $0.itemId, title: $0.paperTitle) }
        guard let toolbar = window?.toolbar else { return }
        let index = toolbar.items.firstIndex { $0.itemIdentifier == ToolbarID.tabs }
        if !tabs.isEmpty, index == nil,
           let separator = toolbar.items.firstIndex(where: { $0.itemIdentifier == .sidebarTrackingSeparator }) {
            toolbar.insertItem(withItemIdentifier: ToolbarID.tabs, at: separator + 1)
        }
        tabStrip.update(tabs: tabs, selectedId: activeReader?.itemId ?? TabStripView.Tab.libraryId, landing: landing)
        if tabs.isEmpty {
            // After the last tabs have shrunk away.
            DispatchQueue.main.asyncAfter(deadline: .now() + Theme.Duration.tabResize) { [weak self] in
                guard let self, readers.isEmpty, let toolbar = window?.toolbar,
                      let index = toolbar.items.firstIndex(where: { $0.itemIdentifier == ToolbarID.tabs })
                else { return }
                toolbar.removeItem(at: index)
            }
        }
    }

    @objc func showNextTab(_ sender: Any?) {
        stepTab(by: 1)
    }

    @objc func showPreviousTab(_ sender: Any?) {
        stepTab(by: -1)
    }

    private func stepTab(by step: Int) {
        let ids = [TabStripView.Tab.libraryId] + readers.map(\.itemId)
        let current = ids.firstIndex(of: activeReader?.itemId ?? TabStripView.Tab.libraryId) ?? 0
        selectTab(ids[(current + step + ids.count) % ids.count])
    }

    // MARK: Reader commands

    @objc func zoomIn(_ sender: Any?) { activeReader?.zoomIn() }
    @objc func zoomOut(_ sender: Any?) { activeReader?.zoomOut() }
    @objc func zoomToFitWidth(_ sender: Any?) { activeReader?.fitWidth() }

    @objc private func zoomClicked(_ sender: NSToolbarItemGroup) {
        if sender.selectedIndex == 0 { zoomOut(sender) } else { zoomIn(sender) }
    }

    // MARK: PDF commands

    @objc func findPdf(_ sender: Any?) {
        guard let id = store.selectedItemIds.first else { return }
        store.findPdf(itemId: id)
    }

    @objc func attachPdf(_ sender: Any?) {
        guard let id = store.selectedItemIds.first else { return }
        store.chooseFileToAttach(itemId: id)
    }

    @objc func revealPdf(_ sender: Any?) {
        guard let id = store.selectedItemIds.first, let attachment = store.pdfAttachment(itemId: id) else { return }
        store.revealAttachment(id: attachment.id)
    }

    /// Swaps the library's toolbar buttons for the reader's or back.
    private func setToolbarItems(_ wanted: [NSToolbarItem.Identifier], replacing unwanted: [NSToolbarItem.Identifier]) {
        guard let toolbar = window?.toolbar,
              let start = toolbar.items.firstIndex(where: { unwanted.contains($0.itemIdentifier) })
        else { return }
        for index in toolbar.items.indices.reversed() where unwanted.contains(toolbar.items[index].itemIdentifier) {
            toolbar.removeItem(at: index)
        }
        for (offset, identifier) in wanted.enumerated() {
            toolbar.insertItem(withItemIdentifier: identifier, at: start + offset)
        }
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .sidebarTrackingSeparator, .flexibleSpace] + ToolbarID.library
            + [.inspectorTrackingSeparator, .flexibleSpace, ToolbarID.inspector]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar) + ToolbarID.reader + [ToolbarID.tabs]
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
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Search"
            item.toolTip = "Search the whole library (⌘P)"
            item.image = NSImage(systemSymbolName: Theme.Symbol.search, accessibilityDescription: "Search")
            item.target = self
            item.action = #selector(showSearch(_:))
            item.isBordered = true
            return item
        case ToolbarID.tabs:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Tabs"
            item.view = tabStrip
            item.isBordered = false
            item.visibilityPriority = .high
            return item
        case ToolbarID.zoom:
            let group = NSToolbarItemGroup(
                itemIdentifier: identifier,
                images: [
                    NSImage(systemSymbolName: Theme.Symbol.zoomOut, accessibilityDescription: "Zoom Out")!,
                    NSImage(systemSymbolName: Theme.Symbol.zoomIn, accessibilityDescription: "Zoom In")!,
                ],
                selectionMode: .momentary,
                labels: ["Zoom Out", "Zoom In"],
                target: self,
                action: #selector(zoomClicked(_:))
            )
            group.label = "Zoom"
            group.toolTip = "Zoom out (⌘−) or in (⌘+)"
            return group
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

    /// ⌘W closes the PDF in front before the window; the close button always
    /// closes the window.
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard let reader = activeReader, NSApp.currentEvent?.type == .keyDown else { return true }
        closeReader(itemId: reader.itemId)
        return false
    }

    func windowWillClose(_ notification: Notification) {
        readers.forEach { $0.close() }
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

/// The center of the window: the paper list or one reader, cross faded when
/// the tab changes. Readers stay alive while their tab is open, so switching
/// back keeps the page and zoom.
@MainActor
final class TabContentViewController: NSViewController {
    private(set) var current: NSViewController

    init(showing initial: NSViewController) {
        current = initial
        super.init(nibName: nil, bundle: nil)
        addChild(initial)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        view = root
        pin(current.view)
    }

    func show(_ controller: NSViewController, animated: Bool) {
        guard controller !== current else { return }
        if controller.parent == nil { addChild(controller) }
        if animated, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            let fade = CATransition()
            fade.type = .fade
            fade.duration = Theme.Duration.tabSwitch
            fade.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            view.layer?.add(fade, forKey: "tabSwitch")
        }
        current.view.removeFromSuperview()
        current = controller
        pin(controller.view)
    }

    /// Forgets a closed reader.
    func remove(_ controller: NSViewController) {
        guard controller !== current else { return }
        controller.view.removeFromSuperview()
        controller.removeFromParent()
    }

    private func pin(_ subview: NSView) {
        subview.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(subview)
        NSLayoutConstraint.activate([
            subview.topAnchor.constraint(equalTo: view.topAnchor),
            subview.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            subview.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            subview.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }
}

/// The window's split view. The controller inserts column views as they
/// load, above anything added before; the toasts move back to the top.
final class ToastSplitView: NSSplitView {
    weak var toasts: NSView?

    override func layout() {
        super.layout()
        guard let toasts, subviews.last !== toasts else { return }
        // Reorders without removing, so the toasts keep their constraints.
        sortSubviews({ first, second, _ in
            let firstOnTop = first is ToastHostingView, secondOnTop = second is ToastHostingView
            if firstOnTop == secondOnTop { return .orderedSame }
            return firstOnTop ? .orderedDescending : .orderedAscending
        }, context: nil)
    }
}
