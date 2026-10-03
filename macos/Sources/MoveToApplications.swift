import AppKit

/// A Hover opened where it was downloaded (macOS runs a quarantined app from a random,
/// read-only copy: App Translocation) offers once to move itself to Applications, so its
/// login item, the menu bar and updates point at one Hover that stays put. It copies
/// itself there (replacing an older Hover, never another app), opens that copy and quits.
/// The user already chose to open it, so the copy's quarantine flag is taken off.
enum MoveToApplications {
    /// Where this Hover runs from makes a move worth offering: a translocated copy, or one
    /// still in Downloads. A build folder (a developer's dist/) is left alone.
    static func wanted(_ path: String = Bundle.main.bundlePath) -> Bool {
        let downloads = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Downloads").path
        return path.contains("/AppTranslocation/") || path.hasPrefix(downloads + "/")
    }

    /// Asks, and on yes moves and relaunches: true when this process is quitting.
    static func offer() -> Bool {
        guard wanted(), UserDefaults.standard.bool(forKey: "moveDeclined") == false else { return false }
        let alert = NSAlert()
        alert.messageText = "Move Hover to Applications?"
        alert.informativeText = "Hover is running from your Downloads. In Applications it can open at login and update in place."
        alert.addButton(withTitle: "Move to Applications")
        alert.addButton(withTitle: "Not Now")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { UserDefaults.standard.set(true, forKey: "moveDeclined"); return false }
        do {
            let to = try move()
            // Opened once this one has quit, so LaunchServices starts the new copy, not this one.
            let p = Process(); p.executableURL = URL(fileURLWithPath: "/bin/sh"); p.arguments = ["-c", "sleep 1; /usr/bin/open \"$0\"", to.path]
            try p.run()
            NSApp.terminate(nil)
            return true
        } catch {
            let a = NSAlert(); a.messageText = "Hover couldn’t move itself"; a.informativeText = "\(error.localizedDescription)\n\nDrag Hover to the Applications folder in Finder instead."
            a.runModal()
            return false
        }
    }

    /// Copies this bundle to /Applications (else ~/Applications when that isn't writable).
    static func move() throws -> URL {
        let fm = FileManager.default
        let system = URL(fileURLWithPath: "/Applications", isDirectory: true)
        let folder = fm.isWritableFile(atPath: system.path) ? system : fm.homeDirectoryForCurrentUser.appendingPathComponent("Applications", isDirectory: true)
        try fm.createDirectory(at: folder, withIntermediateDirectories: true)
        let to = folder.appendingPathComponent("Hover.app")
        if fm.fileExists(atPath: to.path) {
            // Only an older Hover is replaced.
            guard Bundle(url: to)?.bundleIdentifier == Bundle.main.bundleIdentifier else { throw HostError(message: "Another app named Hover is already in \(folder.path).") }
            try fm.trashItem(at: to, resultingItemURL: nil)
        }
        try fm.copyItem(at: Bundle.main.bundleURL, to: to)
        let x = Process(); x.executableURL = URL(fileURLWithPath: "/usr/bin/xattr"); x.arguments = ["-dr", "com.apple.quarantine", to.path]
        try? x.run(); x.waitUntilExit()
        return to
    }
}
