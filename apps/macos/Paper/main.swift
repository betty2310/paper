import AppKit

// Set up the app in code, so it also runs under `swift run` without Info.plist.
let delegate = AppDelegate()
let app = NSApplication.shared
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
