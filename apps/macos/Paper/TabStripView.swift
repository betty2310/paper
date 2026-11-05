import AppKit

/// The tabs in the toolbar, right of the window title: Library, then one tab
/// per open PDF. Hidden (and zero wide) while no PDF is open.
///
/// Tabs are laid out by frame so they can grow in and shrink out with
/// animation; the strip's own width follows at once, which nothing else in
/// the toolbar notices because a flexible space comes after it. When the
/// toolbar gives the strip less than the tabs want, the PDF tabs share what
/// there is, down to a minimum width.
@MainActor
final class TabStripView: NSView {
    struct Tab: Equatable {
        static let libraryId = "library"
        let id: String
        let title: String
        var isLibrary: Bool { id == Self.libraryId }
    }

    var onSelect: (String) -> Void = { _ in }
    var onClose: (String) -> Void = { _ in }

    private(set) var tabs: [Tab] = []
    private(set) var selectedId = Tab.libraryId
    private var buttons: [String: TabButton] = [:]

    override var isFlipped: Bool { true }

    override var intrinsicContentSize: NSSize {
        NSSize(width: naturalWidth(), height: Theme.Size.tabHeight)
    }

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
            button.isLanding = tab.id == landing
            buttons[tab.id] = button
            addSubview(button)
            // Grow in from a sliver at the tab's final position.
            let target = targetFrames()[tab.id] ?? .zero
            button.frame = NSRect(x: target.minX, y: target.minY, width: tab.id == landing ? target.width : 0, height: target.height)
            button.alphaValue = tab.id == landing ? 1 : 0
        }
        for tab in newTabs { buttons[tab.id]?.isSelected = tab.id == selectedId }
        invalidateIntrinsicContentSize()

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

    private func naturalWidth() -> CGFloat {
        guard !tabs.isEmpty else { return 0 }
        return tabs.map(TabButton.width(for:)).reduce(0, +) + Theme.Size.tabSpacing * CGFloat(tabs.count - 1)
    }

    /// Library first, then the PDFs; each as wide as its title needs, within
    /// limits, and narrower when the strip is.
    private func targetFrames() -> [String: NSRect] {
        let natural = naturalWidth()
        let available = bounds.width
        let pdfTabs = tabs.filter { !$0.isLibrary }
        // How much each PDF tab gives up so they all fit, never below the minimum.
        var shrink: CGFloat = 0
        if available > 0, natural > available, !pdfTabs.isEmpty {
            shrink = (natural - available) / CGFloat(pdfTabs.count)
        }
        var frames: [String: NSRect] = [:]
        var x: CGFloat = 0
        for tab in tabs {
            let natural = TabButton.width(for: tab)
            let width = tab.isLibrary ? natural : max(Theme.Size.tabMinWidth, natural - shrink)
            frames[tab.id] = NSRect(x: x, y: 0, width: floor(width), height: Theme.Size.tabHeight)
            x += floor(width) + Theme.Size.tabSpacing
        }
        return frames
    }
}

/// One tab: an icon, the title, and a close button for PDF tabs. Hover,
/// pressed, and selected states each have their own fill.
@MainActor
final class TabButton: NSView {
    let tab: TabStripView.Tab
    var onSelect: () -> Void = {}
    var onClose: () -> Void = {}

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

    /// The Library tab is just its icon; a PDF tab fits its title, within limits.
    static func width(for tab: TabStripView.Tab) -> CGFloat {
        if tab.isLibrary { return Theme.Size.tabPadding * 2 + Theme.Size.iconSize }
        let measure = NSTextField(labelWithString: tab.title)
        measure.font = Theme.Font.tabTitle
        let text = ceil(measure.intrinsicContentSize.width)
        let natural = Theme.Size.tabPadding * 2 + Theme.Size.iconSize + Theme.Size.smallGap + text
            + Theme.Size.smallGap + Theme.Size.tabCloseSize
        return min(max(natural, Theme.Size.tabMinWidth), Theme.Size.tabMaxWidth)
    }

    init(tab: TabStripView.Tab) {
        self.tab = tab
        super.init(frame: .zero)
        wantsLayer = true
        layer?.cornerRadius = Theme.Size.tabHeight / 2
        layer?.cornerCurve = .continuous

        icon.image = NSImage(
            systemSymbolName: tab.isLibrary ? Theme.Symbol.allPapers : Theme.Symbol.paper,
            accessibilityDescription: nil
        )
        icon.symbolConfiguration = .init(pointSize: NSFont.systemFontSize, weight: .regular)
        title.stringValue = tab.title
        title.font = Theme.Font.tabTitle
        title.isHidden = tab.isLibrary
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
        closeButton.isHidden = tab.isLibrary

        content.autoresizingMask = [.width, .height]
        addSubview(content)
        for subview in [icon, title, closeButton] as [NSView] {
            content.addSubview(subview)
        }
        toolTip = tab.isLibrary ? "Library" : tab.title
        setAccessibilityElement(true)
        setAccessibilityRole(.radioButton)
        setAccessibilityLabel(tab.isLibrary ? "Library" : tab.title)
        updateState()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        needsLayout = true
    }

    override func layout() {
        super.layout()
        content.frame = bounds
        let height = bounds.height
        let iconSize = Theme.Size.iconSize
        icon.frame = NSRect(x: Theme.Size.tabPadding, y: (height - iconSize) / 2, width: iconSize, height: iconSize)
        let close = Theme.Size.tabCloseSize
        closeButton.frame = NSRect(x: bounds.width - Theme.Size.tabPadding / 2 - close - 2, y: (height - close) / 2, width: close, height: close)
        let titleX = icon.frame.maxX + Theme.Size.smallGap
        let titleEnd = closeButton.frame.minX - Theme.Size.smallGap
        let titleHeight = ceil(title.intrinsicContentSize.height)
        title.frame = NSRect(x: titleX, y: floor((height - titleHeight) / 2), width: max(0, titleEnd - titleX), height: titleHeight)
    }

    override func updateLayer() {
        let fill: NSColor
        if isLanding {
            fill = .clear
        } else if isPressed {
            fill = Theme.Color.tabPressed
        } else if isSelected {
            fill = Theme.Color.accent.withAlphaComponent(Theme.Opacity.tint)
        } else if isHovered {
            fill = Theme.Color.tabHover
        } else {
            fill = .clear
        }
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer?.backgroundColor = fill.cgColor
        }
    }

    /// The fill a landed, selected tab has, for the flight to end on.
    var selectedFill: NSColor { Theme.Color.accent.withAlphaComponent(Theme.Opacity.tint) }

    private func updateState() {
        icon.contentTintColor = isSelected ? Theme.Color.accent : Theme.Color.secondaryText
        title.textColor = isSelected ? Theme.Color.text : Theme.Color.secondaryText
        closeButton.contentTintColor = Theme.Color.secondaryText
        closeButton.isHidden = tab.isLibrary || !(isSelected || isHovered)
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
        isPressed = true
    }

    override func mouseUp(with event: NSEvent) {
        isPressed = false
        if bounds.contains(convert(event.locationInWindow, from: nil)) { onSelect() }
    }

    /// A middle click closes a PDF tab, as in Safari.
    override func otherMouseUp(with event: NSEvent) {
        if event.buttonNumber == 2, !tab.isLibrary { onClose() }
    }

    override func accessibilityPerformPress() -> Bool {
        onSelect()
        return true
    }

    @objc private func closeClicked() {
        onClose()
    }
}
