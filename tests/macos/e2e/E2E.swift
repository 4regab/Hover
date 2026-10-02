import AppKit
import WebKit

// Hover's background E2E run (tests/macos/e2e/run.sh). The real office page, the real
// bridge (OfficeHost.swift), Hover's browser (AgentBrowser.swift), the screen feed and
// control mapping (Screen.swift), and the real packaged backend, with a stand-in agent
// (fake-agent.py) and a stand-in gh. No window is ever made: the app never activates,
// the office lays out in a web view of its own size, and nothing reaches the user's
// screen, pointer, keyboard or apps. Every step is checked and photographed.
let smoke = true
let environment = ProcessInfo.processInfo.environment
let sandboxRoot = environment["HOVER_SANDBOX_ROOT"]

let argv = CommandLine.arguments
let appURL = URL(fileURLWithPath: argv[1]), root = URL(fileURLWithPath: argv[2]), project = argv[3]
let resources = appURL.appendingPathComponent("Contents/Resources")
var failures = 0, checks = 0
func check(_ ok: Bool, _ what: String) { checks += 1; print(ok ? "ok: \(what)" : "FAIL: \(what)"); if !ok { failures += 1 }; fflush(stdout) }

@MainActor
final class Harness {
    let backend = BackendPipe(), browsers = AgentBrowsers(), screen = ScreenFeed()
    var office: Office!
    var latest: [String: Any]?, seen: [[String: Any]] = [], links: [String] = [], inputs: [[String: Any]] = []

    func start() throws {
        office = Office(resources: resources, dataFolder: root.appendingPathComponent("data"), dashboard: false)
        office.web.frame = CGRect(x: 0, y: 0, width: 1300, height: 760)
        // The page's E2E hook, and no throttling while it has no window.
        office.web.configuration.userContentController.addUserScript(WKUserScript(source: "window.hoverE2E = true", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        // Without a window WebKit runs no transitions: everything is shown as it ends up.
        office.web.configuration.userContentController.addUserScript(WKUserScript(source: "document.head.insertAdjacentHTML('beforeend', '<style>*,*::before,*::after{transition:none!important;animation-duration:0s!important;animation-delay:0s!important}</style>')", injectionTime: .atDocumentEnd, forMainFrameOnly: true))
        if #available(macOS 14.0, *) { office.web.configuration.preferences.inactiveSchedulingPolicy = .none }
        office.web.reload()
        office.message = { [weak self] m in self?.handle(m) }
        browsers.reply = { [weak self] m in self?.backend.send(m) }
        browsers.deliver = { [weak self] m in self?.office.deliver(m) }
        screen.deliver = { [weak self] m in self?.office.deliver(m) }
        backend.receive = { [weak self] m in self?.receive(m) }
        var env = environment
        env["HOVER_DATA_DIR"] = root.appendingPathComponent("data").path
        try backend.start(resources: resources, dataFolder: root.appendingPathComponent("data"), key: Data((0..<32).map { UInt8($0) }), env: env)
    }

    // What App.handle does with the page's messages, minus the parts that need a screen.
    func handle(_ m: [String: Any]) {
        switch m["type"] as? String {
        case "ready": backend.send(m)
        case "fold", "settings", "setup": break
        case "link": links.append(m["url"] as? String ?? "")
        case "screen": screen.ask(on: m["on"] as? Bool ?? false, live: m["live"] as? Bool ?? false, apps: m["apps"] as? [String: Any])
        case "screenInput":
            // Recorded and mapped, never sent: the E2E run touches no real app.
            inputs.append(m)
            office.deliver(["type": "screenInput", "error": NSNull()])
        case "browserView":
            guard let id = (m["id"] as? NSNumber)?.intValue, let web = office?.web else { return }
            var rect: CGRect?
            if let r = m["rect"] as? [String: Any], let x = (r["x"] as? NSNumber)?.doubleValue, let y = (r["y"] as? NSNumber)?.doubleValue,
               let w = (r["w"] as? NSNumber)?.doubleValue, let h = (r["h"] as? NSNumber)?.doubleValue {
                // CSS pixels to the web view's points, whatever the page's zoom.
                let k = (r["vw"] as? NSNumber).map { web.bounds.width / CGFloat(max(1, $0.doubleValue)) } ?? 1
                rect = CGRect(x: x * k, y: y * k, width: w * k, height: h * k)
            }
            browsers.view(id, rect: rect, in: office.web)
        case "browserGo": if let id = (m["id"] as? NSNumber)?.intValue, let url = m["url"] as? String { browsers.go(id, url) }
        case "browserNav": if let id = (m["id"] as? NSNumber)?.intValue { browsers.nav(id, m["what"] as? String ?? "reload") }
        case "webError": print("PAGE ERROR: \(m["text"] ?? "")"); failures += 1
        default: backend.send(m)
        }
    }

    func receive(_ m: [String: Any]) {
        seen.append(m)
        switch m["type"] as? String {
        case "state":
            latest = m
            browsers.keep(Set((m["sessions"] as? [[String: Any]] ?? []).compactMap { ($0["id"] as? NSNumber)?.intValue }))
            office.deliver(m)
        case "browser": browsers.handle(m)
        case "toast": print("toast: \(m["text"] ?? "")"); office.deliver(m)
        case "backendFailure": print("BACKEND FAILURE: \(m["text"] ?? "")"); failures += 1
        default: office.deliver(m)
        }
    }

    // MARK: Driving the page

    @discardableResult
    func js(_ code: String) async -> Any? {
        do { return try await office.web.evaluateJavaScript(code) } catch { print("js error: \(error.localizedDescription) in \(code.prefix(120))"); return nil }
    }
    func until(_ what: String, _ seconds: Double = 30, _ test: () async -> Bool) async -> Bool {
        let end = Date().addingTimeInterval(seconds)
        while Date() < end { if await test() { return true }; try? await Task.sleep(nanoseconds: 200_000_000) }
        print("timed out: \(what)"); return false
    }
    func truthy(_ code: String) async -> Bool { (await js("!!(\(code))")) as? Bool == true }
    func state() async -> [String: Any] {
        guard let s = await js("JSON.stringify(window.__office.state())") as? String, let d = s.data(using: .utf8),
              let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return [:] }
        return o
    }
    func session() async -> [String: Any] { ((await state())["sessions"] as? [[String: Any]])?.first ?? [:] }
    var shotN = 0
    func shot(_ name: String) async {
        shotN += 1
        let config = WKSnapshotConfiguration()
        guard let image = try? await office.web.takeSnapshot(configuration: config), let tiff = image.tiffRepresentation,
              let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else { print("no snapshot for \(name)"); return }
        try? png.write(to: root.appendingPathComponent(String(format: "shot-%02d-%@.png", shotN, name)))
    }
    func click(_ selector: String) async -> Bool { await truthy("(() => { const e = document.querySelector(\(json(selector))); if (!e) return false; e.click(); return true; })()") }
    func json(_ s: String) -> String { String(data: try! JSONSerialization.data(withJSONObject: s, options: .fragmentsAllowed), encoding: .utf8)! }
    func text(_ selector: String) async -> String { (await js("document.querySelector(\(json(selector)))?.innerText || ''")) as? String ?? "" }

    // MARK: The run

    // The office's frame clock, turned from here: about 30 frames a second.
    var clock: Task<Void, Never>?
    func turnClock() {
        clock = Task { @MainActor in
            while !Task.isCancelled {
                _ = try? await office.web.evaluateJavaScript("window.__office && window.__office.tick(2)")
                try? await Task.sleep(nanoseconds: 60_000_000)
            }
        }
    }

    func run() async {
        turnClock()
        check(await until("the office loads and Hover's state reaches it", 60) { await truthy("window.__office && document.querySelector('canvas')") && latest != nil }, "the office loads with the real backend")
        backend.send(["type": "saveSettings", "noticeSeen": true, "computerUse": false, "tools": [["id": "codex", "access": "risky"]]])
        check(await until("Codex is ready", 40) { await truthy("document.querySelectorAll('#fabTools [data-tool]').length > 0") }, "the agents' picker lists Codex")
        await shot("office")

        // A new task, as the user starts one: the circle, Codex, the folder, the words.
        _ = await click("#fabMain")
        try? await Task.sleep(nanoseconds: 400_000_000)
        check(await until("Codex is ready to pick", 30) { await click("#fabTools [data-tool=codex]:not(.off)") }, "Codex is picked from the circle")
        office.deliver(["type": "folder", "text": project])
        try? await Task.sleep(nanoseconds: 300_000_000)
        await js("(() => { const t = document.querySelector('#nInput'); t.value = 'Check that sign-in works in the browser'; t.dispatchEvent(new Event('input', { bubbles: true })); })()")
        await shot("new-task")
        check(await click("#nGo"), "the task is sent")
        check(await until("a session starts", 30) { (await session())["id"] != nil }, "a bot takes a desk for the session")

        // Ask first: the command waits for the user, who allows it from the office.
        check(await until("the approval shows", 40) { await truthy("document.querySelector('.askc [data-ans=allow]')") }, "the agent asks before running a command")
        await shot("approval")
        check(await click(".askc [data-ans=allow]"), "Allow is clicked")

        // Two subagents: two helpers at the desk, and the desk card says so.
        let sid = (await session())["id"] as? Int ?? 0
        check(await until("the helpers come out", 30) { ((await session())["minis"] as? Int ?? 0) == 2 }, "two helper bots stand at the desk while two subagents work")
        await js("window.__office.openDeskMenu(\(sid))")
        try? await Task.sleep(nanoseconds: 700_000_000)
        check(await truthy("document.querySelectorAll('#deskMenu .dtile').length === 8"), "the desk card has its eight tiles")
        check((await text("#deskMenu .dtiles [data-surface=agents] .dd")).contains("working"), "the Agents tile says the subagents are working: \(await text("#deskMenu .dtiles [data-surface=agents] .dd"))")
        check((await text("#deskMenu .dhelp")).contains("2 helpers"), "the card says two helpers are out")
        check(await truthy("document.querySelector('#deskMenu .dnow .steps .s')"), "the card shows the live steps in the chat's style")
        await shot("desk-card")
        await js("(() => { const t = document.querySelector('#dmInput'); t.value = 'Also check the greeting'; t.dispatchEvent(new Event('input', { bubbles: true })); })()")
        check(await truthy("!document.querySelector('#dmGo').disabled && !document.querySelector('#dmGo').classList.contains('stop')"), "typing in the card turns its button to Send")
        await js("(() => { const t = document.querySelector('#dmInput'); t.value = ''; t.dispatchEvent(new Event('input', { bubbles: true })); })()")
        check(await truthy("document.querySelector('#dmGo').classList.contains('stop')"), "an empty card box offers Stop while it runs")

        // Hover's browser: the agent drives it, and the Browser panel shows it.
        await js("window.__office.openDesk(\(sid), 'browser')")
        check(await until("the agent opens the page in Hover's browser", 40) {
            if (await self.session())["browsing"] as? Bool == true { return true }
            return await self.truthy("(document.querySelector('#pBody .bnow')||{}).innerText?.includes('using this browser')")
        }, "the panel says the agent is using the browser")
        check(await until("the agent's page loads", 30) { (self.browsers.states().first?["url"] as? String ?? "").contains("index.html") }, "the agent opened the test page in Hover's browser")
        check(await until("the browser is laid over the panel", 10) { self.office.web.subviews.contains { $0 is WKWebView } }, "Hover's browser sits over the Browser panel")
        check(await until("the agent's typing reaches the page", 30) {
            let tab = self.office.web.subviews.compactMap { $0 as? WKWebView }.first
            return ((try? await tab?.evaluateJavaScript("document.getElementById('out')?.textContent || ''")) as? String ?? "").contains("Hello, Ada")
        }, "the agent typed and submitted the form in the browser the user sees")
        if let frame = office.web.subviews.first(where: { $0 is WKWebView })?.frame {
            let box = await js("JSON.stringify(document.querySelector('#pBody .bview').getBoundingClientRect())") as? String ?? ""
            print("browser frame \(frame) over page box \(box)")
            check(frame.width > 200 && frame.height > 150 && office.web.bounds.contains(frame.insetBy(dx: 2, dy: 2)), "the browser fills the panel's page box, inside the office")
            if let tab = office.web.subviews.compactMap({ $0 as? WKWebView }).first, let img = try? await tab.takeSnapshot(configuration: WKSnapshotConfiguration()),
               let tiff = img.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) { try? png.write(to: root.appendingPathComponent("shot-browser-page.png")) }
        }
        check(await truthy("document.querySelector('#bAddr').value.includes('index.html')"), "the address bar shows the agent's page")
        await shot("browser")

        // Computer use: the screen panel is the agent's desktop, its apps, its activity.
        await js("window.__office.openDesk(\(sid), 'screen')")
        check(await until("the agent's app shows on its desktop", 40) { ((await session())["apps"] as? [String: Any]) != nil }, "the app the agent opened is the desktop's (from its steps)")
        check(await until("the activity lists what it did", 20) { (await js("document.querySelectorAll('#pBody .vmrow').length") as? Int ?? 0) >= 2 }, "the activity lists its clicks and typing")
        check((await text("#pBody .vmapps")).contains("Demo"), "the desktop names the agent's app, Demo")
        check(await until("a desktop frame arrives", 15) { await truthy("document.querySelector('#scrImg')?.getAttribute('src')") }, "the desktop shows (the desktop picture without Screen Recording)")
        _ = await until("all three computer-use steps", 15) { (await self.text("#pBody .vmtl")).contains("Typed") }
        let rows = await text("#pBody .vmtl")
        check(rows.contains("Typed") && rows.contains("Clicked") && rows.contains("Opened"), "the activity reads Opened, Clicked and Typed")
        await shot("screen")
        // The frame itself: only the desktop and the agent's apps, never the user's windows.
        if let src = await js("document.querySelector('#scrImg').getAttribute('src')") as? String, let comma = src.firstIndex(of: ","),
           let data = Data(base64Encoded: String(src[src.index(after: comma)...])) { try? data.write(to: root.appendingPathComponent("screen-frame.jpg")) }
        let ctl = await truthy("!document.querySelector('[data-vmcontrol]').disabled")
        check(ctl, "Control is offered once the agent has an app")
        if ctl {
            _ = await click("[data-vmcontrol]")
            await js("(() => { const b = document.querySelector('#pBody .vmscreen'), r = b.getBoundingClientRect(); b.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: r.left + r.width * .4, clientY: r.top + r.height * .3, button: 0 })); })()")
            await js("(() => { const b = document.querySelector('#pBody .vmscreen'); for (const k of 'hi') b.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true })); b.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); })()")
            check(await until("the control input reaches the host", 5) { self.inputs.count >= 3 }, "control sends a click, the typed text and Enter to the host")
            let kinds = inputs.map { $0["kind"] as? String ?? "" }
            check(kinds.starts(with: ["click"]) && kinds.contains("type") && kinds.contains("key"), "they arrive as click, type and key: \(kinds)")
            if let click = inputs.first, let x = (click["x"] as? NSNumber)?.doubleValue, let y = (click["y"] as? NSNumber)?.doubleValue {
                check(abs(x - 0.4) < 0.05 && abs(y - 0.3) < 0.05, "the click is where it was made on the desktop (\(x), \(y))")
                // Mapped as Hover maps it, onto a stand-in window of the agent's app.
                var apps = ScreenFeed.Apps(); apps.pids = [4242]
                let display = CGRect(x: 0, y: 0, width: 1512, height: 982)
                let target = ScreenControl.Target(pid: 4242, window: 7, frame: CGRect(x: 400, y: 200, width: 800, height: 600))
                if let call = ScreenControl.call(click, apps: apps, display: display, scale: 2, targets: [target]) {
                    check(call.tool == "click" && call.args["pid"] as? Int == 4242 && call.args["window_id"] as? Int == 7, "it becomes a Cua click on that app's window")
                    check(abs((call.args["x"] as? Double ?? 0) - ((0.4 * 1512 - 400) * 2).rounded()) < 2, "in window-local screenshot pixels")
                } else { check(false, "the click maps to the agent's window") }
                let outside = ScreenControl.call(["kind": "click", "x": 0.02, "y": 0.02], apps: apps, display: display, scale: 2, targets: [target])
                check(outside == nil, "a click outside the agent's apps goes nowhere")
                let typed = ScreenControl.call(["kind": "type", "text": "hi"], apps: apps, display: display, scale: 2, targets: [target])
                check(typed?.tool == "type_text" && typed?.args["text"] as? String == "hi", "typing goes to the agent's window as type_text")
            }
            await js("document.querySelector('#pBody .vmscreen').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))")
            check(await truthy("!document.querySelector('#pBody .vm').classList.contains('ctl')"), "Esc gives control back")
        }

        // The answer, in the chat.
        check(await until("the run ends", 60) { (await session())["stage"] as? String == "done" }, "the run finishes")
        await js("window.__office.openSession(\(sid))")
        try? await Task.sleep(nanoseconds: 600_000_000)
        check((await text("#thread")).contains("Sign-in works"), "the chat shows the answer")
        check(await truthy("document.querySelector('#thread .ans table')"), "the answer's table is drawn")
        check(await truthy("document.querySelector('#thread .s.k-web') || document.querySelector('#thread .sum')"), "the browser steps are in the chat's timeline")
        await shot("chat")

        // The desk's other panels.
        for (tab, has) in [("terminal", "npm run dev"), ("files", "login.html"), ("diff", "signed in"), ("agents", "explore")] {
            await js("window.__office.openDesk(\(sid), '\(tab)')")
            check(await until("the \(tab) panel", 15) { (await self.text("#pBody .dbody")).contains(has) }, "the \(tab) panel shows \(has)")
        }
        await shot("diff")

        // Pull request: gh is signed in, there is none yet, and one is opened from here.
        await js("window.__office.openDesk(\(sid), 'pr')")
        check(await until("the pull request form", 30) { await truthy("document.querySelector('#pBody [data-prform]')") }, "with no pull request yet the panel offers to open one")
        await shot("pr-form")
        await js("(() => { const f = document.querySelector('#pBody [data-prform]'); f.querySelector('[name=title]').value = 'Check sign-in'; f.requestSubmit(); })()")
        check(await until("the pull request opens", 60) { await truthy("document.querySelector('#pBody .prc [data-ext*=\"/pull/7\"]')") }, "Create pull request pushes the branch, opens it, and the panel shows it")
        let made = (try? String(contentsOf: root.appendingPathComponent("gh-create.txt"), encoding: .utf8)) ?? ""
        check(made.contains("--title\nCheck sign-in") && made.contains("--base\nmain") && made.contains("--head\nhover/"), "gh was asked for the title, main and the new branch")
        check(FileManager.default.fileExists(atPath: root.appendingPathComponent("remote.git/refs/heads").path) && ((try? FileManager.default.contentsOfDirectory(atPath: root.appendingPathComponent("remote.git/refs/heads/hover").path))?.isEmpty == false), "the new branch was pushed")
        await shot("pr-done")

        // A reply from the desk card carries the conversation on.
        await js("window.__office.openDeskMenu(\(sid))")
        try? await Task.sleep(nanoseconds: 500_000_000)
        await js("(() => { const t = document.querySelector('#dmInput'); t.value = 'Rename the button to Signed in'; t.dispatchEvent(new Event('input', { bubbles: true })); document.querySelector('#dmGo').click(); })()")
        check(await until("the reply's answer", 40) {
            guard let s = (self.latest?["sessions"] as? [[String: Any]])?.first, let turns = s["turns"] as? [[String: Any]] else { return false }
            return turns.count == 2 && (turns.last?["answer"] as? String ?? "").contains("Signed in")
        }, "a reply from the desk card gets its answer")
        await shot("after-reply")
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
Task { @MainActor in
    let harness = Harness()
    DispatchQueue.main.asyncAfter(deadline: .now() + 420) { print("FAIL: the run took too long"); harness.backend.stop(); exit(2) }
    do { try harness.start() } catch { print("FAIL: start: \(error)"); exit(1) }
    await harness.run()
    harness.backend.stop()
    print(failures == 0 ? "E2E PASSED: \(checks) checks" : "E2E FAILED: \(failures) of \(checks) checks")
    exit(failures == 0 ? 0 : 1)
}
app.run()
