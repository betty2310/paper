import PaperEngine
import SwiftUI

/// Library settings the user may change: what the metadata APIs see.
struct SettingsView: View {
    @ObservedObject var store: LibraryStore
    @State private var contactEmail: String
    @State private var openalexApiKey: String
    @State private var anthropicApiKey: String

    init(store: LibraryStore) {
        self.store = store
        let settings = store.settings
        _contactEmail = State(initialValue: settings.contactEmail)
        _openalexApiKey = State(initialValue: settings.openalexApiKey)
        _anthropicApiKey = State(initialValue: settings.anthropicApiKey)
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
    }

    private func save() {
        var settings = store.settings
        let email = contactEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        let openalexKey = openalexApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        let anthropicKey = anthropicApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard settings.contactEmail != email || settings.openalexApiKey != openalexKey
            || settings.anthropicApiKey != anthropicKey else { return }
        settings.contactEmail = email
        settings.openalexApiKey = openalexKey
        settings.anthropicApiKey = anthropicKey
        store.updateSettings(settings)
    }
}
