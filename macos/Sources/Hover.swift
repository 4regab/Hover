import AppKit
import WebKit
import Security
import ServiceManagement
import UserNotifications
import Carbon

let smoke = CommandLine.arguments.contains("--smoke-test")
let environment = ProcessInfo.processInfo.environment
let sandboxRoot = environment["HOVER_SANDBOX_ROOT"]

struct HostError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

// Keychain errors fail closed. An inaccessible old key is never replaced.
func historyKey(dataFolder: URL) throws -> Data {
    if smoke {
        guard let root = sandboxRoot, dataFolder.path.hasPrefix(root + "/") else { throw HostError(message: "Smoke tests require the sandbox runner.") }
        let file = dataFolder.appendingPathComponent("test.key")
        if FileManager.default.fileExists(atPath: file.path) { return try Data(contentsOf: file) }
        var bytes = Data(count: 32)
        guard bytes.withUnsafeMutableBytes({ SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!) }) == errSecSuccess else { throw HostError(message: "Random key creation failed.") }
        try bytes.write(to: file)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        return bytes
    }
    let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "dev.hover.history", kSecAttrAccount as String: "history-v1"]
    var found: CFTypeRef?
    var lookup = query; lookup[kSecReturnData as String] = true; lookup[kSecMatchLimit as String] = kSecMatchLimitOne
    let status = SecItemCopyMatching(lookup as CFDictionary, &found)
    if status == errSecSuccess, let key = found as? Data, key.count == 32 { return key }
    guard status == errSecItemNotFound else { throw HostError(message: "Hover could not unlock its history key (Keychain status \(status)).") }
    // A missing Keychain entry alongside existing history needs recovery, not a new key.
    if FileManager.default.fileExists(atPath: dataFolder.appendingPathComponent("agents/index.dat").path) { throw HostError(message: "The history key is missing from Keychain. Restore the key before opening this history.") }
    var key = Data(count: 32)
    guard key.withUnsafeMutableBytes({ SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!) }) == errSecSuccess else { throw HostError(message: "Random key creation failed.") }
    var insert = query; insert[kSecValueData as String] = key
    insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let added = SecItemAdd(insert as CFDictionary, nil)
    guard added == errSecSuccess else { throw HostError(message: "Could not save the history key (Keychain status \(added)).") }
    return key
}

final class BackendPipe {
    let process = Process(), input = Pipe(), output = Pipe(), errors = Pipe()
    var receive: (([String: Any]) -> Void)?
    private var buffer = Data()
    private let io = DispatchQueue(label: "Hover.backend.reader")
    private var stopping = false
#if HOVER_SETTINGS_TESTS
    var settingsRequests = 0
#endif
    private var pending: [[String: Any]] = []
    private var started = false
    /// env comes from the login-shell probe (ShellEnvironment), so agents find the
    /// same tools a terminal does. Messages sent before the start are kept in order.
    func start(resources: URL, dataFolder: URL, key: Data, env baseEnv: [String: String]) throws {
        process.executableURL = resources.appendingPathComponent("hover-guardian")
        process.arguments = [resources.appendingPathComponent("backend/Hover.Backend").path]
        var env = baseEnv
        env["HOVER_DATA_DIR"] = dataFolder.path
        process.environment = env; process.standardInput = input; process.standardOutput = output; process.standardError = errors
        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard let self else { return }
            self.io.async {
                self.buffer.append(data)
                if self.buffer.count > 64 * 1024 * 1024 { self.buffer.removeAll(); return }
                while let newline = self.buffer.firstIndex(of: 10) {
                    let line = self.buffer.prefix(upTo: newline); self.buffer.removeSubrange(...newline)
                    if let value = try? JSONSerialization.jsonObject(with: line) as? [String: Any] {
                        DispatchQueue.main.async { self.receive?(value) }
                    }
                }
            }
        }
        errors.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if !data.isEmpty { FileHandle.standardError.write(data) }
        }
        process.terminationHandler = { [weak self] p in
            DispatchQueue.main.async {
                guard let self, !self.stopping else { return }
                self.receive?(["type": "backendFailure", "text": "The agent backend stopped (\(p.terminationStatus)). Quit and reopen Hover."])
            }
        }
        try process.run()
        started = true
        send(["type": "initialize", "key": key.base64EncodedString()])
        let queued = pending; pending.removeAll()
        for m in queued { send(m) }
    }
    func send(_ message: [String: Any]) {
#if HOVER_SETTINGS_TESTS
        if message["type"] as? String == "getSettings" { settingsRequests += 1 }
#endif
        guard started else { if pending.count < 256 { pending.append(message) }; return }
        guard process.isRunning, let data = try? JSONSerialization.data(withJSONObject: message) else { return }
        do { try input.fileHandleForWriting.write(contentsOf: data + Data([10])) }
        catch { receive?(["type": "toast", "text": "The backend connection closed."]) }
    }
    func stop() {
        guard started else { return }
        stopping = true
        send(["type": "shutdown"])
        try? input.fileHandleForWriting.close()
        // The guardian flushes the backend, then kills its entire process group.
        if process.isRunning { process.waitUntilExit() }
        output.fileHandleForReading.readabilityHandler = nil; errors.fileHandleForReading.readabilityHandler = nil
    }
}

final class LocalFiles: NSObject, WKURLSchemeHandler {
    let resources: URL, dataFolder: URL
    var folders: [String: URL] = [:]
    init(resources: URL, dataFolder: URL) { self.resources = resources; self.dataFolder = dataFolder }
    func resolve(_ url: URL) throws -> URL {
        let parts = url.path.split(separator: "/").map(String.init)
        let root: URL, relative: [String]
        switch url.host {
        case "office": root = resources.appendingPathComponent("office"); relative = parts
        case "images": root = dataFolder.appendingPathComponent("kiro-images"); relative = parts
        case "files":
            guard let key = parts.first, let folder = folders[key] else { throw HostError(message: "Unknown session folder.") }
            root = folder; relative = Array(parts.dropFirst())
        default: throw HostError(message: "Unknown local resource.")
        }
        guard !relative.isEmpty, !relative.contains(".."), !relative.contains(where: { $0.contains("\\") || $0.contains("\0") }) else { throw HostError(message: "Invalid local path.") }
        let canonicalRoot = root.resolvingSymlinksInPath().standardizedFileURL
        let candidate = relative.reduce(root) { $0.appendingPathComponent($1) }.resolvingSymlinksInPath().standardizedFileURL
        guard candidate.path.hasPrefix(canonicalRoot.path + "/") else { throw HostError(message: "Local path is outside the session folder.") }
        if url.host != "office" {
            guard ["png", "jpg", "jpeg", "gif", "webp", "avif", "heic"].contains(candidate.pathExtension.lowercased()) else { throw HostError(message: "Only local images can be served.") }
        }
        return candidate
    }
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        do {
            guard let url = task.request.url else { throw HostError(message: "Missing URL.") }
            let file = try resolve(url)
            let size = (try FileManager.default.attributesOfItem(atPath: file.path)[.size] as? NSNumber)?.intValue ?? 0
            guard size <= 32 * 1024 * 1024 else { throw HostError(message: "Resource is too large.") }
            let data = try Data(contentsOf: file)
            let types = ["html": "text/html", "m4a": "audio/mp4", "png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif", "webp": "image/webp", "avif": "image/avif", "heic": "image/heic"]
            task.didReceive(URLResponse(url: url, mimeType: types[file.pathExtension.lowercased()] ?? "application/octet-stream", expectedContentLength: data.count, textEncodingName: "utf-8"))
            task.didReceive(data); task.didFinish()
        } catch { task.didFailWithError(error) }
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}

// The office takes the click that brings its window forward: a click on a bot or a
// desk in a window behind another app acts at once, as it does in the notch.
final class OfficeWebView: WKWebView {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class Office: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
    let web: WKWebView
    let files: LocalFiles
    var message: (([String: Any]) -> Void)?
    var ready = false
    var lastState: [String: Any]?
    let dashboard: Bool
    init(resources: URL, dataFolder: URL, dashboard: Bool) {
        self.dashboard = dashboard
        files = LocalFiles(resources: resources, dataFolder: dataFolder)
        let config = WKWebViewConfiguration()
        if smoke { config.websiteDataStore = .nonPersistent() }
        config.setURLSchemeHandler(files, forURLScheme: "hover")
        let bridge = """
        (() => {
          const listeners = new Set();
          window.hoverHost = {
            postMessage: m => window.webkit.messageHandlers.hover.postMessage(m),
            addEventListener: (name, fn) => { if (name === 'message') listeners.add(fn); }
          };
          window.hoverReceive = m => { for (const fn of listeners) fn({ data: m }); };
          window.addEventListener('error', e => window.hoverHost.postMessage({ type: 'webError', text: e.message }));
        })();
        """
        config.userContentController.addUserScript(WKUserScript(source: bridge, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        web = OfficeWebView(frame: .zero, configuration: config)
        super.init()
        config.userContentController.add(self, name: "hover")
        web.navigationDelegate = self
        web.setValue(false, forKey: "drawsBackground")
        web.load(URLRequest(url: URL(string: "hover://office/kiro-office.html")!))
    }
    func userContentController(_ controller: WKUserContentController, didReceive event: WKScriptMessage) {
        guard event.frameInfo.isMainFrame, event.frameInfo.request.url?.scheme == "hover", event.frameInfo.request.url?.host == "office", var m = event.body as? [String: Any] else { return }
        if m["type"] as? String == "ready" { ready = true; if let lastState { deliver(lastState) } }
        // Which office asked: the desk's browser shows in that one.
        m["dashboard"] = dashboard
        message?(m)
    }
    func deliver(_ incoming: [String: Any]) {
        var m = incoming
        if m["type"] as? String == "state" {
            // The desk's Browser panel shows Hover's own browser (AgentBrowsers) here.
            m["window"] = dashboard; m["browser"] = true; lastState = m
            for s in m["sessions"] as? [[String: Any]] ?? [] { mount(s) }
        }
        if m["type"] as? String == "transcript", let s = m["session"] as? [String: Any] { mount(s) }
        guard ready, let data = try? JSONSerialization.data(withJSONObject: m), let json = String(data: data, encoding: .utf8) else { return }
        web.evaluateJavaScript("window.hoverReceive(\(json))", completionHandler: nil)
    }
    func mount(_ session: [String: Any]) {
        if let key = session["key"] as? String, let path = session["folder"] as? String { files.folders[key] = URL(fileURLWithPath: path, isDirectory: true) }
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let url = action.request.url
        // The page itself stays Hover's; only the desk's browser panel (a frame inside
        // it) goes to the web, mostly the agent's own local server. Frames can't post
        // to Hover: the bridge answers the main frame only.
        let frame = action.targetFrame.map { !$0.isMainFrame } ?? false
        let web = ["http", "https", "about"].contains(url?.scheme ?? "")
        decisionHandler((url?.scheme == "hover" && url?.host == "office" && action.targetFrame != nil) || (frame && web) ? .allow : .cancel)
    }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { ready = false; webView.reload() }
}

final class App: NSObject, NSApplicationDelegate, NSWindowDelegate {
    let backend = BackendPipe()
    let screen = ScreenFeed()
    let browsers = AgentBrowsers()
    var resources: URL!, dataFolder: URL!, notch: Notch!, office: Office!, dashboardOffice: Office?, dashboard: NSWindow?, settings: SettingsWindow!
    var menuBar: MenuBar?, poll: Timer?, clock: Timer?, hotKey: EventHotKeyRef?, hotHandler: EventHandlerRef?
    var voice: VoiceController?, voiceKey: EventHotKeyRef?, escapeKey: EventHotKeyRef?, voiceHeld = false
    var settingsSnapshot: [String: Any]?, firstPreferencesHandled = false, hoverNeedsExit = false
    var hoverOpens = true, previous: NSRunningApplication?, lastInside = Date(), latest: [String: Any]?, smokeFinished = false
    var openedByHover = false, hoverSince: Date?, ended: (tool: String, task: String, ok: Bool, until: Date)?
    func applicationDidFinishLaunching(_ note: Notification) {
        if let i = CommandLine.arguments.firstIndex(of: "--voice-probe"), CommandLine.arguments.count > i + 1 {
            let rest = CommandLine.arguments.dropFirst(i + 1)
            Task { @MainActor in await VoiceProbe.run(report: rest.first!, audio: rest.dropFirst().first); NSApp.terminate(nil) }
            return
        }
        do {
            resources = Bundle.main.resourceURL!
            if smoke {
                guard let root = sandboxRoot, let folder = environment["HOVER_DATA_DIR"], folder.hasPrefix(root + "/") else { throw HostError(message: "Run smoke tests only through scripts/test-macos.sh.") }
                dataFolder = URL(fileURLWithPath: folder, isDirectory: true)
            } else { dataFolder = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Hover", isDirectory: true) }
            try FileManager.default.createDirectory(at: dataFolder, withIntermediateDirectories: true)
            backend.receive = { [weak self] m in self?.receive(m) }
            screen.deliver = { [weak self] m in self?.office.deliver(m); self?.dashboardOffice?.deliver(m) }
            browsers.reply = { [weak self] m in self?.backend.send(m) }
            browsers.deliver = { [weak self] m in self?.office.deliver(m); self?.dashboardOffice?.deliver(m) }
            settings = SettingsWindow()
            settings.model.send = { [weak self] m in self?.backend.send(m) }
            settings.model.openOffice = { [weak self] in self?.settings.window.orderOut(nil); self?.expand(true, keyboard: true) }
            settings.model.refreshUsage = { [weak self] in self?.refresh() }
            let key = try historyKey(dataFolder: dataFolder)
            makeNotch()
            if smoke {
                // The sandbox's restricted PATH is the point of the test; no shell probe.
                try backend.start(resources: resources, dataFolder: dataFolder, key: key, env: environment)
                try validateLocalFiles()
                DispatchQueue.main.asyncAfter(deadline: .now() + 35) { [weak self] in self?.finishSmoke(false, "Timed out waiting for the office") }
            } else {
                makeVoice(); makeMenu(); registerHotKey(); startPolling()
                // `open -a Hover --args --settings [start|general|usage|computer-use|<tool>]` opens Settings.
                if let i = CommandLine.arguments.firstIndex(of: "--settings") {
                    let name = CommandLine.arguments.dropFirst(i + 1).first ?? "start"
                    let page: SettingsModel.Page = name == "general" ? .general : name == "usage" ? .usage : name == "computer-use" ? .computerUse : name == "voice" ? .voice
                        : SettingsModel.order.contains(name) ? .tool(name) : .start
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in self?.presentSettings(page) }
                }
                // The notch and menu bar show at once; the backend waits for the
                // login shell's PATH (at most a few seconds) and the office's
                // messages queue meanwhile.
                DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                    let started = Date()
                    let env = ShellEnvironment.resolve(base: environment)
                    FileHandle.standardError.write(Data("Shell environment read in \(Int(Date().timeIntervalSince(started) * 1000)) ms\n".utf8))
                    DispatchQueue.main.async {
                        guard let self else { return }
                        do { try self.backend.start(resources: self.resources, dataFolder: self.dataFolder, key: key, env: env) }
                        catch { self.fatal(error.localizedDescription) }
                    }
                }
            }
        } catch { fatal(error.localizedDescription) }
    }
    func makeNotch() {
        notch = Notch(screen: NSScreen.notchScreen)
        office = Office(resources: resources, dataFolder: dataFolder, dashboard: false)
        office.message = { [weak self] m in self?.handle(m) }
        notch.attach(office.web)
        notch.island.onClick = { [weak self] in self?.expand(true, keyboard: true) }
        notch.island.onAnswer = { [weak self] answer in self?.answerFromNotch(answer) }
        if smoke { notch.setOpen(true, animated: false) }
    }
    func makeVoice() {
        let v = VoiceController()
        v.send = { [weak self] m in self?.backend.send(m) }
        v.noticeSeen = { [weak self] in self?.settings.model.noticeSeen ?? false }
        v.openSettings = { [weak self] page in self?.presentSettings(page) }
        v.openOffice = { [weak self] in self?.expand(true, keyboard: true) }
        v.holdEscape = { [weak self] on in self?.holdEscape(on) }
        settings.model.tryVoice = { [weak self] in self?.settings.window.orderOut(nil); self?.voice?.keyDown(trial: true) }
        voice = v
    }
    func startPolling() {
        // Notchy tracks the pointer with event monitors; the 50 ms timer (Windows'
        // poll) also catches the pointer arriving without a move event (Spaces, wake).
        poll = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.pollPointer(at: NSEvent.mouseLocation, buttons: NSEvent.pressedMouseButtons)
        }
        RunLoop.main.add(poll!, forMode: .common)
        clock = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.tick() }
        NSEvent.addGlobalMonitorForEvents(matching: [.mouseMoved, .leftMouseDragged]) { [weak self] _ in
            self?.pollPointer(at: NSEvent.mouseLocation, buttons: NSEvent.pressedMouseButtons)
        }
        NSEvent.addLocalMonitorForEvents(matching: [.mouseMoved, .leftMouseDragged]) { [weak self] event in
            self?.pollPointer(at: NSEvent.mouseLocation, buttons: NSEvent.pressedMouseButtons); return event
        }
        NotificationCenter.default.addObserver(forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main) { [weak self] _ in
            self?.notch.screenChanged(NSScreen.notchScreen)
        }
        NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            self?.notch.screenChanged(NSScreen.notchScreen); self?.notch.window.orderFrontRegardless()
        }
        NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown]) { [weak self] event in
            // A click in the office takes the keyboard; from then on only a click
            // outside, Esc or Option-N folds it (Notchy's hover vs click modes).
            if let self, event.window === self.notch.window, self.notch.expanded {
                self.notch.window.makeKey(); self.notch.window.makeFirstResponder(self.office.web)
            }
            return event
        }
        NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] _ in
            guard let self, self.notch.expanded, !self.notch.contains(NSEvent.mouseLocation) else { return }
            self.expand(false)
        }
    }
    func pollPointer(at point: NSPoint, buttons: Int) {
        notch.track(point)
        voice?.panel.track(point)
        // Settings in front keeps the notch shut, so the office never covers it.
        guard settings?.window.isKeyWindow != true else { notch.hovered = false; hoverSince = nil; return }
        let inside = notch.contains(point, margin: notch.expanded ? 14 : 0)
        if hoverNeedsExit { if !inside { hoverNeedsExit = false }; notch.hovered = false; hoverSince = nil; return }
        if !notch.expanded {
            notch.hovered = inside
            guard inside else { hoverSince = nil; return }
            let since = hoverSince ?? Date(); hoverSince = since
            // A short dwell, so a pointer crossing the menu bar doesn't open the office.
            // A question waiting keeps the island (as on Windows): Review opens it.
            if hoverOpens && buttons == 0 && notch.state.kind != .waiting && Date().timeIntervalSince(since) >= 0.12 {
                expand(true); openedByHover = true
            }
        } else if inside { lastInside = Date() }
        else if openedByHover && !notch.window.isKeyWindow && Date().timeIntervalSince(lastInside) > 0.35 { expand(false) }
    }
    func expand(_ on: Bool, keyboard: Bool = false, restoreFocus: Bool = true) {
        guard !on || settings?.window.isKeyWindow != true else { return }
        guard notch.expanded != on else { if on && keyboard { openedByHover = false; focusOffice() }; return }
        if on { previous = NSWorkspace.shared.frontmostApplication; openedByHover = false }
        notch.hovered = false
        notch.setOpen(on, animated: !smoke)
        office.deliver(["type": "visible", "on": on]); lastInside = Date()
        if on && keyboard { focusOffice() }
        if !on {
            browsers.detach(from: office.web)
            // Folded under the pointer (Esc, Option-N): don't reopen until it leaves.
            if notch.contains(NSEvent.mouseLocation, margin: 4) { hoverNeedsExit = true }
            notch.window.resignKey(); notch.track(NSEvent.mouseLocation)
            if restoreFocus && NSWorkspace.shared.frontmostApplication?.processIdentifier == ProcessInfo.processInfo.processIdentifier { previous?.activate(options: []) }
        }
    }
    func focusOffice() {
        NSApp.activate(ignoringOtherApps: true)
        notch.window.makeKey(); notch.window.makeFirstResponder(office.web)
    }
    func answerFromNotch(_ answer: String) {
        let s = notch.state
        if answer == "review" { expand(true, keyboard: true); return }
        guard let id = s.sessionId, let ask = s.askId else { return }
        backend.send(["type": "answer", "id": id, "ask": ask, "answer": answer])
    }
    func tick() {
        if let e = ended, Date() >= e.until { ended = nil; updateIsland() }
        else if notch.state.kind == .working { updateIsland() }
    }
    /// The closed notch from the office's state: a question first, then the agents at
    /// work, then what just finished (for six seconds), else the bare notch.
    func updateIsland() {
        let sessions = latest?["sessions"] as? [[String: Any]] ?? []
        let active = sessions.filter { ["working", "waking", "waiting"].contains($0["stage"] as? String ?? "") }
        var island = Island()
        if let w = sessions.first(where: { $0["stage"] as? String == "waiting" }), let ask = w["ask"] as? [String: Any] {
            island.kind = .waiting
            island.tools = [w["tool"] as? String ?? "kiro"]
            let line = (ask["line"] as? String ?? "").trimmingCharacters(in: .whitespaces)
            island.line = line.isEmpty ? (ask["title"] as? String ?? "The agent asks a question") : line
            island.sessionId = w["id"] as? Int; island.askId = ask["id"] as? String
            island.canAllow = (ask["questions"] as? [Any]) == nil
            island.danger = ask["danger"] as? Bool ?? false
        } else if let front = active.first {
            island.kind = .working
            island.tools = active.map { $0["tool"] as? String ?? "kiro" }
            var text = front["stage"] as? String == "waking" ? "Starting" : (front["act"] as? String ?? "Working")
            if let t0 = ((front["turns"] as? [[String: Any]])?.last?["t0"] as? NSNumber)?.doubleValue, t0 > 0 {
                text += " " + Self.duration(Date().timeIntervalSince1970 - t0 / 1000)
            }
            island.text = text
        } else if let e = ended {
            island.kind = .ended(ok: e.ok); island.tools = [e.tool]; island.text = e.task
        }
        notch.state = island
        notch.refresh()
        menuBar?.sessions = sessions
    }
    static func duration(_ seconds: Double) -> String {
        let s = max(0, Int(seconds))
        return s < 60 ? "\(s)s" : s < 3600 ? "\(s / 60)m" : "\(s / 3600)h \(s % 3600 / 60)m"
    }
    func makeMenu() {
        menuBar = MenuBar(); menuBar?.app = self; menuBar?.reading = true
        let appMenu = NSMenu(title: "Hover"); let main = NSMenu(); let root = NSMenuItem(title: "Hover", action: nil, keyEquivalent: ""); root.submenu = appMenu; main.addItem(root)
        let settingsItem = NSMenuItem(title: "Settings…", action: #selector(showSettings), keyEquivalent: ","); settingsItem.target = self; appMenu.addItem(settingsItem)
        appMenu.addItem(.separator())
        let quitItem = NSMenuItem(title: "Quit Hover", action: #selector(quit), keyEquivalent: "q"); quitItem.target = self; appMenu.addItem(quitItem)
        // Edit commands so Cmd-C/V/X/A/Z work in the office's text boxes.
        let editRoot = NSMenuItem(title: "Edit", action: nil, keyEquivalent: ""); let edit = NSMenu(title: "Edit"); editRoot.submenu = edit; main.addItem(editRoot)
        for (title, selector, key) in [("Undo", "undo:", "z"), ("Redo", "redo:", "Z"), ("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            edit.addItem(NSMenuItem(title: title, action: Selector(selector), keyEquivalent: key))
        }
        NSApp.mainMenu = main
    }
    /// Option-N toggles the office; Control-Option-Space is voice (held: talk until let
    /// go; tapped: hands-free until pressed again); Esc is taken only while listening.
    func registerHotKey() {
        let pointer = Unmanaged.passUnretained(self).toOpaque()
        var specs = [EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
                     EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased))]
        InstallEventHandler(GetApplicationEventTarget(), { _, event, data in
            guard let data, let event else { return OSStatus(eventNotHandledErr) }
            var key = EventHotKeyID()
            guard GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &key) == noErr else { return OSStatus(eventNotHandledErr) }
            let app = Unmanaged<App>.fromOpaque(data).takeUnretainedValue()
            let pressed = GetEventKind(event) == UInt32(kEventHotKeyPressed)
            switch key.id {
            case 1: if pressed { app.toggleOffice() }
            case 2: app.voicePress(pressed)
            case 3: if pressed { app.voice?.escape() }
            default: return OSStatus(eventNotHandledErr)
            }
            return noErr
        }, 2, &specs, pointer, &hotHandler)
        let id = EventHotKeyID(signature: OSType(0x48565231), id: 1)
        let result = RegisterEventHotKey(UInt32(kVK_ANSI_N), UInt32(optionKey), id, GetApplicationEventTarget(), 0, &hotKey)
        if result != noErr { FileHandle.standardError.write(Data("Option-N could not be registered: \(result)\n".utf8)) }
        let voiceId = EventHotKeyID(signature: OSType(0x48565231), id: 2)
        let voiceResult = RegisterEventHotKey(UInt32(kVK_Space), UInt32(controlKey | optionKey), voiceId, GetApplicationEventTarget(), 0, &voiceKey)
        if voiceResult != noErr { FileHandle.standardError.write(Data("Control-Option-Space could not be registered: \(voiceResult)\n".utf8)) }
        // While one of Hover's own panels has the keyboard (the office in the notch, the
        // dashboard, Settings), the window server hands it the keys themselves and the
        // hot key never fires: Control-Option-Space reached the office as a key press and
        // only beeped. The same shortcut is caught here then; voicePress tells a press
        // seen both ways from two presses.
        NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp]) { [weak self] e in
            guard let self else { return e }
            let mods = e.modifierFlags.intersection([.command, .control, .option, .shift])
            if e.keyCode == UInt16(kVK_Space) {
                if e.type == .keyDown, mods == [.control, .option] { if !e.isARepeat { self.voicePress(true) }; return nil }
                if e.type == .keyUp, self.voiceHeld { self.voicePress(false); return nil }
            }
            // Esc while listening, when it reached a panel of Hover's instead of the hot key.
            if e.type == .keyDown, e.keyCode == UInt16(kVK_Escape), self.escapeKey != nil { self.voice?.escape(); return nil }
            return e
        }
    }
    /// The voice shortcut went down or up, from the hot key or from Hover's own windows.
    func voicePress(_ down: Bool) {
        guard down != voiceHeld else { return }
        voiceHeld = down
        if down { voice?.keyDown() } else { voice?.keyUp() }
    }
    func holdEscape(_ on: Bool) {
        if on, escapeKey == nil {
            RegisterEventHotKey(UInt32(kVK_Escape), 0, EventHotKeyID(signature: OSType(0x48565231), id: 3), GetApplicationEventTarget(), 0, &escapeKey)
        } else if !on, let k = escapeKey { UnregisterEventHotKey(k); escapeKey = nil }
    }
    @objc func startVoice() { voice?.keyDown(); voice?.keyUp() }
    @objc func toggleOffice() { expand(!notch.expanded, keyboard: true) }
    @objc func showDashboard() {
        if dashboard == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1120, height: 760), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            w.title = "Hover — Agent Office"; w.isReleasedWhenClosed = false; w.delegate = self; w.center()
            let view = Office(resources: resources, dataFolder: dataFolder, dashboard: true); view.message = { [weak self] m in self?.handle(m) }
            w.contentView = view.web; dashboardOffice = view; dashboard = w
            if let latest { view.deliver(latest) }
        }
        dashboardOffice?.deliver(["type": "visible", "on": true]); NSApp.activate(ignoringOtherApps: true); dashboard?.makeKeyAndOrderFront(nil)
    }
    func windowWillClose(_ notification: Notification) {
        if let web = dashboardOffice?.web { browsers.detach(from: web) }
        dashboardOffice?.deliver(["type": "visible", "on": false])
    }
    @objc func showSettings() { presentSettings(nil) }
    func presentSettings(_ page: SettingsModel.Page?) {
        hoverNeedsExit = true
        expand(false, restoreFocus: false)
        // A window shown again reads the backend's latest; one already open keeps going.
        if !settings.window.isVisible { settings.model.request(["type": "getSettings"]) }
        settings.show(page)
    }
    func receivePreferences(_ m: [String: Any], allowPrompt: Bool = !smoke) {
        guard settings.model.receive(m) else { return }
        hoverOpens = settings.model.hover
        menuBar?.enabled = settings.model.quotaItems
        guard !firstPreferencesHandled else { return }
        firstPreferencesHandled = true
        // First run: Settings opens once, on Get Started, until access is acknowledged.
        if allowPrompt && !settings.model.noticeSeen { presentSettings(.start) }
    }
    @objc func refresh() { menuBar?.reading = true; backend.send(["type": "refresh"]) }
    @objc func toggleQuota(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String, let menuBar else { return }
        let on = !menuBar.enabled.contains(id)
        settings.model.setQuota(id, on)
        menuBar.enabled = settings.model.quotaItems; menuBar.reading = on
    }
    @objc func toggleHover() {
        hoverOpens.toggle(); settings.model.setHover(hoverOpens)
    }
    @objc func toggleLogin() {
        do { if SMAppService.mainApp.status == .enabled { try SMAppService.mainApp.unregister() } else { try SMAppService.mainApp.register() } }
        catch { let alert = NSAlert(); alert.messageText = "Login item could not be updated"; alert.informativeText = error.localizedDescription; alert.runModal() }
    }
    @objc func openSession(_ sender: NSMenuItem) {
        if let id = sender.representedObject as? Int { backend.send(["type": "open", "id": id]) }
        expand(true, keyboard: true)
    }
    @objc func quit() { NSApp.terminate(nil) }
    func handle(_ m: [String: Any]) {
        switch m["type"] as? String {
        case "ready":
            backend.send(m)
            // A page made again finds the browsers where they are.
            for b in browsers.states() { office.deliver(b); dashboardOffice?.deliver(b) }
            if smoke { DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in self?.checkSmoke() } }
        case "settings": if !smoke { showSettings() }
        case "setup":
            // A tool the office greyed out: its page, with the one-click button.
            if let id = m["tool"] as? String { if !smoke { presentSettings(.tool(id)) } }
        case "fold": expand(false)
        case "pickFolder":
            guard !smoke else { return }
            let picker = NSOpenPanel(); picker.canChooseDirectories = true; picker.canChooseFiles = false; picker.allowsMultipleSelection = false; picker.title = "Choose the folder the agent works in"
            if let folder = m["folder"] as? String { picker.directoryURL = URL(fileURLWithPath: folder) }
            if picker.runModal() == .OK, let url = picker.url { office.deliver(["type": "folder", "text": url.path]); dashboardOffice?.deliver(["type": "folder", "text": url.path]) }
        case "link":
            if !smoke, let value = m["url"] as? String, let url = URL(string: value), ["http", "https"].contains(url.scheme ?? "") { NSWorkspace.shared.open(url) }
        // The desk's screen panel: the desktop, or the screen live while an agent tests.
        case "screen":
            if smoke { office.deliver(["type": "screen", "live": false, "access": false]); return }
            screen.ask(on: m["on"] as? Bool ?? false, live: m["live"] as? Bool ?? false, apps: m["apps"] as? [String: Any])
        case "screenAccess": if !smoke { screen.requestAccess() }
        // The desk's Browser panel: where the session's browser shows, and the user's
        // address, back and reload.
        case "browserView":
            guard let id = (m["id"] as? NSNumber)?.intValue else { return }
            let web = m["dashboard"] as? Bool == true ? dashboardOffice?.web : office.web
            guard let web else { return }
            var rect: CGRect?
            if let r = m["rect"] as? [String: Any], let x = (r["x"] as? NSNumber)?.doubleValue, let y = (r["y"] as? NSNumber)?.doubleValue,
               let w = (r["w"] as? NSNumber)?.doubleValue, let h = (r["h"] as? NSNumber)?.doubleValue { rect = CGRect(x: x, y: y, width: w, height: h) }
            browsers.view(id, rect: smoke ? nil : rect, in: web)
        case "browserGo":
            if let id = (m["id"] as? NSNumber)?.intValue, let url = m["url"] as? String, !smoke { browsers.go(id, url) }
        case "browserNav":
            if let id = (m["id"] as? NSNumber)?.intValue { browsers.nav(id, m["what"] as? String ?? "reload") }
        case "webError":
            FileHandle.standardError.write(Data("Office JavaScript error: \(m["text"] ?? "unknown")\n".utf8))
            if smoke { finishSmoke(false, "Office JavaScript error") }
        default: backend.send(m)
        }
    }
    func receive(_ m: [String: Any]) {
        switch m["type"] as? String {
        case "initialized": settings.model.request(["type": "getSettings"])
        case "state":
            latest = m
            browsers.keep(Set((m["sessions"] as? [[String: Any]] ?? []).compactMap { ($0["id"] as? NSNumber)?.intValue }))
            office.deliver(m); dashboardOffice?.deliver(m)
            settings.model.receiveState(m)
            voice?.state = m
            updateIsland()
        case "preferences":
            receivePreferences(m)
        case "computerUse":
            settings.model.receiveComputerUse(m)
        // An agent's call to Hover's browser (BrowserTool in the backend).
        case "browser": browsers.handle(m)
        case "quotas":
            var values: [String: QuotaValue] = [:]
            for (id, q) in m["values"] as? [String: [String: Any]] ?? [:] {
                values[id] = QuotaValue(ok: q["ok"] as? Bool ?? false, used: (q["used"] as? NSNumber)?.doubleValue, detail: q["detail"] as? String ?? "")
            }
            menuBar?.quotas = values; menuBar?.reading = false
            settings.model.quotas = values
        case "ended":
            ended = (m["tool"] as? String ?? "kiro", m["task"] as? String ?? "", m["ok"] as? Bool ?? false, Date().addingTimeInterval(6))
            updateIsland()
            if !smoke && UserDefaults.standard.bool(forKey: "notifications") {
                let content = UNMutableNotificationContent(); content.title = m["title"] as? String ?? "Agent finished"; content.body = m["text"] as? String ?? ""; content.sound = .default
                UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil), withCompletionHandler: nil)
            }
        case "readClaudeCredentials":
            // Only requested after the user opts into the Claude usage reader.
            // Test mode never accesses any real Keychain entry.
            if smoke { backend.send(["type": "claudeCredentials"]); break }
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "Claude Code-credentials", kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
                var item: CFTypeRef?
                let status = SecItemCopyMatching(query as CFDictionary, &item)
                let json = status == errSecSuccess ? (item as? Data).flatMap { String(data: $0, encoding: .utf8) } : nil
                DispatchQueue.main.async {
                    var reply: [String: Any] = ["type": "claudeCredentials"]
                    if let json { reply["json"] = json }
                    self?.backend.send(reply)
                }
            }
        case "toast":
            // A voice task the backend turned down says so on the voice card.
            if voice?.backendToast(m["text"] as? String ?? "") != true { office.deliver(m); dashboardOffice?.deliver(m) }
        case "backendFailure": fatal(m["text"] as? String ?? "Backend stopped")
        default: office.deliver(m); dashboardOffice?.deliver(m)
        }
    }
    func validateLocalFiles() throws {
        guard let root = sandboxRoot else { throw HostError(message: "Missing sandbox") }
        let base = URL(fileURLWithPath: root).appendingPathComponent("local-files")
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        let outside = URL(fileURLWithPath: root).appendingPathComponent("outside.png")
        try Data([1, 2, 3]).write(to: outside)
        try Data([4, 5, 6]).write(to: base.appendingPathComponent("inside.png"))
        let link = base.appendingPathComponent("escape.png")
        if !FileManager.default.fileExists(atPath: link.path) { try FileManager.default.createSymbolicLink(at: link, withDestinationURL: outside) }
        office.files.folders["fixture"] = base
        _ = try office.files.resolve(URL(string: "hover://files/fixture/inside.png")!)
        for path in ["escape.png", "%2e%2e/outside.png", "secret.txt"] {
            do { _ = try office.files.resolve(URL(string: "hover://files/fixture/" + path)!); throw HostError(message: "Local image boundary failed: " + path) }
            catch let error as HostError { if error.message.hasPrefix("Local image boundary failed") { throw error } }
        }
    }
    func checkSmoke() {
#if HOVER_SETTINGS_TESTS
        do { try checkSettingsInteractions() }
        catch { finishSmoke(false, error.localizedDescription); return }
#endif
        office.web.evaluateJavaScript("JSON.stringify({host:!!window.hoverHost,canvas:!!document.querySelector('canvas'),tools:document.querySelectorAll('#fab [data-tool]').length,body:document.body.className})") { [weak self] value, error in
            guard let self else { return }
            guard error == nil, let result = value as? String, result.contains("\"canvas\":true"), self.latest != nil else { self.finishSmoke(false, "Office bridge or canvas did not load: \(String(describing: error))"); return }
            let config = WKSnapshotConfiguration()
            self.office.web.takeSnapshot(with: config) { image, error in
                guard let image, let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff), let png = bitmap.representation(using: .png, properties: [:]), let root = sandboxRoot else { self.finishSmoke(false, "Office snapshot failed: \(String(describing: error))"); return }
                do { try png.write(to: URL(fileURLWithPath: root).appendingPathComponent("office-smoke.png")); self.finishSmoke(true, result) }
                catch { self.finishSmoke(false, error.localizedDescription) }
            }
        }
    }
    func finishSmoke(_ success: Bool, _ text: String) {
        guard !smokeFinished else { return }; smokeFinished = true
        if let root = sandboxRoot { try? JSONSerialization.data(withJSONObject: ["success": success, "detail": text]).write(to: URL(fileURLWithPath: root).appendingPathComponent("smoke-result.json")) }
        FileHandle.standardError.write(Data("Smoke \(success ? "passed" : "failed"): \(text)\n".utf8)); NSApp.terminate(nil)
    }
    func fatal(_ text: String) {
        if smoke { finishSmoke(false, text); return }
        let alert = NSAlert(); alert.messageText = "Hover could not start"; alert.informativeText = text; alert.runModal(); NSApp.terminate(nil)
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool { if !smoke { showDashboard() }; return false }
    func applicationWillTerminate(_ note: Notification) { poll?.invalidate(); clock?.invalidate(); if let hotKey { UnregisterEventHotKey(hotKey) }; if let voiceKey { UnregisterEventHotKey(voiceKey) }; holdEscape(false); voice?.cancel(); backend.stop() }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = App(); app.delegate = delegate
app.run()
