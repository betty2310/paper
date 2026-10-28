import AppKit

// Started with `swift run`, so there is no Info.plist; set up the app in code.
let delegate = AppDelegate()
let app = NSApplication.shared
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
