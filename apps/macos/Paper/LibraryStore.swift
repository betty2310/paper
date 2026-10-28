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
        engine = try Engine.openLibrary(path: libraryPath)
        items = engine.listItems(query: "")
        engine.setObserver(observer: ObserverBridge { [weak self] _ in
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
        items = engine.listItems(query: query)
        onItemsChanged?()
    }
}

/// Implements the engine's observer protocol. Its methods run on engine threads,
/// so it only forwards to a closure that hops to the main actor.
private final class ObserverBridge: EngineObserver {
    private let forward: @Sendable ([String]) -> Void

    init(forward: @escaping @Sendable ([String]) -> Void) {
        self.forward = forward
    }

    func itemsChanged(ids: [String]) {
        forward(ids)
    }
}
