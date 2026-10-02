import AppKit
import WebKit

/// The agents' own desktops (Cua Spaces) on a Mac: each session's live viewer laid over
/// its Screen panel, and an app or files dragged onto the notch sent to a desktop.

/// Cua's own HTML5 viewer for a Space (it refuses to be framed, so it can't live in the
/// office page): one web view per session, laid over the panel's screen box while it
/// shows, and given back to no window otherwise. It is interactive: a click or key in
/// it is the user stepping into the agent's desktop, never their own screen. Its own
/// non-persistent store; it may load only from the Space's viewer address.
final class SpaceViewers: NSObject, WKNavigationDelegate, WKUIDelegate {
    private var views: [Int: (web: WKWebView, url: String)] = [:]

    func show(_ id: Int, url: String, rect: CGRect?, in office: WKWebView) {
        MainActor.assumeIsolated {
            for (k, v) in views where k != id && v.web.superview === office { v.web.removeFromSuperview() }
            guard let rect, rect.width > 40, rect.height > 40, let u = URL(string: url), ["http", "https"].contains(u.scheme ?? "") else { views[id]?.web.removeFromSuperview(); return }
            var v = views[id]
            if v == nil || v!.url != url {
                v?.web.removeFromSuperview()
                let config = WKWebViewConfiguration()
                config.websiteDataStore = .nonPersistent()
                config.mediaTypesRequiringUserActionForPlayback = []
                let web = WKWebView(frame: .zero, configuration: config)
                web.navigationDelegate = self; web.uiDelegate = self
                web.setValue(false, forKey: "drawsBackground")
                web.load(URLRequest(url: u))
                v = (web, url); views[id] = v
            }
            let web = v!.web
            if web.superview !== office { web.removeFromSuperview(); office.addSubview(web) }
            let y = office.isFlipped ? rect.minY : office.bounds.height - rect.maxY
            web.frame = CGRect(x: rect.minX, y: y, width: rect.width, height: rect.height).integral
        }
    }

    func detach(from office: WKWebView) { MainActor.assumeIsolated { for v in views.values where v.web.superview === office { v.web.removeFromSuperview() } } }
    func keep(_ ids: Set<Int>) { MainActor.assumeIsolated { for (k, v) in views where !ids.contains(k) { v.web.removeFromSuperview(); views[k] = nil } } }

    // The viewer stays on its own address; links out go to the user's browser.
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let here = views.values.first { $0.web === webView }.flatMap { URL(string: $0.url) }
        let to = action.request.url
        if action.targetFrame?.isMainFrame != false, let to, let here, to.host != here.host || to.port != here.port {
            if ["http", "https"].contains(to.scheme ?? ""), action.navigationType == .linkActivated { NSWorkspace.shared.open(to) }
            decisionHandler(.cancel); return
        }
        decisionHandler(.allow)
    }
    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin, initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType, decisionHandler: @escaping (WKPermissionDecision) -> Void) {
        // The viewer's microphone uplink is never wanted here.
        decisionHandler(.deny)
    }
}

/// An app's window dragged onto the notch, or files and apps dragged from Finder or the
/// Dock: the notch opens on the agents' desktops and a drop sends it to one, as Cua's
/// own notch does. A window drag is told apart from a click or a resize by watching the
/// window under the pointer move (its bounds through CGWindowList, no permission
/// needed); a Finder drag by the drag pasteboard changing. Nothing is sent until the
/// drop lands on an agent's desktop.
final class TeleportDrag {
    struct Drag { var app: String; var bundle: String?; var files: [URL]; var pid: pid_t? }
    /// Called with "start", "over", "drop" or "cancel", where the pointer is (screen
    /// points), and what is dragged.
    var phase: ((String, CGPoint, Drag) -> Void)?
    /// Whether a point is close enough to the notch to start.
    var near: ((CGPoint) -> Bool)?
    private var candidate: (id: CGWindowID, pid: pid_t, frame: CGRect, start: CGPoint)?
    private var drag: Drag?, active = false, pasteboard = 0
    private var monitors: [Any] = []

    func start() {
        monitors.append(NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown]) { [weak self] _ in self?.down(NSEvent.mouseLocation) } as Any)
        monitors.append(NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDragged]) { [weak self] _ in self?.moved(NSEvent.mouseLocation) } as Any)
        monitors.append(NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseUp]) { [weak self] _ in self?.up(NSEvent.mouseLocation) } as Any)
    }

    /// The drop view saw the drag end over the notch: the drop is done there.
    func finishedByDropView() { active = false; drag = nil; candidate = nil }

    private func down(_ p: CGPoint) {
        pasteboard = NSPasteboard(name: .drag).changeCount
        candidate = nil; drag = nil; active = false
        guard let w = Self.window(at: p) else { return }
        candidate = (w.id, w.pid, w.frame, p)
    }

    private func moved(_ p: CGPoint) {
        if drag == nil {
            if let c = candidate, let now = Self.frame(of: c.id), now.size == c.frame.size, abs(now.minX - c.frame.minX) + abs(now.minY - c.frame.minY) > 6 {
                // The window itself moves with the pointer: a window drag.
                let app = NSRunningApplication(processIdentifier: c.pid)
                drag = Drag(app: app?.localizedName ?? "the app", bundle: app?.bundleIdentifier, files: [], pid: c.pid)
            } else if NSPasteboard(name: .drag).changeCount != pasteboard, let urls = NSPasteboard(name: .drag).readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
                // Files, or an app from Finder or the Dock.
                let apps = urls.filter { $0.pathExtension == "app" }
                let bundle = apps.count == 1 && urls.count == 1 ? Bundle(url: apps[0])?.bundleIdentifier : nil
                drag = Drag(app: bundle != nil ? apps[0].deletingPathExtension().lastPathComponent : urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) items", bundle: bundle, files: bundle == nil ? urls : [], pid: nil)
            }
        }
        guard let d = drag else { return }
        if !active, near?(p) == true { active = true; phase?("start", p, d) }
        else if active { phase?("over", p, d) }
    }

    private func up(_ p: CGPoint) {
        defer { candidate = nil; drag = nil; active = false }
        guard active, let d = drag else { return }
        phase?("drop", p, d)
    }

    /// The top ordinary window of another app under a point (screen points, bottom left).
    static func window(at p: CGPoint) -> (id: CGWindowID, pid: pid_t, frame: CGRect)? {
        let top = (NSScreen.screens.first?.frame.maxY ?? 0) - p.y
        let me = ProcessInfo.processInfo.processIdentifier
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return nil }
        for w in list {
            guard (w[kCGWindowLayer as String] as? Int) == 0, let pid = w[kCGWindowOwnerPID as String] as? pid_t, pid != me,
                  let b = w[kCGWindowBounds as String] as? [String: Any], let f = CGRect(dictionaryRepresentation: b as CFDictionary),
                  let id = w[kCGWindowNumber as String] as? CGWindowID, f.contains(CGPoint(x: p.x, y: top)) else { continue }
            return (id, pid, f)
        }
        return nil
    }
    static func frame(of id: CGWindowID) -> CGRect? {
        guard let w = (CGWindowListCopyWindowInfo([.optionIncludingWindow], id) as? [[String: Any]])?.first, let b = w[kCGWindowBounds as String] as? [String: Any] else { return nil }
        return CGRect(dictionaryRepresentation: b as CFDictionary)
    }
}

/// Over the open notch while files or an app are dragged from Finder or the Dock: it
/// takes the drop (the office's web view would take the files itself otherwise). Shown
/// only during such a drag, so it never takes a click.
final class NotchDropView: NSView {
    var dropped: ((CGPoint, [URL]) -> Void)?
    var hovering: ((CGPoint) -> Void)?
    override init(frame: NSRect) { super.init(frame: frame); registerForDraggedTypes([.fileURL]); isHidden = true }
    required init?(coder: NSCoder) { nil }
    override func draggingExited(_ sender: NSDraggingInfo?) { exited?() }
    var exited: (() -> Void)?
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation { hovering?(NSEvent.mouseLocation); return .copy }
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { hovering?(NSEvent.mouseLocation); return .copy }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        dropped?(NSEvent.mouseLocation, urls); return !urls.isEmpty
    }
}
