import AppKit
import ImageIO
import Combine
import PaperEngine
import os

/// The preview pictures of papers: the first figure of a paper's PDF, or the
/// first letter of its title when it has no PDF or no figure.
///
/// Figures load on first use. The engine says where the figure is, the
/// reader's PDFKit code draws that part of the page, and the PNG is cached
/// under the library's `cache/previews/`, named after the attachment, so a
/// replaced PDF gets a new picture. A PDF without a figure leaves an empty
/// `.none` file there, so it is read only once.
@MainActor
final class PaperPreviews: ObservableObject {
    enum Preview {
        /// The whole figure, and the part of it the row tile shows.
        case figure(NSImage, row: NSImage)
        /// The paper has a PDF whose figure is not read yet.
        case loading
        case letter(String, Theme.Preview.Tile)
    }

    private enum State {
        case figure(NSImage, row: NSImage)
        case noFigure
    }

    /// Figures read at once. Each takes an engine thread, which lookups and
    /// downloads share.
    private static let maxLoading = 2

    @Published private var states: [String: State] = [:]
    private unowned let store: LibraryStore
    private let cacheDir: URL
    private var requested: Set<String> = []
    private var waiting: [String] = []
    private var loading = 0

    init(store: LibraryStore) {
        self.store = store
        cacheDir = URL(fileURLWithPath: store.libraryPath).appending(path: "cache/previews", directoryHint: .isDirectory)
    }

    /// What to show for a paper now. Starts reading its figure if needed;
    /// the picture is published when it is ready.
    func preview(itemId: String, title: String, hasPdf: Bool) -> Preview {
        let letter = Preview.letter(Self.letter(of: title), Self.tile(for: title))
        guard hasPdf else { return letter }
        switch states[itemId] {
        case .figure(let image, let row): return .figure(image, row: row)
        case .noFigure: return letter
        case nil:
            request(itemId)
            return .loading
        }
    }

    private func request(_ itemId: String) {
        guard requested.insert(itemId).inserted else { return }
        waiting.append(itemId)
        startWaiting()
    }

    private func startWaiting() {
        while loading < Self.maxLoading, !waiting.isEmpty {
            let itemId = waiting.removeFirst()
            loading += 1
            Task {
                states[itemId] = await load(itemId)
                loading -= 1
                startWaiting()
            }
        }
    }

    private func load(_ itemId: String) async -> State {
        guard let attachment = store.pdfAttachment(itemId: itemId) else { return .noFigure }
        let png = cacheDir.appending(path: "\(attachment.id).png")
        let noFigure = cacheDir.appending(path: "\(attachment.id).none")
        let fileManager = FileManager.default
        if fileManager.fileExists(atPath: noFigure.path) { return .noFigure }
        if let data = try? Data(contentsOf: png) { return await figureState(from: data) }
        do {
            try fileManager.createDirectory(at: cacheDir, withIntermediateDirectories: true)
            guard let figure = try await store.firstFigure(attachmentId: attachment.id) else {
                fileManager.createFile(atPath: noFigure.path, contents: nil)
                return .noFigure
            }
            let path = try store.attachmentFile(id: attachment.id).path
            let (maxPixels, maxScale) = (Theme.Size.previewPixels, Theme.Size.previewMaxScale)
            let data = await Task.detached(priority: .utility) {
                ReaderFigureImage.png(pdfPath: path, figure: figure, maxPixels: maxPixels, maxScale: maxScale)
            }.value
            guard let data else { return .noFigure }
            try data.write(to: png, options: .atomic)
            return await figureState(from: data)
        } catch {
            // Not cached, so the next launch tries again.
            Self.logger.error("No preview for \(itemId, privacy: .public): \(String(describing: error), privacy: .public)")
            return .noFigure
        }
    }

    /// The figure and its row image, cut where its ink is densest.
    private func figureState(from data: Data) async -> State {
        let aspect = (Theme.Size.rowPreviewWidth - Theme.Size.rowPreviewInset * 2)
            / (Theme.Size.rowPreviewHeight - Theme.Size.rowPreviewInset * 2)
        let crop = await Task.detached(priority: .utility) { Self.densestCrop(of: data, aspect: aspect) }.value
        guard let image = NSImage(data: data),
              let full = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
        else { return .noFigure }
        let row = crop.flatMap { full.cropping(to: $0) }.map { NSImage(cgImage: $0, size: .zero) } ?? image
        return .figure(image, row: row)
    }

    /// Side of the grayscale copy the crop is searched on, in pixels.
    private nonisolated static let cropGrid: CGFloat = 96
    /// Darker than this (of 255) counts as the figure, not the paper.
    private nonisolated static let inkThreshold = 24
    /// Window sizes tried, as shares of the largest window that fits.
    private nonisolated static let cropScales: [CGFloat] = [1, 0.85, 0.7, 0.55]
    /// A window's score is its ink over its area to this power: below 1, so a
    /// smaller window wins only when the ink is clearly more concentrated.
    private nonisolated static let cropAreaPower = 0.7

    /// The part of a figure image, with the given width to height ratio, that
    /// holds the most ink for its size, in the image's pixels with the origin
    /// at the top left. Line art in a whole figure is too thin to read in a
    /// small tile, so the tile shows this part. `nil` for a blank image.
    nonisolated static func densestCrop(of data: Data, aspect: CGFloat) -> CGRect? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
        else { return nil }
        let scale = min(1, cropGrid / CGFloat(max(image.width, image.height)))
        let width = max(1, Int(CGFloat(image.width) * scale))
        let height = max(1, Int(CGFloat(image.height) * scale))
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue
        ) else { return nil }
        context.setFillColor(gray: 1, alpha: 1)
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.interpolationQuality = .medium
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let pixels = context.data?.assumingMemoryBound(to: UInt8.self) else { return nil }

        // Ink sums over every rectangle from the top left corner, and the box
        // around all ink. Bitmap memory starts at the top row.
        let stride = width + 1
        var sums = [Double](repeating: 0, count: stride * (height + 1))
        var (minX, minY, maxX, maxY) = (width, height, -1, -1)
        for y in 0..<height {
            var row = 0.0
            for x in 0..<width {
                let ink = 255 - Int(pixels[y * context.bytesPerRow + x])
                if ink > inkThreshold {
                    (minX, minY, maxX, maxY) = (min(minX, x), min(minY, y), max(maxX, x), max(maxY, y))
                }
                row += Double(ink)
                sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row
            }
        }
        guard maxX >= minX else { return nil }
        func ink(_ x: Int, _ y: Int, _ w: Int, _ h: Int) -> Double {
            sums[(y + h) * stride + x + w] - sums[y * stride + x + w] - sums[(y + h) * stride + x] + sums[y * stride + x]
        }

        let (boxWidth, boxHeight) = (CGFloat(maxX - minX + 1), CGFloat(maxY - minY + 1))
        let fitWidth = min(boxWidth, boxHeight * aspect)
        var best: (score: Double, rect: CGRect)?
        for share in cropScales {
            let w = max(2, Int(fitWidth * share))
            let h = max(2, Int(fitWidth * share / aspect))
            for y in minY...max(minY, maxY + 1 - h) {
                for x in minX...max(minX, maxX + 1 - w) where x + w <= width && y + h <= height {
                    let score = ink(x, y, w, h) / pow(Double(w * h), cropAreaPower)
                    if score > best?.score ?? -1 {
                        best = (score, CGRect(x: x, y: y, width: w, height: h))
                    }
                }
            }
        }
        guard let rect = best?.rect else { return nil }
        return CGRect(x: rect.minX / scale, y: rect.minY / scale, width: rect.width / scale, height: rect.height / scale).integral
    }

    private static let logger = Logger(subsystem: "com.betty2310.paper", category: "previews")

    /// The first letter or digit of a title, as a capital.
    nonisolated static func letter(of title: String) -> String {
        title.first { $0.isLetter || $0.isNumber }.map { String($0).uppercased() } ?? "?"
    }

    /// A tile color that stays the same for a title across launches.
    static func tile(for title: String) -> Theme.Preview.Tile {
        // FNV-1a, because `hashValue` changes on every launch.
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in title.utf8 {
            hash = (hash ^ UInt64(byte)) &* 0x100_0000_01b3
        }
        return Theme.Preview.tiles[Int(hash % UInt64(Theme.Preview.tiles.count))]
    }
}
