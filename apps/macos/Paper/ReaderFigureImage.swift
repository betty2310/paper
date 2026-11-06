import AppKit
import PDFKit
import PaperEngine

/// Draws the part of a PDF page that holds a paper's first figure, for its
/// preview. Lives with the reader because PDFKit calls stay in the reader.
enum ReaderFigureImage {
    /// A PNG of `figure` from the PDF at `path`, its longer side at most
    /// `Theme.Size.previewPixels`. Slow; call it off the main thread.
    nonisolated static func png(pdfPath: String, figure: Figure, maxPixels: CGFloat, maxScale: CGFloat) -> Data? {
        guard let document = PDFDocument(url: URL(fileURLWithPath: pdfPath)),
              let page = document.page(at: Int(figure.pageIndex))
        else { return nil }
        // The engine measures in PDF user space; PDFKit draws the media box
        // with its origin at the context's origin.
        let mediaBox = page.bounds(for: .mediaBox)
        let region = CGRect(
            x: figure.rect.x0, y: figure.rect.y0,
            width: figure.rect.x1 - figure.rect.x0, height: figure.rect.y1 - figure.rect.y0
        )
        guard region.width > 0, region.height > 0 else { return nil }
        let scale = min(maxPixels / max(region.width, region.height), maxScale)
        let width = Int((region.width * scale).rounded())
        let height = Int((region.height * scale).rounded())
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return nil }
        context.setFillColor(Theme.Preview.figureBackgroundCG)
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.interpolationQuality = .high
        context.scaleBy(x: scale, y: scale)
        context.translateBy(x: mediaBox.minX - region.minX, y: mediaBox.minY - region.minY)
        page.draw(with: .mediaBox, to: context)
        guard let image = context.makeImage() else { return nil }
        return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    }
}
