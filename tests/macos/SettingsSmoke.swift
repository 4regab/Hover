import AppKit

// Native hit tests and control actions; no global input injection or Accessibility permission.
extension App {
    func checkSettingsInteractions() throws {
        func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
            if !condition() { throw HostError(message: "Settings regression: " + message) }
        }
        let model = settings.model
        var sent: [[String: Any]] = []
        let originalSend = model.send
        model.send = { sent.append($0) }
        let fixture: [String: Any] = ["type": "preferences", "hover": true, "noticeSeen": false, "maxRunning": 3,
            "tools": ["kiro", "codex", "cursor", "opencode"].map { ["id": $0, "access": "full", "idle": 5, "hideSteps": false] as [String: Any] }, "quotaItems": []]
        // First run: one reply opens Settings once, on Get Started, with no extra request.
        while model.outstanding > 0 { model.receive(fixture) }
        firstPreferencesHandled = false
        model.request(["type": "getSettings"]); sent.removeAll()
        receivePreferences(fixture, allowPrompt: true)
        let window = settings.window
        try require(window.isVisible && model.page == .start && !notch.expanded, "first run did not open Get Started")
        try require(sent.filter { $0["type"] as? String == "getSettings" }.count == 1, "first run asked for preferences \(sent.count) times")
        window.makeKeyAndOrderFront(nil)
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        // Edits apply at once and survive stale echoes of older requests.
        while model.outstanding > 0 { model.receive(fixture) }
        sent.removeAll()
        model.setNotice(true); model.setHover(false); model.setMaxRunning(5)
        model.setPref("kiro") { $0.access = "always"; $0.idle = 15 }
        model.setQuota("codex", true)
        try require(sent.count == 5 && sent.allSatisfy { $0["type"] as? String == "saveSettings" }, "edits were not saved at once: \(sent)")
        try require((sent[3]["tools"] as? [[String: Any]])?.first?["access"] as? String == "always", "tool access payload")
        for _ in 0..<4 { model.receive(fixture) }
        try require(model.noticeSeen && !model.hover && model.maxRunning == 5 && model.pref("kiro").access == "always" && model.quotaItems == ["codex"], "a stale reply reset an edit")
        var settled = fixture; settled["noticeSeen"] = true; settled["hover"] = false; settled["maxRunning"] = 5; settled["quotaItems"] = ["codex"]
        settled["tools"] = [["id": "kiro", "access": "always", "idle": 15, "hideSteps": false]]
        try require(model.receive(settled) && model.outstanding == 0, "the newest reply was not applied")
        try require(model.pref("kiro").idle == 15 && !model.hover, "settled reply")
        // Replies nobody asked for don't reopen or bounce the window.
        window.close()
        for _ in 0..<10 { receivePreferences(fixture, allowPrompt: true) }
        try require(!window.isVisible, "Close bounced back open")
        // With Settings in front, the notch stays shut under the pointer.
        showSettings(); window.makeKeyAndOrderFront(nil)
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        if window.isKeyWindow {
            for _ in 0..<20 { pollPointer(at: NSPoint(x: notch.shapeFrame.midX, y: notch.shapeFrame.midY), buttons: 1) }
            try require(!notch.expanded, "the notch opened over Settings")
        }
        // Setup states from the office's tool list.
        model.receiveState(["type": "state", "tools": [
            ["id": "kiro", "name": "Kiro", "checkedYet": true, "installed": true, "signedIn": false, "ready": false, "canSetup": true, "setup": ["busy": false, "needs": []]],
            ["id": "codex", "name": "Codex", "checkedYet": true, "installed": false, "signedIn": false, "ready": false, "canSetup": true,
             "setup": ["busy": false, "needs": ["Installing Codex's ACP adapter"]]],
            ["id": "cursor", "name": "Cursor", "checkedYet": true, "installed": false, "ready": false, "canSetup": true,
             "setup": ["busy": true, "step": "installing", "line": "Downloading cursor-agent…", "needs": ["Installing the Cursor CLI"]]],
            ["id": "opencode", "name": "OpenCode", "checkedYet": true, "installed": false, "ready": false, "canSetup": true, "setup": ["error": "Couldn’t install OpenCode: offline", "needs": []]],
        ]])
        try require(model.tools.map(\.id) == ["codex", "kiro", "cursor", "opencode"], "tool order \(model.tools.map(\.id))")
        try require(model.status("codex").action == "Set Up" && model.status("codex").summary == "Installs Codex's ACP adapter", "install state \(model.status("codex").summary)")
        try require(model.status("kiro").action == "Sign In" && model.status("kiro").phase == .needsSignIn, "sign-in state")
        try require(model.status("cursor").action == "Cancel" && model.status("cursor").summary == "Downloading cursor-agent…", "installing state")
        try require(model.status("opencode").phase == .failed && model.status("opencode").action == "Try Again", "failed state")
        sent.removeAll(); model.setup("codex"); model.setup("cursor")
        try require(sent.count == 2 && sent[0]["step"] as? String == "auto" && sent[1]["step"] as? String == "cancel", "setup clicks \(sent)")
        // Computer use: the switch saves at once; the driver's button follows its state.
        while model.outstanding > 0 { model.receive(fixture) }
        sent.removeAll(); model.setComputerUse(true)
        try require(sent.count == 1 && sent[0]["computerUse"] as? Bool == true && model.computerUse, "computer use switch \(sent)")
        model.receive(fixture)
        try require(!model.computerUse, "the settled reply sets computer use")
        model.receiveComputerUse(["checked": true, "installed": false, "permissions": "unknown"])
        try require(model.cua.action?.step == "install", "missing driver offers Install")
        model.receiveComputerUse(["checked": true, "installed": true, "permissions": "missing", "canGrant": true])
        try require(model.cua.action?.step == "grant" && model.cua.action?.title == "Grant Access", "ungranted driver offers Grant Access")
        model.receiveComputerUse(["checked": true, "installed": true, "permissions": "granted", "ready": true, "version": "cua-driver 0.31.0"])
        try require(model.cua.action == nil && model.cua.summary == "Ready · cua-driver 0.31.0", "ready driver \(model.cua.summary)")
        model.receiveComputerUse(["checked": true, "installed": true, "busy": true, "step": "granting", "line": ""])
        try require(model.cua.action?.step == "cancel", "a running setup offers Cancel")
        sent.removeAll(); model.cuaSetup("grant")
        try require(sent.count == 1 && sent[0]["type"] as? String == "computerUseSetup" && sent[0]["step"] as? String == "grant", "setup message \(sent)")
        model.quotas = ["codex": QuotaValue(ok: true, used: 42, detail: "5h 42% · week 18%"), "cursor": QuotaValue(ok: true, used: 74, detail: "74% of the plan")]
        model.quotaItems = ["codex", "cursor"]
        // Pictures of each page, for review (the window is laid out at its real size).
        if let root = sandboxRoot, let content = window.contentView {
            model.receiveComputerUse(["type": "computerUse", "on": true, "checked": true, "installed": true, "version": "cua-driver 0.31.0",
                                      "permissions": "missing", "ready": false, "canGrant": true, "busy": false,
                                      "hint": "CuaDriver needs Accessibility and Screen Recording. Grant them once; agents can’t drive apps until then."])
            model.computerUse = true
            for (name, page) in [("start", SettingsModel.Page.start), ("general", .general), ("usage", .usage), ("computer-use", .computerUse), ("tool", .tool("codex"))] {
                model.page = page
                RunLoop.current.run(until: Date().addingTimeInterval(0.4))
                content.layoutSubtreeIfNeeded()
                if let bitmap = content.bitmapImageRepForCachingDisplay(in: content.bounds) {
                    content.cacheDisplay(in: content.bounds, to: bitmap)
                    try bitmap.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: root).appendingPathComponent("settings-\(name).png"))
                }
            }
        }
        window.close()
        model.send = originalSend
        let center = NSPoint(x: notch.shapeFrame.midX, y: notch.shapeFrame.midY)
        hoverNeedsExit = true
        pollPointer(at: center, buttons: 0)
        try require(!notch.expanded, "notch reopened before pointer left")
        pollPointer(at: NSPoint(x: notch.shapeFrame.minX - 10, y: notch.shapeFrame.minY - 10), buttons: 0)
        // Hover opens after a short dwell, so a pointer crossing the menu bar doesn't.
        pollPointer(at: center, buttons: 0)
        try require(!notch.expanded && notch.hovered, "a passing pointer opened the notch")
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        pollPointer(at: center, buttons: 0)
        try require(notch.expanded, "hover did not resume after pointer left")
        // Opened by hover, the office folds once the pointer leaves it.
        let away = NSPoint(x: notch.shapeFrame.minX - 60, y: notch.shapeFrame.minY - 60)
        pollPointer(at: away, buttons: 0)
        RunLoop.current.run(until: Date().addingTimeInterval(0.45))
        pollPointer(at: away, buttons: 0)
        try require(!notch.expanded, "hover-opened office did not fold when the pointer left")
        try checkIsland()
        expand(true)
        let report: [String: Any] = ["success": true, "foregroundActivationVerified": NSApp.isActive, "checks": ["first-run", "instant apply", "stale replies", "close", "notch suppression", "setup states", "setup clicks", "computer use", "hover dwell", "hover leave", "island states", "notch geometry", "menu bar", "shell environment"]]
        try JSONSerialization.data(withJSONObject: report, options: .prettyPrinted).write(to: URL(fileURLWithPath: sandboxRoot!).appendingPathComponent("settings-interactions.json"))
    }

    /// The island's states from office snapshots, the shapes they take and their hit areas.
    func checkIsland() throws {
        func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
            if !condition() { throw HostError(message: "Island regression: " + message) }
        }
        func settle() { RunLoop.current.run(until: Date().addingTimeInterval(0.8)) }
        let g = notch.geometry
        try require(g.notchWidth > 40 && g.notchHeight >= 24, "notch geometry \(g)")
        let saved = latest
        latest = ["type": "state", "sessions": []]; updateIsland(); settle()
        let idle = notch.shapeFrame
        try require(notch.state.kind == .idle && abs(idle.height - g.notchHeight) < 1, "idle shape is not the notch: \(idle)")
        let now = Date().timeIntervalSince1970 * 1000
        latest = ["type": "state", "sessions": [["id": 1, "tool": "codex", "stage": "working", "act": "Editing", "turns": [["t0": now - 125_000]]],
                                                ["id": 2, "tool": "kiro", "stage": "working", "act": "Reading", "turns": []]]]
        updateIsland(); settle()
        try require(notch.state.kind == .working && notch.state.tools == ["codex", "kiro"] && notch.state.text == "Editing 2m", "working island \(notch.state)")
        let working = notch.shapeFrame
        try require(working.width > idle.width + 40 && abs(working.midX - idle.midX) < 1, "working wings are not centred on the notch")
        latest = ["type": "state", "sessions": [["id": 3, "tool": "cursor", "stage": "waiting", "turns": [],
                  "ask": ["id": "a1", "line": "Run npm test", "title": "Run a command", "danger": false, "questions": NSNull()]]]]
        updateIsland(); settle()
        try require(notch.state.kind == .waiting && notch.state.line == "Run npm test" && notch.state.canAllow, "waiting island \(notch.state)")
        try require(notch.shapeFrame.height > g.notchHeight + 30, "question card did not grow")
        // Hovering a waiting question must not open the office (as on Windows).
        let c = NSPoint(x: notch.shapeFrame.midX, y: notch.shapeFrame.maxY - 4)
        pollPointer(at: c, buttons: 0); RunLoop.current.run(until: Date().addingTimeInterval(0.2)); pollPointer(at: c, buttons: 0)
        try require(!notch.expanded, "hover opened the office over a waiting question")
        notch.island.display()
        var sent: String?
        notch.island.onAnswer = { sent = $0 }
        try require(notch.island.buttonTitles == ["review", "allow", "deny"], "question buttons \(notch.island.buttonTitles)")
        notch.island.press("deny")
        try require(sent == "deny", "Deny did not answer")
        notch.island.onAnswer = { [weak self] answer in self?.answerFromNotch(answer) }
        ended = ("kiro", "Fix the build", true, Date().addingTimeInterval(6))
        latest = ["type": "state", "sessions": []]; updateIsland()
        try require(notch.state.kind == .ended(ok: true) && notch.state.text == "Fix the build", "ended island \(notch.state)")
        ended = nil; latest = saved; updateIsland(); settle()
        if let root = sandboxRoot, let image = notch.window.contentView.flatMap({ v in v.bitmapImageRepForCachingDisplay(in: v.bounds).map { (v, $0) } }) {
            image.0.cacheDisplay(in: image.0.bounds, to: image.1)
            try image.1.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: root).appendingPathComponent("island-smoke.png"))
        }
        let bar = MenuBar(); bar.enabled = ["codex", "cursor"]; bar.quotas = ["codex": QuotaValue(ok: true, used: 42, detail: "5h 42%"), "cursor": QuotaValue(ok: false, used: nil, detail: "Sign in")]
        try require((bar.item.button?.image?.size.width ?? 0) > 60, "menu bar usage image is missing")
        try require(MenuBar.percent(bar.quotas["codex"]) == "42%" && MenuBar.percent(bar.quotas["cursor"]) == "—", "menu bar percentages")
        NSStatusBar.system.removeStatusItem(bar.item)
        // T3 Code's marker protocol: values between markers, empty ones dropped, PATHs merged in order.
        let output = "motd noise\n__HOVER_ENV_PATH_START__\n/opt/homebrew/bin:/usr/bin\n__HOVER_ENV_PATH_END__\n__HOVER_ENV_LANG_START__\n__HOVER_ENV_LANG_END__\n"
        let values = ShellEnvironment.extract(output, names: ["PATH", "LANG"])
        try require(values == ["PATH": "/opt/homebrew/bin:/usr/bin"], "shell markers \(values)")
        try require(ShellEnvironment.merge(["/a:/b", nil, "/b:/c:", "/a"]) == "/a:/b:/c", "PATH merge")
        try require(ShellEnvironment.command(for: ["PATH", "bad;name"]).contains("printenv PATH") && !ShellEnvironment.command(for: ["bad;name"]).contains("bad"), "unsafe variable names are not dropped")
    }
}
