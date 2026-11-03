import PaperEngine
import SwiftUI

/// Library settings the user may change: what the metadata APIs see.
struct SettingsView: View {
    @ObservedObject var store: LibraryStore
    @State private var contactEmail: String
    @State private var openalexApiKey: String

    init(store: LibraryStore) {
        self.store = store
        let settings = store.settings
        _contactEmail = State(initialValue: settings.contactEmail)
        _openalexApiKey = State(initialValue: settings.openalexApiKey)
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
                LabeledContent("Library", value: store.libraryPath)
                    .font(Theme.Font.swiftUISecondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: Theme.Size.settingsWidth)
        .onChange(of: contactEmail) { save() }
        .onChange(of: openalexApiKey) { save() }
    }

    private func save() {
        var settings = store.settings
        let email = contactEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        let key = openalexApiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard settings.contactEmail != email || settings.openalexApiKey != key else { return }
        settings.contactEmail = email
        settings.openalexApiKey = key
        store.updateSettings(settings)
    }
}
