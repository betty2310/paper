import AppKit
import QuartzCore

/// Carries a paper from its row in the list into its new tab.
///
/// A picture of the selected row lifts off and turns into a capsule that
/// carries the paper's title. The capsule shrinks and glides along a curve
/// into the tab's place in the toolbar, where the real tab takes over. The
/// layers live in a borderless child window over the library window, so they
/// can cross from the list into the toolbar.
@MainActor
enum TabFlight {
    /// Rises first, then glides: how far along the way the curve's control
    /// point sits horizontally.
    private static let curveLead: CGFloat = 0.25
    /// When the row picture has faded into the capsule.
    private static let liftEnd: NSNumber = 0.35
    /// When the tab's own title starts to show through.
    private static let landStart: Double = 0.65
    private static let liftShadowOpacity: Float = 0.18
    private static let liftShadowRadius: CGFloat = 12
    private static let timing = CAMediaTimingFunction(controlPoints: 0.3, 0.7, 0.1, 1)

    /// The row as it looked when the user opened it, in screen coordinates.
    /// Taken before the list fades out, because the flight starts a moment
    /// later, once the toolbar has laid out the new tab.
    struct Source {
        let rowImage: CGImage
        let rowRect: NSRect
        let titleImage: CGImage
        let titleRect: NSRect
        let titleFont: NSFont
    }

    static func capture(row: NSView, title: NSView, titleFont: NSFont) -> Source? {
        // The row's picture leaves the title out, so the title that flies on
        // its own is the only one on screen.
        guard let window = row.window, let titleImage = snapshot(title) else { return nil }
        title.isHidden = true
        let rowImage = snapshot(row)
        title.isHidden = false
        guard let rowImage else { return nil }
        return Source(
            rowImage: rowImage,
            rowRect: window.convertToScreen(row.convert(row.bounds, to: nil)),
            titleImage: titleImage,
            titleRect: window.convertToScreen(title.convert(title.bounds, to: nil)),
            titleFont: titleFont
        )
    }

    /// Flies `source` into `tab` over `window`, then calls `completion`.
    static func fly(_ source: Source, to tab: TabButton, over window: NSWindow, completion: @escaping () -> Void) {
        guard let tabWindow = tab.window, !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else {
            completion()
            return
        }

        let overlay = NSWindow(contentRect: window.frame, styleMask: .borderless, backing: .buffered, defer: false)
        overlay.isOpaque = false
        overlay.backgroundColor = .clear
        overlay.hasShadow = false
        overlay.ignoresMouseEvents = true
        overlay.isReleasedWhenClosed = false
        overlay.animationBehavior = .none
        let canvas = NSView(frame: NSRect(origin: .zero, size: window.frame.size))
        canvas.wantsLayer = true
        overlay.contentView = canvas
        overlay.level = window.level
        window.addChildWindow(overlay, ordered: .above)
        overlay.orderFront(nil)
        guard let root = canvas.layer else {
            completion()
            return
        }
        let scale = window.backingScaleFactor

        // Every rect in the overlay's coordinates (same frame as the window).
        func local(_ screen: NSRect) -> NSRect {
            screen.offsetBy(dx: -overlay.frame.minX, dy: -overlay.frame.minY)
        }
        func screen(_ view: NSView, _ rect: NSRect) -> NSRect {
            tabWindow.convertToScreen(view.convert(rect, to: nil))
        }
        let rowRect = local(source.rowRect)
        let tabRect = local(screen(tab, tab.bounds))
        let titleStart = local(source.titleRect)
        let tabTitle = local(screen(tab, tab.titleFrame))
        let rowImage = source.rowImage
        let titleImage = source.titleImage
        // The title shrinks from the list's font to the tab's.
        let fontScale = tab.titleFont.pointSize / source.titleFont.pointSize
        let titleEndSize = NSSize(
            width: min(titleStart.width * fontScale, tabTitle.width),
            height: titleStart.height * fontScale
        )
        let titleEnd = NSRect(x: tabTitle.minX, y: tabTitle.midY - titleEndSize.height / 2, width: titleEndSize.width, height: titleEndSize.height)

        var rowFill = NSColor.selectedContentBackgroundColor
        var tabFill = tab.selectedFill
        window.contentView?.effectiveAppearance.performAsCurrentDrawingAppearance {
            rowFill = NSColor(cgColor: NSColor.selectedContentBackgroundColor.cgColor) ?? rowFill
            tabFill = NSColor(cgColor: tab.selectedFill.cgColor) ?? tabFill
        }

        let capsule = CALayer()
        capsule.backgroundColor = tabFill.cgColor
        capsule.cornerRadius = Theme.Size.tabHeight / 2
        capsule.cornerCurve = .continuous
        capsule.shadowColor = NSColor.black.cgColor
        capsule.shadowOffset = CGSize(width: 0, height: -2)
        capsule.shadowRadius = liftShadowRadius

        let picture = CALayer()
        picture.contents = rowImage
        picture.contentsScale = scale
        picture.contentsGravity = .resize
        picture.opacity = 0

        let label = CALayer()
        label.contents = titleImage
        label.contentsScale = scale
        label.contentsGravity = .resize
        label.opacity = 0

        for layer in [capsule, picture, label] {
            root.addSublayer(layer)
        }

        let duration = Theme.Duration.tabFlight
        CATransaction.begin()
        CATransaction.setCompletionBlock {
            MainActor.assumeIsolated {
                window.removeChildWindow(overlay)
                overlay.orderOut(nil)
                completion()
            }
        }

        move(capsule, from: rowRect, to: tabRect, duration: duration)
        move(picture, from: rowRect, to: tabRect, duration: duration)
        move(label, from: titleStart, to: titleEnd, duration: duration)
        // A title too long for the tab loses its end rather than being squeezed.
        animate(label, "contentsRect", from: NSValue(rect: NSRect(x: 0, y: 0, width: 1, height: 1)),
                to: NSValue(rect: NSRect(x: 0, y: 0, width: min(1, tabTitle.width / (titleStart.width * fontScale)), height: 1)),
                duration: duration)

        animate(capsule, "backgroundColor", from: rowFill.cgColor, to: tabFill.cgColor, duration: duration)
        animate(capsule, "cornerRadius", from: Theme.Size.cardRadius as NSNumber, to: Theme.Size.tabHeight / 2 as NSNumber, duration: duration)
        let shadow = NSNumber(value: liftShadowOpacity)
        keyframes(capsule, "shadowOpacity", values: [0, shadow, shadow, 0], times: [0, liftEnd, 0.7, 1], duration: duration)
        keyframes(picture, "opacity", values: [1, 0, 0], times: [0, liftEnd, 1], duration: duration)
        keyframes(label, "opacity", values: [1, 1, 0], times: [0, NSNumber(value: landStart), 1], duration: duration)
        CATransaction.commit()

        // The tab's own icon and title fade in under the departing capsule.
        DispatchQueue.main.asyncAfter(deadline: .now() + duration * landStart) {
            tab.revealContent()
        }
    }

    /// Moves a layer along a curve that rises first and then glides, while
    /// its size changes from `start` to `end`.
    private static func move(_ layer: CALayer, from start: NSRect, to end: NSRect, duration: TimeInterval) {
        layer.bounds = CGRect(origin: .zero, size: end.size)
        layer.position = CGPoint(x: end.midX, y: end.midY)

        let path = CGMutablePath()
        path.move(to: CGPoint(x: start.midX, y: start.midY))
        path.addQuadCurve(
            to: CGPoint(x: end.midX, y: end.midY),
            control: CGPoint(x: start.midX + (end.midX - start.midX) * curveLead, y: end.midY)
        )
        let position = CAKeyframeAnimation(keyPath: "position")
        position.path = path
        position.duration = duration
        position.timingFunction = timing
        layer.add(position, forKey: "position")
        animate(layer, "bounds", from: NSValue(rect: CGRect(origin: .zero, size: start.size)),
                to: NSValue(rect: CGRect(origin: .zero, size: end.size)), duration: duration)
    }

    private static func animate(_ layer: CALayer, _ keyPath: String, from: Any, to: Any, duration: TimeInterval) {
        let animation = CABasicAnimation(keyPath: keyPath)
        animation.fromValue = from
        animation.toValue = to
        animation.duration = duration
        animation.timingFunction = timing
        layer.setValue(to, forKeyPath: keyPath)
        layer.add(animation, forKey: keyPath)
    }

    private static func keyframes(_ layer: CALayer, _ keyPath: String, values: [NSNumber], times: [NSNumber], duration: TimeInterval) {
        let animation = CAKeyframeAnimation(keyPath: keyPath)
        animation.values = values
        animation.keyTimes = times
        animation.duration = duration
        layer.setValue(values.last, forKeyPath: keyPath)
        layer.add(animation, forKey: keyPath)
    }

    /// The view as it looks on screen, at the screen's resolution.
    private static func snapshot(_ view: NSView) -> CGImage? {
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
        view.cacheDisplay(in: view.bounds, to: rep)
        return rep.cgImage
    }
}

