import CoreGraphics
import Foundation
import PaperEngine

/// Converts annotation geometry between the engine and PDFKit, both ways.
///
/// The engine stores PDF user space: points from the bottom left of the
/// unrotated page. PDFKit's page coordinates are the same space, so rects
/// pass through unchanged. The one real conversion is for quads: the engine
/// keeps them absolute like the PDF `QuadPoints` array, while PDFKit wants
/// `quadrilateralPoints` relative to the annotation's bounds.
enum AnnotationGeometry {
    static func cgRect(_ rect: PageRect) -> CGRect {
        CGRect(x: min(rect.x0, rect.x1), y: min(rect.y0, rect.y1), width: abs(rect.x1 - rect.x0), height: abs(rect.y1 - rect.y0))
    }

    static func pageRect(_ rect: CGRect) -> PageRect {
        PageRect(x0: rect.minX, y0: rect.minY, x1: rect.maxX, y1: rect.maxY)
    }

    /// The quad of one line of selected text, from its bounds on the page.
    static func quad(line: CGRect) -> Quad {
        Quad(
            topLeft: PagePoint(x: line.minX, y: line.maxY),
            topRight: PagePoint(x: line.maxX, y: line.maxY),
            bottomLeft: PagePoint(x: line.minX, y: line.minY),
            bottomRight: PagePoint(x: line.maxX, y: line.minY)
        )
    }

    /// PDFKit's `quadrilateralPoints` for quads under an annotation whose
    /// `bounds` are `bounds`: four points per quad in `QuadPoints` order,
    /// each relative to the bounds' origin.
    static func quadrilateralPoints(_ quads: [Quad], relativeTo bounds: CGRect) -> [NSValue] {
        quads.flatMap { quad in
            [quad.topLeft, quad.topRight, quad.bottomLeft, quad.bottomRight].map { point in
                NSValue(point: CGPoint(x: point.x - bounds.minX, y: point.y - bounds.minY))
            }
        }
    }

    /// Whether a page point lies on one of the quads. Quads of text lines are
    /// upright rectangles unless the text is rotated, so this tests each
    /// quad's bounding box, grown by `hitSlop` so the leading between two
    /// highlighted lines counts too.
    static func contains(_ quads: [Quad], _ point: CGPoint) -> Bool {
        quads.contains { quad in
            let corners = [quad.topLeft, quad.topRight, quad.bottomLeft, quad.bottomRight]
            let xs = corners.map(\.x), ys = corners.map(\.y)
            return (xs.min()! ... xs.max()!).contains(point.x)
                && (ys.min()! - hitSlop ... ys.max()! + hitSlop).contains(point.y)
        }
    }

    /// PDF points; about half the gap PDFKit leaves between selected lines.
    private static let hitSlop: CGFloat = 2

    /// A sticky note's square, centered on the click and kept on the page.
    static func noteRect(centeredAt point: CGPoint, size: CGFloat, on page: CGRect) -> PageRect {
        let x = min(max(point.x - size / 2, page.minX), page.maxX - size)
        let y = min(max(point.y - size / 2, page.minY), page.maxY - size)
        return PageRect(x0: x, y0: y, x1: x + size, y1: y + size)
    }
}
