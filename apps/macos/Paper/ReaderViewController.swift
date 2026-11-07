import AppKit
import Combine
import PDFKit
import PaperEngine
import SwiftUI

/// One open PDF in its own tab: a `PDFView` in continuous single page mode.
///
/// The document is opened on a background task and handed to the main actor,
/// then the pages fade in. The page and zoom are written back to the engine a
/// moment after they stop changing, so the next open lands in the same place.
/// The file itself is only read; PDFKit never writes it back.
///
/// Following a link inside the PDF, such as a citation, remembers where the
/// reader was, and Back returns there. Highlights and sticky notes are stored
/// by the engine and drawn as `PDFAnnotation`s on the in-memory document.
/// The PDF's outline is the table of contents in the sidebar, and ⌘F finds
/// text with PDFKit's search, marking every match on the pages. The detail
/// panel lists the annotations as cards and holds the paper's Markdown note.
@MainActor
final class ReaderViewController: NSViewController, ReaderPDFViewDelegate, NSPopoverDelegate {
    let store: LibraryStore
    let itemId: String
    let attachmentId: String
    /// The paper's title, shown in the tab.
    let paperTitle: String
    /// The table of contents and the pages, shown in the sidebar while this tab is in front.
    let sidebar = ReaderSidebarViewController()
    /// The annotations and the note, shown in the detail panel while this tab is in front.
    let panel: ReaderPanelViewController

    private let file: AttachmentFile
    private let pdfView = ReaderPDFView()
    private let failureLabel = NSTextField(labelWithString: "This PDF could not be opened.")
    private var saveTimer: Timer?
    private var observers: [NSObjectProtocol] = []

    /// Where the reader was before each followed link, oldest first.
    private var backStack: [PDFDestination] = []
    private var forwardStack: [PDFDestination] = []
    /// Where a click on an internal link started, until the click ends.
    private var linkOrigin: PDFDestination?
    /// The controls floating over the top of the page.
    private let barModel = ReaderBarModel()
    private lazy var readerBar = NSHostingView(rootView: ReaderBar(model: barModel))

    /// The engine's annotations by id, and what PDFKit draws for each.
    private var annotations: [String: Annotation] = [:]
    private var drawn: [String: PDFAnnotation] = [:]
    private enum MarkupTarget {
        case selection(PDFSelection)
        case highlight(String)
    }
    /// What the floating toolbar is showing for, if it shows.
    private var markupTarget: MarkupTarget?
    private let markupModel = MarkupBarModel()
    private lazy var markupBar = NSHostingView(rootView: MarkupBar(model: markupModel))
    /// The color the last highlight used, for ⇧⌘H.
    private static var lastHighlightColor = Theme.Markup.swatches[0].hex
    /// After Add Note, the next click on a page places it.
    private(set) var isPlacingNote = false
    private var noteEditor: (id: String, model: NoteEditorModel, popover: NSPopover)?
    /// The find bar, which floats under the controls at the trailing edge while it is open.
    private let findModel = ReaderFindModel()
    private lazy var findBar = NSHostingView(rootView: ReaderFindBar(model: findModel))
    /// The current search's matches in document order, as PDFKit finds them.
    private var matches: [PDFSelection] = []
    private var matchIndex: Int?
    /// Searches asked for whose start PDFKit has not announced yet. Matches
    /// that arrive before then belong to a search that was replaced.
    private var pendingFinds = 0
    private var findTimer: Timer?
    /// Characters of page text kept on each side of a highlight, so it can be
    /// found again if the PDF is ever replaced.
    private static let contextLength = 32

    init(store: LibraryStore, itemId: String, file: AttachmentFile, paperTitle: String) {
        self.store = store
        self.itemId = itemId
        self.file = file
        self.attachmentId = file.id
        self.paperTitle = paperTitle
        panel = ReaderPanelViewController(store: store, itemId: itemId)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let root = NSView()
        root.wantsLayer = true
        root.layer?.backgroundColor = Theme.Color.readerBackground.cgColor

        pdfView.displayMode = .singlePageContinuous
        pdfView.displayDirection = .vertical
        pdfView.displaysPageBreaks = true
        pdfView.backgroundColor = Theme.Color.readerBackground
        pdfView.minScaleFactor = Theme.Size.minZoom
        pdfView.maxScaleFactor = Theme.Size.maxZoom
        pdfView.autoScales = true
        pdfView.setAccessibilityLabel(paperTitle)

        failureLabel.font = Theme.Font.body
        failureLabel.textColor = Theme.Color.secondaryText
        failureLabel.isHidden = true

        pdfView.readerDelegate = self
        readerBar.isHidden = true
        markupBar.translatesAutoresizingMaskIntoConstraints = true
        markupModel.pick = { [weak self] swatch in self?.pick(swatch) }
        markupModel.copy = { [weak self] in self?.copySelection() }
        markupModel.delete = { [weak self] in self?.removeTargetHighlight() }
        barModel.back = { [weak self] in self?.goBack() }
        barModel.forward = { [weak self] in self?.goForward() }
        barModel.zoomIn = { [weak self] in self?.zoomIn() }
        barModel.zoomOut = { [weak self] in self?.zoomOut() }
        barModel.fitWidth = { [weak self] in self?.fitWidth() }
        barModel.highlight = { [weak self] in self?.highlightSelection() }
        findModel.changed = { [weak self] in self?.scheduleFind() }
        findModel.next = { [weak self] in self?.findNext() }
        findModel.previous = { [weak self] in self?.findPrevious() }
        findModel.close = { [weak self] in self?.closeFind() }
        sidebar.outline.onSelect = { [weak self] destination in self?.go(to: destination) }
        sidebar.pages.onSelect = { [weak self] index in
            guard let page = self?.pdfView.document?.page(at: index) else { return }
            self?.go(to: PDFDestination(page: page, at: NSPoint(x: 0, y: kPDFDestinationUnspecifiedValue)))
        }
        panel.model.select = { [weak self] id in self?.goToAnnotation(id) }
        panel.model.saveComment = { [weak self] id, text in
            guard let self, let annotation = annotations[id] else { return }
            update(id, patch: AnnotationPatch(color: nil, comment: text), actionName: annotation.kind == .note ? "Edit Note" : "Edit Comment")
        }
        panel.model.delete = { [weak self] id in
            guard let self, let annotation = annotations[id] else { return }
            if case .highlight(id) = markupTarget { hideMarkupBar() }
            remove(id, actionName: annotation.kind == .note ? "Delete Note" : "Remove Highlight")
        }
        barModel.addNote = { [weak self] in
            guard let self else { return }
            if isPlacingNote { stopPlacingNote() } else { startPlacingNote() }
        }

        for subview in [pdfView, failureLabel, readerBar] as [NSView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(subview)
        }
        NSLayoutConstraint.activate([
            pdfView.topAnchor.constraint(equalTo: root.topAnchor),
            pdfView.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            pdfView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            pdfView.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            failureLabel.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            failureLabel.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            readerBar.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            // The hosting view includes room for the capsule's shadow.
            readerBar.topAnchor.constraint(equalTo: root.topAnchor, constant: Theme.Size.readerBarInset - Theme.Toast.shadowRadius - Theme.Toast.shadowY),
        ])
        view = root
    }

    /// Opens the document off the main actor and returns once the view has
    /// it, so the tab never shows an empty canvas. PDFKit draws the pages on
    /// its own threads.
    func open() async {
        _ = view
        let url = URL(fileURLWithPath: file.path)
        let opened = await Task.detached(priority: .userInitiated) {
            PDFKitBox(value: PDFDocument(url: url))
        }.value
        show(opened.value)
    }

    private var restoredPage = false

    /// The last page is restored once the view has its real size, because
    /// scrolling before that lands on the wrong spot with fit width.
    override func viewDidAppear() {
        super.viewDidAppear()
        guard !restoredPage, let document = pdfView.document else { return }
        restoredPage = true
        guard let last = file.lastPage, last > 0, let page = document.page(at: min(Int(last), document.pageCount - 1)) else { return }
        // Fit width settles its scale in the layout pass after the view
        // joins the window; scrolling before that lands on the wrong page.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            view.layoutSubtreeIfNeeded()
            pdfView.layoutDocumentView()
            pdfView.go(to: page)
        }
    }

    private func show(_ document: PDFDocument?) {
        guard let document, document.pageCount > 0 else {
            failureLabel.isHidden = false
            return
        }
        pdfView.document = document
        sidebar.load(document)
        for annotation in store.annotations(attachmentId: attachmentId) {
            draw(annotation)
        }
        if let zoom = file.lastZoom, zoom > 0 {
            pdfView.autoScales = false
            pdfView.scaleFactor = CGFloat(zoom)
        }
        observers = [
            NotificationCenter.default.addObserver(forName: .PDFViewPageChanged, object: pdfView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pageChanged() }
            },
            NotificationCenter.default.addObserver(forName: .PDFViewScaleChanged, object: pdfView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.pageChanged()
                    self?.updateBar()
                    self?.positionMarkupBar()
                }
            },
            NotificationCenter.default.addObserver(forName: .PDFViewSelectionChanged, object: pdfView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.selectionChanged() }
            },
            NotificationCenter.default.addObserver(forName: .PDFDocumentDidBeginFind, object: document, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.findBegan() }
            },
            NotificationCenter.default.addObserver(forName: .PDFDocumentDidFindMatch, object: document, queue: .main) { [weak self] notification in
                let match = PDFKitBox(value: notification.userInfo?["PDFDocumentFoundSelection"] as? PDFSelection)
                MainActor.assumeIsolated { if let selection = match.value { self?.found(selection) } }
            },
            NotificationCenter.default.addObserver(forName: .PDFDocumentDidEndFind, object: document, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.findEnded() }
            },
        ]
        // Scrolling moves the text under the toolbar; the toolbar follows.
        if let clipView = pdfView.documentView?.enclosingScrollView?.contentView {
            clipView.postsBoundsChangedNotifications = true
            observers.append(NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: clipView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.positionMarkupBar()
                    self?.updateOutline()
                }
            })
        }
        updateBar()
        readerBar.isHidden = false
        // Opening counts as reading: the paper leaves the Unread list now.
        if file.lastPage == nil { savePosition() }
    }

    /// Gives the pages keyboard focus, so arrow keys and Space scroll at once.
    func focus() {
        view.window?.makeFirstResponder(pdfView)
    }

    // MARK: Zoom

    func zoomIn() {
        setZoom(pdfView.scaleFactor * Theme.Size.zoomStep)
    }

    func zoomOut() {
        setZoom(pdfView.scaleFactor / Theme.Size.zoomStep)
    }

    func fitWidth() {
        pdfView.autoScales = true
        updateBar()
        scheduleSave()
    }

    var canZoomIn: Bool { pdfView.document != nil && pdfView.scaleFactor < Theme.Size.maxZoom }
    var canZoomOut: Bool { pdfView.document != nil && pdfView.scaleFactor > Theme.Size.minZoom }

    private func setZoom(_ scale: CGFloat) {
        guard pdfView.document != nil else { return }
        pdfView.autoScales = false
        pdfView.scaleFactor = min(max(scale, Theme.Size.minZoom), Theme.Size.maxZoom)
        updateBar()
        scheduleSave()
    }

    // MARK: Reading position

    private func pageChanged() {
        updateBar()
        updateOutline()
        scheduleSave()
    }

    /// Keeps the table of contents and the pages on the part at the top of the view.
    private func updateOutline() {
        guard let document = pdfView.document, let here = pdfView.currentDestination, let page = here.page else { return }
        sidebar.reveal(here, pageIndex: document.index(for: page))
    }

    /// Brings the floating controls up to date with the page and zoom.
    private func updateBar() {
        guard let document = pdfView.document else { return }
        barModel.pageCount = document.pageCount
        barModel.page = pdfView.currentPage.map { document.index(for: $0) + 1 } ?? 1
        barModel.zoomPercent = Int((pdfView.scaleFactor * 100).rounded())
        barModel.fitsWidth = pdfView.autoScales
        barModel.canZoomIn = canZoomIn
        barModel.canZoomOut = canZoomOut
    }

    private func scheduleSave() {
        saveTimer?.invalidate()
        saveTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.readingPositionSave, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.savePosition() }
        }
    }

    /// Writes the page and zoom now. Zoom 0 means fit width.
    func savePosition() {
        saveTimer?.invalidate()
        guard let document = pdfView.document, let page = pdfView.currentPage else { return }
        store.setReadingPosition(
            attachmentId: attachmentId,
            page: document.index(for: page),
            zoom: pdfView.autoScales ? 0 : Double(pdfView.scaleFactor)
        )
    }

    /// Called when the tab closes.
    func close() {
        findTimer?.invalidate()
        pdfView.document?.cancelFindString()
        finishEditingNote()
        savePosition()
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
        store.undoManager.removeAllActions(withTarget: self)
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        positionMarkupBar()
    }

    // MARK: Following links

    var canGoBack: Bool { !backStack.isEmpty }
    var canGoForward: Bool { !forwardStack.isEmpty }

    func goBack() {
        guard let target = backStack.popLast() else { return }
        if let here = pdfView.currentDestination { forwardStack.append(here) }
        pdfView.go(to: target)
        historyChanged()
    }

    func goForward() {
        guard let target = forwardStack.popLast() else { return }
        if let here = pdfView.currentDestination { backStack.append(here) }
        pdfView.go(to: target)
        historyChanged()
    }

    /// A click on a link ended. PDFKit has moved the view by now if the link
    /// pointed inside the document; if it did, the old place goes on the
    /// back stack.
    private func linkFollowed(from origin: PDFDestination) {
        guard let now = pdfView.currentDestination, Self.differ(origin, now) else { return }
        backStack.append(origin)
        forwardStack.removeAll()
        historyChanged()
    }

    private static func differ(_ first: PDFDestination, _ second: PDFDestination) -> Bool {
        first.page != second.page || abs(first.point.y - second.point.y) > 1 || abs(first.point.x - second.point.x) > 1
    }

    /// A table of contents entry or a page was picked. Like a link, Back returns.
    private func go(to destination: PDFDestination) {
        let origin = pdfView.currentDestination
        pdfView.go(to: destination)
        guard let origin else { return }
        DispatchQueue.main.async { [weak self] in self?.linkFollowed(from: origin) }
    }

    private func historyChanged() {
        barModel.backPage = backStack.last.flatMap(pageNumber)
        barModel.forwardPage = forwardStack.last.flatMap(pageNumber)
    }

    private func pageNumber(_ destination: PDFDestination) -> Int? {
        guard let page = destination.page, let document = pdfView.document else { return nil }
        return document.index(for: page) + 1
    }

    /// Links that move within this PDF: citations, figure and section references.
    private static func isInternalLink(_ annotation: PDFAnnotation) -> Bool {
        annotation.destination != nil || annotation.action is PDFActionGoTo
    }

    // MARK: Clicks

    func readerView(_ view: ReaderPDFView, mouseDown event: NSEvent) -> Bool {
        hideMarkupBar()
        let placing = isPlacingNote
        guard let (page, point) = pageLocation(of: event, nearest: placing) else { return false }
        if placing {
            stopPlacingNote()
            addNote(on: page, at: point)
            return true
        }
        if let id = annotationId(of: .note, at: point, on: page) {
            editNote(id)
            return true
        }
        if page.annotations.contains(where: { $0.bounds.contains(point) && Self.isInternalLink($0) }) {
            linkOrigin = pdfView.currentDestination
        }
        return false
    }

    func readerView(_ view: ReaderPDFView, mouseUp event: NSEvent) {
        if let origin = linkOrigin {
            linkOrigin = nil
            // PDFKit may finish the jump after this event; look on the next turn.
            DispatchQueue.main.async { [weak self] in self?.linkFollowed(from: origin) }
        }
        if let selection = selectedText {
            showMarkupBar(for: .selection(selection))
        } else if let (page, point) = pageLocation(of: event, nearest: false),
                  let id = annotationId(of: .highlight, at: point, on: page) {
            showMarkupBar(for: .highlight(id))
            panel.model.selectedId = id
        }
    }

    func readerView(_ view: ReaderPDFView, navigate step: Int) {
        if step < 0 { goBack() } else { goForward() }
    }

    func readerView(_ view: ReaderPDFView, menuItemsFor event: NSEvent) -> [NSMenuItem] {
        var items: [NSMenuItem] = []
        if selectedText != nil {
            items.append(MenuAction("Highlight") { [weak self] in self?.highlightSelection() })
        }
        guard let (page, point) = pageLocation(of: event, nearest: false) else { return items }
        if let id = annotationId(of: .note, at: point, on: page) {
            items.append(MenuAction("Edit Note") { [weak self] in self?.editNote(id) })
            items.append(MenuAction("Delete Note") { [weak self] in self?.remove(id, actionName: "Delete Note") })
        } else {
            if let id = annotationId(of: .highlight, at: point, on: page) {
                items.append(MenuAction("Remove Highlight") { [weak self] in self?.remove(id, actionName: "Remove Highlight") })
            }
            items.append(MenuAction("Add Note Here") { [weak self] in self?.addNote(on: page, at: point) })
        }
        return items
    }

    /// The page under the pointer and the point on it in page space.
    private func pageLocation(of event: NSEvent, nearest: Bool) -> (PDFPage, CGPoint)? {
        let point = pdfView.convert(event.locationInWindow, from: nil)
        guard let page = pdfView.page(for: point, nearest: nearest) else { return nil }
        return (page, pdfView.convert(point, to: page))
    }

    /// The topmost of Paper's annotations of `kind` under a page point.
    private func annotationId(of kind: AnnotationKind, at point: CGPoint, on page: PDFPage) -> String? {
        guard let index = pdfView.document?.index(for: page) else { return nil }
        return annotations.values
            .filter { $0.kind == kind && Int($0.pageIndex) == index }
            .sorted { $0.createdAt > $1.createdAt }
            .first { annotation in
                switch annotation.kind {
                case .highlight: AnnotationGeometry.contains(annotation.quads, point)
                case .note: AnnotationGeometry.cgRect(annotation.rect).contains(point)
                }
            }?.id
    }

    // MARK: Highlights

    /// The selection, when it holds more than white space.
    private var selectedText: PDFSelection? {
        guard let selection = pdfView.currentSelection,
              let text = selection.string, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return nil }
        return selection
    }

    var canHighlight: Bool { selectedText != nil }

    /// Highlights the selected text, one annotation per page it covers.
    func highlightSelection(color: String? = nil) {
        guard let selection = selectedText, let document = pdfView.document else { return }
        let color = color ?? Self.lastHighlightColor
        Self.lastHighlightColor = color
        let lines = selection.selectionsByLine()
        for page in selection.pages {
            let onPage = lines.filter { $0.pages.first == page }
            let rects = onPage.map { $0.bounds(for: page) }.filter { !$0.isEmpty }
            guard !rects.isEmpty else { continue }
            let context = context(of: rects, on: page)
            add(NewAnnotation(
                attachmentId: attachmentId,
                kind: .highlight,
                pageIndex: Int32(document.index(for: page)),
                quads: rects.map(AnnotationGeometry.quad(line:)),
                rect: nil,
                color: color,
                selectedText: onPage.compactMap(\.string).joined(separator: " "),
                comment: nil,
                textBefore: context.before,
                textAfter: context.after
            ))
        }
        store.undoManager.setActionName("Highlight")
        pdfView.clearSelection()
        hideMarkupBar()
    }

    /// The page text just before the first line and just after the last.
    private func context(of lines: [CGRect], on page: PDFPage) -> (before: String?, after: String?) {
        guard let text = page.string as NSString?, let first = lines.first, let last = lines.last else { return (nil, nil) }
        let start = page.characterIndex(at: CGPoint(x: first.minX + 1, y: first.midY))
        let end = page.characterIndex(at: CGPoint(x: last.maxX - 1, y: last.midY))
        guard start != NSNotFound, end != NSNotFound, start <= end, end < text.length else { return (nil, nil) }
        let before = NSRange(location: max(0, start - Self.contextLength), length: min(Self.contextLength, start))
        let after = NSRange(location: end + 1, length: min(Self.contextLength, text.length - end - 1))
        return (text.substring(with: before), text.substring(with: after))
    }

    private func pick(_ swatch: Theme.Swatch) {
        switch markupTarget {
        case .selection:
            highlightSelection(color: swatch.hex)
        case .highlight(let id):
            Self.lastHighlightColor = swatch.hex
            update(id, patch: AnnotationPatch(color: swatch.hex, comment: nil), actionName: "Change Color")
            markupModel.mode = .highlight(color: swatch.hex)
        case nil:
            break
        }
    }

    private func copySelection() {
        guard case .selection(let selection) = markupTarget, let text = selection.string else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        hideMarkupBar()
    }

    private func removeTargetHighlight() {
        guard case .highlight(let id) = markupTarget else { return }
        hideMarkupBar()
        remove(id, actionName: "Remove Highlight")
    }

    private func selectionChanged() {
        barModel.canHighlight = selectedText != nil
        if case .selection = markupTarget, selectedText == nil { hideMarkupBar() }
    }

    // MARK: Markup toolbar

    private func showMarkupBar(for target: MarkupTarget) {
        markupTarget = target
        switch target {
        case .selection: markupModel.mode = .selection
        case .highlight(let id): markupModel.mode = .highlight(color: annotations[id]?.color ?? "")
        }
        markupBar.alphaValue = 0
        positionMarkupBar()
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Theme.Duration.panel
            markupBar.animator().alphaValue = 1
        }
    }

    /// The toolbar leaves the view rather than hiding, so its buttons get
    /// fresh hover tracking each time it comes back.
    private func hideMarkupBar() {
        markupTarget = nil
        markupBar.removeFromSuperview()
    }

    /// The text the toolbar belongs to, in this view's coordinates.
    private func markupAnchor() -> CGRect? {
        let page: PDFPage?, rect: CGRect
        switch markupTarget {
        case .selection(let selection):
            // Over the part on the first page, where the reading started.
            page = selection.pages.first
            guard let page else { return nil }
            rect = selection.bounds(for: page)
        case .highlight(let id):
            guard let annotation = annotations[id] else { return nil }
            page = pdfView.document?.page(at: Int(annotation.pageIndex))
            rect = AnnotationGeometry.cgRect(annotation.rect)
        case nil:
            return nil
        }
        guard let page else { return nil }
        return view.convert(pdfView.convert(rect, from: page), from: pdfView)
    }

    /// Centers the toolbar 8 pt above its text, or below when there is no
    /// room above, and keeps it inside the reader.
    private func positionMarkupBar() {
        guard markupTarget != nil else { return }
        let visible = view.convert(pdfView.bounds, from: pdfView)
        guard let anchor = markupAnchor(), anchor.intersects(visible) else {
            markupBar.removeFromSuperview()
            return
        }
        // Placed by frame, over the text it belongs to.
        if markupBar.superview == nil { view.addSubview(markupBar) }
        let size = markupBar.fittingSize
        // The hosting view is larger than the card by the shadow's room.
        let inset = Theme.Markup.shadowRadius
        var origin = CGPoint(x: anchor.midX - size.width / 2, y: anchor.maxY + Theme.Size.markupBarOffset - inset)
        if origin.y + size.height - inset > visible.maxY {
            origin.y = anchor.minY - Theme.Size.markupBarOffset - size.height + inset
        }
        origin.x = min(max(origin.x, visible.minX), visible.maxX - size.width)
        origin.y = min(max(origin.y, visible.minY), visible.maxY - size.height)
        markupBar.frame = view.backingAlignedRect(CGRect(origin: origin, size: size), options: .alignAllEdgesNearest)
    }

    // MARK: Find

    var hasFindMatches: Bool { findBar.superview != nil && !matches.isEmpty }

    /// Opens the find bar, or focuses its field again. Opening it with a one
    /// line selection finds that text.
    func showFind() {
        guard pdfView.document != nil else { return }
        if findBar.superview == nil, let text = selectedText?.string?.trimmingCharacters(in: .whitespacesAndNewlines), !text.contains(where: \.isNewline) {
            findModel.query = text
        }
        if findBar.superview == nil {
            // The hosting view includes room for the capsule's shadow.
            let shadow = Theme.Toast.shadowRadius + Theme.Toast.shadowY
            findBar.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(findBar)
            NSLayoutConstraint.activate([
                // Under the controls, so the two never overlap in a narrow window.
                findBar.topAnchor.constraint(equalTo: readerBar.bottomAnchor, constant: Theme.Size.gap - shadow * 2),
                findBar.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: shadow - Theme.Size.readerBarInset),
            ])
            findBar.alphaValue = 0
            NSAnimationContext.runAnimationGroup { context in
                context.duration = Theme.Duration.panel
                findBar.animator().alphaValue = 1
            }
            startFind()
        }
        // SwiftUI moves focus to the field only inside a hosting view that
        // is already the first responder, and once it is in the window.
        view.window?.makeFirstResponder(findBar)
        DispatchQueue.main.async { [weak self] in self?.findModel.focusRequest += 1 }
    }

    /// Closes the find bar and clears the marks. The text stays for next time.
    func closeFind() {
        findTimer?.invalidate()
        pdfView.document?.cancelFindString()
        matches = []
        matchIndex = nil
        pdfView.highlightedSelections = nil
        pdfView.clearSelection()
        // Removed rather than hidden, so its buttons get fresh hover tracking.
        findBar.removeFromSuperview()
        focus()
    }

    func findNext() { step(by: 1) }
    func findPrevious() { step(by: -1) }

    private func step(by offset: Int) {
        guard !matches.isEmpty else { return }
        let start = matchIndex ?? (offset > 0 ? -1 : 0)
        select(match: (start + offset + matches.count) % matches.count)
    }

    private func scheduleFind() {
        findTimer?.invalidate()
        findTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.findDelay, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.startFind() }
        }
    }

    /// Starts a search for the field's text on PDFKit's thread. Matches come
    /// back one at a time through the document's notifications.
    private func startFind() {
        guard let document = pdfView.document else { return }
        document.cancelFindString()
        matches = []
        matchIndex = nil
        pdfView.highlightedSelections = nil
        let query = findModel.query.trimmingCharacters(in: .whitespacesAndNewlines)
        findModel.matchCount = 0
        findModel.current = 0
        findModel.isSearching = !query.isEmpty
        guard !query.isEmpty else { return }
        pendingFinds += 1
        document.beginFindString(query, withOptions: [.caseInsensitive, .diacriticInsensitive])
    }

    private func findBegan() {
        pendingFinds = max(0, pendingFinds - 1)
    }

    /// Counts the match, and goes to the first one at or after the page
    /// being read.
    private func found(_ match: PDFSelection) {
        guard pendingFinds == 0, findBar.superview != nil, let document = pdfView.document else { return }
        match.color = Theme.Color.findMatch
        matches.append(match)
        findModel.matchCount = matches.count
        guard matchIndex == nil, let page = match.pages.first, let current = pdfView.currentPage,
              document.index(for: page) >= document.index(for: current)
        else { return }
        select(match: matches.count - 1)
    }

    private func findEnded() {
        guard pendingFinds == 0, findBar.superview != nil else { return }
        findModel.isSearching = false
        if matchIndex == nil, !matches.isEmpty {
            select(match: 0)
        } else {
            showMatches()
        }
    }

    private func select(match index: Int) {
        matchIndex = index
        findModel.current = index + 1
        pdfView.go(to: matches[index])
        // The current match is selected, with PDFKit's pulse, as in Preview.
        pdfView.setCurrentSelection(matches[index], animate: true)
        showMatches()
    }

    /// Marks every match on the pages.
    private func showMatches() {
        pdfView.highlightedSelections = matches
    }

    // MARK: Notes

    /// Add Note: the next click on a page places a note there. Escape cancels.
    func startPlacingNote() {
        guard pdfView.document != nil else { return }
        hideMarkupBar()
        isPlacingNote = true
        barModel.isPlacingNote = true
        NSCursor.crosshair.set()
    }

    private func stopPlacingNote() {
        isPlacingNote = false
        barModel.isPlacingNote = false
        NSCursor.arrow.set()
    }

    override func cancelOperation(_ sender: Any?) {
        if isPlacingNote {
            stopPlacingNote()
        } else if markupTarget != nil {
            hideMarkupBar()
        } else if findBar.superview != nil {
            closeFind()
        } else {
            super.cancelOperation(sender)
        }
    }

    private func addNote(on page: PDFPage, at point: CGPoint) {
        guard let document = pdfView.document else { return }
        let rect = AnnotationGeometry.noteRect(centeredAt: point, size: Theme.Size.noteIconSize, on: page.bounds(for: pdfView.displayBox))
        guard let note = add(NewAnnotation(
            attachmentId: attachmentId,
            kind: .note,
            pageIndex: Int32(document.index(for: page)),
            quads: [],
            rect: rect,
            color: Theme.Markup.noteColor,
            selectedText: nil,
            comment: nil,
            textBefore: nil,
            textAfter: nil
        )) else { return }
        store.undoManager.setActionName("Add Note")
        editNote(note.id)
    }

    /// Opens the note's text in a popover beside its icon. The text is saved
    /// when the popover closes.
    private func editNote(_ id: String) {
        finishEditingNote()
        guard let note = annotations[id], let icon = drawn[id], let page = icon.page else { return }
        let model = NoteEditorModel(text: note.comment ?? "")
        let popover = NSPopover()
        popover.behavior = .transient
        popover.delegate = self
        popover.contentViewController = NSHostingController(rootView: NoteEditor(model: model))
        model.done = { [weak popover] in popover?.performClose(nil) }
        model.delete = { [weak self, weak popover] in
            self?.remove(id, actionName: "Delete Note")
            popover?.performClose(nil)
        }
        noteEditor = (id, model, popover)
        panel.model.selectedId = id
        popover.show(relativeTo: pdfView.convert(icon.bounds, from: page), of: pdfView, preferredEdge: .maxX)
    }

    func popoverDidClose(_ notification: Notification) {
        if noteEditor?.popover === notification.object as? NSPopover { finishEditingNote() }
    }

    /// Closes the note editor, if one is open, and saves its text.
    private func finishEditingNote() {
        guard let editor = noteEditor else { return }
        noteEditor = nil
        editor.popover.performClose(nil)
        // A deleted note has nothing left to save.
        guard let note = annotations[editor.id], editor.model.text != (note.comment ?? "") else { return }
        update(editor.id, patch: AnnotationPatch(color: nil, comment: editor.model.text), actionName: "Edit Note")
    }

    // MARK: Storing and drawing

    /// Stores a new annotation, draws it, and registers its undo.
    @discardableResult
    private func add(_ new: NewAnnotation) -> Annotation? {
        guard let annotation = store.createAnnotation(new) else { return nil }
        draw(annotation)
        store.undoManager.registerUndo(withTarget: self) { reader in
            reader.remove(annotation.id, actionName: nil)
        }
        return annotation
    }

    private func remove(_ id: String, actionName: String?) {
        guard let annotation = annotations[id], store.deleteAnnotation(id: id) else { return }
        erase(id)
        annotationsChanged()
        store.undoManager.registerUndo(withTarget: self) { reader in
            reader.add(NewAnnotation(
                attachmentId: annotation.attachmentId, kind: annotation.kind, pageIndex: annotation.pageIndex,
                quads: annotation.quads, rect: annotation.rect, color: annotation.color,
                selectedText: annotation.selectedText, comment: annotation.comment, textBefore: nil, textAfter: nil
            ))
        }
        if let actionName { store.undoManager.setActionName(actionName) }
    }

    private func update(_ id: String, patch: AnnotationPatch, actionName: String) {
        guard let before = annotations[id], let after = store.updateAnnotation(id: id, patch: patch) else { return }
        erase(id)
        draw(after)
        store.undoManager.registerUndo(withTarget: self) { reader in
            reader.update(id, patch: AnnotationPatch(color: before.color, comment: before.comment ?? ""), actionName: actionName)
        }
        store.undoManager.setActionName(actionName)
    }

    /// Scrolls the annotation into view just below the floating controls,
    /// from its card in the panel. Like a link, Back returns.
    private func goToAnnotation(_ id: String) {
        guard let annotation = annotations[id], let page = pdfView.document?.page(at: Int(annotation.pageIndex)) else { return }
        panel.model.selectedId = id
        let rect = AnnotationGeometry.cgRect(annotation.rect)
        // View points, converted to page points at the current zoom.
        let clearOfBar = (Theme.Size.readerBarInset * 2 + Theme.Toast.height) / pdfView.scaleFactor
        go(to: PDFDestination(page: page, at: CGPoint(x: kPDFDestinationUnspecifiedValue, y: rect.maxY + clearOfBar)))
    }

    /// The panel lists the annotations in page order, top to bottom on each
    /// page, as they are read.
    private func annotationsChanged() {
        panel.model.annotations = annotations.values.sorted { first, second in
            if first.pageIndex != second.pageIndex { return first.pageIndex < second.pageIndex }
            let (a, b) = (AnnotationGeometry.cgRect(first.rect), AnnotationGeometry.cgRect(second.rect))
            if a.maxY != b.maxY { return a.maxY > b.maxY }
            return a.minX < b.minX
        }
        if let id = panel.model.selectedId, annotations[id] == nil { panel.model.selectedId = nil }
    }

    private func draw(_ annotation: Annotation) {
        annotations[annotation.id] = annotation
        defer { annotationsChanged() }
        guard let page = pdfView.document?.page(at: Int(annotation.pageIndex)) else { return }
        let bounds = AnnotationGeometry.cgRect(annotation.rect)
        let shape: PDFAnnotation
        switch annotation.kind {
        case .highlight:
            shape = PDFAnnotation(bounds: bounds, forType: .highlight, withProperties: nil)
            shape.quadrilateralPoints = AnnotationGeometry.quadrilateralPoints(annotation.quads, relativeTo: bounds)
        case .note:
            shape = PDFAnnotation(bounds: bounds, forType: .text, withProperties: nil)
            shape.iconType = .comment
            shape.contents = annotation.comment
        }
        shape.color = NSColor(hexString: annotation.color) ?? Theme.Markup.swatches[0].color
        shape.userName = Theme.Markup.author
        page.addAnnotation(shape)
        drawn[annotation.id] = shape
        pdfView.annotationsChanged(on: page)
    }

    private func erase(_ id: String) {
        annotations[id] = nil
        guard let shape = drawn.removeValue(forKey: id), let page = shape.page else { return }
        page.removeAnnotation(shape)
        pdfView.annotationsChanged(on: page)
    }
}

/// A context menu item that runs a closure.
private final class MenuAction: NSMenuItem {
    private let run: () -> Void

    init(_ title: String, run: @escaping () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(runAction), keyEquivalent: "")
        target = self
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("init(coder:) is not used") }

    @objc private func runAction() { run() }
}

/// Carries a document from the task that opened it, or a find match from
/// PDFKit's search thread, to the main actor. PDFKit's objects are not
/// `Sendable`, but nothing touches them in between.
struct PDFKitBox<Value>: @unchecked Sendable {
    let value: Value
}
