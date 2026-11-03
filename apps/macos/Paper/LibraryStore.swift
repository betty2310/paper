import AppKit
import Combine
import PaperEngine
import os

/// The one object in the app that holds the engine for an open library.
///
/// It lives on the main actor and publishes snapshots that AppKit controllers
/// and SwiftUI views observe. Engine events arrive on engine threads and are
/// hopped to the main actor here before anything else sees them.
@MainActor
final class LibraryStore: ObservableObject {
    /// One line of feedback at the bottom of the list, with an optional action.
    struct Notice: Equatable {
        enum Kind { case info, error }
        let kind: Kind
        let message: String
        let actionTitle: String?
        let action: (() -> Void)?

        static func == (lhs: Notice, rhs: Notice) -> Bool {
            lhs.message == rhs.message && lhs.kind == rhs.kind
        }
    }

    let engine: Engine
    let libraryPath: String
    let undoManager = UndoManager()

    @Published private(set) var items: [ItemRow] = []
    @Published private(set) var collections: [CollectionNode] = []
    @Published private(set) var tags: [Tag] = []
    @Published private(set) var smartListCounts: [Scope: Int] = [:]
    @Published private(set) var detail: ItemDetail?
    @Published private(set) var notice: Notice?

    @Published var scope: Scope = .all { didSet { reloadItems() } }
    @Published var searchText = "" { didSet { reloadItems() } }
    @Published var sort: SortColumn = .added { didSet { reloadItems() } }
    @Published var ascending = false { didSet { reloadItems() } }
    @Published var selectedItemIds: [String] = [] { didSet { reloadDetail() } }

    private var noticeTimer: Timer?
    private var noteSaveTimer: Timer?
    private var pendingNote: (itemId: String, markdown: String)?

    init(libraryPath: String, create: Bool) throws {
        engine = create
            ? try Engine.createLibrary(path: libraryPath)
            : try Engine.openLibrary(path: libraryPath)
        self.libraryPath = engine.libraryPath()
        engine.setObserver(observer: ObserverBridge { [weak self] event in
            Task { @MainActor in self?.handle(event) }
        })
        reloadItems()
        reloadCollections()
        reloadTags()
    }

    var libraryName: String {
        URL(fileURLWithPath: libraryPath).lastPathComponent
    }

    var selectedItems: [ItemRow] {
        items.filter { selectedItemIds.contains($0.id) }
    }

    // MARK: Reads

    private static let logger = Logger(subsystem: "com.betty2310.paper", category: "store")

    private func reloadItems() {
        let query = ItemQuery(text: searchText, scope: scope, sort: sort, ascending: ascending)
        let started = ContinuousClock.now
        items = (try? engine.listItems(query: query)) ?? []
        let elapsed = ContinuousClock.now - started
        Self.logger.info("listItems scope=\(String(describing: query.scope), privacy: .public) text=\(query.text.count) rows=\(self.items.count) took \(elapsed.formatted(.units(allowed: [.milliseconds], fractionalPart: .show(length: 2))), privacy: .public)")
        var counts: [Scope: Int] = [:]
        for smart in [Scope.all, .recentlyAdded, .noPdf, .unread] {
            let query = ItemQuery(text: "", scope: smart, sort: .added, ascending: false)
            counts[smart] = (try? engine.listItems(query: query).count) ?? 0
        }
        smartListCounts = counts
        let live = Set(items.map(\.id))
        let stillSelected = selectedItemIds.filter(live.contains)
        if stillSelected != selectedItemIds {
            selectedItemIds = stillSelected
        } else {
            reloadDetail()
        }
    }

    /// Whole-library search for the palette, independent of the list's scope.
    /// With no text it returns the most recently added papers.
    func search(text: String, limit: Int) -> [ItemRow] {
        let scope: Scope = text.isEmpty ? .recentlyAdded : .all
        let query = ItemQuery(text: text, scope: scope, sort: .added, ascending: false)
        var rows = (try? engine.listItems(query: query)) ?? []
        if rows.isEmpty, text.isEmpty {
            rows = (try? engine.listItems(query: ItemQuery(text: "", scope: .all, sort: .added, ascending: false))) ?? []
        }
        return Array(rows.prefix(limit))
    }

    func searchCount(text: String) -> Int {
        let query = ItemQuery(text: text, scope: .all, sort: .added, ascending: false)
        return (try? engine.listItems(query: query).count) ?? 0
    }

    private func reloadCollections() {
        collections = (try? engine.collectionTree()) ?? []
    }

    private func reloadTags() {
        tags = (try? engine.tags()) ?? []
    }

    private func reloadDetail() {
        guard selectedItemIds.count == 1, let id = selectedItemIds.first else {
            detail = nil
            return
        }
        detail = try? engine.itemDetail(id: id)
    }

    func collection(id: String) -> CollectionNode? {
        collections.first { $0.id == id }
    }

    func tag(id: String) -> Tag? {
        tags.first { $0.id == id }
    }

    // MARK: Events

    private func handle(_ event: ObserverBridge.Event) {
        switch event {
        case .items, .lookup:
            reloadItems()
        case .collections:
            reloadCollections()
            reloadItems()
        case .tags:
            reloadTags()
            reloadDetail()
        }
    }

    // MARK: Adding papers

    /// Adds a paper by DOI, doi.org URL, or arXiv ID. The row appears at once
    /// through the observer with a pending status; this suspends until the
    /// lookup finishes and then selects the new row.
    func add(identifier: String) {
        Task {
            do {
                let row = try await engine.addByIdentifier(text: identifier)
                select(row.id)
            } catch {
                report(error)
            }
        }
    }

    func retryLookup(itemId: String) {
        Task {
            do {
                _ = try await engine.retryLookup(itemId: itemId)
            } catch {
                report(error)
            }
        }
    }

    /// Selects the item, switching to All Papers if the current scope hides it.
    func select(_ itemId: String) {
        if !items.contains(where: { $0.id == itemId }) {
            searchText = ""
            scope = .all
        }
        selectedItemIds = [itemId]
    }

    // MARK: Editing

    func updateMetadata(itemId: String, patch: MetadataPatch) {
        perform { try engine.updateMetadata(id: itemId, patch: patch) }
    }

    func delete(itemIds: [String]) {
        guard !itemIds.isEmpty else { return }
        perform {
            try engine.deleteItems(ids: itemIds)
            undoManager.registerUndo(withTarget: self) { store in
                Task { @MainActor in store.undoDelete() }
            }
            undoManager.setActionName(itemIds.count == 1 ? "Delete Paper" : "Delete \(itemIds.count) Papers")
            show(Notice(
                kind: .info,
                message: itemIds.count == 1 ? "Paper moved to the trash." : "\(itemIds.count) papers moved to the trash.",
                actionTitle: "Undo",
                action: { [weak self] in self?.undoManager.undo() }
            ))
        }
    }

    func undoDelete() {
        perform {
            let restored = try engine.undoDelete()
            guard !restored.isEmpty else { return }
            undoManager.registerUndo(withTarget: self) { store in
                Task { @MainActor in store.delete(itemIds: restored) }
            }
            selectedItemIds = restored
            dismissNotice()
        }
    }

    func setTags(itemId: String, names: [String]) {
        perform { try engine.setTags(itemId: itemId, names: names) }
    }

    func setTagColor(tagId: String, color: String?) {
        perform { try engine.setTagColor(tagId: tagId, color: color) }
    }

    /// Debounced; the engine write happens after a short pause in typing.
    func scheduleNoteSave(itemId: String, markdown: String) {
        pendingNote = (itemId, markdown)
        noteSaveTimer?.invalidate()
        noteSaveTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.noteAutosave, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.flushNote() }
        }
    }

    func flushNote() {
        noteSaveTimer?.invalidate()
        guard let (itemId, markdown) = pendingNote else { return }
        pendingNote = nil
        // Saving reloads the detail; the editor keeps its own text so the cursor stays.
        perform { try engine.saveNote(itemId: itemId, markdown: markdown) }
    }

    // MARK: Collections

    @discardableResult
    func createCollection(name: String, parentId: String?) -> String? {
        var id: String?
        perform { id = try engine.createCollection(name: name, parentId: parentId) }
        return id
    }

    func renameCollection(id: String, name: String) {
        perform { try engine.renameCollection(id: id, name: name) }
    }

    func deleteCollection(id: String) {
        perform {
            try engine.deleteCollection(id: id)
            if case .collection(let selected) = scope, selected == id { scope = .all }
        }
    }

    func addToCollection(collectionId: String, itemIds: [String]) {
        perform { try engine.addToCollection(collectionId: collectionId, itemIds: itemIds) }
    }

    func removeFromCollection(collectionId: String, itemIds: [String]) {
        perform { try engine.removeFromCollection(collectionId: collectionId, itemIds: itemIds) }
    }

    // MARK: Settings

    var settings: Settings { engine.settings() }

    func updateSettings(_ settings: Settings) {
        perform { try engine.updateSettings(settings: settings) }
    }

    func close() {
        flushNote()
        engine.close()
    }

    // MARK: Notices

    func show(_ notice: Notice) {
        self.notice = notice
        noticeTimer?.invalidate()
        if notice.kind == .info {
            noticeTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.notice, repeats: false) { [weak self] _ in
                Task { @MainActor in self?.dismissNotice() }
            }
        }
    }

    func dismissNotice() {
        noticeTimer?.invalidate()
        notice = nil
    }

    private func perform(_ work: () throws -> Void) {
        do {
            try work()
        } catch {
            report(error)
        }
    }

    /// Turns an engine error into a notice with the right action.
    func report(_ error: Error) {
        switch error as? PaperError {
        case .Duplicate(let doi, let itemId):
            show(Notice(kind: .info, message: "\(doi) is already in your library.", actionTitle: "Show",
                        action: { [weak self] in self?.select(itemId) }))
        case .InvalidIdentifier(let text):
            show(Notice(kind: .error, message: "“\(text)” is not a DOI or arXiv ID.", actionTitle: nil, action: nil))
        case .MetadataNotFound(let doi, let itemId):
            show(Notice(kind: .error, message: "No metadata found for \(doi).", actionTitle: "Retry",
                        action: { [weak self] in self?.retryLookup(itemId: itemId) }))
        case .Lookup(let doi, let itemId, let message):
            show(Notice(kind: .error, message: "Could not look up \(doi): \(message)", actionTitle: "Retry",
                        action: { [weak self] in self?.retryLookup(itemId: itemId) }))
        case .Network(let message):
            show(Notice(kind: .error, message: "Network error: \(message)", actionTitle: nil, action: nil))
        case .some(let other):
            show(Notice(kind: .error, message: Self.describe(other), actionTitle: nil, action: nil))
        case nil:
            show(Notice(kind: .error, message: error.localizedDescription, actionTitle: nil, action: nil))
        }
    }

    static func describe(_ error: PaperError) -> String {
        switch error {
        case .InvalidIdentifier(let text): "Not a DOI or arXiv ID: \(text)"
        case .Duplicate(let doi, _): "Already in the library: \(doi)"
        case .NotFound(let id): "Not found: \(id)"
        case .Library(let message), .Database(let message), .Io(let message),
             .Network(let message), .Settings(let message): message
        case .MetadataNotFound(let doi, _): "No metadata found for \(doi)"
        case .Lookup(let doi, _, let message): "Could not look up \(doi): \(message)"
        case .Closed: "The library is closed."
        }
    }
}

/// Implements the engine's observer protocol. Its methods run on engine
/// threads, so it only forwards to a closure that hops to the main actor.
final class ObserverBridge: EngineObserver, Sendable {
    enum Event: Sendable {
        case items([String])
        case collections
        case tags
        case lookup(String, LookupStatus)
    }

    private let forward: @Sendable (Event) -> Void

    init(forward: @escaping @Sendable (Event) -> Void) {
        self.forward = forward
    }

    func itemsChanged(ids: [String]) { forward(.items(ids)) }
    func collectionsChanged() { forward(.collections) }
    func tagsChanged() { forward(.tags) }
    func lookupProgress(itemId: String, status: LookupStatus) { forward(.lookup(itemId, status)) }
}

extension ItemRow {
    /// "Ho et al. · NeurIPS · 2020", leaving out what is missing.
    var subtitle: String {
        [authors.isEmpty ? nil : authors, journal, year.map(String.init)]
            .compactMap { $0 }
            .joined(separator: " · ")
    }
}

extension ItemDetail {
    var authorsLine: String {
        fields.creators.map(\.displayName).joined(separator: ", ")
    }

    /// "Nature · 2015 · 521(7553) · 436–444".
    var venueLine: String {
        var parts: [String] = []
        if let venue = fields.containerShort ?? fields.containerTitle { parts.append(venue) }
        if let year = fields.year { parts.append(String(year)) }
        if let volume = fields.volume {
            parts.append(fields.issue.map { "\(volume)(\($0))" } ?? volume)
        }
        if let pages = fields.pages { parts.append(pages) }
        return parts.joined(separator: " · ")
    }
}

extension Creator {
    var displayName: String {
        switch (given, family, literal) {
        case (let given?, let family?, _): "\(given) \(family)"
        case (nil, let family?, _): family
        case (_, nil, let literal?): literal
        case (let given?, nil, nil): given
        case (nil, nil, nil): ""
        }
    }
}

extension ItemKind {
    var label: String {
        switch self {
        case .journalArticle: "Journal article"
        case .conferencePaper: "Conference paper"
        case .preprint: "Preprint"
        case .other: "Paper"
        }
    }
}

extension SortColumn: @retroactive CaseIterable {
    public static let allCases: [SortColumn] = [.added, .title, .firstAuthor, .journal, .year]

    var label: String {
        switch self {
        case .title: "Title"
        case .firstAuthor: "First Author"
        case .journal: "Journal"
        case .year: "Year"
        case .added: "Date Added"
        }
    }
}
