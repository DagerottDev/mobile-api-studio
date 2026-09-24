import AppKit
import Foundation

let outputDirectory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)

let width = 760
let height = 220
let ink = NSColor(srgbRed: 0.18, green: 0.13, blue: 0.08, alpha: 1)
let gold = NSColor(srgbRed: 1.00, green: 0.86, blue: 0.18, alpha: 1)
let cream = NSColor(srgbRed: 1.00, green: 0.98, blue: 0.90, alpha: 1)

func rounded(_ rect: NSRect, radius: CGFloat, color: NSColor) {
    color.setFill()
    NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
}

func label(_ text: String, at point: NSPoint, size: CGFloat, bold: Bool = false) {
    let font = bold ? NSFont.boldSystemFont(ofSize: size) : NSFont.systemFont(ofSize: size)
    NSAttributedString(string: text, attributes: [
        .font: font,
        .foregroundColor: ink,
    ]).draw(at: point)
}

for frame in 0..<20 {
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0
    ), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        fatalError("Unable to make image frame")
    }

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    context.imageInterpolation = .high

    rounded(NSRect(x: 0, y: 0, width: width, height: height), radius: 28, color: gold)
    rounded(NSRect(x: 8, y: 8, width: width - 16, height: height - 16), radius: 23, color: cream)
    rounded(NSRect(x: 18, y: 18, width: width - 36, height: height - 36), radius: 19, color: gold)

    let float = CGFloat(sin(Double(frame) * .pi / 10)) * 2
    rounded(NSRect(x: 78, y: 53 + float, width: 119, height: 13), radius: 7, color: ink)
    rounded(NSRect(x: 91, y: 73 + float, width: 89, height: 79), radius: 16, color: ink)
    rounded(NSRect(x: 98, y: 81 + float, width: 75, height: 66), radius: 12, color: cream)
    rounded(NSRect(x: 158, y: 98 + float, width: 40, height: 45), radius: 20, color: ink)
    rounded(NSRect(x: 166, y: 106 + float, width: 23, height: 28), radius: 12, color: gold)
    rounded(NSRect(x: 101, y: 135 + float, width: 69, height: 12), radius: 6, color: ink)

    ink.setStroke()
    for index in 0..<3 {
        let path = NSBezierPath()
        path.lineWidth = 4
        path.lineCapStyle = .round
        let x = CGFloat(111 + index * 23)
        let sway = CGFloat(sin(Double(frame) * .pi / 10 + Double(index))) * 5
        path.move(to: NSPoint(x: x, y: 160 + float))
        path.curve(to: NSPoint(x: x + sway, y: 189 + float),
                   controlPoint1: NSPoint(x: x - 10 + sway, y: 171 + float),
                   controlPoint2: NSPoint(x: x + 10 + sway, y: 179 + float))
        path.stroke()
    }

    label("Buy me a coffee", at: NSPoint(x: 231, y: 111), size: 42, bold: true)
    label("Support Mobile API Studio", at: NSPoint(x: 234, y: 78), size: 22)
    label("INTERNATIONAL SUPPORT  ↗", at: NSPoint(x: 236, y: 43), size: 15, bold: true)

    NSGraphicsContext.restoreGraphicsState()
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        fatalError("Unable to encode image frame")
    }
    try png.write(to: outputDirectory.appendingPathComponent(String(format: "frame-%02d.png", frame)))
}
