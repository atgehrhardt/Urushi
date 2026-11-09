// Renders every platform's app icon from assets/icon/icon.svg (the design source).
//
//   swift scripts/render-icons.swift        (or: make icons)
//
// Uses only Apple system frameworks (AppKit draws SVG on macOS 13+) and `iconutil`, so
// no third-party rasterizer is needed (see docs/dependencies.md). The outputs are
// committed, so normal builds need no tools; re-run this after changing the SVG.
//
// Outputs:
//   assets/icon/Urushi.icns              macOS: the tile on Apple's icon grid (824 of 1024)
//   assets/icon/window-64.rgba           Windows/Linux window icon: raw 64×64 RGBA
//   assets/icon/mark-128.rgba            The in-app logo (sidebar header): the mark alone,
//                                        transparent, raw 128×128 RGBA
//   apps/ios/icons/AppIcon*.png          iOS: full bleed (iOS rounds the corners)
//   apps/android/res/mipmap-*/ic_launcher_foreground.png
//                                        Android adaptive icon: the mark in the safe zone

import AppKit
import Foundation

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let ink = "#15110F"

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("render-icons: \(message)\n".utf8))
    exit(1)
}

/// The drawing inside an SVG file's root element, so it can be placed in other layouts.
func inner(_ file: String) -> String {
    guard let text = try? String(contentsOf: root.appendingPathComponent(file), encoding: .utf8),
          let open = text.range(of: ">"), let close = text.range(of: "</svg>", options: .backwards)
    else { fail("cannot read \(file)") }
    return String(text[open.upperBound..<close.lowerBound])
}

/// Draws `svg` (a `size`×`size` viewBox) into an `n`×`n` sRGB bitmap.
func render(_ svg: String, viewBox size: Int, pixels n: Int) -> NSBitmapImageRep {
    let doc = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"\(size)\" height=\"\(size)\" viewBox=\"0 0 \(size) \(size)\">\(svg)</svg>"
    guard let image = NSImage(data: Data(doc.utf8)) else { fail("AppKit cannot read the SVG") }
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: n, pixelsHigh: n, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: n * 4, bitsPerPixel: 32)?.retagging(with: .sRGB)
    else { fail("cannot allocate a bitmap") }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: n, height: n))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

func writePNG(_ rep: NSBitmapImageRep, _ path: String) {
    let url = root.appendingPathComponent(path)
    try? FileManager.default.createDirectory(
        at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    guard let data = rep.representation(using: .png, properties: [:]),
          (try? data.write(to: url)) != nil
    else { fail("cannot write \(path)") }
}

/// Raw, non-premultiplied RGBA rows, top to bottom (what `winit::window::Icon` takes).
func writeRGBA(_ rep: NSBitmapImageRep, _ path: String) {
    guard let pixels = rep.bitmapData else { fail("no bitmap data") }
    var out = [UInt8](repeating: 0, count: rep.pixelsWide * rep.pixelsHigh * 4)
    for i in stride(from: 0, to: out.count, by: 4) {
        let a = pixels[i + 3]
        out[i + 3] = a
        for c in 0..<3 {
            // AppKit stores premultiplied colour; undo it.
            out[i + c] = a == 0 ? 0 : UInt8(min(255, (Int(pixels[i + c]) * 255 + Int(a) / 2) / Int(a)))
        }
    }
    guard (try? Data(out).write(to: root.appendingPathComponent(path))) != nil
    else { fail("cannot write \(path)") }
}

let tile = inner("assets/icon/icon.svg")          // 200×200, rounded, lacquer black
let mark = inner("assets/icon/mark-dark.svg")     // 200×200, transparent

// macOS: the tile at 824 px within 1024 (Apple's grid), transparent margin.
let macos = "<g transform=\"translate(100 100) scale(4.12)\">\(tile)</g>"
let iconset = "build/Urushi.iconset"
try? FileManager.default.removeItem(at: root.appendingPathComponent(iconset))
for (points, scale) in [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)] {
    let name = scale == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
    writePNG(render(macos, viewBox: 1024, pixels: points * scale), "\(iconset)/\(name)")
}
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset, "-o", "assets/icon/Urushi.icns"]
iconutil.currentDirectoryURL = root
try iconutil.run()
iconutil.waitUntilExit()
if iconutil.terminationStatus != 0 { fail("iconutil failed") }

// Windows/Linux window icon: the tile as is.
writeRGBA(render(tile, viewBox: 200, pixels: 64), "assets/icon/window-64.rgba")

// The in-app logo beside the name: the mark alone, on transparent.
writeRGBA(render(mark, viewBox: 200, pixels: 128), "assets/icon/mark-128.rgba")

// iOS: full bleed; the system applies its own corner mask.
let ios = "<rect width=\"200\" height=\"200\" fill=\"\(ink)\"/><g transform=\"translate(100 100) scale(0.74) translate(-100 -100)\">\(mark)</g>"
for (name, pixels) in [("AppIcon60x60@2x", 120), ("AppIcon60x60@3x", 180), ("AppIcon76x76@2x", 152), ("AppIcon83.5x83.5@2x", 167)] {
    writePNG(render(ios, viewBox: 200, pixels: pixels), "apps/ios/icons/\(name).png")
}

// Android adaptive icon foreground: 108 dp, of which the launcher shows the middle
// 72 dp. The mark sits in that middle area as on the tile (0.74 of it); the background
// colour is apps/android/res/values/ic_launcher_background.xml.
let android = "<g transform=\"translate(54 54) scale(\(72.0 * 0.74 / 200.0)) translate(-100 -100)\">\(mark)</g>"
for (density, pixels) in [("mdpi", 108), ("hdpi", 162), ("xhdpi", 216), ("xxhdpi", 324), ("xxxhdpi", 432)] {
    writePNG(render(android, viewBox: 108, pixels: pixels),
             "apps/android/res/mipmap-\(density)/ic_launcher_foreground.png")
}
print("rendered icons from assets/icon/icon.svg")
