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
    let backend = BackendPipe(), browsers = AgentBrowsers(), screen = ScreenFeed(), spaces = SpaceViewers()
    var windowAsks: [[String: Any]] = []
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
        case "window": windowAsks.append(m)
        case "spaceOverlay":
            guard let id = m["space"] as? String, let web = office?.web else { return }
            var rect: CGRect?
            if let r = m["rect"] as? [String: Any], let x = (r["x"] as? NSNumber)?.doubleValue, let y = (r["y"] as? NSNumber)?.doubleValue,
               let w = (r["w"] as? NSNumber)?.doubleValue, let h = (r["h"] as? NSNumber)?.doubleValue {
                let k = (r["vw"] as? NSNumber).map { web.bounds.width / CGFloat(max(1, $0.doubleValue)) } ?? 1
                rect = CGRect(x: x * k, y: y * k, width: w * k, height: h * k)
            }
            spaces.show(id, url: m["url"] as? String ?? "", rect: rect, in: web)
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
        backend.send(["type": "saveSettings", "noticeSeen": true, "computerUse": true, "agentSpaces": true, "spaceImage": "macos", "tools": [["id": "codex", "access": "risky"]]])
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

        // Its own desktop: a Cua Space, made before its run, driven over the relay.
        let cuaLog = { (try? String(contentsOf: root.appendingPathComponent("cua.log"), encoding: .utf8)) ?? "" }
        check(cuaLog().contains("spaces create macos:26 --name hover-project-"), "the project's Space was made before the run (cua spaces create, named for the folder)")
        check(cuaLog().range(of: #"--cpus [2-6] --memory-mb (4096|6144|8192)"#, options: .regularExpression) != nil, "it is made with room to be smooth (cores and memory for this Mac)")
        check(cuaLog().contains("--display 1280x800"), "and a screen size that's sharp in the panel (lume set --display)")
        await js("window.__office.openDesk(\(sid), 'screen')")
        check(await until("the Space's viewer opens over the panel", 30) { self.office.web.subviews.contains { ($0 as? WKWebView)?.url?.path.hasPrefix("/viewer") == true } }, "Cua's live viewer is laid over the Screen panel")
        if let v = office.web.subviews.compactMap({ $0 as? WKWebView }).first(where: { $0.url?.path.hasPrefix("/viewer") == true }) {
            let box = await js("JSON.stringify(document.querySelector('#pBody .spbox').getBoundingClientRect())") as? String ?? ""
            print("viewer frame \(v.frame) over box \(box)")
            check(v.frame.width > 300 && office.web.bounds.contains(v.frame.insetBy(dx: 2, dy: 2)), "the viewer fills the panel's box, inside the office")
            _ = await until("the viewer page loads", 10) { ((try? await v.evaluateJavaScript("document.body?.innerText || ''")) as? String ?? "").contains("FAKE SPACE VIEWER") }
            let said = (try? await v.evaluateJavaScript("document.body.innerText")) as? String ?? ""
            check(said.contains("ticket ok"), "the viewer got its ticket (its address is the Space's own)")
            if let img = try? await v.takeSnapshot(configuration: WKSnapshotConfiguration()), let tiff = img.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) { try? png.write(to: root.appendingPathComponent("shot-space-viewer.png")) }
        }
        check(!(await truthy("document.querySelector('#scrImg')")), "nothing of the user's own screen is captured or shown")
        check(await until("its desktop activity", 30) { (await self.text("#pBody .vmtl")).contains("Typed") }, "the activity lists what it did on its desktop")
        let rows = await text("#pBody .vmtl")
        check(rows.contains("Clicked") && rows.contains("Typed"), "the activity reads Clicked and Typed")
        check(await until("its calls reached the Space", 10) { cuaLog().contains("call local:hover-") && cuaLog().contains("computer_click") }, "the agent's computer use went to its own Space, through Hover's relay to cua mcp")
        await shot("screen")

        // The viewer link is kept: the panel opened again shows the same live view.
        await js("window.__office.openDesk(\(sid), 'terminal')"); try? await Task.sleep(nanoseconds: 400_000_000)
        await js("window.__office.openDesk(\(sid), 'screen')"); try? await Task.sleep(nanoseconds: 1_500_000_000)
        check(cuaLog().components(separatedBy: "sb view").count - 1 == 1, "opening the Screen panel again reuses the live view (one cua sb view)")

        // Dragging is polled, so window drags the window server moves itself are seen.
        do {
            let drag = TeleportDrag()
            var frame = CGRect(x: 300, y: 300, width: 800, height: 600)
            drag.windowAt = { _ in (id: 77, pid: 4242, frame: CGRect(x: 300, y: 300, width: 800, height: 600)) }
            drag.frameOf = { _ in frame }
            drag.appOf = { _ in ("Google Chrome", "com.google.Chrome") }
            drag.near = { p in p.y > 950 }
            var seen: [(String, String?)] = []
            drag.phase = { ph, _, d in seen.append((ph, d.bundle)) }
            drag.poll(CGPoint(x: 600, y: 600), buttons: 1)                       // pressed on its title bar
            drag.poll(CGPoint(x: 600, y: 600), buttons: 0)                       // a click: nothing
            check(seen.isEmpty, "a click on a window is not a drag")
            drag.poll(CGPoint(x: 600, y: 600), buttons: 1)
            frame.size.width += 40; drag.poll(CGPoint(x: 640, y: 600), buttons: 1) // a resize: nothing
            check(seen.isEmpty, "resizing a window is not a drag to the notch")
            drag.poll(CGPoint(x: 640, y: 600), buttons: 0)
            frame = CGRect(x: 300, y: 300, width: 800, height: 600)
            drag.poll(CGPoint(x: 600, y: 600), buttons: 1)
            frame.origin.y += 200; drag.poll(CGPoint(x: 600, y: 800), buttons: 1)  // moving, not near yet
            check(seen.isEmpty, "a window moved about the screen doesn't open the notch")
            frame.origin.y += 160; drag.poll(CGPoint(x: 700, y: 970), buttons: 1)  // at the notch
            try? await Task.sleep(nanoseconds: 50_000_000)
            drag.poll(CGPoint(x: 720, y: 975), buttons: 1)
            drag.poll(CGPoint(x: 720, y: 975), buttons: 0)                       // let go there
            check(seen.map(\.0) == ["start", "over", "drop"] && seen.allSatisfy { $0.1 == "com.google.Chrome" }, "a window dragged to the notch opens it and drops there, naming the app: \(seen.map(\.0))")
        }

        // An app dragged onto the notch: the agents' desktops open as drop targets.
        office.deliver(["type": "teleportDrag", "phase": "start", "app": "Google Chrome", "bundle": "com.google.Chrome", "files": [], "x": 300, "y": 120, "vw": 1300])
        check(await until("the drop targets show", 5) { await self.truthy("!document.querySelector('#tdrop').hidden && document.querySelector('#tdrop [data-tdrop]')") }, "dragging an app to the notch shows the agents' desktops")
        await shot("teleport-drop")
        let tile = await js("JSON.stringify(document.querySelector('#tdrop [data-tdrop=\"\(sid)\"]').getBoundingClientRect())") as? String ?? "{}"
        if let d = tile.data(using: .utf8), let r = try? JSONSerialization.jsonObject(with: d) as? [String: Double], let x = r["x"], let y = r["y"] {
            office.deliver(["type": "teleportDrag", "phase": "over", "app": "Google Chrome", "bundle": "com.google.Chrome", "files": [], "x": x + 30, "y": y + 30, "vw": 1300])
            check(await until("the tile under the pointer lights", 3) { await self.truthy("document.querySelector('#tdrop [data-tdrop=\"\(sid)\"]').classList.contains('on')") }, "the agent's tile lights under the pointer")
            office.deliver(["type": "teleportDrag", "phase": "drop", "app": "Google Chrome", "bundle": "com.google.Chrome", "files": [], "x": x + 30, "y": y + 30, "vw": 1300])
            check(await until("the app is sent", 15) { cuaLog().contains("teleport push --app com.google.Chrome --sandbox local:hover-") }, "dropping it teleports the app into that agent's Space (cua teleport push)")
            check(await until("the office says so", 10) { (await self.text("#toast")).contains("is on") }, "the office says the app is on the agent's desktop: \(await text("#toast"))")
            // Files dropped the same way land in its Downloads.
            let file = root.appendingPathComponent("project/login.html").path
            office.deliver(["type": "teleportDrag", "phase": "start", "app": "login.html", "files": [file], "x": 300, "y": 120, "vw": 1300])
            office.deliver(["type": "teleportDrag", "phase": "drop", "app": "login.html", "files": [file], "x": x + 30, "y": y + 30, "vw": 1300])
            check(await until("the file is sent", 15) { cuaLog().contains("sb cp") && cuaLog().contains("login.html local:hover-") && cuaLog().contains(":/Users/lume/Downloads/login.html") }, "dropped files go to the desktop's Downloads (cua sb cp)")
        } else { check(false, "the agent's tile is there") }

        // Control mapping on the user's own desktop (the fallback without Spaces), as Hover maps it.
        var apps = ScreenFeed.Apps(); apps.pids = [4242]
        let display = CGRect(x: 0, y: 0, width: 1512, height: 982)
        let target = ScreenControl.Target(pid: 4242, window: 7, frame: CGRect(x: 400, y: 200, width: 800, height: 600))
        let mapped = ScreenControl.call(["kind": "click", "x": 0.4, "y": 0.3], apps: apps, display: display, scale: 2, targets: [target])
        check(mapped?.tool == "click" && mapped?.args["window_id"] as? Int == 7, "without Spaces, Control still maps to the agent's own window only")
        check(ScreenControl.call(["kind": "click", "x": 0.02, "y": 0.02], apps: apps, display: display, scale: 2, targets: [target]) == nil, "and a click outside its apps goes nowhere")

        // The answer, in the chat.
        check(await until("the run ends", 60) { (await session())["stage"] as? String == "done" }, "the run finishes")
        await js("window.__office.openSession(\(sid))")
        try? await Task.sleep(nanoseconds: 600_000_000)
        check((await text("#thread")).contains("Sign-in works"), "the chat shows the answer")
        check(await truthy("document.querySelector('#thread .ans table')"), "the answer's table is drawn")
        check(await truthy("document.querySelector('#thread .s.k-web') || document.querySelector('#thread .sum')"), "the browser steps are in the chat's timeline")
        await shot("chat")

        // The full-screen button: from the notch, the office in a window, with what is open.
        await js("window.__office.openDesk(\(sid), 'browser')")
        try? await Task.sleep(nanoseconds: 300_000_000)
        check(await truthy("document.querySelector('#bigBtn svg')"), "the office has its full-screen button at the top right")
        _ = await click("#bigBtn")
        check(await until("the window message", 5) { self.windowAsks.contains { ($0["open"] as? [String: Any])?["tab"] as? String == "browser" } }, "it asks for the window, carrying the open desk and tab")

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

        // The desktop was turned off and left small: the next task sizes it, then starts it.
        let vms = root.appendingPathComponent("vms.json")
        if var v = (try? JSONSerialization.jsonObject(with: Data(contentsOf: vms))) as? [String: [String: Any]], let n = v.keys.first {
            v[n]?["status"] = "stopped"; v[n]?["cpuCount"] = 2; v[n]?["memorySize"] = 4 << 30
            try? JSONSerialization.data(withJSONObject: v).write(to: vms)
        }
        // A second agent in the same project works on the same desktop.
        backend.send(["type": "new", "tool": "codex", "folder": project, "prompt": "Also check sign-out", "access": "risky"])
        let both: () -> [[String: Any]] = { self.latest?["sessions"] as? [[String: Any]] ?? [] }
        check(await until("the second agent finishes", 60) { both().count == 2 && both().allSatisfy { $0["stage"] as? String == "done" } }, "a second agent in the same project runs and finishes")
        let creates = cuaLog().components(separatedBy: "spaces create").count - 1
        check(creates == 1, "it reuses the project's desktop: one Space made for both agents (\(creates))")
        let log2 = cuaLog()
        if let set = log2.range(of: "lume set hover-project-", options: .backwards), let start = log2.range(of: "spaces start local:hover-project-", options: .backwards) {
            check(set.lowerBound < start.lowerBound && log2[set.lowerBound...].contains("--cpu"), "the desktop that was off is given its room back, then started")
        } else { check(false, "the desktop that was off is sized and started (lume set, then spaces start)") }
        let names = Set(both().compactMap { ($0["space"] as? [String: Any])?["name"] as? String })
        check(names.count == 1, "both agents' desks name the same desktop: \(names)")
        let mates = both().map { (($0["space"] as? [String: Any])?["with"] as? [Any])?.count ?? 0 }
        check(mates == [1, 1], "each knows it shares the desktop with the other")
        let servers = (try? String(contentsOf: root.appendingPathComponent("agent.log"), encoding: .utf8))?.components(separatedBy: "\n").filter { $0.hasPrefix("mcpServers") } ?? []
        let tokens = servers.map { line -> String in (line.range(of: #"cua-space"[^\]]*\]"#, options: .regularExpression).map { String(line[$0]) } ?? "") }
        check(tokens.count == 2 && tokens[0] == tokens[1] && !tokens[0].isEmpty, "both sessions were handed the same desktop server")
        await js("window.__office.openDesk(\(both().last?["id"] as? Int ?? 0), 'screen')")
        check(await until("the shared badge", 10) { (await self.text("#pBody .spwith")).contains("Shared with") }, "the Screen panel says the desktop is shared: \(await text("#pBody .spwith"))")
        await shot("shared-desktop")

        // The project's desktop goes with its last session, not before.
        let keys = both().compactMap { $0["key"] as? String }
        backend.send(["type": "delete", "key": keys[0]])
        try? await Task.sleep(nanoseconds: 3_000_000_000)
        check(!cuaLog().contains("spaces delete"), "deleting one of the project's sessions keeps the desktop")
        backend.send(["type": "delete", "key": keys[1]])
        check(await until("its Space is deleted", 15) { cuaLog().contains("spaces delete local:hover-") }, "deleting the project's last session deletes its desktop")
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
