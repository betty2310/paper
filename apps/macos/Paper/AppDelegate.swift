import AppKit
import PaperEngine
import Sparkle
import SwiftUI

/// Owns the main menu, the welcome window, the settings window, and one
/// `MainWindowController` per open library.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    private var libraryWindows: [MainWindowController] = []
    private var welcomeWindow: NSWindow?
    private var settingsWindow: NSWindow?
    private var recentMenu: NSMenu!
    /// Sparkle, which checks the appcast in `SUFeedURL` for new releases. Off
    /// without a feed: under `swift run`, and in builds without an update key.
    private var updater: SPUStandardUpdaterController?

    // MARK: Launch

    func applicationDidFinishLaunching(_ notification: Notification) {
        if Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") != nil {
            updater = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
        }
        NSApp.mainMenu = makeMainMenu()
        // Under `swift run` there is no Info.plist to name the icon, so the
        // Dock, ⌘Tab, and the About panel take it from here. Paper.app names
        // it with CFBundleIconFile.
        if Bundle.main.object(forInfoDictionaryKey: "CFBundleIconFile") == nil,
           let url = Bundle.module.url(forResource: "AppIcon", withExtension: "icns") {
            NSApp.applicationIconImage = NSImage(contentsOf: url)
        }
        NSApp.activate(ignoringOtherApps: true)

        // `swift run Paper <library>` or PAPER_LIBRARY=<library> opens that library
        // directly; otherwise the most recent library opens, and with none the
        // welcome window asks.
        let argument = CommandLine.arguments.dropFirst().first { !$0.hasPrefix("-") }
        if let path = argument ?? ProcessInfo.processInfo.environment["PAPER_LIBRARY"] {
            open(libraryAt: path, create: !isLibrary(path: path))
        } else if let recent = recentLibraries().first(where: isLibrary(path:)) {
            open(libraryAt: recent, create: false)
        } else {
            showWelcome()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { showWelcome() }
        return true
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls where isLibrary(path: url.path) {
            open(libraryAt: url.path, create: false)
        }
    }

    // MARK: Libraries

    /// Opens or creates the library and shows its window; a library that is
    /// already open just comes to the front.
    func open(libraryAt path: String, create: Bool) {
        let standardized = URL(fileURLWithPath: path).standardizedFileURL.path
        if let existing = libraryWindows.first(where: { $0.store.libraryPath == standardized }) {
            existing.showWindow(nil)
            return
        }
        do {
            let store = try LibraryStore(libraryPath: standardized, create: create)
            let controller = MainWindowController(store: store)
            libraryWindows.append(controller)
            welcomeWindow?.close()
            controller.showWindow(nil)
            refreshRecentMenu()
        } catch {
            let alert = NSAlert()
            alert.messageText = create ? "Could Not Create Library" : "Could Not Open Library"
            alert.informativeText = (error as? PaperError).map(LibraryStore.describe) ?? error.localizedDescription
            alert.alertStyle = .warning
            alert.runModal()
            if libraryWindows.isEmpty { showWelcome() }
        }
    }

    func windowClosed(_ controller: MainWindowController) {
        libraryWindows.removeAll { $0 === controller }
        if libraryWindows.isEmpty, NSApp.isRunning { showWelcome() }
    }

    @objc func openLibrary(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.title = "Open Library"
        panel.message = "Choose a Paper library folder."
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Open"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        if isLibrary(path: url.path) {
            open(libraryAt: url.path, create: false)
        } else {
            let alert = NSAlert()
            alert.messageText = "Not a Paper Library"
            alert.informativeText = "“\(url.lastPathComponent)” has no library.db. Choose File › New Library to start one here."
            alert.runModal()
        }
    }

    @objc func newLibrary(_ sender: Any?) {
        let panel = NSSavePanel()
        panel.title = "New Library"
        panel.message = "Choose where to keep the library. Paper creates a folder with the database and your PDFs."
        panel.nameFieldLabel = "Library Name:"
        panel.nameFieldStringValue = "Paper Library"
        panel.prompt = "Create"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        open(libraryAt: url.path, create: true)
    }

    @objc private func openRecent(_ sender: NSMenuItem) {
        guard let path = sender.representedObject as? String else { return }
        open(libraryAt: path, create: false)
    }

    @objc private func clearRecent(_ sender: Any?) {
        refreshRecentMenu()
    }

    private func refreshRecentMenu() {
        recentMenu.removeAllItems()
        let recents = recentLibraries().filter(isLibrary(path:))
        for path in recents {
            let item = NSMenuItem(title: URL(fileURLWithPath: path).lastPathComponent, action: #selector(openRecent(_:)), keyEquivalent: "")
            item.representedObject = path
            item.toolTip = path
            item.target = self
            item.image = NSImage(systemSymbolName: Theme.Symbol.library, accessibilityDescription: nil)
            recentMenu.addItem(item)
        }
        if recents.isEmpty {
            recentMenu.addItem(withTitle: "No Recent Libraries", action: nil, keyEquivalent: "")
        }
    }

    // MARK: Welcome

    func showWelcome() {
        if welcomeWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: Theme.Size.welcomeWidth, height: Theme.Size.welcomeHeight),
                styleMask: [.titled, .closable, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            window.title = "Welcome to Paper"
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.isMovableByWindowBackground = true
            window.isReleasedWhenClosed = false
            window.contentViewController = NSHostingController(rootView: WelcomeView(
                recents: recentLibraries().filter(isLibrary(path:)),
                openRecent: { [weak self] path in self?.open(libraryAt: path, create: false) },
                openOther: { [weak self] in self?.openLibrary(nil) },
                createNew: { [weak self] in self?.newLibrary(nil) }
            ))
            window.center()
            welcomeWindow = window
        } else if let host = welcomeWindow?.contentViewController as? NSHostingController<WelcomeView> {
            host.rootView.recents = recentLibraries().filter(isLibrary(path:))
        }
        welcomeWindow?.makeKeyAndOrderFront(nil)
    }

    // MARK: Settings

    /// The standard panel reads its icon from the bundle, which a bare
    /// executable lacks, so hand it the one set at launch.
    @objc func showAbout(_ sender: Any?) {
        var options: [NSApplication.AboutPanelOptionKey: Any] = [:]
        if let icon = NSApp.applicationIconImage { options[.applicationIcon] = icon }
        NSApp.orderFrontStandardAboutPanel(options: options)
    }

    @objc func showSettings(_ sender: Any?) {
        guard let store = frontStore else { return }
        if settingsWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: Theme.Size.settingsWidth, height: 0),
                styleMask: [.titled, .closable],
                backing: .buffered,
                defer: false
            )
            window.title = "Settings"
            window.isReleasedWhenClosed = false
            window.center()
            settingsWindow = window
        }
        settingsWindow?.contentViewController = NSHostingController(rootView: SettingsView(store: store))
        settingsWindow?.makeKeyAndOrderFront(nil)
    }

    private var frontStore: LibraryStore? {
        (NSApp.keyWindow?.windowController as? MainWindowController)?.store
            ?? (NSApp.mainWindow?.windowController as? MainWindowController)?.store
            ?? libraryWindows.last?.store
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        switch menuItem.action {
        case #selector(showSettings(_:)):
            return frontStore != nil
        default:
            return true
        }
    }

    // MARK: Menu

    private func makeMainMenu() -> NSMenu {
        let mainMenu = NSMenu()

        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About Paper", action: #selector(showAbout(_:)), keyEquivalent: "")
        if let updater {
            let checkItem = appMenu.addItem(withTitle: "Check for Updates…", action: #selector(SPUStandardUpdaterController.checkForUpdates(_:)), keyEquivalent: "")
            checkItem.target = updater
        }
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Settings…", action: #selector(showSettings(_:)), keyEquivalent: ",")
        appMenu.addItem(.separator())
        let servicesItem = appMenu.addItem(withTitle: "Services", action: nil, keyEquivalent: "")
        servicesItem.submenu = NSMenu(title: "Services")
        NSApp.servicesMenu = servicesItem.submenu
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide Paper", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let hideOthers = appMenu.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Paper", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        let fileMenu = NSMenu(title: "File")
        let newLibraryItem = fileMenu.addItem(withTitle: "New Library…", action: #selector(newLibrary(_:)), keyEquivalent: "n")
        newLibraryItem.keyEquivalentModifierMask = [.command, .shift, .option]
        fileMenu.addItem(withTitle: "Open Library…", action: #selector(openLibrary(_:)), keyEquivalent: "o")
        let recentItem = fileMenu.addItem(withTitle: "Open Recent", action: nil, keyEquivalent: "")
        recentMenu = NSMenu(title: "Open Recent")
        recentItem.submenu = recentMenu
        fileMenu.addItem(.separator())
        fileMenu.addItem(withTitle: "Add Paper…", action: #selector(MainWindowController.addPaper(_:)), keyEquivalent: "n")
        let newCollectionItem = fileMenu.addItem(withTitle: "New Collection", action: #selector(MainWindowController.newCollection(_:)), keyEquivalent: "n")
        newCollectionItem.keyEquivalentModifierMask = [.command, .shift]
        fileMenu.addItem(withTitle: "New Subcollection", action: #selector(MainWindowController.newSubcollection(_:)), keyEquivalent: "")
        fileMenu.addItem(.separator())
        fileMenu.addItem(withTitle: "Reveal Library in Finder", action: #selector(MainWindowController.revealLibraryInFinder(_:)), keyEquivalent: "")
        fileMenu.addItem(.separator())
        fileMenu.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")

        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = editMenu.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "Delete", action: #selector(PaperListViewController.delete(_:)), keyEquivalent: "\u{8}")
        editMenu.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editMenu.addItem(.separator())
        editMenu.addItem(withTitle: "Search Library…", action: #selector(MainWindowController.showSearch(_:)), keyEquivalent: "p")
        // ⌘F is what Mac users reach for first; it opens the same palette
        // without a second visible menu entry.
        let find = editMenu.addItem(withTitle: "Find", action: #selector(MainWindowController.showSearch(_:)), keyEquivalent: "f")
        find.isHidden = true

        let paperMenu = NSMenu(title: "Paper")
        paperMenu.addItem(withTitle: "Copy DOI", action: #selector(PaperListViewController.copyDOI(_:)), keyEquivalent: "")
        paperMenu.addItem(withTitle: "Open Publisher Page", action: #selector(PaperListViewController.openPublisherPage(_:)), keyEquivalent: "")
        paperMenu.addItem(withTitle: "Retry Lookup", action: #selector(PaperListViewController.retryLookup(_:)), keyEquivalent: "")
        paperMenu.addItem(.separator())
        let sortItem = paperMenu.addItem(withTitle: "Sort By", action: nil, keyEquivalent: "")
        let sortMenu = NSMenu(title: "Sort By")
        for column in SortColumn.allCases {
            let item = sortMenu.addItem(withTitle: column.label, action: #selector(MainWindowController.sortBy(_:)), keyEquivalent: "")
            item.representedObject = column
        }
        sortItem.submenu = sortMenu

        let viewMenu = NSMenu(title: "View")
        let sidebarItem = viewMenu.addItem(withTitle: "Hide Sidebar", action: #selector(MainWindowController.toggleSidebar(_:)), keyEquivalent: "s")
        sidebarItem.keyEquivalentModifierMask = [.command, .control]
        let inspectorItem = viewMenu.addItem(withTitle: "Hide Inspector", action: #selector(MainWindowController.toggleInspector(_:)), keyEquivalent: "i")
        inspectorItem.keyEquivalentModifierMask = [.command, .option]
        viewMenu.addItem(.separator())
        let fullScreen = viewMenu.addItem(withTitle: "Enter Full Screen", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
        fullScreen.keyEquivalentModifierMask = [.command, .control]

        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        windowMenu.addItem(.separator())
        windowMenu.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        NSApp.windowsMenu = windowMenu

        let helpMenu = NSMenu(title: "Help")
        NSApp.helpMenu = helpMenu

        for submenu in [appMenu, fileMenu, editMenu, paperMenu, viewMenu, windowMenu, helpMenu] {
            let item = NSMenuItem()
            item.submenu = submenu
            mainMenu.addItem(item)
        }
        refreshRecentMenu()
        return mainMenu
    }
}
