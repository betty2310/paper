import AppKit
import PDFKit

/// What the reader decides about clicks that PDFKit would otherwise handle
/// on its own: following links, Paper's notes and highlights, and the
/// context menu.
@MainActor
protocol ReaderPDFViewDelegate: AnyObject {
    /// Return true when the reader handled the click and PDFKit must not see it.
    func readerView(_ view: ReaderPDFView, mouseDown event: NSEvent) -> Bool
    /// After PDFKit finished the click or drag.
    func readerView(_ view: ReaderPDFView, mouseUp event: NSEvent)
    /// Items to put above PDFKit's own context menu.
    func readerView(_ view: ReaderPDFView, menuItemsFor event: NSEvent) -> [NSMenuItem]
    /// The mouse's back (-1) or forward (1) button.
    func readerView(_ view: ReaderPDFView, navigate step: Int)
    /// Whether the pointer should be a crosshair for placing a note.
    var isPlacingNote: Bool { get }
}

/// The reader's `PDFView`. It only forwards events; the reader decides.
@MainActor
final class ReaderPDFView: PDFView {
    weak var readerDelegate: ReaderPDFViewDelegate?

    /// A click the reader has not seen the end of yet.
    private var clickPending = false

    override func mouseDown(with event: NSEvent) {
        if readerDelegate?.readerView(self, mouseDown: event) == true { return }
        clickPending = true
        super.mouseDown(with: event)
        // PDFView tracks the click or drag inside mouseDown and usually takes
        // the mouse up itself, so mouseUp below never runs for it.
        if let current = NSApp.currentEvent, current.type == .leftMouseUp { finishClick(current) }
    }

    override func mouseUp(with event: NSEvent) {
        super.mouseUp(with: event)
        finishClick(event)
    }

    private func finishClick(_ event: NSEvent) {
        guard clickPending else { return }
        clickPending = false
        readerDelegate?.readerView(self, mouseUp: event)
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        if readerDelegate?.isPlacingNote == true { NSCursor.crosshair.set() }
    }

    override func otherMouseUp(with event: NSEvent) {
        // Buttons 3 and 4 are the side buttons most mice label back and forward.
        switch event.buttonNumber {
        case 3: readerDelegate?.readerView(self, navigate: -1)
        case 4: readerDelegate?.readerView(self, navigate: 1)
        default: super.otherMouseUp(with: event)
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = super.menu(for: event) ?? NSMenu()
        let items = readerDelegate?.readerView(self, menuItemsFor: event) ?? []
        guard !items.isEmpty else { return menu }
        if !menu.items.isEmpty { menu.insertItem(.separator(), at: 0) }
        for item in items.reversed() { menu.insertItem(item, at: 0) }
        return menu
    }
}
