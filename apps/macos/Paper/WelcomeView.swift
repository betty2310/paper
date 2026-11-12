import AppKit
import SwiftUI

/// Shown when no library is open: create one, open one, or pick a recent one.
struct WelcomeView: View {
    var recents: [String]
    let openRecent: (String) -> Void
    let openOther: () -> Void
    let createNew: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: Theme.Size.gap) {
                Image(systemName: Theme.Symbol.allPapers)
                    .font(.system(size: 44, weight: .light))
                    .foregroundStyle(Theme.Color.swiftUIAccent)
                    .padding(.bottom, Theme.Size.gap)
                Text("Paper")
                    .font(Theme.Font.swiftUIWelcomeTitle)
                Text("A reference manager for scientific papers. Paste a DOI, and Paper looks up the rest.")
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button(action: createNew) {
                    Label("New Library…", systemImage: Theme.Symbol.newLibrary)
                        .frame(maxWidth: .infinity)
                }
                .controlSize(.large)
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                Button(action: openOther) {
                    Label("Open Library…", systemImage: Theme.Symbol.openLibrary)
                        .frame(maxWidth: .infinity)
                }
                .controlSize(.large)
                .keyboardShortcut("o", modifiers: .command)
            }
            .padding(Theme.Size.padding * 2)
            .frame(width: Theme.Size.welcomeWidth * 0.52)

            recentList
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color(nsColor: Theme.Color.listBackground))
        }
        .frame(width: Theme.Size.welcomeWidth, height: Theme.Size.welcomeHeight)
        .font(Theme.Font.swiftUIDefault)
    }

    private var recentList: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Recent")
                .font(Theme.Font.swiftUISectionLabel)
                .foregroundStyle(Theme.Color.swiftUITertiaryText)
                .textCase(.uppercase)
                .padding(.horizontal, Theme.Size.padding)
                .padding(.top, Theme.Size.padding * 2)
                .padding(.bottom, Theme.Size.gap)
            if recents.isEmpty {
                Spacer()
                Text("Libraries you open show up here.")
                    .font(Theme.Font.swiftUISecondary)
                    .foregroundStyle(Theme.Color.swiftUISecondaryText)
                    .frame(maxWidth: .infinity)
                Spacer()
            } else {
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(recents, id: \.self) { path in
                            RecentLibraryRow(path: path) { openRecent(path) }
                        }
                    }
                    .padding(.horizontal, Theme.Size.gap)
                }
            }
        }
    }
}

/// One recent library: name, folder, and a hover highlight.
struct RecentLibraryRow: View {
    let path: String
    let open: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: open) {
            HStack(spacing: Theme.Size.gap + Theme.Size.smallGap) {
                Image(systemName: Theme.Symbol.library)
                    .font(.system(size: Theme.Size.iconSize + 4, weight: .light))
                    .foregroundStyle(Theme.Color.swiftUIAccent)
                    .frame(width: Theme.Size.iconSize + 12)
                VStack(alignment: .leading, spacing: 1) {
                    Text(URL(fileURLWithPath: path).lastPathComponent)
                        .font(Theme.Font.swiftUIBody)
                        .foregroundStyle(Theme.Color.swiftUIText)
                    Text(URL(fileURLWithPath: path).deletingLastPathComponent().path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                        .font(Theme.Font.swiftUICaption)
                        .foregroundStyle(Theme.Color.swiftUISecondaryText)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Theme.Size.gap)
            .padding(.vertical, Theme.Size.gap - Theme.Size.smallGap / 2)
            .background(
                RoundedRectangle(cornerRadius: Theme.Size.chipRadius, style: .continuous)
                    .fill(hovering ? Theme.Color.swiftUICard : .clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(path)
    }
}
