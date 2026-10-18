// Waits until a new on-screen window owned by a process whose name contains the first argument
// appears (ignoring pids listed after it), then prints the wall-clock time in seconds and the pid.
// Window metadata needs no screen-recording permission.
import CoreGraphics
import Foundation

let needle = CommandLine.arguments[1].lowercased()
let ignore = Set(CommandLine.arguments.dropFirst(2).compactMap { Int($0) })
let deadline = Date().addingTimeInterval(30)
while Date() < deadline {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list {
        let owner = ((w[kCGWindowOwnerName as String] as? String) ?? "").lowercased()
        let pid = w[kCGWindowOwnerPID as String] as? Int ?? 0
        let layer = w[kCGWindowLayer as String] as? Int ?? 0
        let alpha = w[kCGWindowAlpha as String] as? Double ?? 1
        if layer == 0, alpha > 0, owner.contains(needle), !ignore.contains(pid) {
            print(String(format: "%.6f", Date().timeIntervalSince1970), pid)
            exit(0)
        }
    }
    usleep(500)
}
exit(1)
