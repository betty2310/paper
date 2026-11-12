# UI design

Decided 2026-10-27. The reference mockups are `docs/design/library.png` and `docs/design/reader.png`. The implementation should match them to the pixel where the mockups are specific, and follow the rules below where they are not.

## Principles

1. The library is the main view and opens first. Each PDF opens in a tab next to the Library tab in the same window, never in a separate window.
2. The interface is elegant and quiet. Paper titles are set in a serif typeface, everything else in a sans-serif. There is one accent color, used only to mark selection, links, the primary button, and the active tab.
3. Everything is placed on a 4 px grid, borders are exactly one physical pixel, text sits on consistent baselines, and icons are one size. Nothing is blurry, misaligned, or off by a pixel. Check every screen at 1x and 2x device scale.
4. Nothing moves without a reason. Transitions are at most 150 ms and only for panels opening, tabs switching, and the selection toolbar appearing.

## Layout

The window is a three pane layout under a unified toolbar, in the style of Rex by Superlogical: the toolbar, the sidebar, the detail panel, and the gaps between them are one translucent chrome (the system's sidebar material), and the center sits on that chrome too, with no card of its own.

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●  ▯ │ All Papers  (▥ │ <PDF title> ✕ │ <PDF title>)            ▯ │ toolbar
│          │ 14 papers                                                      │
│ sidebar  │                                             detail panel       │
│ on the   │   center: paper list or PDF, on the chrome  on the chrome,     │
│ chrome   │                                             hideable           │
│          │                                                                │
└──────────────────────────────────────────────────────────────────────────────┘
```

The toolbar draws no background or separator of its own. It holds the sidebar toggle, the current list's name with its paper count under it, and, while any PDF is open, the tabs: a quiet track as tall as the two title lines that holds the Library icon and one tab per PDF, with the active tab raised out of it in white with a hairline border and a soft shadow. Library and PDF tabs use the same icon size. The tabs share the toolbar's free width equally and shrink to their icon when crowded. A PDF tab can be pinned from its context menu (right or Control click) or Window > Pin Tab: pinned tabs sit right after the Library icon, in the order they were pinned, as narrow as it, and show the paper's first letter on its preview color instead of the document icon and title. They have no close button and ignore a middle click; Close Tab in the context menu closes and unpins one. Unpinning moves the tab to just after the pinned tabs. Pinned tabs reopen behind the library at the next launch, and a paper deleted since, or without its PDF, drops out. The center starts under the toolbar and reaches the window's bottom edge and the panels on either side with no margin. It has no card, fill, border, or rounded corners: the paper list sits on the chrome like the sidebar, and the reader's page canvas fills the center. The split view dividers are invisible. The sidebar is shared by all tabs and can be collapsed with ⌘B. While a PDF tab is in front, the sidebar shows that PDF's table of contents instead of the library's lists: a CONTENTS group with the outline tree (top level entries expanded) and each entry's page number at the trailing edge. Going from the library to a PDF, the lists slide out to the leading side as the contents slide in from the trailing side (24 px, cross faded, the tab switch duration), and back the other way; between two PDFs the contents cross fade. Clicking an entry goes there, and Back returns, as after a link. The entry for the part at the top of the view stays selected while scrolling. A PDF without an outline shows "This PDF has no table of contents." in muted text. A segmented control at the top of the PDF sidebar switches between Contents and Pages; the choice is shared by all PDFs and remembered across launches. Pages is the thumbnail strip: every page as an 88 px wide image with a hairline border and its number under it, and a 2 px accent outline just outside the current page, whose number is accent colored. The strip follows the reader as it scrolls. Hovering a page darkens its border, and clicking a page goes there, with Back returning as after a link; the arrow keys move between pages. The detail panel belongs to the active tab and can be collapsed with ⌘⇧B. Both dividers are draggable. The window frame, both panel widths, and whether each panel is hidden are remembered across launches, so the window opens the way it was left; hiding or showing a panel is saved at once.

### Sidebar

Three groups with 11 px uppercase letter spaced labels: LIBRARY (All papers, Unread, Recently added, No PDF), COLLECTIONS (a tree with disclosure chevrons and folder icons, a "+" at the group label), and TAGS (color dot and name, a "+" at the group label; a tag's context menu lists the tag colors with a checkmark on its own and changes it). Rows are 36 px tall with 16 px left padding plus 24 px per nesting level. The selected row has the accent tint background and accent text. The bottom of the sidebar shows the library path in muted text with a switch icon that opens the library chooser.

### Library tab

A 52 px toolbar row holds a search field that fills the width and the primary button "Add DOI" at the right. The search field accepts a DOI, a doi.org URL, or an arXiv ID; when the text matches an identifier, pressing Enter adds the paper instead of searching, and the field shows an inline hint "Press Enter to add". Below the toolbar is a table with columns Title (serif, fills), Authors (first author et al.), Journal (short name), Year, Added, and PDF (icon or an outlined "No PDF" badge). Column headers are 11 px uppercase. Rows are 44 px tall with hairline dividers. A selected row has the accent tint. Double click or Enter opens the PDF in a tab. Sorting is by clicking a header, and the sort column shows a small chevron. Favorite papers come first in every list (All papers, the smart lists, collections, tags, and search results), each group in the chosen sort order. A favorite shows a filled accent star before the date on its title line; other rows show an outlined star while the pointer is over them. Clicking the star, ⌘D, Paper > Add to Favorites, the row's context menu, or the star button in the inspector's actions toggles it; with several papers selected, the command marks them all unless all are favorites already, which clears them. The star is a 20 px round target with the hover and pressed fills of the other small icon buttons, and the focus ring only for keyboard focus.

Dragging PDFs from Finder over the paper list covers it with an 8 px inset accent outline on a light accent tint and a card that says "Add 2 PDFs". Dropping adds one row per file, named after the file, which the engine then identifies. Each file gets a toast: "Added to library" with Open when the file named its DOI, "Added by matching its title" with Show when only the title matched, "Could not identify the PDF" with Show when nothing did, and "Already in your library" for a paper the library has. A paper nothing identified is listed with a warning and "Not identified. Add its DOI.", and its detail panel offers "Add DOI…" instead of Retry.

### Detail panel

The selected paper's title in 22 px serif, authors in 15 px sans, venue line "NeurIPS · 2020 · 33(1)" in muted sans, the DOI as an accent colored link with an external link icon, then a row of two buttons: "Copy BibTeX" and "Cite" (copies the APA reference, as rich text with its italics and as plain text). The PDF is opened from its row in the PDF section. Below are collapsible sections with uppercase labels and hairline dividers: ABSTRACT, ATTACHMENTS (file rows with size), TAGS (chips plus a "+" chip; each chip is filled with its tag's color at 16% behind the name, which is set in the same color made 45% darker in Light Mode and 35% lighter in Dark Mode so every color reads as text, and its dot opens a popover with the tag colors as 16 px dots, the current one ringed in the accent color), and NOTES (a quiet card that opens the note editor on click). A "⋯" menu at the end of the button row holds Copy DOI, Copy Citation Key, Export BibTeX…, Edit Metadata…, Open Publisher Page, Retry Lookup, Attach PDF…, Reveal PDF in Finder, and Delete. The DETAILS section lists the paper's citation key, and the metadata editor has a Citation key field; clearing it makes a new key from the metadata. With several papers selected, the panel offers Copy BibTeX, Cite, Export BibTeX…, and Delete for all of them.

The citation commands act on the list's selection, or on the paper in front while a PDF tab is. They are in the Paper menu (Copy BibTeX, Copy APA Citation, Copy Citation Key), in the paper list's context menu with Export BibTeX…, and Export BibTeX… is in the File menu. A collection's context menu in the sidebar exports the papers directly in it to a .bib file named after the collection; a single paper's file is named after its key. Each copy or export confirms with a toast, and an export's toast has Show to reveal the file in Finder.

Topic tags are ordinary tags, listed in the TAGS group and on the chips with the tags the user types. When Jev tags a paper that was just added, a toast with the paper's title says which tags were added, such as "Tagged GNSS spoofing, GNSS, and Inertial navigation"; that toast's message may take a second line so no tag is cut off, and the toast keeps the capsule's 20 px corner radius when it grows. Paper > Tag with Topics and the paper list's context menu ask Jev again for the paper in front or the selected papers, and one toast reports what was added. The Settings window has a Jev API key field with an Edit Topics… button that opens a sheet listing each topic's name and description, with Add Topic, a remove button on each row (the round 28 pt icon button of the reader's controls), Cancel, and Save. Below it, a Send crash reports toggle with the same explanation as the system alert that asks the question the first time Paper.app runs. The Settings window is resizable, with a toolbar of two tabs: General holds the fields above, and Appearance holds two font pickers, Title font (paper titles in the list, tabs, inspector, and toasts; New York by default) and Interface font (everything else; San Francisco by default), each listing the system font first and then every installed family, with a preview of a paper's title and metadata line. A change applies at once in every open window and is kept for every library. Icons and code keep the system fonts.

### Reader tab

The reader fills the center of its tab, and the detail panel shows the reader panel instead of the inspector while the tab is in front.

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ Title (serif 17)                 [3]/12  − 110% +  Fit width    🔍 ☰ Annotations│ 56 px toolbar
│ Authors · Venue year (muted 12)                                               │
├───────┬──────────────────────────────────────────────────────┬───────────────┤
│ thumbs│ page canvas, warm gray, pages centered, 24 px gaps    │ [Annotations| │
│ 112 px│                                                       │  Notes]       │
│       │                                                       │ cards or the  │
│       │                                                       │ note editor   │
│       │                                                       │               │
└───────┴──────────────────────────────────────────────────────┴───────────────┘
```

The reader has no toolbar row of its own: its controls float in one capsule centered 16 px below the top of the page, styled like the toasts (an opaque capsule with a hairline border and a soft shadow). From left to right it holds Back and Forward after following a link (Back shows the page it returns to, such as "‹ p. 3"), the current page "3 of 12", zoom out, the zoom percentage, zoom in, and the "Fit width" toggle, then Highlight selection and Add note. While a note is being placed the page label reads "Click where the note goes" and the note button stays tinted. The thumbnail strip is the Pages view of the sidebar. The canvas background is warm gray, pages are white with a hairline border and no shadow, continuous vertical scrolling, zoom from 50% to 400% around the cursor with ⌘ and scroll or pinch, and ⌘0 for fit width.

Selection and markup: dragging over text draws the selection in a translucent accent blue. Releasing shows a floating toolbar 8 px above the selection with five color dots (yellow, green, blue, pink, purple), highlight, underline, strikethrough, note, and a "Copy" text button. Clicking a color applies the last used markup kind in that color. Highlights are drawn with multiply blending so text stays sharp. A sticky note shows a 20 px icon in the margin and opens a small popover for the comment. The reader panel has a segmented control at the top that switches between Annotations and Notes; like the sidebar's, the choice is shared by all PDFs and remembered across launches. The outline is not here: it is the Contents view of the sidebar. Annotations lists every highlight and sticky note of the PDF as cards in page order, top to bottom on each page: a white card with a hairline border and 8 px radius, a 3 px bar in the annotation's color at the leading edge, the quoted text (up to six lines), the comment in italics, and a muted line with the kind's icon and "Page 4". Hovering a card darkens it and shows "Add a comment…" on a highlight without one. Clicking a card scrolls the page so the annotation sits just below the reader's controls and marks the card with the accent tint and outline, and Back returns, as after a link; clicking a highlight or a note on the page marks its card and scrolls the list to it. Clicking a comment edits it in place; Return or leaving the field saves it, Escape cancels, and the edit is undoable. A card's context menu edits the comment or removes the annotation. With no annotations the panel reads "No annotations yet. Select text to highlight it, or add a note." in muted text. Notes holds the paper's one Markdown note, the same note as the inspector's NOTES section: a plain editor in a white card, autosaved after a short pause, and a Preview button (⌥⌘P) that shows it rendered, with headings, paragraphs, bullet, numbered, and task lists, quotes, fenced code, rules, and inline emphasis, code, and links. Whether the note shows rendered is shared and remembered like the panel's mode. Switching between the reader and the library saves the note at once, so the inspector and the reader panel always show the same text. Exporting a PDF with annotations comes later, with its button at the bottom of the panel. ⌘F opens the find bar, a capsule styled like the reader's controls floating 8 px below them at the page's trailing edge, 16 px in from it: a search icon, the field, the count ("3 of 97", "No matches"), Previous and Next, and Done. Matches are found case and diacritic insensitive as the user types, every match is marked on the pages, and the current one is selected. Return and ⌘G go to the next match, ⇧Return and ⇧⌘G to the previous one, and Escape closes the bar. In the library tab ⌘F opens the search palette.

## Typography

| Role | Font | Size and weight | Line height |
|---|---|---|---|
| Paper titles in table, tabs, and reader toolbar | Source Serif 4 | 15 px regular (table), 14 px (tabs), 17 px (reader toolbar) | 20 / 20 / 22 px |
| Detail panel title | Source Serif 4 | 22 px semibold | 28 px |
| Body and controls | Inter | 13 px regular, 13 px medium for buttons | 18 px |
| Authors line in detail | Inter | 15 px regular | 20 px |
| Section labels, column headers | Inter | 11 px medium, uppercase, letter spacing 0.06 em | 16 px |
| Muted metadata | Inter | 12 px regular | 16 px |
| Sidebar group labels | Inter | 11 px medium, uppercase | 16 px |

Both fonts are bundled under the SIL Open Font License. On macOS they ship in the app bundle and are registered through `ATSApplicationFontsPath` in `Info.plist`, so the same files render on every platform. Numbers in tables use tabular figures. Text is never scaled fractionally; sizes are chosen so that at 1x and 2x the glyphs land on whole pixels.

## Color

Light theme is the default and the only theme in the MVP. A dark theme uses the same structure with inverted neutrals and is scheduled after 1.0.

| Token | Value | Use |
|---|---|---|
| bg.canvas | #FBFAF7 | center background |
| bg.sidebar | #F4F2EE | sidebar and thumbnail strip |
| bg.panel | #FFFFFF | detail panel, cards, table rows |
| bg.reader | #EFEDE8 | reader page canvas |
| border | #E6E2DB | all hairlines |
| text.primary | #1E1D1A | titles and body |
| text.secondary | #5F5B54 | authors, metadata |
| text.muted | #8E8A82 | labels, placeholders |
| accent | #2F4A7A | selection text, links, primary button, active tab |
| accent.tint | #E8EEF8 | selected row and selected sidebar item background |
| selection | #2F4A7A at 22% | text selection on the page |
| highlight.yellow | #FFE58A | |
| highlight.green | #CFE8B8 | |
| highlight.blue | #BFD9F2 | |
| highlight.pink | #F7C6D0 | |
| highlight.purple | #DCCBEE | |
| danger | #B4473A | delete confirmations |
| tag.red | #D25F55 | |
| tag.orange | #DE8A3E | |
| tag.yellow | #D9AE36 | |
| tag.green | #67A55A | |
| tag.teal | #3E9A92 | |
| tag.blue | #4F87C9 | |
| tag.purple | #8C6CC6 | |
| tag.pink | #D3699A | |

The tag colors are the highlight hues plus red, orange, and teal, deep enough that a 10 px dot stands out in Light and Dark Mode. A new tag gets the tag color that the fewest tags have, so tags look different without the user picking; ties go through the palette in steps of three (red, green, purple, orange, teal, pink, yellow, blue), so the first tags are far apart in hue. Tags made before tags had colors get theirs the same way when the library opens. The user changes a tag's color from its context menu in the sidebar or from the dot on its chip in the detail panel, and the color shows everywhere the tag does. Colors are stored by hex with the tag, so a tag keeps its color across launches, and removing a tag from its last paper deletes it with its color.

## Spacing and shape

The grid is 4 px. Horizontal padding is 16 px in every pane. Rows are 36 px in the sidebar and 44 px in the table. Buttons are 32 px tall with 12 px horizontal padding and 6 px radius. Inputs are 36 px tall. Cards and the note preview use 8 px radius. Chips are 24 px tall with 10 px padding and full radius. Icons are 16 px, stroke based, in text.secondary. Borders are 1 physical pixel, so at 2x scale the border width is 0.5 logical px. There are no drop shadows except under popovers and the floating selection toolbar, which use a 1 px border plus a soft 8 px shadow at 8% black.

## States

Every interactive element has hover (background 3% darker), pressed (6% darker), focused (2 px accent ring outside the element, visible only for keyboard focus), and disabled (50% opacity) states. Empty states have a short sentence in text.muted and one action, for example "No papers yet. Paste a DOI above to add one." Loading states use a thin 2 px accent progress line at the top of the row or panel that is loading, never a spinner over content. Errors appear as a toast at the bottom right with one line of text and a "Retry" action when retrying is possible.

## Keyboard

| Action | macOS | Windows and Linux |
|---|---|---|
| Add DOI or focus search | ⌘L | Ctrl+L |
| Command palette | ⌘K | Ctrl+K |
| Open selected paper | Enter | Enter |
| Close tab | ⌘W | Ctrl+W |
| Next and previous tab | ⌘⇧] and ⌘⇧[ | Ctrl+Tab and Ctrl+Shift+Tab |
| Add to or remove from favorites | ⌘D | Ctrl+D |
| Toggle sidebar, detail panel | ⌘B, ⌘⇧B | Ctrl+B, Ctrl+Shift+B |
| Toggle thumbnails | ⌘2 | Ctrl+2 |
| Find in document | ⌘F | Ctrl+F |
| Note preview in the reader panel | ⌥⌘P | Ctrl+Alt+P |
| Next and previous match | ⌘G and ⇧⌘G | F3 and Shift+F3 |
| Highlight, underline, strikethrough selection | ⌘⇧H, ⌘⇧U, ⌘⇧S | Ctrl+Shift+H, U, S |
| Add sticky note (then click where it goes) | ⌃⌘N, because ⌘⇧N is New Collection | Ctrl+Shift+N |
| Back and forward after following a link | ⌘[ and ⌘] | Alt+Left and Alt+Right |
| Copy BibTeX, copy citation | ⌥⌘B, ⌘⇧C | Ctrl+Alt+B, Ctrl+Shift+C |
| Export BibTeX | ⌘⇧E | Ctrl+Shift+E |
| Zoom in, out, fit width | ⌘+, ⌘−, ⌘0 | Ctrl++, Ctrl+−, Ctrl+0 |

## Implementation notes for the macOS app

AppKit owns the structure, and SwiftUI draws the leaf content. The tab bar is a custom `NSView` in the full size content area next to the traffic lights. The sidebar is an `NSOutlineView` with source list styling turned off and custom 36 px row views, so the tint, the group labels, and the indentation follow the tokens above rather than the system source list look. The library table is a view based `NSTableView` with 44 px rows, custom cell views, hairline grid lines drawn in the row view, and column headers restyled to 11 px uppercase. The reader is a `PDFView` with the warm gray background, page shadows off, and a hairline border drawn around each page by a page overlay view. The detail panel, the reader panel with its annotation cards and note editor, the empty states, the toasts, and the settings window are SwiftUI views hosted in `NSHostingView`, which is where SwiftUI is good on macOS 13 and where exact row heights do not matter.

Define every size, color, font, and duration once in a `Theme.swift` enum as named constants (`Theme.Color.accent`, `Theme.Size.rowHeight`, `Theme.Font.title`) and never write a literal pixel value or color in a view. Colors are `NSColor` and SwiftUI `Color` pairs created from the same hex values, and the light values above are the only ones until the dark theme is decided (see the open questions in `docs/PRD.md`). Hairlines are drawn at one physical pixel using the window's `backingScaleFactor`, so they are 1 pt at 1x and 0.5 pt at 2x. Hover, pressed, focused, and disabled states are implemented in the custom cell and button views with the percentages above, and keyboard focus rings use `NSFocusRingType.exterior` tinted with the accent color.

Build each screen state (empty library, library with selection, reader with selection toolbar, annotations panel with an open comment) as a case in the snapshot test target that loads a fixture library, so it can be rendered at 1x and 2x and compared with the mockups in `docs/design/` and later with the checked in reference images.
