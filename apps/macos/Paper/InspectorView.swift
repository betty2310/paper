import AppKit
import PaperEngine
import SwiftUI
import UniformTypeIdentifiers

/// Hosts the SwiftUI inspector in the right split view item.
@MainActor
final class InspectorViewController: NSViewController {
    let store: LibraryStore

    init(store: LibraryStore) {
        self.store = store
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func loadView() {
        let host = NSHostingView(rootView: InspectorView(store: store))
        // The split view item sizes this pane; SwiftUI's own size constraints
        // would otherwise pull the window and the other panes around.
        host.sizingOptions = []
        host.setFrameSize(NSSize(width: Theme.Size.inspectorWidth, height: Theme.Size.windowHeight))
        view = host
    }
}

/// The selected paper: title, people, venue, identifiers, abstract, tags,
/// collections, and the note. Nothing here calls the engine directly.
struct InspectorView: View {
    @ObservedObject var store: LibraryStore

    var body: some View {
        Group {
            if let detail = store.detail {
                ItemDetailView(store: store, detail: detail)
                    .id(detail.id)
            } else if store.selectedItemIds.count > 1 {
                MultipleSelectionView(store: store)
            } else {
                NoSelectionView()
            }
        }
        // No background of its own: the inspector sits on the window's chrome.
        .frame(minWidth: Theme.Size.inspectorMinWidth, maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// The paper's preview, larger: the whole figure on white, or the title's
/// first letter on its soft color. Double clicking a figure opens the PDF.
struct InspectorPreview: View {
    @ObservedObject var previews: PaperPreviews
    let itemId: String
    let title: String
    let hasPdf: Bool
    let open: () -> Void
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous)
        Group {
            switch previews.preview(itemId: itemId, title: title, hasPdf: hasPdf) {
            case .figure(let image, _):
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: .infinity, maxHeight: Theme.Size.inspectorPreviewMaxHeight)
                    .padding(Theme.Size.gap)
                    .background(Theme.Preview.swiftUIFigureBackground)
                    .contentShape(shape)
                    .onTapGesture(count: 2, perform: open)
                    .help("Double click to read")
                    .accessibilityLabel("First figure")
            case .loading:
                Theme.Preview.swiftUIFigureBackground
                    .frame(height: Theme.Size.inspectorLetterHeight)
            case .letter(let letter, let tile):
                Text(letter)
                    .font(Theme.Font.swiftUIInspectorPreviewLetter)
                    .foregroundStyle(tile.swiftUILetter)
                    .frame(maxWidth: .infinity)
                    .frame(height: Theme.Size.inspectorLetterHeight)
                    .background(tile.swiftUIBackground)
                    .accessibilityHidden(true)
            }
        }
        .clipShape(shape)
        .overlay(shape.strokeBorder(Theme.Preview.swiftUIBorder, lineWidth: 1 / displayScale))
    }
}

struct NoSelectionView: View {
    var body: some View {
        VStack(spacing: Theme.Size.gap) {
            Image(systemName: Theme.Symbol.paper)
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
            Text("Select a paper to see its details.")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
        }
        .padding(Theme.Size.padding)
    }
}

struct MultipleSelectionView: View {
    @ObservedObject var store: LibraryStore

    var body: some View {
        VStack(spacing: Theme.Size.gap) {
            Image(systemName: "doc.on.doc")
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
            Text("\(store.selectedItemIds.count) papers selected")
                .font(Theme.Font.swiftUIBody)
                .foregroundStyle(Theme.Color.swiftUIText)
            Text("Drag them onto a collection, or use the context menu.")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .multilineTextAlignment(.center)
            Button("Delete", role: .destructive) {
                store.delete(itemIds: store.selectedItemIds)
            }
            .padding(.top, Theme.Size.gap)
        }
        .padding(Theme.Size.padding)
    }
}

struct ItemDetailView: View {
    @ObservedObject var store: LibraryStore
    let detail: ItemDetail
    @State private var showEditor = false
    @State private var newTag = ""
    @State private var note: String
    @State private var abstractExpanded = false

    init(store: LibraryStore, detail: ItemDetail) {
        self.store = store
        self.detail = detail
        _note = State(initialValue: detail.note ?? "")
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Size.padding) {
                InspectorPreview(
                    previews: store.previews, itemId: detail.id, title: detail.fields.title,
                    hasPdf: pdfAttachment != nil, open: openPDF
                )
                header
                if detail.lookupStatus == .pending {
                    lookupPending
                } else if detail.lookupStatus == .failed {
                    lookupFailed
                }
                actions
                // A PDF Paper could not identify is still the user's to read.
                if detail.lookupStatus != .pending {
                    section("PDF") { pdf }
                }
                if let abstractText = detail.fields.abstractText, !abstractText.isEmpty {
                    section("Abstract") {
                        Text(abstractText)
                            .font(Theme.Font.swiftUIBody)
                            .foregroundStyle(Theme.Color.swiftUIText)
                            .lineLimit(abstractExpanded ? nil : 6)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                        Button(abstractExpanded ? "Show Less" : "Show More") { abstractExpanded.toggle() }
                            .buttonStyle(.link)
                            .font(Theme.Font.swiftUISecondary)
                    }
                }
                section("Tags") { tags }
                section("Collections") { collections }
                section("Notes") { notes }
                section("Details") { details }
            }
            .padding(Theme.Size.padding)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .sheet(isPresented: $showEditor) {
            MetadataEditor(mode: .edit(detail) { patch in
                store.updateMetadata(itemId: detail.id, patch: patch)
            })
        }
    }

    // MARK: Header

    private var header: some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            Text(detail.fields.kind?.label ?? ItemKind.other.label)
                .font(Theme.Font.swiftUISectionLabel)
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
                .textCase(.uppercase)
            Text(detail.fields.title)
                .font(Theme.Font.swiftUIInspectorTitle)
                .foregroundStyle(Theme.Color.swiftUIText)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            if !detail.authorsLine.isEmpty {
                Text(detail.authorsLine)
                    .font(Theme.Font.swiftUIBody)
                    .foregroundStyle(Theme.Color.swiftUIText)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !detail.venueLine.isEmpty {
                Text(detail.venueLine)
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .textSelection(.enabled)
            }
            if let doi = detail.fields.doi, let url = URL(string: "https://doi.org/\(doi)") {
                Link(destination: url) {
                    HStack(spacing: Theme.Size.smallGap) {
                        Text(doi)
                        Image(systemName: Theme.Symbol.link)
                    }
                    .font(Theme.Font.swiftUISecondary)
                }
                .help("Open on doi.org")
            }
            if let arxiv = detail.fields.arxivId, let url = URL(string: "https://arxiv.org/abs/\(arxiv)") {
                Link(destination: url) {
                    HStack(spacing: Theme.Size.smallGap) {
                        Text("arXiv:\(arxiv)")
                        Image(systemName: Theme.Symbol.link)
                    }
                    .font(Theme.Font.swiftUISecondary)
                }
            }
        }
    }

    private var lookupPending: some View {
        HStack(spacing: Theme.Size.gap) {
            ProgressView().controlSize(.small)
            Text("Looking up metadata…")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
        }
    }

    private var lookupFailed: some View {
        HStack(spacing: Theme.Size.gap) {
            Image(systemName: Theme.Symbol.failed)
                .foregroundStyle(Theme.Color.swiftUIWarning)
            // A dropped PDF nothing identified has no DOI to look up again.
            Text(hasIdentifier ? "The metadata lookup failed." : "Paper could not identify this PDF. Add its DOI to look it up.")
                .font(Theme.Font.swiftUISecondary)
                .foregroundStyle(Theme.Color.swiftUISecondaryText)
                .fixedSize(horizontal: false, vertical: true)
            Spacer()
            if hasIdentifier {
                Button("Retry") { store.retryLookup(itemId: detail.id) }
                    .controlSize(.small)
            } else {
                Button("Add DOI…") { showEditor = true }
                    .controlSize(.small)
            }
        }
        .padding(Theme.Size.gap + Theme.Size.smallGap)
        .background(Theme.Color.swiftUICard, in: RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous))
    }

    /// Whether the paper has a DOI or arXiv ID to look up and search by.
    private var hasIdentifier: Bool {
        detail.fields.doi != nil || detail.fields.arxivId != nil
    }

    private var pdfAttachment: AttachmentRow? {
        detail.attachments.first { $0.mime == "application/pdf" }
    }

    private func openPDF() {
        NSApp.sendAction(#selector(MainWindowController.openSelectedPaper(_:)), to: nil, from: nil)
    }

    private var actions: some View {
        HStack(spacing: Theme.Size.gap) {
            if let doi = detail.fields.doi {
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(doi, forType: .string)
                    store.show(Toast(id: "copied", style: .done, message: "DOI copied"))
                } label: {
                    Label("Copy DOI", systemImage: Theme.Symbol.copy)
                }
                .help("Copy the DOI to the clipboard")
            }
            Button {
                showEditor = true
            } label: {
                Label("Edit", systemImage: Theme.Symbol.edit)
            }
            .help("Edit the metadata")
            Spacer()
            Menu {
                if let url = detail.fields.url.flatMap(URL.init(string:)) {
                    Button("Open Publisher Page") { NSWorkspace.shared.open(url) }
                }
                Button("Retry Lookup") { store.retryLookup(itemId: detail.id) }
                    .disabled(detail.lookupStatus == .pending || !hasIdentifier)
                Button("Attach PDF…") { store.chooseFileToAttach(itemId: detail.id) }
                if let attachment = pdfAttachment {
                    Button("Reveal PDF in Finder") { store.revealAttachment(id: attachment.id) }
                }
                Divider()
                Button("Delete", role: .destructive) { store.delete(itemIds: [detail.id]) }
            } label: {
                Image(systemName: Theme.Symbol.more)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("More actions")
        }
        .controlSize(.regular)
    }

    // MARK: Sections

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            Divider()
            Text(title)
                .font(Theme.Font.swiftUISectionLabel)
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
                .textCase(.uppercase)
                .padding(.top, Theme.Size.smallGap)
            content()
        }
    }

    /// The attached PDF, or where the search for one is.
    @ViewBuilder
    private var pdf: some View {
        if let attachment = pdfAttachment {
            HStack(spacing: Theme.Size.gap) {
                Image(systemName: Theme.Symbol.pdf)
                    .font(Theme.Font.swiftUIPaletteIcon)
                    .foregroundStyle(Theme.Color.swiftUIAccent)
                VStack(alignment: .leading, spacing: 0) {
                    Text(URL(fileURLWithPath: attachment.relativePath).lastPathComponent)
                        .font(Theme.Font.swiftUIBody)
                        .foregroundStyle(Theme.Color.swiftUIText)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Text(ByteCountFormatter.string(fromByteCount: attachment.byteSize, countStyle: .file))
                        .font(Theme.Font.swiftUICaption)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
                Spacer(minLength: 0)
                Button("Open", action: openPDF)
                    .buttonStyle(.borderedProminent)
                    .help("Read the PDF in a new tab (⌘↓)")
            }
            .padding(Theme.Size.gap + Theme.Size.smallGap)
            .background(Theme.Color.swiftUICard, in: RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous))
            .contentShape(Rectangle())
            .onTapGesture(count: 2, perform: openPDF)
            .help("Double click to read")
            .contextMenu {
                Button("Open PDF", action: openPDF)
                Button("Reveal in Finder") { store.revealAttachment(id: attachment.id) }
            }
        } else {
            switch store.pdfStatus[detail.id] {
            case .searching:
                HStack(spacing: Theme.Size.gap) {
                    ProgressView().controlSize(.small)
                    Text("Downloading the open access PDF…")
                        .font(Theme.Font.swiftUISecondary)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
            case .notFound:
                pdfMissing("No open access PDF was found.", retryTitle: "Search Again")
            case .failed:
                pdfMissing("The PDF could not be downloaded.", retryTitle: "Try Again")
            case .found, nil:
                pdfMissing(nil, retryTitle: "Find PDF")
            }
        }
    }

    private func pdfMissing(_ message: String?, retryTitle: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            if let message {
                Text(message)
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
            }
            HStack(spacing: Theme.Size.gap) {
                // Open access sources are searched by DOI or arXiv ID.
                if hasIdentifier {
                    Button {
                        store.findPdf(itemId: detail.id)
                    } label: {
                        Label(retryTitle, systemImage: Theme.Symbol.findPdf)
                    }
                    .help("Look for an open access PDF in arXiv, OpenAlex, Unpaywall, and Europe PMC")
                }
                Button {
                    store.chooseFileToAttach(itemId: detail.id)
                } label: {
                    Label("Attach…", systemImage: Theme.Symbol.attach)
                }
                .help("Copy a PDF from your Mac into the library")
            }
            .controlSize(.small)
        }
    }

    private var tags: some View {
        FlowLayout(spacing: Theme.Size.gap - Theme.Size.smallGap) {
            ForEach(detail.tags, id: \.id) { tag in
                TagChip(tag: tag) {
                    store.setTags(itemId: detail.id, names: detail.tags.filter { $0.id != tag.id }.map(\.name))
                }
            }
            TextField("Add tag", text: $newTag)
                .textFieldStyle(.plain)
                .font(Theme.Font.swiftUISecondary)
                .frame(minWidth: 72, maxWidth: 140)
                .padding(.horizontal, Theme.Size.gap)
                .padding(.vertical, Theme.Size.smallGap)
                .background(Theme.Color.swiftUICard, in: RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous))
                .onSubmit {
                    let name = newTag.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !name.isEmpty else { return }
                    store.setTags(itemId: detail.id, names: detail.tags.map(\.name) + [name])
                    newTag = ""
                }
        }
    }

    private var collections: some View {
        VStack(alignment: .leading, spacing: Theme.Size.smallGap) {
            let member = store.collections.filter { detail.collectionIds.contains($0.id) }
            ForEach(member, id: \.id) { collection in
                HStack(spacing: Theme.Size.gap) {
                    Image(systemName: Theme.Symbol.collection)
                        .foregroundStyle(Theme.Color.swiftUIAccent)
                    Text(collection.name)
                        .font(Theme.Font.swiftUIBody)
                    Spacer()
                    Button {
                        store.removeFromCollection(collectionId: collection.id, itemIds: [detail.id])
                    } label: {
                        Image(systemName: Theme.Symbol.close)
                    }
                    .buttonStyle(.borderless)
                    .controlSize(.small)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .accessibilityLabel("Remove from \(collection.name)")
                }
            }
            let others = store.collections.filter { !detail.collectionIds.contains($0.id) }
            if !others.isEmpty {
                Menu {
                    ForEach(others, id: \.id) { collection in
                        Button(String(repeating: "    ", count: Int(collection.depth)) + collection.name) {
                            store.addToCollection(collectionId: collection.id, itemIds: [detail.id])
                        }
                    }
                } label: {
                    Label("Add to Collection", systemImage: Theme.Symbol.addPaper)
                        .font(Theme.Font.swiftUISecondary)
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            } else if member.isEmpty {
                Text("No collections yet.")
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
            }
        }
    }

    private var notes: some View {
        ZStack(alignment: .topLeading) {
            if note.isEmpty {
                Text("Write a note in Markdown…")
                    .font(Theme.Font.swiftUIBody)
                    .foregroundStyle(Theme.Color.swiftUITertiaryText)
                    .padding(.horizontal, Theme.Size.gap + 1)
                    .padding(.vertical, Theme.Size.gap)
                    .allowsHitTesting(false)
            }
            TextEditor(text: $note)
                .font(Theme.Font.swiftUIBody)
                .scrollContentBackground(.hidden)
                .padding(Theme.Size.smallGap)
                .frame(minHeight: Theme.Size.noteMinHeight)
                .onChange(of: note) { _, text in
                    store.scheduleNoteSave(itemId: detail.id, markdown: text)
                }
                // The note may have been edited next to the PDF.
                .onChange(of: store.noteRevision) {
                    note = store.note(itemId: detail.id)
                }
        }
        .background(Theme.Color.swiftUICard, in: RoundedRectangle(cornerRadius: Theme.Size.cardRadius, style: .continuous))
    }

    private var details: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: Theme.Size.padding, verticalSpacing: Theme.Size.smallGap) {
            detailRow("Venue", detail.fields.containerTitle)
            detailRow("Publisher", detail.fields.publisher)
            detailRow("Published", detail.fields.publishedDate)
            detailRow("ISSN", detail.fields.issn)
            detailRow("Language", detail.fields.language)
            detailRow("Added", Self.dateFormatter.string(from: detail.addedAt))
            detailRow("Modified", Self.dateFormatter.string(from: detail.modifiedAt))
        }
    }

    @ViewBuilder
    private func detailRow(_ label: String, _ value: String?) -> some View {
        if let value, !value.isEmpty {
            GridRow {
                Text(label)
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .gridColumnAlignment(.trailing)
                Text(value)
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUIText)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}

struct TagChip: View {
    let tag: Tag
    let remove: () -> Void

    var body: some View {
        HStack(spacing: Theme.Size.smallGap) {
            Circle()
                .fill(Color(nsColor: NSColor(hexString: tag.color) ?? Theme.Color.defaultTag))
                .frame(width: Theme.Size.tagDotSize - 2, height: Theme.Size.tagDotSize - 2)
            Text(tag.name)
                .font(Theme.Font.swiftUISecondary)
            Button(action: remove) {
                Image(systemName: Theme.Symbol.close)
                    .font(.system(size: 9, weight: .bold))
            }
            .buttonStyle(.borderless)
            .foregroundStyle(Theme.Color.swiftUISecondaryText)
            .accessibilityLabel("Remove tag \(tag.name)")
        }
        .padding(.horizontal, Theme.Size.gap)
        .padding(.vertical, Theme.Size.smallGap)
        .background(Theme.Color.swiftUICard, in: Capsule())
    }
}

/// Wraps chips onto as many lines as they need.
struct FlowLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        return place(in: width, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = place(in: bounds.width, subviews: subviews)
        for (index, origin) in result.origins.enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func place(in width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var lineHeight: CGFloat = 0
        var maxX: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                x = 0
                y += lineHeight + spacing
                lineHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            lineHeight = max(lineHeight, size.height)
            maxX = max(maxX, x - spacing)
        }
        return (CGSize(width: maxX, height: y + lineHeight), origins)
    }
}

/// Edits a paper's fields, or takes them for a new paper typed in by hand,
/// such as a manuscript with no DOI yet. Only the title is required.
struct MetadataEditor: View {
    enum Mode {
        /// Saves one `MetadataPatch` for the paper.
        case edit(ItemDetail, save: (MetadataPatch) -> Void)
        /// Adds a paper with these fields and the PDF, if one was picked.
        /// Returns whether it was added; the sheet stays open when not.
        case create(add: (ItemFields, URL?) -> Bool)
    }

    @Environment(\.dismiss) private var dismiss
    let mode: Mode
    /// Closes the sheet when AppKit presented it; SwiftUI sheets use `dismiss`.
    var close: (() -> Void)?

    @State private var title: String
    @State private var authors: String
    @State private var container: String
    @State private var containerShort: String
    @State private var year: String
    @State private var volume: String
    @State private var issue: String
    @State private var pages: String
    @State private var abstractText: String
    @State private var doi: String
    @State private var arxivId: String
    @State private var url: String
    @State private var kind: ItemKind
    @State private var pdf: URL?
    @FocusState private var titleFocused: Bool

    init(mode: Mode, close: (() -> Void)? = nil) {
        self.mode = mode
        self.close = close
        var fields = ItemFields(kind: .other, doi: nil, arxivId: nil, title: "", creators: [], containerTitle: nil,
                                containerShort: nil, publisher: nil, year: nil, publishedDate: nil, volume: nil, issue: nil,
                                pages: nil, abstractText: nil, url: nil, issn: nil, language: nil, extra: nil)
        if case .edit(let detail, _) = mode { fields = detail.fields }
        _title = State(initialValue: fields.title)
        _authors = State(initialValue: fields.creators.map(\.displayName).joined(separator: "\n"))
        _container = State(initialValue: fields.containerTitle ?? "")
        _containerShort = State(initialValue: fields.containerShort ?? "")
        _year = State(initialValue: fields.year.map(String.init) ?? "")
        _volume = State(initialValue: fields.volume ?? "")
        _issue = State(initialValue: fields.issue ?? "")
        _pages = State(initialValue: fields.pages ?? "")
        _abstractText = State(initialValue: fields.abstractText ?? "")
        _doi = State(initialValue: fields.doi ?? "")
        _arxivId = State(initialValue: fields.arxivId ?? "")
        _url = State(initialValue: fields.url ?? "")
        _kind = State(initialValue: fields.kind ?? .other)
    }

    private var isNew: Bool {
        if case .create = mode { return true }
        return false
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    TextField("Title", text: $title, axis: .vertical)
                        .lineLimit(1...4)
                        .focused($titleFocused)
                    Picker("Type", selection: $kind) {
                        ForEach([ItemKind.journalArticle, .conferencePaper, .preprint, .other], id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }
                    TextField("Authors", text: $authors, prompt: Text("One per line, given name first"), axis: .vertical)
                        .lineLimit(2...8)
                } header: {
                    if isNew {
                        Text("New Paper")
                            .font(.headline)
                            .foregroundStyle(Theme.Color.swiftUIText)
                    }
                }
                Section("Abstract") {
                    TextField("Abstract", text: $abstractText, prompt: Text("Optional"), axis: .vertical)
                        .lineLimit(3...8)
                        .labelsHidden()
                }
                if isNew {
                    Section("PDF") { pdfRow }
                }
                Section("Venue") {
                    TextField("Journal or proceedings", text: $container)
                    TextField("Short name", text: $containerShort)
                    HStack {
                        TextField("Year", text: $year)
                        TextField("Volume", text: $volume)
                        TextField("Issue", text: $issue)
                        TextField("Pages", text: $pages)
                    }
                }
                Section("Identifiers") {
                    TextField("DOI", text: $doi)
                    TextField("arXiv ID", text: $arxivId)
                    TextField("URL", text: $url)
                }
            }
            .formStyle(.grouped)
            Divider()
            HStack {
                if isNew {
                    Text("Only the title is required.")
                        .font(Theme.Font.swiftUISecondary)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
                Spacer()
                Button("Cancel", action: finish)
                    .keyboardShortcut(.cancelAction)
                Button(isNew ? "Add Paper" : "Save", action: save)
                    .keyboardShortcut(.defaultAction)
                    .disabled(trimmed(title).isEmpty)
            }
            .padding(Theme.Size.padding)
        }
        .frame(width: Theme.Size.metadataEditorWidth, height: Theme.Size.metadataEditorHeight)
        .onAppear { if isNew { titleFocused = true } }
    }

    /// The picked PDF with a way to drop it, or a button to pick one. A PDF
    /// dragged from Finder onto the row is picked too.
    private var pdfRow: some View {
        HStack(spacing: Theme.Size.gap) {
            Image(systemName: Theme.Symbol.pdf)
                .foregroundStyle(pdf == nil ? Theme.Color.swiftUITertiaryText : Theme.Color.swiftUIAccent)
            Text(pdf?.lastPathComponent ?? "No PDF yet. You can attach one later.")
                .foregroundStyle(pdf == nil ? Theme.Color.swiftUISecondaryText : Theme.Color.swiftUIText)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
            if pdf != nil {
                Button("Remove") { pdf = nil }
            }
            Button(pdf == nil ? "Choose…" : "Change…", action: choosePdf)
        }
        .dropDestination(for: URL.self) { urls, _ in
            guard let url = urls.first(where: { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .pdf) == true }) else { return false }
            pdf = url
            return true
        }
    }

    private func choosePdf() {
        let panel = NSOpenPanel()
        panel.title = "Choose PDF"
        panel.message = "Paper copies the PDF into the library and leaves the original alone."
        panel.allowedContentTypes = [.pdf]
        panel.allowsMultipleSelection = false
        panel.prompt = "Choose"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        pdf = url
    }

    private func save() {
        guard !trimmed(title).isEmpty else { return }
        switch mode {
        case .edit(_, let save):
            save(patch)
        case .create(let add):
            guard add(newFields, pdf) else { return }
        }
        finish()
    }

    private func finish() {
        if let close { close() } else { dismiss() }
    }

    private func trimmed(_ text: String) -> String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Typed text, or nil for a field left blank.
    private func optional(_ text: String) -> String? {
        let text = trimmed(text)
        return text.isEmpty ? nil : text
    }

    private var creators: [Creator] {
        authors
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .map { name -> Creator in
                var parts = name.split(separator: " ").map(String.init)
                let family = parts.popLast()
                return Creator(role: .author, family: family, given: parts.isEmpty ? nil : parts.joined(separator: " "), literal: nil, orcid: nil)
            }
    }

    private var patch: MetadataPatch {
        MetadataPatch(
            kind: kind,
            doi: trimmed(doi),
            arxivId: trimmed(arxivId),
            title: trimmed(title),
            creators: creators,
            containerTitle: container,
            containerShort: containerShort,
            publisher: nil,
            year: Int32(trimmed(year)),
            publishedDate: nil,
            volume: volume,
            issue: issue,
            pages: pages,
            abstractText: trimmed(abstractText),
            url: url
        )
    }

    private var newFields: ItemFields {
        ItemFields(
            kind: kind,
            doi: optional(doi),
            arxivId: optional(arxivId),
            title: trimmed(title),
            creators: creators,
            containerTitle: optional(container),
            containerShort: optional(containerShort),
            publisher: nil,
            year: Int32(trimmed(year)),
            publishedDate: nil,
            volume: optional(volume),
            issue: optional(issue),
            pages: optional(pages),
            abstractText: optional(abstractText),
            url: optional(url),
            issn: nil,
            language: nil,
            extra: nil
        )
    }
}
