import AppKit
import Combine
import PDFKit
import PaperEngine

/// One open PDF in its own tab: a `PDFView` in continuous single page mode.
///
/// The document is opened on a background task and handed to the main actor,
/// then the pages fade in. The page and zoom are written back to the engine a
/// moment after they stop changing, so the next open lands in the same place.
/// The file itself is only read; PDFKit never writes it back.
@MainActor
final class ReaderViewController: NSViewController {
    let store: LibraryStore
    let itemId: String
    let attachmentId: String
    /// The paper's title, shown in the tab.
    let paperTitle: String


    private let file: AttachmentFile
    private let pdfView = PDFView()
    private let failureLabel = NSTextField(labelWithString: "This PDF could not be opened.")
    private let pageIndicator = PageIndicator()
    private var saveTimer: Timer?
    private var observers: [NSObjectProtocol] = []

    init(store: LibraryStore, itemId: String, file: AttachmentFile, paperTitle: String) {
        self.store = store
        self.itemId = itemId
        self.file = file
        self.attachmentId = file.id
        self.paperTitle = paperTitle
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

        for subview in [pdfView, failureLabel, pageIndicator] as [NSView] {
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
            pageIndicator.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            pageIndicator.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -Theme.Size.pageIndicatorInset),
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
            DocumentBox(document: PDFDocument(url: url))
        }.value
        show(opened.document)
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
        if let zoom = file.lastZoom, zoom > 0 {
            pdfView.autoScales = false
            pdfView.scaleFactor = CGFloat(zoom)
        }
        observers = [
            NotificationCenter.default.addObserver(forName: .PDFViewPageChanged, object: pdfView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pageChanged() }
            },
            NotificationCenter.default.addObserver(forName: .PDFViewScaleChanged, object: pdfView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pageChanged() }
            },
        ]
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
        scheduleSave()
    }

    var canZoomIn: Bool { pdfView.document != nil && pdfView.scaleFactor < Theme.Size.maxZoom }
    var canZoomOut: Bool { pdfView.document != nil && pdfView.scaleFactor > Theme.Size.minZoom }

    private func setZoom(_ scale: CGFloat) {
        guard pdfView.document != nil else { return }
        pdfView.autoScales = false
        pdfView.scaleFactor = min(max(scale, Theme.Size.minZoom), Theme.Size.maxZoom)
        scheduleSave()
    }

    // MARK: Reading position

    private func pageChanged() {
        updatePageLabel()
        scheduleSave()
    }

    /// Shows "3 of 15" over the pages for a moment.
    private func updatePageLabel() {
        guard let document = pdfView.document, let page = pdfView.currentPage else { return }
        pageIndicator.show("\(document.index(for: page) + 1) of \(document.pageCount)")
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
        savePosition()
        observers.forEach(NotificationCenter.default.removeObserver)
        observers = []
    }
}

/// "3 of 15" in a small rounded HUD that fades in while the page changes and
/// fades out once it stops, like Preview's.
@MainActor
private final class PageIndicator: NSVisualEffectView {
    private let label = NSTextField(labelWithString: "")
    private var hideTimer: Timer?

    init() {
        super.init(frame: .zero)
        material = .hudWindow
        blendingMode = .withinWindow
        state = .active
        wantsLayer = true
        layer?.cornerRadius = Theme.Size.pageIndicatorHeight / 2
        layer?.cornerCurve = .continuous
        alphaValue = 0
        label.font = Theme.Font.pageIndicator
        label.textColor = Theme.Color.secondaryText
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            heightAnchor.constraint(equalToConstant: Theme.Size.pageIndicatorHeight),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Theme.Size.pageIndicatorPadding),
            label.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Theme.Size.pageIndicatorPadding),
        ])
        setAccessibilityElement(true)
        setAccessibilityRole(.staticText)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func show(_ text: String) {
        label.stringValue = text
        setAccessibilityValue(text)
        hideTimer?.invalidate()
        if alphaValue < 1 {
            NSAnimationContext.runAnimationGroup { context in
                context.duration = Theme.Duration.documentFade
                animator().alphaValue = 1
            }
        }
        hideTimer = Timer.scheduledTimer(withTimeInterval: Theme.Duration.pageIndicator, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                NSAnimationContext.runAnimationGroup { context in
                    context.duration = Theme.Duration.documentFade
                    self?.animator().alphaValue = 0
                }
            }
        }
    }
}

/// Carries a document from the task that opened it to the main actor.
/// `PDFDocument` is not `Sendable`, but nothing touches it in between.
private struct DocumentBox: @unchecked Sendable {
    let document: PDFDocument?
}
