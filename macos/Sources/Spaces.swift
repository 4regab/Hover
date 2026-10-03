import AppKit
import WebKit

/// The agents' own desktops (Cua Spaces) on a Mac: each session's live viewer laid over
/// its Screen panel, and an app or files dragged onto the notch sent to a desktop.

/// Cua's own HTML5 viewer for a Space (it refuses to be framed, so it can't live in the
/// office page): one web view per project's desktop (its agents share it), laid over the
/// Screen panel's box while it
/// shows, and given back to no window otherwise. It is interactive: a click or key in
/// it is the user stepping into the agent's desktop, never their own screen. Its own
/// non-persistent store; it may load only from the Space's viewer address.
final class SpaceViewers: NSObject, WKNavigationDelegate, WKUIDelegate {
    private var views: [String: (web: WKWebView, url: String)] = [:]

    func show(_ id: String, url: String, rect: CGRect?, in office: WKWebView) {
        MainActor.assumeIsolated {
            for (k, v) in views where k != id && v.web.superview === office { v.web.removeFromSuperview() }
            // Taken out only of the office that asked: the other may show it now.
            guard let rect, rect.width > 40, rect.height > 40, let u = URL(string: url), ["http", "https"].contains(u.scheme ?? "") else { if let w = views[id]?.web, w.superview === office { w.removeFromSuperview() }; return }
            var v = views[id]
            if v == nil || v!.url != url {
                v?.web.removeFromSuperview()
                let config = WKWebViewConfiguration()
                config.websiteDataStore = .nonPersistent()
                config.mediaTypesRequiringUserActionForPlayback = []
                // A live desktop: never throttled for Hover not being the active app.
                config.preferences.inactiveSchedulingPolicy = .none
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
    func keep(_ ids: Set<String>) { MainActor.assumeIsolated { for (k, v) in views where !ids.contains(k) { v.web.removeFromSuperview(); views[k] = nil } } }

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

/// A browser's tabs on this Mac, read and closed through osascript (Apple Events; macOS
/// asks the user once per browser): Safari's and Chromium's (Chrome, Brave, Edge, …), which
/// share one dictionary. Only a browser that is running is asked (a `tell` would launch one
/// that isn't). Each tab has an id that says where it is: Chromium's window and tab ids,
/// Safari's window id and the tab's place in it (Safari's tabs have no id of their own).
enum BrowserTabs {
    struct Tab: Equatable { var id: String; var title: String; var url: String }
    /// What asking a browser for its tabs found: the tabs, or that macOS won't let Hover ask
    /// (Hover's switch for that browser is off in Privacy & Security → Automation).
    struct Read { var tabs: [Tab]; var denied: Bool }
    static let chromium: Set<String> = ["com.google.Chrome", "com.google.Chrome.beta", "com.google.Chrome.dev", "com.google.Chrome.canary", "org.chromium.Chromium",
                                        "com.brave.Browser", "com.microsoft.edgemac", "com.vivaldi.Vivaldi"]
    static func handles(_ bundle: String?) -> Bool { bundle == "com.apple.Safari" || bundle.map(chromium.contains) == true }
    static func running(_ bundle: String) -> Bool { !NSRunningApplication.runningApplications(withBundleIdentifier: bundle).isEmpty }
    /// The Automation pane, where the user lets Hover ask a browser again.
    static let automationSettings = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")!

    /// Every web tab of every window, front window first; at most 50. Blocks: off the main thread.
    static func read(_ bundle: String) -> Read {
        guard handles(bundle), running(bundle), bundle.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "." || $0 == "-" }) else { return Read(tabs: [], denied: false) }
        let tabId = bundle == "com.apple.Safari" ? "i" : "(id of t)"
        let title = bundle == "com.apple.Safari" ? "name of t" : "title of t"
        let script = """
        set us to ASCII character 31
        set rs to ASCII character 30
        set out to ""
        tell application id "\(bundle)"
          repeat with w in windows
            try
              set wid to id of w
              set i to 0
              repeat with t in tabs of w
                set i to i + 1
                set u to URL of t
                if u is not missing value then set out to out & wid & ":" & \(tabId) & us & (\(title)) & us & u & rs
              end repeat
            end try
          end repeat
        end tell
        return out
        """
        let (status, text, error) = osascript(script)
        let tabs = text.split(separator: "\u{1e}").compactMap { r -> Tab? in
            let f = r.split(separator: "\u{1f}", maxSplits: 2, omittingEmptySubsequences: false).map(String.init)
            guard f.count == 3 else { return nil }
            let url = f[2].trimmingCharacters(in: .whitespacesAndNewlines)
            guard url.hasPrefix("https://") || url.hasPrefix("http://") else { return nil }
            return Tab(id: f[0].trimmingCharacters(in: .whitespacesAndNewlines), title: String(f[1].prefix(200)), url: url)
        }
        DragTrace.log("\(bundle) tabs: \(tabs.count) (osascript exit \(status)) \(error)")
        return Read(tabs: Array(tabs.prefix(50)), denied: status != 0 && notPermitted(error))
    }

    /// osascript's error for an Apple Event macOS refused (errAEEventNotPermitted).
    static func notPermitted(_ error: String) -> Bool { error.contains("(-1743)") }

    /// Closes the tabs that went (each still at the address it had when it was read, so a
    /// tab that has moved on or another in its place is left alone). Returns how many it
    /// closed and how many windows the browser has left, or nil if it couldn't be asked.
    static func close(_ tabs: [Tab], in bundle: String) -> (closed: Int, windows: Int)? {
        guard handles(bundle), running(bundle), !tabs.isEmpty, bundle.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "." || $0 == "-" }) else { return nil }
        let script = closeScript(tabs, in: bundle)
        let (status, text, error) = osascript(script)
        DragTrace.log("\(bundle) closing \(tabs.count) tabs (osascript exit \(status)): \(text) \(error)")
        let f = text.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ",").compactMap { Int($0) }
        return status == 0 && f.count == 2 ? (f[0], f[1]) : nil
    }

    /// The script `close` runs: each tab closed only if it is still where it was, counted,
    /// then "closed,windows left".
    static func closeScript(_ tabs: [Tab], in bundle: String) -> String {
        func q(_ s: String) -> String { "\"" + s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\"" }
        var lines: [String] = []
        // Safari's tabs are known by their place: the last first, so the others keep theirs.
        let parsed = tabs.compactMap { t -> (w: Int, n: Int, url: String)? in
            let p = t.id.split(separator: ":"); guard p.count == 2, let w = Int(p[0]), let n = Int(p[1]) else { return nil }
            return (w, n, t.url)
        }.sorted { ($0.w, -$0.n) < ($1.w, -$1.n) }
        for t in parsed {
            // A window gone with its last tab makes the rest of its lines fail: each is tried.
            lines.append(bundle == "com.apple.Safari"
                ? "try\n if URL of tab \(t.n) of window id \(t.w) is \(q(t.url)) then\n close tab \(t.n) of window id \(t.w)\n set n to n + 1\n end if\nend try"
                : "try\n if (count of (every tab of window id \(t.w) whose id is \(t.n) and URL is \(q(t.url)))) > 0 then\n close (every tab of window id \(t.w) whose id is \(t.n) and URL is \(q(t.url)))\n set n to n + 1\n end if\nend try")
        }
        return "set n to 0\ntell application id \"\(bundle)\"\n\(lines.joined(separator: "\n"))\nreturn (n as text) & \",\" & ((count of windows) as text)\nend tell"
    }

    private static func osascript(_ script: String) -> (Int32, String, String) {
        let p = Process(); p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript"); p.arguments = ["-e", script]
        let pipe = Pipe(), errors = Pipe(); p.standardOutput = pipe; p.standardError = errors
        do { try p.run() } catch { return (-1, "", "") }
        // The first time, macOS's consent prompt waits on the user: a minute at most.
        DispatchQueue.global().asyncAfter(deadline: .now() + 60) { if p.isRunning { p.terminate() } }
        // Its errors are a line or two: read after, they never fill the pipe.
        let text = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        let error = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        p.waitUntilExit()
        return (p.terminationStatus, text, error.trimmingCharacters(in: .whitespacesAndNewlines))
    }
}

/// What leaves this Mac once a send to a project's desktop has gone, from the backend's
/// answer and what Hover itself noted when the user sent it (never on the page's word
/// alone): the tabs that went, from a browser (all of it when its whole session went);
/// any other app itself, which quits.
enum Moved: Equatable {
    case nothing, quit, close([BrowserTabs.Tab])
    /// An app the user sent while it ran here: which, and the tabs Hover read of it.
    struct Sent { var bundle: String; var pid: pid_t; var tabs: [BrowserTabs.Tab]; var at: Date }

    static func after(_ m: [String: Any], sent: Sent?, now: Date = Date()) -> Moved {
        guard let s = sent, s.bundle == m["bundle"] as? String, now.timeIntervalSince(s.at) < 30 * 60,
              let d = m["data"] as? [String: Any], !(d["error"] is String), d["moved"] as? Bool == true else { return .nothing }
        // A browser stays (its other tabs are the user's); only the tabs Hover read and that
        // went close, unless every tab it had went with its session.
        guard BrowserTabs.handles(s.bundle) else { return s.bundle == "com.apple.finder" ? .nothing : .quit }
        if d["whole"] as? Bool == true { return .quit }
        let went = Set(d["tabs"] as? [String] ?? [])
        let tabs = s.tabs.filter { !$0.id.isEmpty && went.contains($0.id) }
        return tabs.isEmpty ? .nothing : .close(tabs)
    }
}

/// HOVER_DRAG_TRACE=1 writes each step of a drag to stderr (for finding why one wasn't seen).
enum DragTrace {
    static let on = ProcessInfo.processInfo.environment["HOVER_DRAG_TRACE"] != nil
    static func log(_ s: @autoclosure () -> String) { if on { FileHandle.standardError.write(Data("drag \(String(format: "%.2f", Date().timeIntervalSince1970.truncatingRemainder(dividingBy: 1000))) \(s())\n".utf8)) } }
}

/// Files and folders (or an app) dragged from Finder or the Dock to the notch: it opens
/// on the projects' desktops and a drop sends them to one. Polled, not hooked: while
/// the left button is held (pressedMouseButtons, no permission needed) it watches the
/// drag pasteboard for file URLs; the drop itself lands on NotchDropView. An app's
/// window goes by "Send to Hover VM" instead (SendToVM): a window dragged near the top
/// of the screen is how windows are moved and tiled, and must never count as a drop.
final class TeleportDrag {
    struct Drag { var app: String; var bundle: String?; var path: String?; var files: [URL]; var pid: pid_t? }
    /// "start", "over", "drop" or "cancel", where the pointer is (screen points), and what.
    var phase: ((String, CGPoint, Drag) -> Void)?
    /// Whether a point is close enough to the notch to start.
    var near: ((CGPoint) -> Bool)?
    private var held = false, active = false, pasteboard = -1
    private var drag: Drag?, lastSent = CGPoint(x: -1, y: -1), lastAt = Date.distantPast, lastPoint = CGPoint.zero
    /// What is on the drag pasteboard (swapped in tests).
    var dragged: () -> (count: Int, urls: [URL]) = { let p = NSPasteboard(name: .drag); return (p.changeCount, p.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []) }

    /// Called with the pointer every poll (and on pointer events), cheap when idle.
    func poll(_ p: CGPoint, buttons: Int) {
        let down = buttons & 1 != 0
        if down && !held { pressed(p) }
        // The drag pasteboard as it was before this press: read when the button comes up
        // (and at the start), since a drag can begin before the first poll sees the press.
        if !down && held || pasteboard < 0 { pasteboard = dragged().count }
        held = down
        if down { moved(p) } else if active || drag != nil { released() }
    }

    /// The drop view took a Finder drop: the drag is over.
    func finishedByDropView() { active = false; drag = nil }
    var dragging: Bool { active }

    private func pressed(_ p: CGPoint) {
        if pasteboard < 0 { pasteboard = dragged().count }
        drag = nil; active = false
    }

    private func moved(_ p: CGPoint) {
        lastPoint = p
        if drag == nil {
            let (count, urls) = dragged()
            // The source clears the pasteboard first and writes its items a moment later:
            // read again on the next poll until the files are there.
            if count != pasteboard, !urls.isEmpty {
                // Files and folders, or an app from Finder or the Dock.
                let apps = urls.filter { $0.pathExtension == "app" }
                let bundle = apps.count == 1 && urls.count == 1 ? Bundle(url: apps[0])?.bundleIdentifier : nil
                drag = Drag(app: bundle != nil ? apps[0].deletingPathExtension().lastPathComponent : urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) items", bundle: bundle, path: bundle != nil ? apps[0].path : nil, files: bundle == nil ? urls : [], pid: nil)
                pasteboard = count
                DragTrace.log("finder drag: \(urls.count) item(s), app \(bundle ?? "-")")
            }
        }
        guard let d = drag else { return }
        if !active { if near?(p) == true { active = true; send("start", p, d) }; return }
        // Followed at up to 30 a second, and only when it moved.
        if Date().timeIntervalSince(lastAt) >= 0.033 && hypot(p.x - lastSent.x, p.y - lastSent.y) >= 2 { send("over", p, d) }
    }

    private func released() {
        DragTrace.log("up at \(Int(lastPoint.x)),\(Int(lastPoint.y)), \(drag == nil ? "no drag" : active ? "over the notch" : "never reached the notch")")
        defer { drag = nil; active = false }
        guard active, let d = drag else { return }
        send("drop", lastPoint, d)
    }

    private func send(_ name: String, _ p: CGPoint, _ d: Drag) {
        if name != "over" { DragTrace.log("\(name) at \(Int(p.x)),\(Int(p.y))") }
        lastSent = p; lastAt = Date(); phase?(name, p, d)
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
            // Only an ordinary app's window: background helpers keep invisible overlays
            // over the whole screen (Cua Driver's agent cursor is one), which never move.
            guard isApp(pid), (w[kCGWindowAlpha as String] as? Double ?? 1) > 0.05 else { continue }
            return (id, pid, f)
        }
        return nil
    }
    /// How far below a window's top a press is on its title bar or toolbar (Safari's,
    /// Finder's and most apps' unified toolbars fit in it).
    static let titleBand: CGFloat = 60
    static func isApp(_ pid: pid_t) -> Bool { NSRunningApplication(processIdentifier: pid)?.activationPolicy == .regular }
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
