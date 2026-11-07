import AppKit

/// The tabs in the toolbar, right after the window title: a quiet track
/// holding the Library icon, then one tab per open PDF, with the active tab
/// raised out of it. Out of the toolbar while no PDF is open.
///
/// The strip is flexible: it takes the room a flexible space would, up to the
/// buttons on its right, and the PDF tabs share that room equally, like
/// Safari's. Tabs are laid out by frame so they can grow in and shrink out
/// with animation. Crowded tabs shrink to their icon, so the toolbar's own
/// buttons keep their place; only past that does the toolbar overflow.
/// Pinned tabs come right after the Library icon, as narrow as it, and keep
/// that width however many tabs are open.
@MainActor
final class TabStripView: NSView {
    struct Tab: Equatable {
        static let libraryId = "library"
        let id: String
        let title: String
        var isPinned = false
        var isLibrary: Bool { id == Self.libraryId }
    }

    var onSelect: (String) -> Void = { _ in }
    var onClose: (String) -> Void = { _ in }
    var onTogglePin: (String) -> Void = { _ in }

    private(set) var tabs: [Tab] = []
    private(set) var selectedId = Tab.libraryId
    private var buttons: [String: TabButton] = [:]

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override func updateLayer() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer?.backgroundColor = Theme.Color.tabTrack.cgColor
        }
    }

    /// A toolbar item whose view has a width range instead of one width is
    /// flexible: the toolbar gives it the room between its neighbors.
    private lazy var minimumWidthConstraint = widthAnchor.constraint(greaterThanOrEqualToConstant: 0)

    init() {
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = Theme.Size.tabTrackHeight / 2
        layer?.cornerCurve = .continuous
        setContentHuggingPriority(.init(1), for: .horizontal)
        NSLayoutConstraint.activate([
            minimumWidthConstraint,
            widthAnchor.constraint(lessThanOrEqualToConstant: Self.fillWidth),
            heightAnchor.constraint(equalToConstant: Theme.Size.tabTrackHeight),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// The window's title bar handles right clicks in the toolbar itself, so
    /// they never reach a tab; this sends a tab's right click to the tab.
    private var rightClickMonitor: Any?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let rightClickMonitor { NSEvent.removeMonitor(rightClickMonitor) }
        rightClickMonitor = nil
        guard window != nil else { return }
        rightClickMonitor = NSEvent.addLocalMonitorForEvents(matching: .rightMouseDown) { [weak self] event in
            // Local monitors run on the main thread.
            nonisolated(unsafe) let event = event
            let handled = MainActor.assumeIsolated { self?.showMenu(for: event) ?? false }
            return handled ? nil : event
        }
    }

    /// Shows the menu of the tab under a right click, if there is one.
    private func showMenu(for event: NSEvent) -> Bool {
        guard event.window === window,
              let button = buttons.values.first(where: { $0.bounds.contains($0.convert(event.locationInWindow, from: nil)) }),
              !button.tab.isLibrary
        else { return false }
        button.showMenu(for: event)
        return true
    }

    private static let fillWidth: CGFloat = 10_000

    private var isAnimating = false

    override func layout() {
        super.layout()
        guard !isAnimating else { return }
        let frames = targetFrames()
        for (id, button) in buttons {
            if let frame = frames[id] { button.frame = frame }
        }
    }

    /// Replaces the tabs. A tab in `landing` is laid out but stays invisible
    /// until `land(_:)`, so a flight animation can arrive in its place; other
    /// new tabs grow in, and removed tabs shrink out.
    func update(tabs newTabs: [Tab], selectedId: String, landing: String? = nil) {
        let removed = tabs.filter { old in !newTabs.contains { $0.id == old.id } }
        tabs = newTabs
        self.selectedId = selectedId
        for tab in newTabs where buttons[tab.id] == nil {
            let button = TabButton(tab: tab)
            button.onSelect = { [weak self] in self?.onSelect(tab.id) }
            button.onClose = { [weak self] in self?.onClose(tab.id) }
            button.onTogglePin = { [weak self] in self?.onTogglePin(tab.id) }
            button.isLanding = tab.id == landing
            buttons[tab.id] = button
            addSubview(button)
            // Grow in from a sliver at the tab's final position.
            let target = targetFrames()[tab.id] ?? .zero
            button.frame = NSRect(x: target.minX, y: target.minY, width: tab.id == landing ? target.width : 0, height: target.height)
            button.alphaValue = tab.id == landing ? 1 : 0
        }
        for tab in newTabs {
            buttons[tab.id]?.tab = tab
            buttons[tab.id]?.isSelected = tab.id == selectedId
        }
        minimumWidthConstraint.constant = minimumWidth()

        let frames = targetFrames()
        isAnimating = true
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Theme.Duration.tabResize
            context.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            context.allowsImplicitAnimation = true
            for tab in newTabs {
                guard let button = buttons[tab.id], let frame = frames[tab.id] else { continue }
                button.animator().frame = frame
                if !button.isLanding { button.animator().alphaValue = 1 }
            }
            for tab in removed {
                guard let button = buttons.removeValue(forKey: tab.id) else { continue }
                button.animator().alphaValue = 0
                button.animator().frame = NSRect(x: button.frame.minX, y: button.frame.minY, width: 0, height: button.frame.height)
            }
        } completionHandler: { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                self.isAnimating = false
                self.needsLayout = true
                for subview in self.subviews where !self.buttons.values.contains(where: { $0 === subview }) {
                    subview.removeFromSuperview()
                }
            }
        }
    }

    /// The tab's button, for a flight animation to aim at.
    func button(for id: String) -> TabButton? {
        buttons[id]
    }

    /// Shows a tab that a flight animation has just reached.
    func land(_ id: String) {
        buttons[id]?.isLanding = false
    }

    private func minimumWidth() -> CGFloat {
        guard !tabs.isEmpty else { return 0 }
        let pinnedCount = CGFloat(tabs.filter(\.isPinned).count)
        let pdfCount = CGFloat(tabs.filter { !$0.isLibrary && !$0.isPinned }.count)
        return Theme.Size.tabTrackInset * 2 + TabButton.libraryWidth
            + pinnedCount * (Theme.Size.tabSpacing + TabButton.pinnedWidth)
            + pdfCount * (Theme.Size.tabSpacing + Theme.Size.tabMinWidth)
    }

    /// The Library icon first, then the pinned tabs as narrow as it, then the
    /// other PDFs in equal shares of the rest. Edges are rounded from the
    /// exact shares, so the tabs end flush with the strip and no tab is more
    /// than a point wider than another.
    private func targetFrames() -> [String: NSRect] {
        let pdfTabs = tabs.filter { !$0.isLibrary && !$0.isPinned }
        let pinnedCount = CGFloat(tabs.filter(\.isPinned).count)
        let inset = Theme.Size.tabTrackInset
        let start = inset + TabButton.libraryWidth + Theme.Size.tabSpacing
            + pinnedCount * (TabButton.pinnedWidth + Theme.Size.tabSpacing)
        let room = bounds.width - inset - start - Theme.Size.tabSpacing * CGFloat(max(0, pdfTabs.count - 1))
        let share = max(Theme.Size.tabMinWidth, room / CGFloat(max(1, pdfTabs.count)))
        var frames: [String: NSRect] = [:]
        var index = 0
        var pinnedX = inset + TabButton.libraryWidth + Theme.Size.tabSpacing
        for tab in tabs {
            if tab.isLibrary {
                frames[tab.id] = NSRect(x: inset, y: inset, width: TabButton.libraryWidth, height: Theme.Size.tabHeight)
                continue
            }
            if tab.isPinned {
                frames[tab.id] = NSRect(x: pinnedX, y: inset, width: TabButton.pinnedWidth, height: Theme.Size.tabHeight)
                pinnedX += TabButton.pinnedWidth + Theme.Size.tabSpacing
                continue
            }
            let offset = start + CGFloat(index) * Theme.Size.tabSpacing
            let minX = round(offset + CGFloat(index) * share)
            let maxX = round(offset + CGFloat(index + 1) * share)
            frames[tab.id] = NSRect(x: minX, y: inset, width: maxX - minX, height: Theme.Size.tabHeight)
            index += 1
        }
        return frames
    }
}

/// One tab: an icon, the title, and a close button for PDF tabs. A pinned
/// tab is only the paper's letter tile, with no close button. Hover and
/// pressed tabs get a quiet fill; the selected tab is raised out of the
/// track; with full keyboard access a focused tab gets the accent ring.
@MainActor
final class TabButton: NSView {
    var tab: TabStripView.Tab {
        didSet {
            guard tab != oldValue else { return }
            configure()
            needsLayout = true
        }
    }
    var onSelect: () -> Void = {}
    var onClose: () -> Void = {}
    var onTogglePin: () -> Void = {}

    var isSelected = false { didSet { updateState() } }
    /// While a flight animation is on its way, the tab is an empty slot:
    /// no fill, and its icon and title wait for `revealContent`.
    var isLanding = false {
        didSet {
            guard isLanding != oldValue else { return }
            if isLanding {
                content.alphaValue = 0
            } else {
                revealContent()
            }
            needsDisplay = true
        }
    }

    /// Fades the icon and title in.
    func revealContent() {
        guard content.alphaValue < 1 else { return }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = Theme.Duration.documentFade
            content.animator().alphaValue = 1
        }
    }

    private let content = NSView()
    private let icon = NSImageView()
    private let title = NSTextField(labelWithString: "")
    private let closeButton = NSButton()
    private var isHovered = false { didSet { updateState() } }
    private var isPressed = false { didSet { needsDisplay = true } }

    /// The title's frame in this view, where a flight puts the flying title.
    var titleFrame: NSRect { title.frame.offsetBy(dx: content.frame.minX, dy: content.frame.minY) }
    var titleFont: NSFont { title.font ?? Theme.Font.tabTitle }

    /// The Library tab is just its icon, and so is a pinned tab.
    static let libraryWidth = Theme.Size.tabPadding * 2 + Theme.Size.iconSize
    static let pinnedWidth = libraryWidth

    init(tab: TabStripView.Tab) {
        self.tab = tab
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = Theme.Size.tabHeight / 2
        layer?.cornerCurve = .continuous

        icon.symbolConfiguration = .init(pointSize: NSFont.systemFontSize, weight: .regular)
        title.font = Theme.Font.tabTitle
        title.lineBreakMode = .byTruncatingTail
        title.maximumNumberOfLines = 1
        title.cell?.truncatesLastVisibleLine = true
        closeButton.image = NSImage(systemSymbolName: Theme.Symbol.close, accessibilityDescription: "Close Tab")
        closeButton.symbolConfiguration = .init(pointSize: NSFont.smallSystemFontSize - 1, weight: .semibold)
        closeButton.isBordered = false
        closeButton.bezelStyle = .accessoryBarAction
        closeButton.target = self
        closeButton.action = #selector(closeClicked)
        closeButton.toolTip = "Close Tab (⌘W)"

        content.autoresizingMask = [.width, .height]
        addSubview(content)
        for subview in [icon, title, closeButton] as [NSView] {
            content.addSubview(subview)
        }
        focusRingType = .exterior
        setAccessibilityElement(true)
        setAccessibilityRole(.radioButton)
        configure()
    }

    /// The icon, title, and labels for the tab as it is now.
    private func configure() {
        if tab.isPinned {
            icon.image = Self.letterTile(for: tab.title)
        } else {
            icon.image = NSImage(
                systemSymbolName: tab.isLibrary ? Theme.Symbol.allPapers : Theme.Symbol.paper,
                accessibilityDescription: nil
            )
        }
        title.stringValue = tab.title
        toolTip = tab.isLibrary ? "Library" : tab.title
        setAccessibilityLabel(tab.isLibrary ? "Library" : tab.isPinned ? "\(tab.title), pinned" : tab.title)
        updateState()
    }

    /// The paper's first letter on its preview color, the same tile the
    /// list shows when a paper has no figure, so pinned papers tell apart.
    private static func letterTile(for title: String) -> NSImage {
        let tile = PaperPreviews.tile(for: title)
        let letter = PaperPreviews.letter(of: title) as NSString
        let size = NSSize(width: Theme.Size.iconSize, height: Theme.Size.iconSize)
        return NSImage(size: size, flipped: false) { rect in
            tile.background.setFill()
            NSBezierPath(roundedRect: rect, xRadius: Theme.Size.tabPinnedRadius, yRadius: Theme.Size.tabPinnedRadius).fill()
            let attributes: [NSAttributedString.Key: Any] = [
                .font: Theme.Font.tabPinnedLetter,
                .foregroundColor: tile.letter,
            ]
            let text = letter.size(withAttributes: attributes)
            letter.draw(at: NSPoint(x: rect.midX - text.width / 2, y: rect.midY - text.height / 2), withAttributes: attributes)
            return true
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsLayout = true
    }

    /// Too narrow for a title: the icon sits in the middle, and the close
    /// button takes its place while the pointer is on the tab.
    private var isCompact: Bool { !tab.isLibrary && bounds.width < Theme.Size.tabCompactWidth }
    private var isIconOnly: Bool { tab.isLibrary || tab.isPinned }

    override func layout() {
        super.layout()
        content.frame = bounds
        let height = bounds.height
        let iconSize = Theme.Size.iconSize
        let close = Theme.Size.tabCloseSize
        if isCompact || isIconOnly {
            icon.frame = NSRect(x: round((bounds.width - iconSize) / 2), y: (height - iconSize) / 2, width: iconSize, height: iconSize)
            closeButton.frame = NSRect(x: round((bounds.width - close) / 2), y: (height - close) / 2, width: close, height: close)
        } else {
            icon.frame = NSRect(x: Theme.Size.tabPadding, y: (height - iconSize) / 2, width: iconSize, height: iconSize)
            closeButton.frame = NSRect(x: bounds.width - Theme.Size.tabPadding / 2 - close - 2, y: (height - close) / 2, width: close, height: close)
        }
        title.isHidden = isIconOnly || isCompact
        updateState()
        let titleX = icon.frame.maxX + Theme.Size.smallGap
        let titleEnd = closeButton.frame.minX - Theme.Size.smallGap
        let titleHeight = ceil(title.intrinsicContentSize.height)
        title.frame = NSRect(x: titleX, y: floor((height - titleHeight) / 2), width: max(0, titleEnd - titleX), height: titleHeight)
    }

    override func updateLayer() {
        let raised = isSelected && !isLanding
        let fill: NSColor
        if isLanding {
            fill = .clear
        } else if isSelected {
            fill = selectedFill
        } else if isPressed {
            fill = Theme.Color.tabPressed
        } else if isHovered {
            fill = Theme.Color.tabHover
        } else {
            fill = .clear
        }
        guard let layer else { return }
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer.backgroundColor = fill.cgColor
            layer.borderColor = Theme.Color.separator.cgColor
        }
        layer.borderWidth = raised ? 1 / (window?.backingScaleFactor ?? 2) : 0
        layer.shadowColor = NSColor.black.cgColor
        layer.shadowOpacity = raised ? Theme.Tab.shadowOpacity : 0
        layer.shadowRadius = Theme.Tab.shadowRadius
        layer.shadowOffset = CGSize(width: 0, height: -Theme.Tab.shadowY)
    }

    /// The fill a landed, selected tab has, for the flight to end on.
    var selectedFill: NSColor { Theme.Color.tabSelected }

    private func updateState() {
        // The letter tile keeps its own colors.
        icon.contentTintColor = tab.isPinned ? nil : isSelected ? Theme.Color.accent : Theme.Color.secondaryText
        title.textColor = isSelected ? Theme.Color.text : Theme.Color.secondaryText
        closeButton.contentTintColor = Theme.Color.secondaryText
        closeButton.isHidden = isIconOnly || !(isCompact ? isHovered : isSelected || isHovered)
        icon.isHidden = !isIconOnly && isCompact && isHovered
        setAccessibilityValue(isSelected)
        needsDisplay = true
    }

    // MARK: Mouse

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect], owner: self))
    }

    override func mouseEntered(with event: NSEvent) { isHovered = true }
    override func mouseExited(with event: NSEvent) { isHovered = false }

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.control) {
            showMenu(for: event)
            return
        }
        isPressed = true
    }

    override func mouseUp(with event: NSEvent) {
        isPressed = false
        if bounds.contains(convert(event.locationInWindow, from: nil)) { onSelect() }
    }

    /// A middle click closes a PDF tab, as in Safari; pinned tabs stay.
    override func otherMouseUp(with event: NSEvent) {
        if event.buttonNumber == 2, !isIconOnly { onClose() }
    }

    func showMenu(for event: NSEvent) {
        guard let menu = menu(for: event) else { return }
        NSMenu.popUpContextMenu(menu, with: event, for: self)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard !tab.isLibrary else { return nil }
        let menu = NSMenu()
        let pin = menu.addItem(withTitle: tab.isPinned ? "Unpin Tab" : "Pin Tab", action: #selector(pinClicked), keyEquivalent: "")
        pin.target = self
        menu.addItem(.separator())
        let close = menu.addItem(withTitle: "Close Tab", action: #selector(closeClicked), keyEquivalent: "")
        close.target = self
        return menu
    }

    @objc private func pinClicked() {
        onTogglePin()
    }

    // MARK: Keyboard

    /// Tabs join the key view loop only with full keyboard access, like the
    /// system's segmented controls.
    override var acceptsFirstResponder: Bool { NSApp.isFullKeyboardAccessEnabled }
    override var canBecomeKeyView: Bool { NSApp.isFullKeyboardAccessEnabled }

    override func keyDown(with event: NSEvent) {
        switch event.charactersIgnoringModifiers {
        case " ", "\r": onSelect()
        default: super.keyDown(with: event)
        }
    }

    /// The ring is for keyboard focus only, not for focus a click gave.
    override func becomeFirstResponder() -> Bool {
        focusRingType = NSApp.isMouseEvent ? .none : .exterior
        return true
    }

    override var focusRingMaskBounds: NSRect { bounds }

    override func drawFocusRingMask() {
        NSBezierPath(roundedRect: bounds, xRadius: bounds.height / 2, yRadius: bounds.height / 2).fill()
    }

    override func accessibilityPerformPress() -> Bool {
        onSelect()
        return true
    }

    @objc private func closeClicked() {
        onClose()
    }
}
