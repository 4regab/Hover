import AppKit

/// "Send to Hover VM": an app's window right-clicked on its title bar or toolbar gets a
/// small menu of Hover's that sends the app to a project's desktop (a Cua Space), asking
/// which project when there is more than one. macOS lets no app add to another app's
/// own menus, so Hover's shows only where the app shows none: a right-click the app
/// answers with a menu of its own (Safari's toolbar, Chrome's tabs) is left to it. The
/// same item is in Hover's menu bar menu for every running app. No permissions: the
/// right-click is only watched (a global monitor), never taken, and the windows are
/// read from the window list (their bounds and owners).
final class SendToVM: NSObject {
    struct Project: Equatable { var name: String; var folder: String }
    /// The projects a desktop can be for: the office's, the default one first.
    var projects: () -> [Project] = { [] }
    /// Agent desktops are on.
    var enabled: () -> Bool = { false }
    var send: (NSRunningApplication, Project) -> Void = { _, _ in }
    var turnOn: () -> Void = {}
    private var pending: DispatchWorkItem?

    /// A right-click anywhere (screen points): on another app's title bar or toolbar,
    /// Hover's menu comes a moment later, unless the app opened one of its own there.
    func rightMouseDown(at p: CGPoint) {
        pending?.cancel()
        guard let w = TeleportDrag.window(at: p), let app = NSRunningApplication(processIdentifier: w.pid), app.bundleURL != nil else { return }
        let top = (NSScreen.screens.first?.frame.maxY ?? 0) - p.y
        guard (0..<TeleportDrag.titleBand).contains(top - w.frame.minY) else { return }
        let work = DispatchWorkItem { [weak self] in
            guard let self, !Self.menuOpen(of: w.pid), hypot(NSEvent.mouseLocation.x - p.x, NSEvent.mouseLocation.y - p.y) < 12 else { return }
            self.menu(for: app).popUp(positioning: nil, at: p, in: nil)
        }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.22, execute: work)
    }

    /// Whether an app has a menu open now (its pop-up menu windows, level 101).
    static func menuOpen(of pid: pid_t) -> Bool {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return false }
        return list.contains { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && (100...102).contains($0[kCGWindowLayer as String] as? Int ?? 0) }
    }

    /// "Send “App” to Hover VM", straight to the one project, or a project to pick.
    func menu(for app: NSRunningApplication) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let name = app.localizedName ?? "the app"
        guard enabled() else {
            let on = NSMenuItem(title: "Send “\(name)” to Hover VM…", action: #selector(turnOnPicked), keyEquivalent: "")
            on.target = self; on.image = Self.icon
            on.toolTip = "Turn on agent desktops in Settings → Computer Use first."
            menu.addItem(on)
            return menu
        }
        let list = projects()
        if list.count == 1, let only = list.first {
            let item = NSMenuItem(title: "Send “\(name)” to Hover VM", action: #selector(picked(_:)), keyEquivalent: "")
            item.target = self; item.representedObject = Pick(app: app, project: only); item.image = Self.icon
            item.toolTip = "Opens \(name) on the \(only.name) desktop, for its agents (only the app, never its data)."
            menu.addItem(item)
        } else {
            let root = NSMenuItem(title: "Send “\(name)” to Hover VM", action: nil, keyEquivalent: ""); root.image = Self.icon
            let sub = NSMenu(); sub.autoenablesItems = false
            sub.addItem(.sectionHeader(title: "Which project’s desktop?"))
            for p in list {
                let item = NSMenuItem(title: p.name, action: #selector(picked(_:)), keyEquivalent: "")
                item.target = self; item.representedObject = Pick(app: app, project: p); item.toolTip = p.folder
                item.image = NSImage(systemSymbolName: "macwindow", accessibilityDescription: nil)
                sub.addItem(item)
            }
            if list.isEmpty {
                let none = NSMenuItem(title: "Start a task in a project first", action: nil, keyEquivalent: ""); none.isEnabled = false
                sub.addItem(none)
            }
            root.submenu = sub
            menu.addItem(root)
        }
        return menu
    }

    /// Hover's menu bar item: every app that has a window, the front one first.
    func appsMenu() -> NSMenu {
        let menu = NSMenu(); menu.autoenablesItems = false
        let me = ProcessInfo.processInfo.processIdentifier
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let apps = NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && $0.processIdentifier != me && $0.bundleURL != nil && $0.bundleIdentifier != "com.apple.finder" }
            .sorted { ($0.processIdentifier == front ? 0 : 1, $0.localizedName ?? "") < ($1.processIdentifier == front ? 0 : 1, $1.localizedName ?? "") }
        for app in apps.prefix(14) {
            let one = self.menu(for: app)
            guard let first = one.items.first else { continue }
            one.removeItem(first)
            first.title = app.localizedName ?? "App"
            first.image = app.icon.map { i in let c = i.copy() as! NSImage; c.size = NSSize(width: 16, height: 16); return c }
            menu.addItem(first)
        }
        if apps.isEmpty { let none = NSMenuItem(title: "No apps open", action: nil, keyEquivalent: ""); none.isEnabled = false; menu.addItem(none) }
        return menu
    }

    private final class Pick: NSObject { let app: NSRunningApplication, project: Project; init(app: NSRunningApplication, project: Project) { self.app = app; self.project = project } }
    @objc private func picked(_ sender: NSMenuItem) { if let p = sender.representedObject as? Pick { send(p.app, p.project) } }
    @objc private func turnOnPicked() { turnOn() }

    static var icon: NSImage? {
        let i = NSImage(systemSymbolName: "macwindow.and.cursorarrow", accessibilityDescription: "Hover VM") ?? NSImage(systemSymbolName: "macwindow", accessibilityDescription: nil)
        return i
    }
}
