// Prints "<window id> <owner> <x> <y> <w> <h>" for on-screen windows whose owner name contains
// the first argument (case-insensitive). Used by the bench scripts to screenshot one window.
import CoreGraphics
import Foundation

let needle = CommandLine.arguments.count > 1 ? CommandLine.arguments[1].lowercased() : ""
let options = CGWindowListOption(arrayLiteral: .optionOnScreenOnly, .excludeDesktopElements)
let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let owner = (w[kCGWindowOwnerName as String] as? String) ?? ""
    let layer = (w[kCGWindowLayer as String] as? Int) ?? 0
    guard layer == 0, owner.lowercased().contains(needle) else { continue }
    let id = w[kCGWindowNumber as String] as? Int ?? 0
    let pid = w[kCGWindowOwnerPID as String] as? Int ?? 0
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    print(id, pid, owner.replacingOccurrences(of: " ", with: "_"), Int(b["X"] ?? 0), Int(b["Y"] ?? 0), Int(b["Width"] ?? 0), Int(b["Height"] ?? 0))
}
