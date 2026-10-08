// Draws the app icon: a blue Bible with a cross on a cream tile.
// Usage: swift scripts/make-icon.swift OUT.iconset   (from the repo root)
// then:  iconutil -c icns OUT.iconset -o assets/AppIcon.icns
// Also writes assets/AppIcon-64.png and AppIcon-256.png, which the app shows
// in its toolbar and Settings window (app_icon.rs).
import AppKit

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "AppIcon.iconset"
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)

func rgb(_ hex: Int) -> NSColor {
    NSColor(red: CGFloat(hex >> 16 & 0xff) / 255, green: CGFloat(hex >> 8 & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: 1)
}

let ink = rgb(0x1E2733)
let blue = rgb(0x3F80B2)
let blueDeep = rgb(0x2E6592)
let spine = rgb(0x285A83)
let pages = rgb(0xE4E6EA)
let gold = rgb(0xE0A93B)

/// Fill and outline a path; coordinates are on a 1024 grid, y down.
func paint(_ path: NSBezierPath, _ fill: NSColor?, stroke: CGFloat = 18) {
    if let fill {
        fill.setFill()
        path.fill()
    }
    ink.setStroke()
    path.lineWidth = stroke
    path.lineJoinStyle = .round
    path.lineCapStyle = .round
    path.stroke()
}

func line(_ x0: CGFloat, _ y0: CGFloat, _ x1: CGFloat, _ y1: CGFloat, width: CGFloat = 18) {
    let p = NSBezierPath()
    p.move(to: NSPoint(x: x0, y: y0))
    p.line(to: NSPoint(x: x1, y: y1))
    paint(p, nil, stroke: width)
}

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

    // macOS icon grid: 824/1024 tile, centered, with ~22.5% corner radius.
    let inset = s * 100 / 1024
    let tile = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let path = NSBezierPath(roundedRect: tile, xRadius: tile.width * 0.225, yRadius: tile.width * 0.225)
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.3)
    shadow.shadowBlurRadius = s * 0.02
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.01)
    NSGraphicsContext.saveGraphicsState()
    shadow.set()
    NSGradient(starting: rgb(0xFBF5E8), ending: rgb(0xEADCBF))!.draw(in: path, angle: -90)
    NSGraphicsContext.restoreGraphicsState()

    // Everything below is drawn on a 1024 grid with y pointing down.
    let flip = NSAffineTransform()
    flip.translateX(by: 0, yBy: s)
    flip.scaleX(by: s / 1024, yBy: -s / 1024)
    flip.concat()

    // Soft shadow under the book.
    NSGraphicsContext.saveGraphicsState()
    let bookShadow = NSShadow()
    bookShadow.shadowColor = NSColor.black.withAlphaComponent(0.22)
    bookShadow.shadowBlurRadius = 30
    bookShadow.shadowOffset = NSSize(width: 0, height: -14)
    bookShadow.set()
    ink.setFill()
    NSBezierPath(roundedRect: NSRect(x: 300, y: 196, width: 432, height: 636), xRadius: 48, yRadius: 48).fill()
    NSGraphicsContext.restoreGraphicsState()

    // Back board and the page block below the front cover.
    paint(NSBezierPath(roundedRect: NSRect(x: 300, y: 300, width: 412, height: 528), xRadius: 44, yRadius: 44), spine)
    let block = NSBezierPath(roundedRect: NSRect(x: 352, y: 712, width: 360, height: 82), xRadius: 18, yRadius: 18)
    paint(block, pages)
    line(410, 754, 560, 754, width: 14)
    line(600, 754, 650, 754, width: 14)

    // Ribbon bookmark hanging out of the pages.
    let ribbon = NSBezierPath()
    ribbon.move(to: NSPoint(x: 404, y: 780))
    ribbon.line(to: NSPoint(x: 404, y: 880))
    ribbon.line(to: NSPoint(x: 428, y: 856))
    ribbon.line(to: NSPoint(x: 452, y: 880))
    ribbon.line(to: NSPoint(x: 452, y: 780))
    paint(ribbon, gold, stroke: 14)

    // Front cover with a darker spine band down its left side.
    let cover = NSBezierPath(roundedRect: NSRect(x: 300, y: 196, width: 432, height: 532), xRadius: 48, yRadius: 48)
    NSGraphicsContext.saveGraphicsState()
    cover.addClip()
    NSGradient(starting: blue, ending: blueDeep)!.draw(in: cover, angle: -60)
    spine.setFill()
    NSRect(x: 300, y: 196, width: 70, height: 532).fill()
    // A faint diagonal sheen across the cover.
    NSColor.white.withAlphaComponent(0.10).setFill()
    let sheen = NSBezierPath()
    sheen.move(to: NSPoint(x: 560, y: 196))
    sheen.line(to: NSPoint(x: 732, y: 196))
    sheen.line(to: NSPoint(x: 732, y: 260))
    sheen.line(to: NSPoint(x: 480, y: 728))
    sheen.line(to: NSPoint(x: 330, y: 728))
    sheen.close()
    sheen.fill()
    NSGraphicsContext.restoreGraphicsState()
    paint(cover, nil)
    line(370, 210, 370, 714)

    // Cross: one outline so the arms don't show seams.
    let cx: CGFloat = 551, bar: CGFloat = 38, top: CGFloat = 278, foot: CGFloat = 556
    let armY: CGFloat = 362, arm: CGFloat = 112
    let cross = NSBezierPath()
    for (x, y) in [
        (cx - bar, top), (cx + bar, top), (cx + bar, armY - bar), (cx + arm, armY - bar),
        (cx + arm, armY + bar), (cx + bar, armY + bar), (cx + bar, foot), (cx - bar, foot),
        (cx - bar, armY + bar), (cx - arm, armY + bar), (cx - arm, armY - bar), (cx - bar, armY - bar),
    ] {
        cross.isEmpty ? cross.move(to: NSPoint(x: x, y: y)) : cross.line(to: NSPoint(x: x, y: y))
    }
    cross.close()
    paint(cross, rgb(0xFAFBFC))

    // Title lines.
    line(452, 620, 650, 620, width: 16)
    line(482, 660, 620, 660, width: 16)

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for base in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = scale == 1 ? "icon_\(base)x\(base).png" : "icon_\(base)x\(base)@2x.png"
        try! render(base * scale).write(to: URL(fileURLWithPath: "\(out)/\(name)"))
    }
}
for px in [64, 256] {
    try! render(px).write(to: URL(fileURLWithPath: "assets/AppIcon-\(px).png"))
}
