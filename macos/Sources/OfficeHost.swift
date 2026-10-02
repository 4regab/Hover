import AppKit
import WebKit
import Security

// The office's bridge to the page and the backend, apart from the app delegate so the
// background E2E harness (tests/macos/e2e) runs the same classes with no window.
// Uses the entry point's smoke, environment and sandboxRoot.
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
        if m["type"] as? String == "ready" { ready = true; if let lastState { deliver(lastState) }; let held = pending; pending.removeAll(); for h in held { deliver(h) } }
        // Which office asked: the desk's browser shows in that one.
        m["dashboard"] = dashboard
        message?(m)
    }
    /// A message for once the page has loaded (a window's office made just now).
    var pending: [[String: Any]] = []
    func later(_ m: [String: Any]) { if ready { deliver(m) } else { pending.append(m) } }
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
