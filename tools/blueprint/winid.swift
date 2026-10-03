// Prints the on-screen window of a process: `<window-id> <x> <y> <width> <height>`
// (points), or nothing when the process has no normal window yet.
// Usage: winid <pid>
import CoreGraphics
import Foundation
import AppKit

if CommandLine.arguments.dropFirst().first == "--screen" {
    let screen = NSScreen.main!
    print(Int(screen.frame.width), Int(screen.frame.height),
          Int(screen.visibleFrame.width), Int(screen.visibleFrame.height))
    exit(0)
}

let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as! [[String: Any]]
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
    let b = w[kCGWindowBounds as String] as! [String: Double]
    print(w[kCGWindowNumber as String]!, Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!))
    break
}
