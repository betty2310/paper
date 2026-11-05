import AppKit
import SwiftUI

/// A floating message in the window's bottom right corner: what the app is
/// doing in the background (looking up a paper, finding and downloading its
/// PDF) and short feedback such as "DOI copied." One paper keeps one toast,
/// which changes in place as its work moves on.
struct Toast: Identifiable, Equatable {
    enum Style: Equatable {
        /// `progress` from 0 to 1, or nil while the amount is unknown.
        case working(progress: Double?)
        case done
        case warning
        case error

        /// Finished toasts leave on their own; work in progress and errors stay.
        var lingers: Bool {
            switch self {
            case .working, .error: false
            case .done, .warning: true
            }
        }
    }

    struct Action {
        let title: String
        let perform: () -> Void
    }

    var id = UUID().uuidString
    var style: Style
    var message: String
    /// Secondary text after the message, such as "1.2 MB of 2.2 MB".
    var detail: String?
    /// The paper the toast is about, on a second line in the title font.
    var subject: String?
    var action: Action?

    static func == (lhs: Toast, rhs: Toast) -> Bool {
        lhs.id == rhs.id && lhs.style == rhs.style && lhs.message == rhs.message
            && lhs.detail == rhs.detail && lhs.subject == rhs.subject && lhs.action?.title == rhs.action?.title
    }

    /// The toast that follows a paper's lookup and PDF search.
    static func itemId(_ itemId: String) -> String { "item:\(itemId)" }
}

/// The toasts, newest at the bottom, pinned to the bottom right corner.
struct ToastStack: View {
    @ObservedObject var store: LibraryStore
    /// Where the toasts are, in the hosting view's flipped coordinates, so
    /// clicks anywhere else go through to the window.
    var framesChanged: ([CGRect]) -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private static let space = "toasts"

    var body: some View {
        VStack(alignment: .trailing, spacing: Theme.Size.gap) {
            ForEach(store.toasts) { toast in
                ToastView(toast: toast) { store.dismissToast(id: toast.id) }
                    .background(GeometryReader { proxy in
                        Color.clear.preference(key: ToastFrames.self, value: [proxy.frame(in: .named(Self.space))])
                    })
                    .transition(reduceMotion ? .opacity : .asymmetric(
                        insertion: .offset(y: Theme.Toast.enterOffset).combined(with: .opacity),
                        removal: .scale(scale: Theme.Toast.removalScale, anchor: .trailing).combined(with: .opacity)
                    ))
            }
        }
        .padding(Theme.Size.padding)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        .coordinateSpace(name: Self.space)
        .onPreferenceChange(ToastFrames.self) { framesChanged($0) }
        .animation(reduceMotion ? .easeOut(duration: Theme.Duration.panel) : Theme.Toast.spring, value: store.toasts.map(\.id))
    }
}

private struct ToastFrames: PreferenceKey {
    static let defaultValue: [CGRect] = []
    static func reduce(value: inout [CGRect], nextValue: () -> [CGRect]) {
        value += nextValue()
    }
}

/// One capsule: status icon, message, optional action, close button. A toast
/// about a paper keeps one width while its text changes; short feedback is
/// as wide as its text.
struct ToastView: View {
    let toast: Toast
    let dismiss: () -> Void
    @Environment(\.displayScale) private var displayScale

    private var fixedWidth: Bool { toast.subject != nil }

    /// What cross fades when it changes. Byte counts and the ring's progress
    /// change many times a second and update in place instead.
    private var phase: [String?] {
        let kind = switch toast.style {
        case .working: "working"
        case .done: "done"
        case .warning: "warning"
        case .error: "error"
        }
        return [kind, toast.message, toast.subject, toast.action?.title]
    }

    var body: some View {
        HStack(spacing: Theme.Size.gap + Theme.Size.smallGap) {
            ToastIcon(style: toast.style)
            text
            HStack(spacing: Theme.Size.smallGap) {
                if let action = toast.action {
                    Button(action.title, action: action.perform)
                        .buttonStyle(ToastActionStyle())
                }
                Button(action: dismiss) {
                    Image(systemName: Theme.Symbol.close)
                        .font(Theme.Font.swiftUIToastClose)
                }
                .buttonStyle(ToastCloseStyle())
                .help("Dismiss")
                .accessibilityLabel("Dismiss")
            }
        }
        .padding(.leading, Theme.Toast.leadingPadding)
        .padding(.trailing, Theme.Toast.trailingPadding)
        .padding(.vertical, Theme.Toast.verticalPadding)
        .frame(width: fixedWidth ? Theme.Toast.width : nil)
        .frame(minHeight: Theme.Toast.height)
        .background(Theme.Color.swiftUIToastBackground, in: Capsule())
        .overlay(Capsule().strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1 / displayScale))
        .shadow(color: Theme.Toast.shadowColor, radius: Theme.Toast.shadowRadius, y: Theme.Toast.shadowY)
        .shadow(color: Theme.Toast.contactShadowColor, radius: Theme.Toast.contactShadowRadius, y: Theme.Toast.contactShadowY)
        .animation(Theme.Toast.contentAnimation, value: phase)
        .accessibilityElement(children: .contain)
        .accessibilityLabel([toast.message, toast.detail, toast.subject].compactMap { $0 }.joined(separator: ", "))
    }

    @ViewBuilder private var text: some View {
        let lines = VStack(alignment: .leading, spacing: Theme.Toast.lineSpacing) {
            HStack(alignment: .firstTextBaseline, spacing: Theme.Size.gap) {
                Text(toast.message)
                    .font(Theme.Font.swiftUIToastMessage)
                    .foregroundStyle(Theme.Color.swiftUIText)
                if let detail = toast.detail {
                    Text(detail)
                        .font(Theme.Font.swiftUIToastDetail)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
            }
            .lineLimit(1)
            if let subject = toast.subject {
                Text(subject)
                    .font(Theme.Font.swiftUIToastSubject)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
        if fixedWidth {
            lines.frame(maxWidth: .infinity, alignment: .leading)
        } else {
            lines
                .frame(maxWidth: Theme.Toast.textMaxWidth, alignment: .leading)
                .fixedSize(horizontal: true, vertical: false)
        }
    }
}

/// A spinner that becomes a progress ring once the size is known, and a
/// symbol when the work ends.
private struct ToastIcon: View {
    let style: Toast.Style

    var body: some View {
        ZStack {
            switch style {
            case .working(let progress):
                ProgressRing(progress: progress)
                    .transition(.opacity)
            case .done:
                symbol("checkmark.circle.fill", Theme.Color.swiftUIAccent)
            case .warning:
                symbol("exclamationmark.circle.fill", Theme.Color.swiftUIWarning)
            case .error:
                symbol("exclamationmark.circle.fill", Theme.Color.swiftUIDanger)
            }
        }
        .frame(width: Theme.Toast.iconSize, height: Theme.Toast.iconSize)
    }

    private func symbol(_ name: String, _ color: SwiftUI.Color) -> some View {
        Image(systemName: name)
            .resizable()
            .symbolRenderingMode(.palette)
            .foregroundStyle(.white, color)
            .transition(.scale(scale: Theme.Toast.iconPopScale).combined(with: .opacity))
    }
}

private struct ProgressRing: View {
    let progress: Double?
    @State private var spinning = false

    var body: some View {
        ZStack {
            Circle()
                .stroke(Theme.Color.swiftUIToastTrack, lineWidth: Theme.Toast.ringWidth)
            Circle()
                .trim(from: 0, to: progress.map { max($0, Theme.Toast.ringMinimum) } ?? Theme.Toast.spinnerArc)
                .stroke(Theme.Color.swiftUIAccent, style: StrokeStyle(lineWidth: Theme.Toast.ringWidth, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .rotationEffect(.degrees(progress == nil && spinning ? 360 : 0))
                .animation(progress == nil && spinning
                    ? .linear(duration: Theme.Duration.spinnerTurn).repeatForever(autoreverses: false)
                    : .easeOut(duration: Theme.Duration.panel), value: spinning)
                .animation(.linear(duration: Theme.Duration.progressStep), value: progress)
        }
        .padding(Theme.Toast.ringWidth / 2)
        .onAppear { spinning = true }
        .accessibilityLabel(progress.map { "\(Int($0 * 100)) percent" } ?? "In progress")
    }
}

/// "Open", "Retry": accent text on a light accent capsule.
private struct ToastActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ToastActionBody(configuration: configuration)
    }

    private struct ToastActionBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var hovering = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            configuration.label
                .font(Theme.Font.swiftUIToastAction)
                .foregroundStyle(Theme.Color.swiftUIAccent)
                .padding(.horizontal, Theme.Toast.actionPadding)
                .frame(height: Theme.Toast.controlSize)
                .background(Theme.Color.swiftUIAccent.opacity(opacity), in: Capsule())
                .overlay(Capsule().strokeBorder(Theme.Color.swiftUIAccent, lineWidth: isFocused ? Theme.Toast.focusWidth : 0))
                .opacity(isEnabled ? 1 : Theme.Toast.disabledOpacity)
                .contentShape(Capsule())
                .onHover { hovering = $0 }
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
        }

        private var opacity: Double {
            configuration.isPressed ? Theme.Opacity.pressedTint : hovering ? Theme.Opacity.hoverTint : Theme.Opacity.tint
        }
    }
}

/// The round gray close button from the reference toast.
private struct ToastCloseStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ToastCloseBody(configuration: configuration)
    }

    private struct ToastCloseBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var hovering = false
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            configuration.label
                .foregroundStyle(hovering ? Theme.Color.swiftUIText : Theme.Color.swiftUISecondaryText)
                .frame(width: Theme.Toast.controlSize, height: Theme.Toast.controlSize)
                .background(fill, in: Circle())
                .overlay(Circle().strokeBorder(Theme.Color.swiftUIAccent, lineWidth: isFocused ? Theme.Toast.focusWidth : 0))
                .opacity(isEnabled ? 1 : Theme.Toast.disabledOpacity)
                .contentShape(Circle())
                .onHover { hovering = $0 }
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
        }

        private var fill: SwiftUI.Color {
            configuration.isPressed ? Theme.Color.swiftUIToastClosePressed
                : hovering ? Theme.Color.swiftUIToastCloseHover : Theme.Color.swiftUIToastClose
        }
    }
}

/// Hosts the stack over the window's content. Only the toasts themselves take
/// clicks, show the arrow cursor, and hold the toasts while the pointer is on
/// them; everywhere else the list, inspector, or reader underneath does.
final class ToastHostingView: NSHostingView<ToastStack> {
    private weak var store: LibraryStore?
    private var toastFrames: [CGRect] = [] {
        didSet {
            guard toastFrames != oldValue else { return }
            updateHover()
        }
    }
    private var hovering = false

    init(store: LibraryStore) {
        self.store = store
        var framesChanged: ([CGRect]) -> Void = { _ in }
        super.init(rootView: ToastStack(store: store, framesChanged: { framesChanged($0) }))
        framesChanged = { [weak self] frames in self?.toastFrames = frames }
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseMoved, .mouseEnteredAndExited, .cursorUpdate, .activeInKeyWindow, .inVisibleRect],
                                       owner: self, userInfo: nil))
        // The window decides this view's size, never the toasts.
        sizingOptions = []
    }

    @available(*, unavailable)
    required init(rootView: ToastStack) { fatalError("init(rootView:) is not used") }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard isOnToast(convert(point, from: superview)) else { return nil }
        return super.hitTest(point)
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        updateHover()
        // A text view underneath sets the I-beam on its own mouse moves, which
        // may come after this one. The arrow goes on once they are all done.
        if hovering { DispatchQueue.main.async { NSCursor.arrow.set() } }
    }

    override func cursorUpdate(with event: NSEvent) {
        if hovering { NSCursor.arrow.set() } else { super.cursorUpdate(with: event) }
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        updateHover()
    }

    /// Holds the toasts while the pointer is on one, including when a toast
    /// moves under a still pointer or leaves from under it.
    private func updateHover() {
        guard let window else { return }
        let point = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        let now = window.isKeyWindow && isOnToast(point)
        guard now != hovering else { return }
        hovering = now
        store?.holdToasts(now)
        // Views underneath, such as the notes editor, keep cursor rects that
        // the window reapplies on every move. They pause while on a toast.
        if now {
            window.disableCursorRects()
            DispatchQueue.main.async { NSCursor.arrow.set() }
        } else {
            window.enableCursorRects()
            window.resetCursorRects()
        }
    }

    /// `point` in this view's coordinates.
    private func isOnToast(_ point: NSPoint) -> Bool {
        toastFrames.contains { appKitRect($0).contains(point) }
    }

    /// SwiftUI's frames are top down; flip them when this view is not.
    private func appKitRect(_ frame: CGRect) -> CGRect {
        isFlipped ? frame : CGRect(x: frame.minX, y: bounds.height - frame.maxY, width: frame.width, height: frame.height)
    }
}
