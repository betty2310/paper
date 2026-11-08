import AppKit
import Combine
import PaperEngine
import SwiftUI

/// One window per open library: a unified toolbar over a three column split
/// view (sidebar, center, inspector). The toolbar, the sidebar, and the
/// inspector share one translucent chrome, and the center sits on it too,
/// showing the paper list or one of the open PDFs, chosen by the tabs in the
/// toolbar. While a PDF is in front, the sidebar shows its table of contents
/// instead of the library's lists, and the detail panel shows its annotations
/// and the paper's note instead of the inspector. Commands are
/// selectors on the responder chain so menu items, toolbar buttons, and
/// SwiftUI buttons share one code path.
@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSMenuItemValidation {
    let store: LibraryStore
    let splitViewController = NSSplitViewController()
    let sidebar: SidebarViewController
    /// The library's lists, or the table of contents of the PDF in front.
    private let sidebarContent: SidebarContentViewController
    let list: PaperListViewController
    let inspector: InspectorViewController
    /// The detail panel: the inspector, or the panel of the PDF in front.
    private let detailContent: TabContentViewController
    private let center: TabContentViewController
    private let tabStrip = TabStripView()
    private let titleView = TitleView()
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
        static let title = NSToolbarItem.Identifier("title")

        /// What the toolbar shows between the title and the inspector button
        /// while the library is in front. The reader's controls float over
        /// its pages instead.
        static let library = [search, sort, add]
    }

    init(store: LibraryStore) {
        self.store = store
        sidebar = SidebarViewController(store: store)
        sidebarContent = SidebarContentViewController(showing: sidebar)
        list = PaperListViewController(store: store)
        inspector = InspectorViewController(store: store)
        detailContent = TabContentViewController(showing: inspector)
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
        // The toolbar draws the title itself (`TitleView`), so the tabs can
        // sit right after it and take the rest of the row. Its background is
        // the window's chrome, the same as the sidebar's.
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.tabbingMode = .disallowed
        window.isReleasedWhenClosed = false

        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebarContent)
        sidebarItem.minimumThickness = Theme.Size.sidebarMinWidth
        sidebarItem.maximumThickness = Theme.Size.sidebarMaxWidth
        sidebarItem.canCollapse = true
        sidebarItem.titlebarSeparatorStyle = .none

        let listItem = NSSplitViewItem(viewController: center)
        listItem.minimumThickness = Theme.Size.listMinWidth

        inspectorItem = NSSplitViewItem(inspectorWithViewController: detailContent)
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
            rootView: AddPaperView(add: { [weak self] identifier in
                self?.addPopover.performClose(nil)
                self?.store.add(identifier: identifier)
            }, newPaper: { [weak self] in
                self?.addPopover.performClose(nil)
                self?.newPaper(nil)
            }, importPdfs: { [weak self] in
                self?.addPopover.performClose(nil)
                self?.importPdfs(nil)
            })
        )

        store.$scope.combineLatest(store.$items)
            .sink { [weak self] scope, items in self?.updateTitle(scope: scope, count: items.count) }
            .store(in: &subscriptions)

        tabStrip.onSelect = { [weak self] id in self?.selectTab(id) }
        tabStrip.onClose = { [weak self] id in self?.closeReader(itemId: id) }
        tabStrip.onTogglePin = { [weak self] id in self?.togglePin(itemId: id) }
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
        // Hidden panels stay hidden, after their widths are in place so
        // showing them again brings back the width the user chose.
        splitViewController.splitViewItems[0].isCollapsed = settings.sidebarCollapsed
        inspectorItem.isCollapsed = settings.detailCollapsed
        // However a panel is hidden or shown (the toolbar buttons, the menu,
        // or dragging its divider shut), the choice is written at once.
        panelObservers = [splitViewController.splitViewItems[0], inspectorItem].map { item in
            item.observe(\.isCollapsed) { [weak self] _, _ in
                DispatchQueue.main.async { self?.savePanelsShown() }
            }
        }
        restorePinnedTabs()
    }

    // MARK: Pinned tabs

    /// Item ids of the pinned tabs, in tab order. Pinned readers come first
    /// in `readers` in this order.
    private var pinnedIds: [String] = []

    /// Reopens the tabs pinned at the last launch, behind the library, in
    /// their order. A paper deleted since, or whose PDF is gone, is dropped.
    private func restorePinnedTabs() {
        let saved = store.pinnedTabs
        for itemId in saved where !pinnedIds.contains(itemId) {
            guard let reader = makeReader(itemId: itemId) else { continue }
            pinnedIds.append(itemId)
            readers.insert(reader, at: pinnedIds.count - 1)
            Task { await reader.open() }
        }
        if pinnedIds != saved { store.setPinnedTabs(pinnedIds) }
        updateTabs()
    }

    @objc func togglePinTab(_ sender: Any?) {
        guard let reader = activeReader else { return }
        togglePin(itemId: reader.itemId)
    }

    /// Pins a tab at the end of the pinned tabs, or unpins it to just after
    /// them, and saves the pins at once.
    private func togglePin(itemId: String) {
        guard let index = readers.firstIndex(where: { $0.itemId == itemId }) else { return }
        let reader = readers.remove(at: index)
        if let pinned = pinnedIds.firstIndex(of: itemId) {
            pinnedIds.remove(at: pinned)
        } else {
            pinnedIds.append(itemId)
        }
        readers.insert(reader, at: pinnedIds.count - (pinnedIds.contains(itemId) ? 1 : 0))
        store.setPinnedTabs(pinnedIds)
        updateTabs()
    }

    // MARK: Favorites

    /// The paper in front, or the selected papers in the library (⌘D).
    private var favoriteTargets: [String] {
        activeReader.map { [$0.itemId] } ?? store.selectedItemIds
    }

    @objc func toggleFavorite(_ sender: Any?) {
        store.toggleFavorite(itemIds: favoriteTargets)
    }

    /// Jev picks topics again for the paper in front or the selected papers.
    @objc func tagWithTopics(_ sender: Any?) {
        store.tagWithTopics(itemIds: favoriteTargets)
    }

    private var panelObservers: [NSKeyValueObservation] = []

    /// Only the hidden flags: mid animation a panel's width is not the one
    /// the user chose. Widths are written with the rest in `saveLayout`.
    private func savePanelsShown() {
        var settings = store.settings
        settings.sidebarCollapsed = splitViewController.splitViewItems[0].isCollapsed
        settings.detailCollapsed = inspectorItem.isCollapsed
        guard settings != store.settings else { return }
        store.updateSettings(settings)
    }

    private func updateTitle(scope: Scope, count: Int) {
        guard let window else { return }
        window.title = scopeTitle(scope)
        window.subtitle = count == 1 ? "1 paper" : "\(count) papers"
        titleView.set(title: window.title, subtitle: window.subtitle)
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

    /// A paper typed in by hand, such as a manuscript with no DOI yet.
    @objc func newPaper(_ sender: Any?) {
        guard let window, window.attachedSheet == nil else { return }
        addPopover.performClose(nil)
        showLibrary()
        let sheet = NSWindow(contentViewController: NSHostingController(rootView: EmptyView()))
        let editor = MetadataEditor(mode: .create { [weak self] fields, pdf in
            self?.store.createPaper(fields: fields, pdf: pdf) ?? false
        }, close: { [weak window, weak sheet] in
            if let sheet { window?.endSheet(sheet) }
        })
        sheet.contentViewController = NSHostingController(rootView: editor)
        window.beginSheet(sheet)
    }

    /// PDFs from an open panel, identified like dropped ones.
    @objc func importPdfs(_ sender: Any?) {
        showLibrary()
        store.choosePdfsToImport()
    }

    /// ⌘F finds text in the PDF in front, and searches the library otherwise.
    @objc func find(_ sender: Any?) {
        if let activeReader {
            activeReader.showFind()
        } else {
            showSearch(sender)
        }
    }

    @objc func findNext(_ sender: Any?) { activeReader?.findNext() }
    @objc func findPrevious(_ sender: Any?) { activeReader?.findPrevious() }

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
        case #selector(togglePages(_:)):
            menuItem.title = activeReader?.sidebar.showsPages == true ? "Show Contents" : "Show Pages"
            return activeReader != nil
        case #selector(zoomIn(_:)):
            return activeReader?.canZoomIn ?? false
        case #selector(zoomOut(_:)):
            return activeReader?.canZoomOut ?? false
        case #selector(zoomToFitWidth(_:)), #selector(addNote(_:)):
            return activeReader != nil
        case #selector(findNext(_:)), #selector(findPrevious(_:)):
            return activeReader?.hasFindMatches ?? false
        case #selector(navigateBack(_:)):
            return activeReader?.canGoBack ?? false
        case #selector(navigateForward(_:)):
            return activeReader?.canGoForward ?? false
        case #selector(highlightSelection(_:)):
            return activeReader?.canHighlight ?? false
        case #selector(toggleFavorite(_:)):
            let ids = favoriteTargets
            let all = !ids.isEmpty && ids.allSatisfy(store.isFavorite(itemId:))
            menuItem.title = all ? "Remove from Favorites" : "Add to Favorites"
            return !ids.isEmpty
        case #selector(tagWithTopics(_:)):
            return !favoriteTargets.isEmpty
        case #selector(togglePinTab(_:)):
            menuItem.title = activeReader.map { pinnedIds.contains($0.itemId) } == true ? "Unpin Tab" : "Pin Tab"
            return activeReader != nil
        case #selector(showNextTab(_:)), #selector(showPreviousTab(_:)):
            return !readers.isEmpty
        case #selector(findPdf(_:)):
            guard let id = store.selectedItemIds.first, store.selectedItemIds.count == 1 else { return false }
            return store.pdfStatus[id] != .searching && store.pdfAttachment(itemId: id) == nil
        case #selector(attachPdf(_:)):
            return store.selectedItemIds.count == 1
        case #selector(copyBibtex(_:)), #selector(copyCitation(_:)), #selector(exportBibtex(_:)):
            return !citedItemIds.isEmpty
        case #selector(copyCitationKey(_:)):
            menuItem.title = citedItemIds.count > 1 ? "Copy Citation Keys" : "Copy Citation Key"
            return !citedItemIds.isEmpty
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
        guard store.pdfAttachment(itemId: itemId) != nil else {
            openWhenReady(itemId: itemId)
            return
        }
        guard let reader = makeReader(itemId: itemId, reportingErrors: true) else { return }
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

    /// A reader for the paper's PDF, not yet opened, or nil when it has none.
    private func makeReader(itemId: String, reportingErrors: Bool = false) -> ReaderViewController? {
        guard let attachment = store.pdfAttachment(itemId: itemId) else { return nil }
        let file: AttachmentFile
        do {
            file = try store.attachmentFile(id: attachment.id)
        } catch {
            if reportingErrors { store.report(error) }
            return nil
        }
        let title = (try? store.engine.itemDetail(id: itemId))?.fields.title ?? URL(fileURLWithPath: file.path).deletingPathExtension().lastPathComponent
        return ReaderViewController(store: store, itemId: itemId, file: file, paperTitle: title)
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
        // So the inspector shows what was typed in the reader's note.
        store.reloadNote()
        center.show(list, animated: true)
        detailContent.show(inspector, animated: true)
        sidebarContent.show(sidebar, from: .reader)
        updateTabs()
        showLibraryToolbarItems(true)
        window?.makeFirstResponder(list.tableView)
    }

    private func activate(_ reader: ReaderViewController, landing: Bool = false) {
        guard activeReader !== reader else { return }
        activeReader?.savePosition()
        sidebarContent.show(reader.sidebar, from: activeReader == nil ? .library : .reader)
        activeReader = reader
        center.show(reader, animated: true)
        reader.panel.willShow()
        detailContent.show(reader.panel, animated: true)
        updateTabs(landing: landing ? reader.itemId : nil)
        showLibraryToolbarItems(false)
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
        if let pinned = pinnedIds.firstIndex(of: itemId) {
            pinnedIds.remove(at: pinned)
            store.setPinnedTabs(pinnedIds)
        }
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
        detailContent.remove(reader.panel)
        sidebarContent.remove(reader.sidebar)
    }

    /// Like `showLibrary`, for when the reader in front has just closed.
    private func showLibraryAfterClosing() {
        store.reloadNote()
        center.show(list, animated: true)
        detailContent.show(inspector, animated: true)
        sidebarContent.show(sidebar, from: .reader)
        updateTabs()
        showLibraryToolbarItems(true)
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
            + readers.map { TabStripView.Tab(id: $0.itemId, title: $0.paperTitle, isPinned: pinnedIds.contains($0.itemId)) }
        guard let toolbar = window?.toolbar else { return }
        let index = toolbar.items.firstIndex { $0.itemIdentifier == ToolbarID.tabs }
        if !tabs.isEmpty, index == nil,
           let title = toolbar.items.firstIndex(where: { $0.itemIdentifier == ToolbarID.title }) {
            // The strip takes the flexible space's place and all of its room.
            if toolbar.items.indices.contains(title + 1), toolbar.items[title + 1].itemIdentifier == .flexibleSpace {
                toolbar.removeItem(at: title + 1)
            }
            toolbar.insertItem(withItemIdentifier: ToolbarID.tabs, at: title + 1)
            // The strip needs its width before the first tabs grow into it.
            window?.layoutIfNeeded()
        }
        tabStrip.update(tabs: tabs, selectedId: activeReader?.itemId ?? TabStripView.Tab.libraryId, landing: landing)
        if tabs.isEmpty {
            // After the last tabs have shrunk away.
            DispatchQueue.main.asyncAfter(deadline: .now() + Theme.Duration.tabResize) { [weak self] in
                guard let self, readers.isEmpty, let toolbar = window?.toolbar,
                      let index = toolbar.items.firstIndex(where: { $0.itemIdentifier == ToolbarID.tabs })
                else { return }
                toolbar.removeItem(at: index)
                toolbar.insertItem(withItemIdentifier: .flexibleSpace, at: index)
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

    /// Back to where the reader was before following a link (⌘[).
    @objc func togglePages(_ sender: Any?) { activeReader?.sidebar.togglePages() }
    @objc func navigateBack(_ sender: Any?) { activeReader?.goBack() }
    @objc func navigateForward(_ sender: Any?) { activeReader?.goForward() }

    @objc func highlightSelection(_ sender: Any?) { activeReader?.highlightSelection() }
    @objc func addNote(_ sender: Any?) { activeReader?.startPlacingNote() }

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

    // MARK: Citations

    /// The papers a citation command cites: the one in the front tab, or the
    /// list's selection while the library is in front.
    private var citedItemIds: [String] {
        if let activeReader { return [activeReader.itemId] }
        return store.selectedItemIds
    }

    @objc func copyBibtex(_ sender: Any?) { store.copyBibtex(itemIds: citedItemIds) }
    @objc func copyCitation(_ sender: Any?) { store.copyCitation(itemIds: citedItemIds) }
    @objc func copyCitationKey(_ sender: Any?) { store.copyCitationKeys(itemIds: citedItemIds) }

    /// One paper's file is named after its key, several after the list.
    @objc func exportBibtex(_ sender: Any?) {
        let ids = citedItemIds
        let name = ids.count == 1
            ? (try? store.engine.itemDetail(id: ids[0]))?.citationKey ?? "Paper"
            : scopeTitle(store.scope)
        store.exportBibtex(itemIds: ids, name: name)
    }

    /// The library's toolbar buttons show only while the library is in front.
    private func showLibraryToolbarItems(_ show: Bool) {
        guard let toolbar = window?.toolbar else { return }
        let shown = toolbar.items.contains { ToolbarID.library.contains($0.itemIdentifier) }
        if show, !shown, let start = toolbar.items.firstIndex(where: { $0.itemIdentifier == .inspectorTrackingSeparator }) {
            for (offset, identifier) in ToolbarID.library.enumerated() {
                toolbar.insertItem(withItemIdentifier: identifier, at: start + offset)
            }
        } else if !show {
            for index in toolbar.items.indices.reversed() where ToolbarID.library.contains(toolbar.items[index].itemIdentifier) {
                toolbar.removeItem(at: index)
            }
        }
    }

    // MARK: Toolbar

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .sidebarTrackingSeparator, ToolbarID.title, .flexibleSpace] + ToolbarID.library
            + [.inspectorTrackingSeparator, .flexibleSpace, ToolbarID.inspector]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar) + [ToolbarID.tabs]
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
        case ToolbarID.title:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Title"
            item.view = titleView
            item.isBordered = false
            item.visibilityPriority = .high
            return item
        case ToolbarID.tabs:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Tabs"
            item.view = tabStrip
            item.isBordered = false
            item.visibilityPriority = .high
            return item
        case ToolbarID.inspector:
            let item = NSToolbarItem(itemIdentifier: identifier)
            item.label = "Inspector"
            item.toolTip = "Show or hide the inspector (⇧⌘B)"
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

    /// Remembers the window's frame, the panel widths, and which panels are
    /// hidden, so the next launch opens the same way. Called when the window
    /// closes and when the app quits, which closes no windows.
    func saveLayout() {
        guard hasShown else { return }
        var settings = store.settings
        let sidebarItem = splitViewController.splitViewItems[0]
        settings.sidebarWidth = sidebarItem.isCollapsed ? settings.sidebarWidth : sidebarContent.view.frame.width
        settings.detailWidth = inspectorItem.isCollapsed ? settings.detailWidth : detailContent.view.frame.width
        settings.sidebarCollapsed = sidebarItem.isCollapsed
        settings.detailCollapsed = inspectorItem.isCollapsed
        if let frame = window?.frame {
            settings.window = WindowState(x: frame.minX, y: frame.minY, width: frame.width, height: frame.height)
        }
        guard settings != store.settings else { return }
        store.updateSettings(settings)
    }

    func windowWillClose(_ notification: Notification) {
        readers.forEach { $0.close() }
        saveLayout()
        store.close()
        (NSApp.delegate as? AppDelegate)?.windowClosed(self)
    }
}

/// The center of the window: the paper list or one reader, cross faded when
/// the tab changes. It sits on the window's chrome with no card of its own,
/// from under the toolbar to the window's bottom edge and the panels beside it. Readers stay alive
/// while their tab is open, so switching back keeps the page and zoom. The
/// detail panel uses one too, for the inspector or a reader's panel. Both
/// start below the toolbar: a SwiftUI panel that reached under it and kept
/// clear with the safe area took clicks and hover one toolbar height above
/// where it drew.
@MainActor
final class TabContentViewController: NSViewController {
    private(set) var current: NSViewController
    /// Holds the column in front; its layer runs the tab switch fade.
    private let content = NSView()

    init(showing initial: NSViewController) {
        current = initial
        super.init(nibName: nil, bundle: nil)
        addChild(initial)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let root = NSView()
        content.wantsLayer = true
        content.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(content)
        NSLayoutConstraint.activate([
            content.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
            content.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
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
            content.layer?.add(fade, forKey: "tabSwitch")
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
        content.addSubview(subview)
        NSLayoutConstraint.activate([
            subview.topAnchor.constraint(equalTo: content.topAnchor),
            subview.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            subview.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            subview.bottomAnchor.constraint(equalTo: content.bottomAnchor),
        ])
    }
}

/// The sidebar column: the library's lists while the library is in front,
/// and the table of contents of the PDF in front otherwise. Going from the
/// library to a PDF, the lists slide out to the leading side as the contents
/// slide in from the trailing side, and back again the other way; between
/// two PDFs the contents cross fade.
@MainActor
final class SidebarContentViewController: NSViewController {
    /// What the sidebar showed before.
    enum Origin {
        case library
        case reader
    }

    private(set) var current: NSViewController
    /// Each shown column's leading constraint, which the slide moves.
    private var leading: [ObjectIdentifier: NSLayoutConstraint] = [:]

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

    func show(_ controller: NSViewController, from origin: Origin) {
        guard controller !== current else { return }
        if controller.parent == nil { addChild(controller) }
        let old = current.view
        let new = controller.view
        current = controller
        pin(new)
        guard view.window != nil, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else {
            unpin(old)
            return
        }
        // Toward the library is backward, so it slides the other way.
        let offset: CGFloat = switch origin {
        case .library: Theme.Size.sidebarSlide
        case .reader: controller is SidebarViewController ? -Theme.Size.sidebarSlide : 0
        }
        new.alphaValue = 0
        leading[ObjectIdentifier(new)]?.constant = offset
        view.layoutSubtreeIfNeeded()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Theme.Duration.tabSwitch
            context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            context.allowsImplicitAnimation = true
            new.animator().alphaValue = 1
            old.animator().alphaValue = 0
            leading[ObjectIdentifier(new)]?.animator().constant = 0
            leading[ObjectIdentifier(old)]?.animator().constant = -offset
            view.layoutSubtreeIfNeeded()
        } completionHandler: { [weak self] in
            MainActor.assumeIsolated {
                // Unless it came back while sliding out.
                guard let self, old !== self.current.view else { return }
                self.unpin(old)
            }
        }
    }

    /// Forgets a closed reader's contents.
    func remove(_ controller: NSViewController) {
        guard controller !== current else { return }
        unpin(controller.view)
        controller.removeFromParent()
    }

    /// Puts a column in place, or back in place if it was sliding out.
    private func pin(_ subview: NSView) {
        subview.alphaValue = 1
        if let constraint = leading[ObjectIdentifier(subview)] {
            constraint.constant = 0
            return
        }
        subview.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(subview)
        let start = subview.leadingAnchor.constraint(equalTo: view.leadingAnchor)
        leading[ObjectIdentifier(subview)] = start
        NSLayoutConstraint.activate([
            start,
            subview.topAnchor.constraint(equalTo: view.topAnchor),
            subview.widthAnchor.constraint(equalTo: view.widthAnchor),
            subview.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
    }

    private func unpin(_ subview: NSView) {
        leading[ObjectIdentifier(subview)] = nil
        subview.removeFromSuperview()
        subview.alphaValue = 1
    }
}

/// The window's split view, over one translucent chrome that shows wherever
/// the columns leave room. Its dividers are invisible: the columns sit on
/// the one chrome. The controller inserts column views as they load,
/// above anything added before; the chrome moves back to the bottom and the
/// toasts to the top.
final class ToastSplitView: NSSplitView {
    weak var toasts: NSView?
    private let chrome = ChromeView()

    override init(frame: NSRect) {
        super.init(frame: frame)
        chrome.material = .sidebar
        chrome.blendingMode = .behindWindow
        chrome.state = .followsWindowActiveState
        chrome.autoresizingMask = [.width, .height]
        addSubview(chrome, positioned: .below, relativeTo: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override var dividerColor: NSColor { .clear }

    override func layout() {
        super.layout()
        chrome.frame = bounds
        guard subviews.first !== chrome || (toasts != nil && subviews.last !== toasts) else { return }
        // Reorders without removing, so the toasts keep their constraints.
        sortSubviews({ first, second, _ in
            func rank(_ view: NSView) -> Int { view is ToastHostingView ? 2 : view is ChromeView ? 0 : 1 }
            let (a, b) = (rank(first), rank(second))
            return a == b ? .orderedSame : a < b ? .orderedAscending : .orderedDescending
        }, context: nil)
    }

    private final class ChromeView: NSVisualEffectView {}
}

/// The window's title and subtitle, drawn as the toolbar's first item the
/// way the system draws them, because the system title would keep the tab
/// strip from starting right after it.
@MainActor
final class TitleView: NSView {
    private let title = NSTextField(labelWithString: "")
    private let subtitle = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        title.font = Theme.Font.windowTitle
        title.textColor = Theme.Color.text
        subtitle.font = Theme.Font.windowSubtitle
        subtitle.textColor = Theme.Color.secondaryText
        for label in [title, subtitle] {
            label.lineBreakMode = .byTruncatingTail
            label.translatesAutoresizingMaskIntoConstraints = false
            addSubview(label)
            NSLayoutConstraint.activate([
                label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Size.windowTitleInset),
                label.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -Theme.Size.gap),
            ])
        }
        // As narrow as the longer line and a gap, so the tabs start right after it.
        let hug = widthAnchor.constraint(equalToConstant: 0)
        hug.priority = .defaultLow
        NSLayoutConstraint.activate([
            hug,
            heightAnchor.constraint(equalToConstant: Theme.Size.windowTitleHeight),
            title.firstBaselineAnchor.constraint(equalTo: topAnchor, constant: Theme.Size.windowTitleBaseline),
            subtitle.firstBaselineAnchor.constraint(equalTo: title.firstBaselineAnchor, constant: Theme.Size.windowSubtitleBaseline),
        ])
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func set(title text: String, subtitle detail: String) {
        title.stringValue = text
        subtitle.stringValue = detail
        setAccessibilityLabel("\(text), \(detail)")
    }
}
