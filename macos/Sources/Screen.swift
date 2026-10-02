import AppKit
import ScreenCaptureKit

/// The desk's screen panel on a Mac: the main display as the agent has it, never the
/// user's own work. It is the desktop (the wallpaper and the desktop's icons, through
/// ScreenCaptureKit, or the desktop picture's file without Screen Recording) with only
/// the windows of the apps the agent's computer use opened or acted on over it (their
/// process ids, bundle ids or names, from its steps), so the user's open apps never
/// show. While the agent tests it is live, four frames a second; at rest a still. The
/// page asks again every few seconds while the panel shows, and a feed nobody asks for
/// stops by itself, so a page dropped mid-stream never leaves it running.
// Unchecked: its state is only touched on the main thread; captures hop back there.
final class ScreenFeed: @unchecked Sendable {
    var deliver: (([String: Any]) -> Void)?
    private var timer: Timer?
    private var lease = Date.distantPast
    private var live = false, stillSent = false, busy = false
    private var still: (image: String, at: Date, apps: Apps)?
    private var apps = Apps()

    /// The agent's apps, as the backend reads them from its computer-use steps.
    struct Apps: Equatable {
        var pids: Set<Int> = [], bundles: Set<String> = [], names: Set<String> = []
        var none: Bool { pids.isEmpty && bundles.isEmpty && names.isEmpty }
        init() {}
        init(_ m: [String: Any]?) {
            pids = Set((m?["pids"] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue })
            bundles = Set((m?["bundles"] as? [Any] ?? []).compactMap { ($0 as? String)?.lowercased() })
            names = Set((m?["names"] as? [Any] ?? []).compactMap { ($0 as? String)?.lowercased() })
        }
        func has(_ app: SCRunningApplication?) -> Bool {
            guard let app else { return false }
            return pids.contains(Int(app.processID)) || bundles.contains(app.bundleIdentifier.lowercased()) || names.contains(app.applicationName.lowercased())
        }
    }
    private static let width: CGFloat = 1280

    /// Whether Hover may read the screen (System Settings → Privacy → Screen Recording).
    var access: Bool { CGPreflightScreenCaptureAccess() }

    /// On or off, and live or the desktop. Each call renews the lease.
    func ask(on: Bool, live: Bool, apps: [String: Any]? = nil) {
        guard on else { stop(); return }
        lease = Date().addingTimeInterval(8)
        let apps = Apps(apps)
        if live != self.live || apps != self.apps { stillSent = false }
        self.live = live; self.apps = apps
        if timer == nil {
            stillSent = false
            timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in self?.tick() }
        }
        tick()
    }

    func stop() { timer?.invalidate(); timer = nil }

    /// Asks for Screen Recording once; after that the system only says no, so its page
    /// in System Settings opens instead.
    func requestAccess() {
        if !CGRequestScreenCaptureAccess(), let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
            NSWorkspace.shared.open(url)
        }
        still = nil; stillSent = false
    }

    private func tick() {
        if Date() > lease { stop(); return }
        guard !busy else { return }
        if live && access {
            busy = true
            let apps = self.apps
            Task { [weak self] in
                let image = await ScreenFeed.capture(apps: apps, cursor: true)
                DispatchQueue.main.async { guard let self else { return }; self.busy = false; if let image { self.send(image, live: true) } }
            }
        } else if !stillSent {
            stillSent = true
            sendStill()
        }
    }

    /// The desktop and the agent's apps, kept a minute (its icons change now and then).
    private func sendStill() {
        if let still, still.apps == apps, Date().timeIntervalSince(still.at) < 60 { send(still.image, live: false); return }
        busy = true
        let allowed = access, apps = self.apps
        Task { [weak self] in
            var image = allowed ? await ScreenFeed.capture(apps: apps, cursor: false) : nil
            if image == nil { image = ScreenFeed.wallpaper() }
            DispatchQueue.main.async {
                guard let self else { return }
                self.busy = false
                if let image { self.still = (image, Date(), apps); self.send(image, live: false) }
                else { self.deliver?(["type": "screen", "live": false, "access": allowed]) }
            }
        }
    }

    private func send(_ image: String, live: Bool) {
        deliver?(["type": "screen", "image": image, "live": live, "access": access])
    }

    /// The main display through ScreenCaptureKit: the desktop's own windows (the
    /// wallpaper and its icons) and the agent's apps' windows, nothing else: not the
    /// user's apps, not Hover. The pointer shows only while the agent drives an app.
    static func capture(apps: Apps, cursor: Bool) async -> String? {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            guard let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() }) ?? content.displays.first else { return nil }
            let windows = shown(content.windows, apps: apps, display: display.frame)
            guard !windows.isEmpty else { return nil }
            let filter = SCContentFilter(display: display, including: windows)
            let config = SCStreamConfiguration()
            let k = min(1, width / CGFloat(max(display.width, 1)))
            config.width = Int(CGFloat(display.width) * k); config.height = Int(CGFloat(display.height) * k)
            config.showsCursor = cursor && !apps.none
            let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
            return jpeg(image, quality: apps.none ? 0.82 : 0.6)
        } catch { return nil }
    }

    /// The windows the panel may show: the desktop's, and the agent's apps' ordinary
    /// windows (their menus and sheets too), on the main display.
    static func shown(_ all: [SCWindow], apps: Apps, display: CGRect) -> [SCWindow] {
        let desktop = Int(CGWindowLevelForKey(.desktopIconWindow))
        let me = ProcessInfo.processInfo.processIdentifier
        return all.filter { w in
            guard w.frame.intersects(display) else { return false }
            if w.windowLayer <= desktop { return true }
            guard let app = w.owningApplication, app.processID != me else { return false }
            return apps.has(app)
        }
    }

    /// The desktop picture's file, drawn to fill the main display as macOS does.
    static func wallpaper() -> String? {
        guard let screen = NSScreen.main ?? NSScreen.screens.first, let url = NSWorkspace.shared.desktopImageURL(for: screen),
              let src = NSImage(contentsOf: url), src.size.width > 0, src.size.height > 0 else { return nil }
        let w = Int(width), h = Int(width * screen.frame.height / max(screen.frame.width, 1))
        guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return nil }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        let k = max(CGFloat(w) / src.size.width, CGFloat(h) / src.size.height)
        let size = CGSize(width: src.size.width * k, height: src.size.height * k)
        src.draw(in: CGRect(x: (CGFloat(w) - size.width) / 2, y: (CGFloat(h) - size.height) / 2, width: size.width, height: size.height))
        NSGraphicsContext.restoreGraphicsState()
        guard let cg = rep.cgImage else { return nil }
        return jpeg(cg, quality: 0.82)
    }

    static func jpeg(_ image: CGImage, quality: Double) -> String? {
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .jpeg, properties: [.compressionFactor: quality]) else { return nil }
        return "data:image/jpeg;base64," + data.base64EncodedString()
    }
}
