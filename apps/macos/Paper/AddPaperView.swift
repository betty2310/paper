import AppKit
import PaperEngine
import SwiftUI

/// The popover under the toolbar's plus button: paste an identifier, see what
/// it will add, press Return.
struct AddPaperView: View {
    let add: (String) -> Void
    @State private var text = ""
    @FocusState private var focused: Bool

    private var parsed: String? { parseIdentifier(text: text) }
    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            Text("Add Paper")
                .font(.headline)
            TextField("DOI, doi.org link, or arXiv ID", text: $text)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit(submit)
                .accessibilityLabel("Identifier")
            HStack(spacing: Theme.Size.smallGap) {
                if trimmed.isEmpty {
                    Text("Paper looks the metadata up for you.")
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                } else if let parsed {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(Theme.Color.swiftUIAccent)
                    Text("Will add ")
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    + Text(parsed).font(.body.monospaced())
                } else {
                    Image(systemName: Theme.Symbol.failed)
                        .foregroundStyle(Theme.Color.swiftUIWarning)
                    Text("Not a DOI or arXiv ID.")
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
                Spacer(minLength: 0)
            }
            .font(Theme.Font.swiftUISecondary)
            .lineLimit(1)
            HStack {
                Spacer()
                Button("Add", action: submit)
                    .keyboardShortcut(.defaultAction)
                    .disabled(parsed == nil)
            }
        }
        .padding(Theme.Size.padding)
        .frame(width: Theme.Size.popoverWidth)
        .onAppear {
            focused = true
            // Start with the clipboard when it already holds an identifier.
            if text.isEmpty, let clip = NSPasteboard.general.string(forType: .string), parseIdentifier(text: clip) != nil {
                text = clip.trimmingCharacters(in: .whitespacesAndNewlines)
            }
        }
    }

    private func submit() {
        guard parsed != nil else { return }
        let identifier = trimmed
        text = ""
        add(identifier)
    }
}
