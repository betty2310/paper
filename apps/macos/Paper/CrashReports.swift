import AppKit
import Sentry

/// Crash reports through Sentry, and nothing else: no tracing, sessions,
/// breadcrumbs, hang reports, or user data. Only Paper.app has a DSN in its
/// Info.plist, so `swift run` never reports. The user chooses on first launch
/// and may change it in Settings.
@MainActor
enum CrashReports {
    static let enabledKey = "sendCrashReports"
    static let explanation = "When Paper quits unexpectedly, it can send a crash report to its developer so the problem gets fixed. A report holds only where in Paper the crash happened, the Paper and macOS versions, and the Mac’s model, memory, and language. It never includes your papers, PDFs, notes, library, or anything that identifies you."

    private static let dsn = Bundle.main.object(forInfoDictionaryKey: "SentryDSN") as? String ?? ""
    static var isAvailable: Bool { !dsn.isEmpty }

    /// Asks the first time Paper.app runs, then starts reporting when the
    /// user agreed.
    static func askOnce() {
        guard isAvailable, UserDefaults.standard.object(forKey: enabledKey) == nil else { return }
        let alert = NSAlert()
        alert.messageText = "Send Crash Reports?"
        alert.informativeText = explanation + " You can change this in Settings."
        alert.addButton(withTitle: "Send Crash Reports")
        alert.addButton(withTitle: "Don’t Send")
        UserDefaults.standard.set(alert.runModal() == .alertFirstButtonReturn, forKey: enabledKey)
        update()
    }

    /// Starts or stops the SDK to match the setting.
    static func update() {
        guard isAvailable else { return }
        guard UserDefaults.standard.bool(forKey: enabledKey) else {
            SentrySDK.close()
            return
        }
        guard !SentrySDK.isEnabled else { return }
        SentrySDK.start { options in
            options.dsn = dsn
            options.sendDefaultPii = false
            options.enableAutoSessionTracking = false
            options.enableAutoBreadcrumbTracking = false
            options.enableNetworkBreadcrumbs = false
            options.maxBreadcrumbs = 0
            options.enableCaptureFailedRequests = false
            options.enableAppHangTracking = false
            options.enableAutoPerformanceTracing = false
            options.enableNetworkTracking = false
            options.beforeSend = scrub
        }
    }

    /// Drops what could tell one Mac from another: the installation id, the
    /// hash of this Mac and app, and the home folder in file paths.
    private nonisolated static func scrub(_ event: Event) -> Event? {
        event.user = nil
        event.context?["app"]?["device_app_hash"] = nil
        let home = NSHomeDirectory()
        func strip(_ path: String?) -> String? { path?.replacingOccurrences(of: home, with: "~") }
        event.debugMeta?.forEach { $0.codeFile = strip($0.codeFile) }
        let stacks = (event.threads ?? []).compactMap(\.stacktrace) + (event.exceptions ?? []).compactMap(\.stacktrace)
        for frame in stacks.flatMap(\.frames) { frame.package = strip(frame.package) }
        return event
    }
}
