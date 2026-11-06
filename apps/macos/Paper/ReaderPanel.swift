import AppKit
import PaperEngine
import SwiftUI

/// The detail panel while a PDF's tab is in front: the PDF's annotations as
/// cards in page order, or the paper's Markdown note. A switch at the top
/// picks one; the choice is shared by every PDF and remembered across
/// launches.
@MainActor
final class ReaderPanelViewController: NSViewController {
    let model = ReaderPanelModel()
    private let store: LibraryStore
    private let itemId: String

    init(store: LibraryStore, itemId: String) {
        self.store = store
        self.itemId = itemId
        super.init(nibName: nil, bundle: nil)
        model.noteEdited = { [weak store] markdown in
            store?.scheduleNoteSave(itemId: itemId, markdown: markdown)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let host = NSHostingView(rootView: ReaderPanel(model: model))
        // The split view item sizes this pane, as with the inspector.
        host.sizingOptions = []
        host.setFrameSize(NSSize(width: Theme.Size.inspectorWidth, height: Theme.Size.windowHeight))
        view = host
    }

    /// Called as the tab comes forward: another PDF may have switched the
    /// panel, and the inspector may have changed the note.
    func willShow() {
        model.mode = ReaderPanelModel.storedMode
        model.note = store.note(itemId: itemId)
    }
}

@MainActor
final class ReaderPanelModel: ObservableObject {
    enum Mode: Int {
        case annotations
        case notes
    }

    private static let modeKey = "readerPanelMode"
    private static let previewKey = "readerNotePreview"

    static var storedMode: Mode {
        Mode(rawValue: UserDefaults.standard.integer(forKey: modeKey)) ?? .annotations
    }

    @Published var mode: Mode = storedMode {
        didSet { UserDefaults.standard.set(mode.rawValue, forKey: Self.modeKey) }
    }
    /// The PDF's annotations in page order, top to bottom on each page.
    @Published var annotations: [Annotation] = []
    /// The card of the annotation last gone to.
    @Published var selectedId: String?
    /// The card whose comment is being edited.
    @Published var editingId: String?
    @Published var note = ""
    /// Whether the note shows rendered, shared like the mode.
    @Published var previewsNote = UserDefaults.standard.bool(forKey: previewKey) {
        didSet { UserDefaults.standard.set(previewsNote, forKey: Self.previewKey) }
    }

    var select: (String) -> Void = { _ in }
    var saveComment: (_ id: String, _ text: String) -> Void = { _, _ in }
    var delete: (String) -> Void = { _ in }
    var noteEdited: (String) -> Void = { _ in }
}

struct ReaderPanel: View {
    @ObservedObject var model: ReaderPanelModel

    var body: some View {
        VStack(spacing: Theme.Size.gap) {
            Picker("Panel", selection: $model.mode) {
                Text("Annotations").tag(ReaderPanelModel.Mode.annotations)
                Text("Notes").tag(ReaderPanelModel.Mode.notes)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, Theme.Size.padding)
            .padding(.top, Theme.Size.gap)
            switch model.mode {
            case .annotations: AnnotationList(model: model)
            case .notes: NotePanel(model: model)
            }
        }
        // No background of its own: the panel sits on the window's chrome.
        .frame(minWidth: Theme.Size.inspectorMinWidth, maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    }
}

// MARK: Annotations

private struct AnnotationList: View {
    @ObservedObject var model: ReaderPanelModel

    var body: some View {
        if model.annotations.isEmpty {
            Text("No annotations yet. Select text to highlight it, or add a note.")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .multilineTextAlignment(.center)
                .padding(Theme.Size.padding)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: Theme.Size.gap) {
                        ForEach(model.annotations, id: \.id) { annotation in
                            AnnotationCard(annotation: annotation, model: model)
                                .id(annotation.id)
                        }
                    }
                    .padding(.horizontal, Theme.Size.padding)
                    .padding(.bottom, Theme.Size.padding)
                }
                // A highlight clicked on the page brings its card into view.
                .onChange(of: model.selectedId) { _, id in
                    guard let id else { return }
                    withAnimation(.easeOut(duration: Theme.Duration.panel)) { proxy.scrollTo(id) }
                }
            }
        }
    }
}

/// One annotation: a bar in its color, the quoted text, the comment in
/// italics, and the page. Clicking the card goes to the annotation; clicking
/// the comment edits it in place.
private struct AnnotationCard: View {
    let annotation: Annotation
    @ObservedObject var model: ReaderPanelModel
    @State private var hovering = false
    @State private var draft = ""
    @FocusState private var commentFocused: Bool
    @Environment(\.displayScale) private var displayScale

    private var isNote: Bool { annotation.kind == .note }
    private var isEditing: Bool { model.editingId == annotation.id }
    private var isSelected: Bool { model.selectedId == annotation.id }

    var body: some View {
        HStack(spacing: 0) {
            Rectangle()
                .fill(SwiftUI.Color(nsColor: NSColor(hexString: annotation.color) ?? Theme.Markup.swatches[0].color))
                .frame(width: Theme.Size.annotationBarWidth)
            VStack(alignment: .leading, spacing: Theme.Size.smallGap) {
                if !isNote, let quote = annotation.selectedText, !quote.isEmpty {
                    Text(quote)
                        .font(Theme.Font.swiftUIBody)
                        .foregroundStyle(Theme.Color.swiftUIText)
                        .lineLimit(Theme.Size.annotationQuoteLines)
                        .fixedSize(horizontal: false, vertical: true)
                        .allowsHitTesting(false)
                }
                comment
                HStack(spacing: Theme.Size.smallGap) {
                    Image(systemName: isNote ? Theme.Symbol.note : Theme.Symbol.highlight)
                    Text("Page \(annotation.pageIndex + 1)")
                }
                .font(Theme.Font.swiftUICaption)
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
                .allowsHitTesting(false)
            }
            .padding(Theme.Size.annotationCardPadding)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        // The card itself is a button behind its text, so it gets pressed and
        // keyboard focus states; the comment above it takes its own clicks.
        .background {
            Button { model.select(annotation.id) } label: { SwiftUI.Color.clear }
                .buttonStyle(AnnotationCardStyle(hovering: hovering, selected: isSelected))
                .accessibilityLabel(accessibilityLabel)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous)
                .strokeBorder(isSelected ? Theme.Color.swiftUIAccent : Theme.Color.swiftUISeparator,
                              lineWidth: isSelected ? Theme.Size.markupRingWidth / 2 : 1 / displayScale)
                .allowsHitTesting(false)
        )
        .onHover { hovering = $0 }
        .contextMenu {
            Button(isNote ? "Edit Note" : "Edit Comment", action: beginEditing)
            Divider()
            Button(isNote ? "Delete Note" : "Remove Highlight", role: .destructive) { model.delete(annotation.id) }
        }
    }

    @ViewBuilder
    private var comment: some View {
        if isEditing {
            TextField(isNote ? "Write the note" : "Add a comment", text: $draft, axis: .vertical)
                .textFieldStyle(.plain)
                .font(Theme.Font.swiftUIComment)
                .foregroundStyle(Theme.Color.swiftUIText)
                .focused($commentFocused)
                .onSubmit(commit)
                .onExitCommand { model.editingId = nil }
                .onChange(of: commentFocused) { _, focused in if !focused { commit() } }
                .onAppear { commentFocused = true }
        } else if let text = annotation.comment, !text.isEmpty {
            Text(text)
                .font(Theme.Font.swiftUIComment)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .fixedSize(horizontal: false, vertical: true)
                .contentShape(Rectangle())
                .onTapGesture(perform: beginEditing)
                .help("Click to edit")
        } else if hovering || isSelected || isNote {
            Text(isNote ? "Empty note" : "Add a comment…")
                .font(Theme.Font.swiftUIComment)
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
                .contentShape(Rectangle())
                .onTapGesture(perform: beginEditing)
        }
    }

    private func beginEditing() {
        draft = annotation.comment ?? ""
        model.editingId = annotation.id
    }

    /// Saves the comment once, when Return is pressed or the field loses focus.
    private func commit() {
        guard isEditing else { return }
        model.editingId = nil
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        if text != (annotation.comment ?? "") { model.saveComment(annotation.id, text) }
    }

    private var accessibilityLabel: String {
        let what = isNote ? "Note" : "Highlight"
        let text = isNote ? annotation.comment : annotation.selectedText
        return "\(what) on page \(annotation.pageIndex + 1)" + (text.map { ": \($0)" } ?? "")
    }
}

/// The card's surface: white, darker under the pointer and while pressed,
/// accent tinted when it is the one last gone to, and an accent ring for
/// keyboard focus.
private struct AnnotationCardStyle: ButtonStyle {
    let hovering: Bool
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        CardBody(configuration: configuration, hovering: hovering, selected: selected)
    }

    private struct CardBody: View {
        let configuration: ButtonStyleConfiguration
        let hovering: Bool
        let selected: Bool
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            Theme.Color.swiftUIPanel
                .overlay(selected ? Theme.Color.swiftUIAccent.opacity(Theme.Opacity.tint) : .clear)
                .overlay(configuration.isPressed ? Theme.Color.swiftUIControlPressed : hovering ? Theme.Color.swiftUIControlHover : .clear)
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous)
                        .strokeBorder(Theme.Color.swiftUIAccent, lineWidth: isFocused ? Theme.Size.markupRingWidth : 0)
                )
                .contentShape(Rectangle())
                .animation(.easeOut(duration: Theme.Duration.hover), value: hovering)
        }
    }
}

// MARK: Note

/// The paper's note: a plain Markdown editor, or the note rendered.
private struct NotePanel: View {
    @ObservedObject var model: ReaderPanelModel
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            HStack {
                Text("Markdown")
                    .font(Theme.Font.swiftUISectionLabel)
                    .foregroundStyle(Theme.Color.swiftUITertiaryText)
                    .textCase(.uppercase)
                Spacer()
                Button {
                    model.previewsNote.toggle()
                } label: {
                    Label(model.previewsNote ? "Edit" : "Preview",
                          systemImage: model.previewsNote ? Theme.Symbol.edit : Theme.Symbol.preview)
                }
                .buttonStyle(MarkupTextButtonStyle())
                .keyboardShortcut("p", modifiers: [.command, .option])
                .help(model.previewsNote ? "Edit the Markdown (⌥⌘P)" : "Show the note rendered (⌥⌘P)")
            }
            Group {
                if model.previewsNote {
                    preview
                } else {
                    editor
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(Theme.Color.swiftUIPanel, in: RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous)
                    .strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1 / displayScale)
            )
        }
        .padding(.horizontal, Theme.Size.padding)
        .padding(.bottom, Theme.Size.padding)
    }

    private var editor: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: Binding(get: { model.note }, set: { text in
                model.note = text
                model.noteEdited(text)
            }))
            .font(Theme.Font.swiftUIBody)
            .scrollContentBackground(.hidden)
            .padding(Theme.Size.gap)
            .accessibilityLabel("Note")
            if model.note.isEmpty {
                Text("Write a note in Markdown…")
                    .font(Theme.Font.swiftUIBody)
                    .foregroundStyle(Theme.Color.swiftUITertiaryText)
                    // TextEditor insets its text by the line fragment padding.
                    .padding(.leading, Theme.Size.gap + Theme.Size.textEditorInset)
                    .padding(.top, Theme.Size.gap)
                    .allowsHitTesting(false)
            }
        }
    }

    @ViewBuilder
    private var preview: some View {
        if model.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Text("Nothing to preview yet.")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .padding(Theme.Size.annotationCardPadding)
        } else {
            ScrollView {
                MarkdownView(markdown: model.note)
                    .padding(Theme.Size.annotationCardPadding)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// Renders the common Markdown blocks: headings, paragraphs, bullet, number,
/// and task lists, quotes, fenced code, and rules. Inline styles, such as
/// emphasis, code, and links, come from Foundation's Markdown parser.
struct MarkdownView: View {
    let markdown: String

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            ForEach(Array(MarkdownBlock.parse(markdown).enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
        .textSelection(.enabled)
    }

    @ViewBuilder
    private func view(for block: MarkdownBlock) -> some View {
        switch block {
        case .heading(let level, let text):
            Text(Self.inline(text))
                .font(level == 1 ? Theme.Font.swiftUINoteHeading1 : level == 2 ? Theme.Font.swiftUINoteHeading2 : Theme.Font.swiftUINoteHeading3)
                .foregroundStyle(Theme.Color.swiftUIText)
                .padding(.top, Theme.Size.smallGap)
        case .paragraph(let text):
            Text(Self.inline(text))
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(Theme.Color.swiftUIText)
                .fixedSize(horizontal: false, vertical: true)
        case .list(let items):
            VStack(alignment: .leading, spacing: Theme.Size.smallGap) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: Theme.Size.smallGap) {
                        marker(item.marker)
                            .frame(minWidth: Theme.Size.listMarkerWidth, alignment: .trailing)
                        Text(Self.inline(item.text))
                            .font(Theme.Font.swiftUIBody)
                            .foregroundStyle(Theme.Color.swiftUIText)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.leading, CGFloat(item.level) * Theme.Size.padding)
                }
            }
        case .quote(let text):
            HStack(spacing: Theme.Size.gap) {
                Rectangle()
                    .fill(Theme.Color.swiftUISeparator)
                    .frame(width: Theme.Size.annotationBarWidth)
                Text(Self.inline(text))
                    .font(Theme.Font.swiftUIComment)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        case .code(let text):
            Text(text)
                .font(Theme.Font.swiftUICode)
                .foregroundStyle(Theme.Color.swiftUIText)
                .padding(Theme.Size.gap)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Theme.Color.swiftUICard, in: RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous))
        case .rule:
            Divider()
        }
    }

    @ViewBuilder
    private func marker(_ marker: MarkdownBlock.Marker) -> some View {
        switch marker {
        case .bullet:
            Text("•").font(Theme.Font.swiftUIBody).foregroundStyle(Theme.Color.swiftUISecondaryText)
        case .number(let number):
            Text("\(number).").font(Theme.Font.swiftUIBody.monospacedDigit()).foregroundStyle(Theme.Color.swiftUISecondaryText)
        case .task(let done):
            Image(systemName: done ? Theme.Symbol.taskDone : Theme.Symbol.task)
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(done ? Theme.Color.swiftUIAccent : Theme.Color.swiftUISecondaryText)
        }
    }

    private static func inline(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}

/// One block of a Markdown note.
enum MarkdownBlock: Equatable {
    enum Marker: Equatable {
        case bullet
        case number(Int)
        case task(done: Bool)
    }

    struct Item: Equatable {
        var marker: Marker
        var text: String
        /// Nesting, from the indentation in steps of two spaces.
        var level: Int
    }

    case heading(Int, String)
    case paragraph(String)
    case list([Item])
    case quote(String)
    case code(String)
    case rule

    static func parse(_ markdown: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var paragraph: [String] = []
        var quote: [String] = []
        var items: [Item] = []
        var code: [String]?

        func flush() {
            if !paragraph.isEmpty { blocks.append(.paragraph(paragraph.joined(separator: " "))) }
            if !quote.isEmpty { blocks.append(.quote(quote.joined(separator: " "))) }
            if !items.isEmpty { blocks.append(.list(items)) }
            paragraph = []
            quote = []
            items = []
        }

        for line in markdown.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if var lines = code {
                if trimmed.hasPrefix("```") {
                    blocks.append(.code(lines.joined(separator: "\n")))
                    code = nil
                } else {
                    lines.append(line)
                    code = lines
                }
                continue
            }
            if trimmed.hasPrefix("```") {
                flush()
                code = []
            } else if trimmed.isEmpty {
                flush()
            } else if let heading = heading(trimmed) {
                flush()
                blocks.append(heading)
            } else if trimmed.count >= 3, let first = trimmed.first, "-*_".contains(first),
                      trimmed.allSatisfy({ $0 == first || $0 == " " }) {
                flush()
                blocks.append(.rule)
            } else if let item = item(line) {
                if !paragraph.isEmpty || !quote.isEmpty { flush() }
                items.append(item)
            } else if trimmed.hasPrefix(">") {
                if !paragraph.isEmpty || !items.isEmpty { flush() }
                quote.append(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces))
            } else if !items.isEmpty, line.first == " " {
                // A wrapped list item continues on an indented line.
                items[items.count - 1].text += " " + trimmed
            } else {
                if !quote.isEmpty || !items.isEmpty { flush() }
                paragraph.append(trimmed)
            }
        }
        if let code { blocks.append(.code(code.joined(separator: "\n"))) }
        flush()
        return blocks
    }

    private static func heading(_ line: String) -> MarkdownBlock? {
        let hashes = line.prefix { $0 == "#" }.count
        guard (1...6).contains(hashes), line.dropFirst(hashes).first == " " else { return nil }
        return .heading(hashes, line.dropFirst(hashes).trimmingCharacters(in: .whitespaces))
    }

    private static func item(_ line: String) -> Item? {
        let indent = line.prefix { $0 == " " }.count
        let rest = line.dropFirst(indent)
        var marker: Marker
        var text: Substring
        if let first = rest.first, "-*+".contains(first), rest.dropFirst().first == " " {
            marker = .bullet
            text = rest.dropFirst(2)
        } else {
            let digits = rest.prefix(while: \.isNumber)
            let after = rest.dropFirst(digits.count)
            guard !digits.isEmpty, digits.count <= 9, let number = Int(digits),
                  let dot = after.first, ".)".contains(dot), after.dropFirst().first == " "
            else { return nil }
            marker = .number(number)
            text = after.dropFirst(2)
        }
        if marker == .bullet, text.count >= 3, text.hasPrefix("["), text.dropFirst(2).hasPrefix("]") {
            let box = text.dropFirst().first
            if box == " " || box == "x" || box == "X" {
                marker = .task(done: box != " ")
                text = text.dropFirst(3)
            }
        }
        return Item(marker: marker, text: text.trimmingCharacters(in: .whitespaces), level: indent / 2)
    }
}
