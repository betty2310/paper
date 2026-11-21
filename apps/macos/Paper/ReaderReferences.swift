import AppKit
import PaperEngine
import PDFKit
import SwiftUI

/// Links from a paper's citations to the papers they cite. Resting the
/// pointer on an in-text citation, or on an entry of the references
/// section, shows a card with the cited paper and a button that opens it on
/// the web; ⌘-click opens it at once. A plain click on a citation still
/// jumps to the references, with Back, as PDFKit does.
@MainActor
final class ReaderReferences {
    /// What the pointer rests on: a citation or an entry, and where it is.
    private struct Target: Equatable {
        let reference: Int
        let pageIndex: Int
        /// In page space.
        let rect: CGRect
        let isCitation: Bool
    }

    private let store: LibraryStore
    private let attachmentId: String
    private weak var pdfView: PDFView?
    private weak var container: NSView?

    private var references: [Reference] = []
    private var citationsByPage: [Int: [CitationLink]] = [:]
    private var partsByPage: [Int: [(rect: CGRect, reference: Int)]] = [:]

    private let model = ReferenceCardModel()
    private lazy var card = NSHostingView(rootView: ReferenceCard(model: model))
    private let tint = PassThroughView()
    private var shown: Target?
    private var pending: Target?
    private var pointerInCard = false
    private var showTimer: Timer?
    private var hideTimer: Timer?
    private var copiedTimer: Timer?
    /// Entries searched since the tab opened, so each is asked about once.
    private var searched: Set<Int> = []
    private var loading: Task<Void, Never>?

    init(store: LibraryStore, attachmentId: String, pdfView: PDFView, container: NSView) {
        self.store = store
        self.attachmentId = attachmentId
        self.pdfView = pdfView
        self.container = container
        card.translatesAutoresizingMaskIntoConstraints = true
        tint.wantsLayer = true
        tint.layer?.backgroundColor = Theme.Color.referenceTint.cgColor
        tint.layer?.cornerRadius = Theme.Size.referenceTintRadius
        model.open = { [weak self] in self?.openShown() }
        model.openInLibrary = { [weak self] in self?.openShownInLibrary() }
        model.add = { [weak self] in self?.addShown() }
        model.copy = { [weak self] in self?.copyShown() }
        model.hovering = { [weak self] inside in self?.cardHovered(inside) }
    }

    /// Reads the references from the file at once, then again after the
    /// engine has matched them with the works OpenAlex lists as cited.
    func load() {
        loading = Task { [weak self, store, attachmentId] in
            if let found = try? await store.engine.references(attachmentId: attachmentId) { self?.apply(found) }
            if let found = try? await store.engine.resolveReferences(attachmentId: attachmentId) { self?.apply(found) }
        }
    }

    func close() {
        loading?.cancel()
        hide()
    }

    private func apply(_ found: References) {
        references = found.references
        citationsByPage = Dictionary(grouping: found.citations) { Int($0.pageIndex) }
        partsByPage = [:]
        for reference in found.references {
            for part in reference.parts {
                partsByPage[Int(part.pageIndex), default: []].append((AnnotationGeometry.cgRect(part.rect), Int(reference.index)))
            }
        }
        if let shown, shown.reference < references.count { model.reference = references[shown.reference] }
    }

    // MARK: Pointer

    func pointerMoved(_ event: NSEvent) {
        if let container, card.superview != nil, card.frame.contains(container.convert(event.locationInWindow, from: nil)) { return }
        let found = target(at: event)
        if found == shown, found != nil {
            hideTimer?.invalidate()
            pending = nil
            showTimer?.invalidate()
            return
        }
        guard let found else {
            pending = nil
            showTimer?.invalidate()
            if shown != nil { scheduleHide() }
            return
        }
        guard found != pending else { return }
        pending = found
        showTimer?.invalidate()
        // Once a card is open, the next citation's card comes quickly.
        let delay = shown == nil ? Theme.Duration.referenceHover : Theme.Duration.referenceSwitch
        showTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.show(found) }
        }
    }

    func pointerExited() {
        pending = nil
        showTimer?.invalidate()
        if shown != nil { scheduleHide() }
    }

    /// A click on the pages. ⌘-click on a citation or entry opens the cited
    /// paper and the reader keeps the click; any other click closes the card.
    func mouseDown(_ event: NSEvent) -> Bool {
        hide()
        guard event.modifierFlags.contains(.command), let found = target(at: event) else { return false }
        open(references[found.reference])
        return true
    }

    /// The reference under the pointer, for the context menu.
    func reference(at event: NSEvent) -> Reference? {
        target(at: event).map { references[$0.reference] }
    }

    private func target(at event: NSEvent) -> Target? {
        guard let pdfView, !references.isEmpty else { return nil }
        let point = pdfView.convert(event.locationInWindow, from: nil)
        guard let page = pdfView.page(for: point, nearest: false), let index = pdfView.document?.index(for: page) else { return nil }
        let onPage = pdfView.convert(point, to: page)
        let slop = -Theme.Size.referenceTintOutset
        if let link = citationsByPage[index]?.first(where: { AnnotationGeometry.cgRect($0.rect).insetBy(dx: slop, dy: slop).contains(onPage) }),
           Int(link.reference) < references.count {
            return Target(reference: Int(link.reference), pageIndex: index, rect: AnnotationGeometry.cgRect(link.rect), isCitation: true)
        }
        if let part = partsByPage[index]?.first(where: { $0.rect.contains(onPage) }) {
            return Target(reference: part.reference, pageIndex: index, rect: part.rect, isCitation: false)
        }
        return nil
    }

    // MARK: Card

    private func show(_ target: Target) {
        guard let container, let pdfView else { return }
        pending = nil
        hideTimer?.invalidate()
        let isNew = card.superview == nil
        shown = target
        model.reference = references[target.reference]
        model.showsText = target.isCitation
        model.copied = false
        model.isAdding = false
        model.isSearching = false
        // A new root view updates at once, so the size below fits the new
        // content rather than the last card's.
        card.rootView = ReferenceCard(model: model)
        if tint.superview == nil { container.addSubview(tint, positioned: .above, relativeTo: pdfView) }
        if isNew {
            container.addSubview(card)
            card.alphaValue = 0
        }
        reposition()
        if isNew {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = Theme.Duration.panel
                card.animator().alphaValue = 1
            }
        }
        searchIfNeeded(target.reference)
    }

    func hide() {
        showTimer?.invalidate()
        hideTimer?.invalidate()
        pending = nil
        shown = nil
        pointerInCard = false
        // Out of the view rather than hidden, so the buttons get fresh hover
        // tracking each time the card comes back.
        card.removeFromSuperview()
        tint.removeFromSuperview()
    }

    private func scheduleHide() {
        hideTimer?.invalidate()
        hideTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.referenceLinger, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.pointerInCard else { return }
                self.hide()
            }
        }
    }

    private func cardHovered(_ inside: Bool) {
        pointerInCard = inside
        if inside { hideTimer?.invalidate() } else if shown != nil { scheduleHide() }
    }

    /// Puts the tint over the citation or entry and the card next to it:
    /// below a citation, so the sentence that cites stays readable, and above
    /// an entry, over the entries already read. Called on scroll and zoom.
    func reposition() {
        guard let shown, let container, let pdfView, let page = pdfView.document?.page(at: shown.pageIndex) else { return }
        let visible = container.convert(pdfView.bounds, from: pdfView)
        let anchor = container.convert(pdfView.convert(shown.rect, from: page), from: pdfView)
        guard anchor.intersects(visible) else {
            hide()
            return
        }
        let outset = Theme.Size.referenceTintOutset
        tint.frame = container.backingAlignedRect(anchor.insetBy(dx: -outset, dy: -outset), options: .alignAllEdgesNearest)

        let size = card.fittingSize
        // The hosting view is larger than the card by the shadow's room.
        let inset = Theme.Markup.shadowRadius
        let gap = Theme.Size.referenceCardOffset + outset
        let below = anchor.minY - gap - size.height + inset
        let above = anchor.maxY + gap - inset
        var y = shown.isCitation ? below : above
        if shown.isCitation, below + inset < visible.minY {
            y = above
        } else if !shown.isCitation, above + size.height - inset > visible.maxY {
            y = below
        }
        var origin = CGPoint(x: shown.isCitation ? anchor.midX - size.width / 2 : anchor.minX - inset, y: y)
        origin.x = min(max(origin.x, visible.minX), visible.maxX - size.width)
        origin.y = min(max(origin.y, visible.minY), visible.maxY - size.height)
        card.frame = container.backingAlignedRect(CGRect(origin: origin, size: size), options: .alignAllEdgesNearest)
    }

    /// Lays the card out again after its content changed size.
    private func refresh() {
        guard card.superview != nil else { return }
        card.rootView = ReferenceCard(model: model)
        reposition()
    }

    // MARK: Actions

    /// Asks Crossref about an entry with no link, once, while its card shows.
    private func searchIfNeeded(_ index: Int) {
        guard references[index].foundBy == .unresolved, !searched.contains(index) else { return }
        searched.insert(index)
        model.isSearching = true
        Task { [weak self, store, attachmentId] in
            let found = try? await store.engine.searchReference(attachmentId: attachmentId, index: UInt32(index))
            guard let self else { return }
            if let found, index < references.count { references[index] = found }
            if shown?.reference == index {
                model.isSearching = false
                model.reference = references[index]
                refresh()
            }
        }
    }

    func open(_ reference: Reference) {
        NSWorkspace.shared.open(ReferenceLink(reference).url)
    }

    private func openShown() {
        guard let reference = model.reference else { return }
        open(reference)
        hide()
    }

    private func openShownInLibrary() {
        guard let itemId = model.reference?.itemId else { return }
        hide()
        store.openPaper(itemId)
    }

    /// Adds the cited paper by its DOI or arXiv ID. The card then offers to
    /// open it.
    private func addShown() {
        guard let reference = model.reference, let identifier = ReferenceLink(reference).identifier else { return }
        let index = Int(reference.index)
        model.isAdding = true
        Task { [weak self, store] in
            var itemId: String?
            do {
                itemId = try await store.engine.addByIdentifier(text: identifier).id
            } catch PaperError.Duplicate(_, let existing) {
                itemId = existing
            } catch {
                store.report(error)
            }
            guard let self else { return }
            if let itemId, index < references.count { references[index].itemId = itemId }
            if shown?.reference == index {
                model.isAdding = false
                model.reference = references[index]
                refresh()
            }
        }
    }

    func copyLink(_ reference: Reference) {
        let link = ReferenceLink(reference)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(link.kind == .search ? reference.text : link.url.absoluteString, forType: .string)
    }

    private func copyShown() {
        guard let reference = model.reference else { return }
        copyLink(reference)
        model.copied = true
        copiedTimer?.invalidate()
        copiedTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.copiedFeedback, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.model.copied = false }
        }
    }
}

/// The tint behind a citation: drawn over the page, but clicks go through.
private final class PassThroughView: NSView {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
