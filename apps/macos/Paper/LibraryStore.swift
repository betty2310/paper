import Foundation
import PaperEngine

/// The one object in the app that holds the engine.
///
/// It lives on the main actor, so views can read `items` directly. Engine
/// events arrive on engine threads and are moved to the main actor here, before
/// anything else sees them.
@MainActor
final class LibraryStore {
    private let engine: Engine
    private(set) var items: [ItemRow]
    private(set) var query = ""

    /// Called on the main actor after `items` changes.
    var onItemsChanged: (() -> Void)?

    init(libraryPath: String) throws {
        engine = try isLibrary(path: libraryPath)
            ? Engine.openLibrary(path: libraryPath)
            : Engine.createLibrary(path: libraryPath)
        items = try engine.listItems(query: Self.query(""))
        engine.setObserver(observer: ObserverBridge { [weak self] in
            Task { @MainActor in self?.reload() }
        })
    }

    var libraryPath: String { engine.libraryPath() }

    /// Filters `items`. A synchronous engine call, fast enough for every keystroke.
    func search(_ text: String) {
        query = text
        reload()
    }

    /// Adds a paper by DOI. Suspends while the engine looks it up on its own
    /// threads. `items` updates through the observer, not through this return value.
    func add(identifier: String) async throws -> ItemRow {
        try await engine.addByIdentifier(text: identifier)
    }

    private func reload() {
        items = (try? engine.listItems(query: Self.query(query))) ?? []
        onItemsChanged?()
    }

    /// The whole library, newest first, filtered by `text`.
    private static func query(_ text: String) -> ItemQuery {
        ItemQuery(text: text, scope: .all, sort: .added, ascending: false)
    }
}

/// Implements the engine's observer protocol. Its methods run on engine threads,
/// so it only forwards to a closure that hops to the main actor.
private final class ObserverBridge: EngineObserver {
    private let forward: @Sendable () -> Void

    init(forward: @escaping @Sendable () -> Void) {
        self.forward = forward
    }

    func itemsChanged(ids: [String]) {
        forward()
    }

    func collectionsChanged() {}

    func tagsChanged() {}

    func lookupProgress(itemId: String, status: LookupStatus) {
        forward()
    }
}
