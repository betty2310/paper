import AppKit
import Combine
import PaperEngine
import UniformTypeIdentifiers
import os

/// The one object in the app that holds the engine for an open library.
///
/// It lives on the main actor and publishes snapshots that AppKit controllers
/// and SwiftUI views observe. Engine events arrive on engine threads and are
/// hopped to the main actor here before anything else sees them.
@MainActor
final class LibraryStore: ObservableObject {
    let engine: Engine
    let libraryPath: String
    let undoManager = UndoManager()

    @Published private(set) var items: [ItemRow] = []
    @Published private(set) var collections: [CollectionNode] = []
    @Published private(set) var tags: [Tag] = []
    @Published private(set) var smartListCounts: [Scope: Int] = [:]
    @Published private(set) var detail: ItemDetail?
    /// The floating toasts, oldest first.
    @Published private(set) var toasts: [Toast] = []
    /// Where each item's PDF search is, for items searched since launch.
    @Published private(set) var pdfStatus: [String: PdfStatus] = [:]
    /// Items whose PDF search just found and attached a file.
    let pdfFound = PassthroughSubject<String, Never>()
    /// Opens a paper's PDF in a tab; the window sets it for the toasts' Open button.
    var openPaper: (String) -> Void = { _ in }

    @Published var scope: Scope = .all { didSet { reloadItems() } }
    @Published var searchText = "" { didSet { reloadItems() } }
    @Published var sort: SortColumn = .added { didSet { reloadItems() } }
    @Published var ascending = false { didSet { reloadItems() } }
    @Published var selectedItemIds: [String] = [] { didSet { reloadDetail() } }

    private var toastTimers: [String: Timer] = [:]
    /// The pointer is over the toasts, so none of them leaves.
    private var holdingToasts = false
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
        case .items:
            reloadItems()
        case .lookup(let itemId, let status):
            reloadItems()
            lookupChanged(itemId: itemId, status: status)
        case .collections:
            reloadCollections()
            reloadItems()
        case .tags:
            reloadTags()
            reloadDetail()
        case .pdf(let itemId, let status):
            pdfStatus[itemId] = status
            pdfChanged(itemId: itemId, status: status)
            if status == .found { pdfFound.send(itemId) }
        case .pdfStep(let itemId, let step):
            pdfStepped(itemId: itemId, step: step)
        }
    }

    // MARK: Progress toasts

    private func lookupChanged(itemId: String, status: LookupStatus) {
        let id = Toast.itemId(itemId)
        switch status {
        case .pending:
            show(Toast(id: id, style: .working(progress: nil), message: "Looking up paper",
                       // A dropped PDF has no DOI yet; its row is named after the file.
                       subject: items.first { $0.id == itemId }.map { $0.doi ?? $0.title }))
        case .done:
            // The PDF search that starts next takes the toast over.
            show(Toast(id: id, style: .done, message: "Added to library", subject: subject(itemId)))
        case .failed:
            // The add or retry that ran the lookup reports the error.
            break
        }
    }

    private func pdfChanged(itemId: String, status: PdfStatus) {
        let id = Toast.itemId(itemId)
        let subject = subject(itemId)
        switch status {
        case .searching:
            show(Toast(id: id, style: .working(progress: nil), message: "Finding PDF", subject: subject))
        case .found:
            show(Toast(id: id, style: .done, message: "PDF downloaded", subject: subject,
                       action: openAction(itemId)))
        case .notFound:
            show(Toast(id: id, style: .warning, message: "No open access PDF", subject: subject,
                       action: .init(title: "Attach…") { [weak self] in
                           self?.dismissToast(id: id)
                           self?.chooseFileToAttach(itemId: itemId)
                       }))
        case .failed:
            show(Toast(id: id, style: .error, message: "PDF download failed", subject: subject,
                       action: .init(title: "Retry") { [weak self] in self?.findPdf(itemId: itemId) }))
        }
    }

    private func pdfStepped(itemId: String, step: PdfStep) {
        // A step that arrives after the search ended is stale.
        guard pdfStatus[itemId] == .searching else { return }
        var toast = Toast(id: Toast.itemId(itemId), style: .working(progress: nil), message: "", subject: subject(itemId))
        switch step {
        case .checking(let source):
            toast.message = "Checking \(Self.sourceName(source))"
        case .downloading(_, let received, let total):
            toast.message = "Downloading PDF"
            if let total, total > 0 {
                toast.style = .working(progress: min(1, Double(received) / Double(total)))
                toast.detail = "\(Self.bytes(received)) of \(Self.bytes(total))"
            } else {
                toast.detail = Self.bytes(received)
            }
        }
        show(toast)
    }

    private func openAction(_ itemId: String) -> Toast.Action {
        .init(title: "Open") { [weak self] in
            self?.dismissToast(id: Toast.itemId(itemId))
            self?.openPaper(itemId)
        }
    }

    private func subject(_ itemId: String) -> String? {
        items.first { $0.id == itemId }?.title ?? (try? engine.itemDetail(id: itemId))?.fields.title
    }

    private static func sourceName(_ source: String) -> String {
        switch source {
        case "arxiv": "arXiv"
        case "openalex": "OpenAlex"
        case "unpaywall": "Unpaywall"
        case "europepmc": "Europe PMC"
        default: source
        }
    }

    private static func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: count), countStyle: .file)
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

    /// Adds a paper for each PDF, such as files dropped on the list. Each row
    /// appears at once named after its file; the engine works out which
    /// paper it is, and the toast says how that went.
    func importPdfs(_ urls: [URL]) {
        for url in urls {
            Task {
                do {
                    let imported = try await engine.importPdf(path: url.path)
                    importFinished(imported)
                    // One file is like adding one paper: its row is selected.
                    if urls.count == 1 { select(imported.item.id) }
                } catch {
                    report(error)
                }
            }
        }
    }

    private func importFinished(_ imported: ImportedPdf) {
        let itemId = imported.item.id
        let id = Toast.itemId(itemId)
        let title = imported.item.title
        let reveal = Toast.Action(title: "Show") { [weak self] in
            self?.dismissToast(id: id)
            self?.select(itemId)
        }
        switch imported.identification {
        case .fileText, .model:
            show(Toast(id: id, style: .done, message: "Added to library", subject: title, action: openAction(itemId)))
        case .titleSearch:
            // Titles can collide, so the toast invites a look.
            show(Toast(id: id, style: .done, message: "Added by matching its title", subject: title, action: reveal))
        case .notFound:
            show(Toast(id: id, style: .warning, message: "Could not identify the PDF", subject: title, action: reveal))
        case .failed(let message):
            show(Toast(id: id, style: .error, message: "Could not identify the PDF", detail: message,
                       subject: title, action: reveal))
        }
    }

    /// Adds a paper typed in by hand, with the PDF if one was picked.
    /// Returns whether the paper was added.
    @discardableResult
    func createPaper(fields: ItemFields, pdf: URL?) -> Bool {
        do {
            let row = try engine.createItem(fields: fields)
            select(row.id)
            if let pdf { attachFile(itemId: row.id, url: pdf) }
            return true
        } catch {
            report(error)
            return false
        }
    }

    /// Asks for PDFs and imports them.
    func choosePdfsToImport() {
        let panel = NSOpenPanel()
        panel.title = "Import PDFs"
        panel.message = "Paper finds which paper each PDF is and copies it into the library."
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = true
        panel.prompt = "Import"
        guard panel.runModal() == .OK else { return }
        importPdfs(panel.urls)
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

    // MARK: PDFs

    /// The item's PDF, if it has one.
    func pdfAttachment(itemId: String) -> AttachmentRow? {
        (try? engine.itemDetail(id: itemId))?.attachments.first { $0.mime == "application/pdf" }
    }

    /// Searches the open access sources again. Progress arrives as `pdfStatus`.
    func findPdf(itemId: String) {
        Task {
            do {
                _ = try await engine.findPdf(itemId: itemId)
            } catch PaperError.PdfNotFound {
                // Searches that ran report this themselves; an item with no
                // identifier to search for fails before it starts.
                pdfStatus[itemId] = .notFound
                pdfChanged(itemId: itemId, status: .notFound)
            } catch {
                report(error)
            }
        }
    }

    /// Copies a PDF the user picked into the library.
    func attachFile(itemId: String, url: URL) {
        Task {
            do {
                _ = try await engine.attachFile(itemId: itemId, path: url.path)
                pdfStatus[itemId] = nil
                show(Toast(id: Toast.itemId(itemId), style: .done, message: "PDF attached", subject: subject(itemId),
                           action: openAction(itemId)))
            } catch {
                report(error)
            }
        }
    }

    /// Asks for a PDF to attach to the item.
    func chooseFileToAttach(itemId: String) {
        let panel = NSOpenPanel()
        panel.title = "Attach PDF"
        panel.message = "Choose a PDF. Paper copies it into the library and leaves the original alone."
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = false
        panel.prompt = "Attach"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        attachFile(itemId: itemId, url: url)
    }

    func attachmentFile(id: String) throws -> AttachmentFile {
        try engine.attachmentFile(id: id)
    }

    /// `zoom` 0 means fit width.
    func setReadingPosition(attachmentId: String, page: Int, zoom: Double) {
        perform { try engine.setReadingPosition(id: attachmentId, page: Int32(page), zoom: zoom) }
    }

    // MARK: Annotations

    func annotations(attachmentId: String) -> [Annotation] {
        do {
            return try engine.annotations(attachmentId: attachmentId)
        } catch {
            report(error)
            return []
        }
    }

    func createAnnotation(_ new: NewAnnotation) -> Annotation? {
        do {
            return try engine.createAnnotation(new: new)
        } catch {
            report(error)
            return nil
        }
    }

    func updateAnnotation(id: String, patch: AnnotationPatch) -> Annotation? {
        do {
            return try engine.updateAnnotation(id: id, patch: patch)
        } catch {
            report(error)
            return nil
        }
    }

    /// Returns whether the annotation is gone.
    func deleteAnnotation(id: String) -> Bool {
        do {
            try engine.deleteAnnotation(id: id)
            return true
        } catch {
            report(error)
            return false
        }
    }

    func revealAttachment(id: String) {
        guard let file = try? engine.attachmentFile(id: id) else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: file.path)])
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
            show(Toast(id: Self.trashToast, style: .done,
                       message: itemIds.count == 1 ? "Paper moved to the trash" : "\(itemIds.count) papers moved to the trash",
                       action: .init(title: "Undo") { [weak self] in self?.undoManager.undo() }))
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
            dismissToast(id: Self.trashToast)
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

    // MARK: Toasts

    private static let trashToast = "trash"
    private static let errorToast = "error"

    /// Shows a toast, or updates the one with the same id in place.
    func show(_ toast: Toast) {
        if let index = toasts.firstIndex(where: { $0.id == toast.id }) {
            if toasts[index] != toast { toasts[index] = toast }
        } else {
            toasts.append(toast)
            if toasts.count > Theme.Toast.maxCount { dismissToast(id: toasts[0].id) }
        }
        scheduleDismissal(of: toast)
    }

    func dismissToast(id: String) {
        toastTimers.removeValue(forKey: id)?.invalidate()
        toasts.removeAll { $0.id == id }
    }

    /// Finished toasts wait while the pointer is over them.
    func holdToasts(_ hold: Bool) {
        holdingToasts = hold
        if hold {
            toastTimers.values.forEach { $0.invalidate() }
            toastTimers = [:]
        } else {
            toasts.forEach(scheduleDismissal(of:))
        }
    }

    private func scheduleDismissal(of toast: Toast) {
        toastTimers.removeValue(forKey: toast.id)?.invalidate()
        guard toast.style.lingers, !holdingToasts else { return }
        let id = toast.id
        toastTimers[id] = Timer.scheduledTimer(withTimeInterval: Theme.Duration.toast, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.dismissToast(id: id) }
        }
    }

    private func perform(_ work: () throws -> Void) {
        do {
            try work()
        } catch {
            report(error)
        }
    }

    /// Turns an engine error into a toast with the right action. Errors about
    /// one paper replace that paper's progress toast.
    func report(_ error: Error) {
        func failed(_ message: String, about itemId: String? = nil, action: Toast.Action? = nil) {
            show(Toast(id: itemId.map(Toast.itemId) ?? Self.errorToast, style: .error, message: message,
                       subject: itemId.flatMap(subject), action: action))
        }
        switch error as? PaperError {
        case .Duplicate(let doi, let itemId):
            // The row added for the lookup or the import is gone, and with
            // it any word on how its lookup ended.
            for toast in toasts where toast.id.hasPrefix(Toast.itemId("")) && toast.style == .working(progress: nil) {
                if (try? engine.itemDetail(id: String(toast.id.dropFirst(Toast.itemId("").count)))) == nil {
                    dismissToast(id: toast.id)
                }
            }
            show(Toast(id: Toast.itemId(itemId), style: .warning, message: "Already in your library", subject: subject(itemId) ?? doi,
                       action: .init(title: "Show") { [weak self] in
                           self?.dismissToast(id: Toast.itemId(itemId))
                           self?.select(itemId)
                       }))
        case .InvalidIdentifier(let text):
            failed("“\(text)” is not a DOI or arXiv ID")
        case .MetadataNotFound(let doi, let itemId):
            show(Toast(id: Toast.itemId(itemId), style: .error, message: "No metadata found", subject: doi,
                       action: .init(title: "Retry") { [weak self] in self?.retryLookup(itemId: itemId) }))
        case .Lookup(let doi, let itemId, let message):
            show(Toast(id: Toast.itemId(itemId), style: .error, message: "Lookup failed", detail: message, subject: doi,
                       action: .init(title: "Retry") { [weak self] in self?.retryLookup(itemId: itemId) }))
        case .Network(let message):
            failed("Network error: \(message)")
        case .NotPdf:
            failed("That file is not a PDF")
        case .some(let other):
            failed(Self.describe(other))
        case nil:
            failed(error.localizedDescription)
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
        case .PdfNotFound: "No open access PDF found."
        case .NotPdf(let path): "Not a PDF: \(URL(fileURLWithPath: path).lastPathComponent)"
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
        case pdf(String, PdfStatus)
        case pdfStep(String, PdfStep)
    }

    private let forward: @Sendable (Event) -> Void

    init(forward: @escaping @Sendable (Event) -> Void) {
        self.forward = forward
    }

    func itemsChanged(ids: [String]) { forward(.items(ids)) }
    func collectionsChanged() { forward(.collections) }
    func tagsChanged() { forward(.tags) }
    func lookupProgress(itemId: String, status: LookupStatus) { forward(.lookup(itemId, status)) }
    func pdfProgress(itemId: String, status: PdfStatus) { forward(.pdf(itemId, status)) }
    func pdfStep(itemId: String, step: PdfStep) { forward(.pdfStep(itemId, step)) }
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
