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
            return has(pid: Int(app.processID), bundle: app.bundleIdentifier, name: app.applicationName)
        }
        func has(pid: Int, bundle: String?, name: String?) -> Bool {
            pids.contains(pid) || bundle.map { bundles.contains($0.lowercased()) } == true || name.map { names.contains($0.lowercased()) } == true
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
            // Live, as many frames as a capture allows, up to eight a second: it should
            // feel like watching a remote desktop, not a slideshow.
            timer = Timer.scheduledTimer(withTimeInterval: 0.125, repeats: true) { [weak self] _ in self?.tick() }
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
        let size = CGDisplayBounds(CGMainDisplayID()).size
        deliver?(["type": "screen", "image": image, "live": live, "access": access, "w": Int(size.width), "h": Int(size.height)])
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
            // Never the user's own pointer: the agent's is Cua's overlay, shown with its windows.
            config.showsCursor = false
            let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
            return jpeg(image, quality: cursor ? 0.55 : 0.82)
        } catch { return nil }
    }

    /// The windows the panel may show: the desktop's, the agent's apps' (their menus and
    /// sheets too), and Cua Driver's agent cursor over them, on the main display.
    static func shown(_ all: [SCWindow], apps: Apps, display: CGRect) -> [SCWindow] {
        // The desktop picture is at the desktop level or below it; Finder's icons are up
        // to the icon level. Window Manager (Stage Manager) keeps a full-screen layer in
        // between that draws black, so the band is taken by owner, not as a whole.
        let picture = Int(CGWindowLevelForKey(.desktopWindow)), icons = Int(CGWindowLevelForKey(.desktopIconWindow))
        let me = ProcessInfo.processInfo.processIdentifier
        return all.filter { w in
            guard w.frame.intersects(display) else { return false }
            if w.windowLayer <= picture - 1 && w.owningApplication?.bundleIdentifier != nil && w.owningApplication?.bundleIdentifier != "" { return true }
            if w.windowLayer <= icons && w.owningApplication?.bundleIdentifier == "com.apple.finder" { return true }
            if w.windowLayer <= icons { return false }
            guard let app = w.owningApplication, app.processID != me else { return false }
            if !apps.none && app.bundleIdentifier.lowercased().hasPrefix(cuaBundle) { return true }
            return apps.has(app)
        }
    }
    static let cuaBundle = "com.trycua"

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


/// Take control, as on Cursor's agent desktops, without a VM: the user's clicks, typing
/// and scrolling in the screen panel go to the agent's own apps through Cua Driver, in
/// the background (no pointer moved, no focus taken, nothing fronted). Only windows of
/// the apps the agent opened can be reached; a click anywhere else does nothing.
enum ScreenControl {
    /// One window on the main display that input may go to.
    struct Target: Equatable { let pid: Int; let window: Int; let frame: CGRect }

    /// The agent's windows on screen, front to back, in points from the main display's
    /// top left (CGWindowList's own space).
    static func targets(_ apps: ScreenFeed.Apps) -> [Target] {
        guard !apps.none, let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return [] }
        return list.compactMap { w in
            guard (w[kCGWindowLayer as String] as? Int) == 0, let pid = w[kCGWindowOwnerPID as String] as? Int, let id = w[kCGWindowNumber as String] as? Int,
                  let b = w[kCGWindowBounds as String] as? [String: Any], let frame = CGRect(dictionaryRepresentation: b as CFDictionary), frame.width > 20 else { return nil }
            let app = NSRunningApplication(processIdentifier: pid_t(pid))
            guard apps.has(pid: pid, bundle: app?.bundleIdentifier, name: (w[kCGWindowOwnerName as String] as? String) ?? app?.localizedName) else { return nil }
            return Target(pid: pid, window: id, frame: frame)
        }
    }

    /// The front-most target under a point (points from the display's top left).
    static func hit(_ point: CGPoint, in targets: [Target]) -> Target? { targets.first { $0.frame.contains(point) } }

    /// The Cua Driver call for one input, or nil when it has nowhere to go. x and y
    /// are the panel's 0…1 across and down the main display.
    static func call(_ m: [String: Any], apps: ScreenFeed.Apps, display: CGRect, scale: CGFloat, targets: [Target]) -> (tool: String, args: [String: Any])? {
        let kind = m["kind"] as? String ?? ""
        let fx = (m["x"] as? NSNumber)?.doubleValue, fy = (m["y"] as? NSNumber)?.doubleValue
        let point = fx.flatMap { x in fy.map { CGPoint(x: display.minX + x * display.width, y: display.minY + $0 * display.height) } }
        // Typing goes to the window last clicked, else the front-most of the agent's.
        let target = point.flatMap { hit($0, in: targets) } ?? ((m["pid"] as? NSNumber).flatMap { p in targets.first { $0.pid == p.intValue } }) ?? (point == nil ? targets.first : nil)
        guard let t = target else { return nil }
        var a: [String: Any] = ["pid": t.pid, "window_id": t.window]
        // Window-local screenshot pixels: Cua undoes the Retina scale itself.
        if let p = point { a["x"] = Double((p.x - t.frame.minX) * scale).rounded(); a["y"] = Double((p.y - t.frame.minY) * scale).rounded() }
        switch kind {
        case "click":
            guard point != nil else { return nil }
            if (m["count"] as? NSNumber)?.intValue == 2 { return ("double_click", a) }
            if m["button"] as? String == "right" { return ("right_click", a) }
            return ("click", a)
        case "scroll":
            let dy = (m["dy"] as? NSNumber)?.doubleValue ?? 0
            a["direction"] = dy < 0 ? "up" : "down"; a["amount"] = max(1, min(15, Int(abs(dy) / 40)))
            return ("scroll", a)
        case "type":
            guard let text = m["text"] as? String, !text.isEmpty, text.count <= 2000 else { return nil }
            a.removeValue(forKey: "x"); a.removeValue(forKey: "y"); a["text"] = text
            return ("type_text", a)
        case "key":
            guard let key = m["key"] as? String, allowedKeys.contains(key) else { return nil }
            a.removeValue(forKey: "x"); a.removeValue(forKey: "y"); a["key"] = key
            if let mods = m["modifiers"] as? [String], !mods.isEmpty {
                let ok = mods.filter { ["cmd", "shift", "option", "ctrl"].contains($0) }
                return ("hotkey", ["pid": t.pid, "window_id": t.window, "keys": ok + [key]])
            }
            return ("press_key", a)
        default: return nil
        }
    }
    static let allowedKeys: Set<String> = Set(["return", "tab", "escape", "delete", "space", "up", "down", "left", "right", "home", "end", "pageup", "pagedown"])
        .union("abcdefghijklmnopqrstuvwxyz0123456789".map(String.init))

    /// Runs `cua-driver call <tool> <json>` off the main thread; done gets an error, or nil.
    static func send(_ tool: String, _ args: [String: Any], exe: String, done: @escaping (String?) -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: exe)
            let json = (try? JSONSerialization.data(withJSONObject: args)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
            p.arguments = ["call", tool, json]
            let out = Pipe(); p.standardOutput = out; p.standardError = out; p.standardInput = FileHandle.nullDevice
            var failure: String?
            do {
                try p.run()
                let deadline = Date().addingTimeInterval(15)
                while p.isRunning && Date() < deadline { usleep(20_000) }
                if p.isRunning { p.terminate(); failure = "Cua Driver didn’t answer." }
                else if p.terminationStatus != 0 {
                    let text = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                    failure = text.split(separator: "\n").last.map(String.init) ?? "Cua Driver refused it."
                }
            } catch { failure = "Cua Driver isn’t installed." }
            DispatchQueue.main.async { done(failure) }
        }
    }

    /// cua-driver, where its installers put it (Hover's PATH is the login shell's).
    static func exe() -> String? {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let path = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map { "\($0)/cua-driver" }
        return (path + ["\(home)/.local/bin/cua-driver", "/Applications/CuaDriver.app/Contents/MacOS/cua-driver", "/opt/homebrew/bin/cua-driver"])
            .first { FileManager.default.isExecutableFile(atPath: $0) }
    }
}
