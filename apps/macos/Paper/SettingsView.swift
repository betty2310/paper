import PaperEngine
import SwiftUI

/// Library settings the user may change: what the metadata APIs see.
struct SettingsView: View {
    @ObservedObject var store: LibraryStore
    @State private var contactEmail: String
    @State private var openalexApiKey: String
    @State private var anthropicApiKey: String
    @State private var jevApiKey: String
    @State private var editingTopics = false

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
            Section {
                LabeledContent("Library", value: store.libraryPath)
                    .font(Theme.Font.swiftUISecondary)
            }
        }
        .formStyle(.grouped)
        // A grouped form scrolls and so has no height of its own to size the window by.
        .frame(width: Theme.Size.settingsWidth, height: Theme.Size.settingsHeight)
        .onChange(of: contactEmail) { save() }
        .onChange(of: openalexApiKey) { save() }
        .onChange(of: anthropicApiKey) { save() }
        .onChange(of: jevApiKey) { save() }
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
