import AppKit
import PaperEngine
import SwiftUI

/// Where an entry of a reference list leads on the web: its DOI, its arXiv
/// page, a web address it prints, or else a Google Scholar search for it.
struct ReferenceLink {
    enum Kind { case doi, arxiv, web, search }

    let kind: Kind
    let url: URL
    /// The address as the card shows it, without the scheme.
    let label: String
    /// What Add to Library takes, when the entry has an identifier.
    let identifier: String?

    private static let scholar = "https://scholar.google.com/scholar"
    private static let maxQueryLength = 300

    init(_ reference: Reference) {
        identifier = reference.doi ?? reference.arxivId
        // An arXiv page is open to everyone; a publisher's page may not be.
        if let id = reference.arxivId, reference.doi == nil || reference.doi?.hasPrefix("10.48550/") == true,
           let url = Self.url("https://arxiv.org/abs/", id) {
            (kind, self.url, label) = (.arxiv, url, "arxiv.org/abs/\(id)")
        } else if let doi = reference.doi, let url = Self.url("https://doi.org/", doi) {
            (kind, self.url, label) = (.doi, url, "doi.org/\(doi)")
        } else if let text = reference.url, let url = URL(string: text), url.host != nil {
            (kind, self.url, label) = (.web, url, (url.host ?? "") + url.path)
        } else {
            var components = URLComponents(string: Self.scholar)!
            let query = reference.title ?? String(reference.text.prefix(Self.maxQueryLength))
            components.queryItems = [URLQueryItem(name: "q", value: query)]
            (kind, url, label) = (.search, components.url!, "Google Scholar")
        }
    }

    var actionTitle: String {
        switch kind {
        case .doi: "Open DOI"
        case .arxiv: "Open on arXiv"
        case .web: "Open Link"
        case .search: "Search Scholar"
        }
    }

    private static func url(_ base: String, _ path: String) -> URL? {
        path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed).flatMap { URL(string: base + $0) }
    }
}

/// The reference the card shows, and what its buttons do.
@MainActor
final class ReferenceCardModel: ObservableObject {
    @Published var reference: Reference?
    /// Off for an entry of the references section: the reader is looking at
    /// its text already.
    @Published var showsText = true
    @Published var isSearching = false
    @Published var isAdding = false
    @Published var copied = false
    var open: () -> Void = {}
    var openInLibrary: () -> Void = {}
    var add: () -> Void = {}
    var copy: () -> Void = {}
    var hovering: (Bool) -> Void = { _ in }
}

/// What a citation cites, in a small floating card: the entry's number,
/// how its link was found, the cited paper's title and year when known, the
/// entry as printed, and buttons to open the paper on the web or in Paper.
struct ReferenceCard: View {
    @ObservedObject var model: ReferenceCardModel

    var body: some View {
        if let reference = model.reference {
            card(reference, link: ReferenceLink(reference))
                .padding(Theme.Size.referenceCardPadding)
                .frame(width: Theme.Size.referenceCardWidth, alignment: .leading)
                .modifier(FloatingGlass(shape: RoundedRectangle(cornerRadius: Theme.Size.referenceCardRadius, style: .continuous)) { content, shape in
                    content
                        .background(Theme.Color.swiftUIPanel, in: shape)
                        .overlay(shape.strokeBorder(Theme.Color.swiftUISeparator, lineWidth: 1))
                        .shadow(color: Theme.Markup.shadowColor, radius: Theme.Markup.shadowRadius, y: Theme.Markup.shadowY)
                })
                .onHover { model.hovering($0) }
                .font(Theme.Font.swiftUIDefault)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(reference.number.map { "Reference \($0)" } ?? "Reference")
                // Room for the shadow inside the hosting view.
                .padding(Theme.Markup.shadowRadius)
                .fixedSize()
        }
    }

    private func card(_ reference: Reference, link: ReferenceLink) -> some View {
        VStack(alignment: .leading, spacing: Theme.Size.gap) {
            HStack(spacing: Theme.Size.gap) {
                if let number = reference.number {
                    Text("\(number)")
                        .font(Theme.Font.swiftUISectionLabel.monospacedDigit())
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                        .padding(.horizontal, Theme.Size.referenceBadgePadding)
                        .frame(height: Theme.Size.referenceBadgeHeight)
                        .background(Theme.Color.swiftUIReferenceBadge, in: Capsule())
                }
                Text(source(reference))
                    .font(Theme.Font.swiftUICaption)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                Spacer(minLength: 0)
                if let year = reference.year {
                    Text(String(year))
                        .font(Theme.Font.swiftUICaption.monospacedDigit())
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                }
            }
            if let title = reference.title {
                Text(title)
                    .font(Theme.Font.swiftUIReferenceTitle)
                    .foregroundStyle(Theme.Color.swiftUIText)
                    .lineLimit(Theme.Size.referenceTitleLines)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if model.showsText {
                Text(reference.text)
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .lineLimit(Theme.Size.referenceTextLines)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            HStack(spacing: Theme.Size.smallGap) {
                if model.isSearching {
                    ProgressView().controlSize(.mini)
                    Text("Looking up this reference…")
                } else {
                    Text(link.label).lineLimit(1).truncationMode(.middle)
                }
            }
            .font(Theme.Font.swiftUICaption)
            .foregroundStyle(Theme.Color.swiftUITertiaryText)
            Divider()
            actions(reference, link: link)
        }
    }

    private func actions(_ reference: Reference, link: ReferenceLink) -> some View {
        HStack(spacing: Theme.Size.smallGap) {
            Button(action: model.open) {
                Label(link.actionTitle, systemImage: link.kind == .search ? Theme.Symbol.search : Theme.Symbol.openLink)
                    .foregroundStyle(Theme.Color.swiftUIAccent)
            }
            .help(link.kind == .search ? "Search Google Scholar for this reference (⌘-click)" : "Open \(link.label) in the browser (⌘-click)")
            if reference.itemId != nil {
                Button(action: model.openInLibrary) {
                    Label("Open in Paper", systemImage: Theme.Symbol.inLibrary)
                }
                .help("This paper is in your library")
            } else if link.identifier != nil {
                Button(model.isAdding ? "Adding…" : "Add to Library", action: model.add)
                    .disabled(model.isAdding)
                    .help("Add the cited paper to your library")
            }
            Spacer(minLength: 0)
            Button(action: model.copy) {
                Image(systemName: model.copied ? Theme.Symbol.copied : Theme.Symbol.copy)
            }
            .help(link.kind == .search ? "Copy the reference" : "Copy the link")
            .accessibilityLabel(link.kind == .search ? "Copy Reference" : "Copy Link")
        }
        .buttonStyle(MarkupTextButtonStyle())
        // The buttons' fills reach into the card's padding; their text lines up with the text above.
        .padding(.horizontal, -Theme.Size.markupButtonPadding)
    }

    private func source(_ reference: Reference) -> String {
        switch reference.foundBy {
        case .deposited: "From the publisher's list"
        case .printed: "Printed in the entry"
        case .openAlex: "Matched in OpenAlex"
        case .search: "Matched by Crossref search"
        case .unresolved: model.isSearching ? "Not linked yet" : "No link found"
        }
    }
}
