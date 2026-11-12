import AppKit
import PaperEngine
import SwiftUI

/// The Settings window's content: one toolbar tab per pane, as Mac apps
/// have. Each pane fills the window, which the user may resize.
final class SettingsViewController: NSTabViewController {
    let store: LibraryStore

    init(store: LibraryStore) {
        self.store = store
        super.init(nibName: nil, bundle: nil)
        tabStyle = .toolbar
        addPane(GeneralSettingsView(store: store), label: "General", symbol: Theme.Symbol.generalSettings)
        addPane(AppearanceSettingsView(), label: "Appearance", symbol: Theme.Symbol.appearanceSettings)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    private func addPane(_ pane: some View, label: String, symbol: String) {
        let host = NSHostingController(rootView: pane)
        // The window takes the selected pane's title.
        host.title = label
        // The window keeps the size the user gave it, whichever pane shows.
        host.sizingOptions = []
        let item = NSTabViewItem(viewController: host)
        item.label = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        addTabViewItem(item)
    }
}

/// Library settings the user may change: what the metadata APIs see, and
/// whether Paper sends crash reports.
struct GeneralSettingsView: View {
    @ObservedObject var store: LibraryStore
    @State private var contactEmail: String
    @State private var openalexApiKey: String
    @State private var anthropicApiKey: String
    @State private var jevApiKey: String
    @State private var editingTopics = false
    /// For the app, not the library; the others are the library's own.
    @AppStorage(CrashReports.enabledKey) private var sendCrashReports = false

    init(store: LibraryStore) {
        self.store = store
        let settings = store.settings
        _contactEmail = State(initialValue: settings.contactEmail)
        _openalexApiKey = State(initialValue: settings.openalexApiKey)
        _anthropicApiKey = State(initialValue: settings.anthropicApiKey)
        _jevApiKey = State(initialValue: settings.jevApiKey)
    }

    var body: some View {
        Form {
            Section {
                TextField("Contact email", text: $contactEmail, prompt: Text("you@example.org"))
                    .textContentType(.emailAddress)
                    .onSubmit(save)
                Text("Sent in the user agent to Crossref and OpenAlex, as their polite usage rules ask. It gets you the faster request pool.")
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
            }
            Section {
                SecureField("OpenAlex API key", text: $openalexApiKey, prompt: Text("Optional"))
                    .onSubmit(save)
                Text("Only needed if you have a key; lookups work without one.")
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
            }
            Section {
                SecureField("Anthropic API key", text: $anthropicApiKey, prompt: Text("Optional"))
                    .onSubmit(save)
                Text("With a key, a dropped PDF that names no DOI and whose title finds nothing is read by Claude Haiku from an image of page 1. Stored in Paper's settings file on this Mac.")
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Section {
                SecureField("Jev API key", text: $jevApiKey, prompt: Text("Optional"))
                    .onSubmit(save)
                Text("With a key, every paper you add gets up to three tags from your topics, which TypeSafe's Jev model picks from its title, abstract, and keywords. Keys are at console.typesafe.ai.")
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .fixedSize(horizontal: false, vertical: true)
                LabeledContent("Topics") {
                    Button("Edit Topics…") { editingTopics = true }
                }
            }
            if CrashReports.isAvailable {
                Section {
                    Toggle("Send crash reports", isOn: $sendCrashReports)
                    Text(CrashReports.explanation)
                        .font(Theme.Font.swiftUICaption)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Section {
                LabeledContent("Library", value: store.libraryPath)
                    .font(Theme.Font.swiftUISecondary)
            }
        }
        .formStyle(.grouped)
        .font(Theme.Font.swiftUIDefault)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onChange(of: contactEmail) { save() }
        .onChange(of: openalexApiKey) { save() }
        .onChange(of: anthropicApiKey) { save() }
        .onChange(of: jevApiKey) { save() }
        .onChange(of: sendCrashReports) { CrashReports.update() }
        .sheet(isPresented: $editingTopics) { TopicsEditor(store: store) }
    }

    private func save() {
        var settings = store.settings
        let email = contactEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        let openalexKey = openalexApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let anthropicKey = anthropicApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let jevKey = jevApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard settings.contactEmail != email || settings.openalexApiKey != openalexKey
            || settings.anthropicApiKey != anthropicKey || settings.jevApiKey != jevKey else { return }
        settings.contactEmail = email
        settings.openalexApiKey = openalexKey
        settings.anthropicApiKey = anthropicKey
        settings.jevApiKey = jevKey
        store.updateSettings(settings)
    }
}

/// The app's look, the same in every library: for now, its two fonts.
struct AppearanceSettingsView: View {
    @Bindable private var fonts = FontChoice.shared
    private let families = NSFontManager.shared.availableFontFamilies.filter { !$0.hasPrefix(".") }

    var body: some View {
        Form {
            Section {
                picker("Title font", selection: $fonts.titleFamily, system: "New York")
                picker("Interface font", selection: $fonts.interfaceFamily, system: "San Francisco")
                Text("The title font sets paper titles in the list, the tabs, and the inspector. The interface font sets everything else.")
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Section {
                VStack(alignment: .leading, spacing: Theme.Size.smallGap) {
                    Text("Attention Is All You Need")
                        .font(Theme.Font.swiftUIInspectorTitle)
                        .foregroundStyle(Theme.Color.swiftUIText)
                    Text("Vaswani, Shazeer, Parmar, et al. · NeurIPS · 2017")
                        .font(Theme.Font.swiftUISecondary)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
                .padding(.vertical, Theme.Size.smallGap)
                .accessibilityElement(children: .combine)
            } header: {
                // A header set in the form's font would lose the system's weight.
                Text("Preview").font(Theme.Font.swiftUIHeadline)
            }
        }
        .formStyle(.grouped)
        .font(Theme.Font.swiftUIDefault)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// The system's font first, then every installed family. A family
    /// picked once and since removed still shows as picked.
    private func picker(_ label: String, selection: Binding<String?>, system: String) -> some View {
        let missing = selection.wrappedValue.flatMap { families.contains($0) ? nil : $0 }
        return Picker(label, selection: selection) {
            Text("\(system) (System)").tag(String?.none)
            Divider()
            ForEach((missing.map { [$0] } ?? []) + families, id: \.self) { family in
                Text(family).tag(String?.some(family))
            }
        }
    }
}

/// The library's topic list: the names automatic tags can have, each with
/// the description Jev reads to decide whether a paper is about it.
struct TopicsEditor: View {
    @ObservedObject var store: LibraryStore
    @Environment(\.dismiss) private var dismiss
    @State private var topics: [Row]
    @State private var error: String?
    @FocusState private var focused: Row.ID?

    struct Row: Identifiable {
        let id = UUID()
        var name: String
        var description: String
    }

    init(store: LibraryStore) {
        self.store = store
        _topics = State(initialValue: store.topics().map { Row(name: $0.name, description: $0.description) })
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    ForEach($topics) { $topic in
                        HStack(alignment: .firstTextBaseline, spacing: Theme.Size.gap) {
                            VStack(alignment: .leading, spacing: Theme.Size.smallGap) {
                                TextField("Name", text: $topic.name, prompt: Text("Topic name"))
                                    .labelsHidden()
                                    .focused($focused, equals: topic.id)
                                TextField("Description", text: $topic.description, prompt: Text("What it covers, and what it does not"), axis: .vertical)
                                    .labelsHidden()
                                    .lineLimit(1...4)
                                    .font(Theme.Font.swiftUISecondary)
                                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                            }
                            Button {
                                topics.removeAll { $0.id == topic.id }
                            } label: {
                                Image(systemName: Theme.Symbol.removeTopic)
                            }
                            .buttonStyle(ReaderBarButtonStyle())
                            .help("Remove \(topic.name)")
                            .accessibilityLabel("Remove \(topic.name)")
                        }
                    }
                } header: {
                    Text("Jev gives each new paper up to three of these topics as tags. A description that says what a topic does not cover, such as “Not radar jamming”, keeps look-alike papers out.")
                        .font(Theme.Font.swiftUICaption)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                        .textCase(nil)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .formStyle(.grouped)
            Divider()
            HStack {
                Button {
                    let row = Row(name: "", description: "")
                    topics.append(row)
                    focused = row.id
                } label: {
                    Label("Add Topic", systemImage: Theme.Symbol.addTopic)
                }
                if let error {
                    Text(error)
                        .font(Theme.Font.swiftUISecondary)
                        .foregroundStyle(Theme.Color.swiftUIDanger)
                        .lineLimit(2)
                }
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Save", action: save)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(Theme.Size.padding)
        }
        .frame(width: Theme.Size.topicsEditorWidth, height: Theme.Size.topicsEditorHeight)
        .font(Theme.Font.swiftUIDefault)
    }

    /// Rows left without a name are dropped; the engine refuses a name twice.
    private func save() {
        let list = topics
            .map { Topic(name: $0.name.trimmingCharacters(in: .whitespacesAndNewlines),
                         description: $0.description.trimmingCharacters(in: .whitespacesAndNewlines)) }
            .filter { !$0.name.isEmpty }
        do {
            try store.setTopics(list)
            dismiss()
        } catch let paperError as PaperError {
            error = LibraryStore.describe(paperError)
        } catch {
            self.error = error.localizedDescription
        }
    }
}
