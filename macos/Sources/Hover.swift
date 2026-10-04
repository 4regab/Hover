import AppKit
import WebKit
import Security
import ServiceManagement
import UserNotifications
import Carbon

let smoke = CommandLine.arguments.contains("--smoke-test")
let environment = ProcessInfo.processInfo.environment
let sandboxRoot = environment["HOVER_SANDBOX_ROOT"]

final class App: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var backend = BackendPipe()
    /// What the backend was started with, to start it again if it dies, and when it did.
    var backendStart: (key: Data, env: [String: String])?, backendUp = false, backendRestarts: [Date] = []
    let screen = ScreenFeed()
    let browsers = AgentBrowsers()
    let spaceViewers = SpaceViewers(), teleport = TeleportDrag(), sendToVM = SendToVM()
    var dropView: NotchDropView?, dragging: TeleportDrag.Drag?, stayOpenUntil = Date.distantPast
    var zoom: Zoom?, zooming = false, screenWants: [Bool: Date] = [:]
    /// What the user last dropped on the notch: only those files may be sent to a desktop.
    var droppedFiles = Set<String>()
    /// Apps the user sent to a desktop while they ran here, by bundle id: once one has gone it
    /// leaves this Mac (Moved). The last app dragged to the notch, for a drop the page sends.
    var moving: [String: Moved.Sent] = [:], lastDrag: (path: String, at: Date)?
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
            // Opened from Downloads: offered once to move to Applications (and reopen there).
            if !smoke && MoveToApplications.offer() { return }
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
                backendStart = (key, environment)
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
                        self.backendStart = (key, env)
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
        // Over the open office while Finder or Dock drags pass: it takes their drop.
        if let content = notch.window.contentView {
            let drop = NotchDropView(frame: content.bounds); drop.autoresizingMask = [.width, .height]
            content.addSubview(drop); dropView = drop
            drop.hovering = { [weak self] p in self?.dragPhase("over", p) }
            drop.dropped = { [weak self] p, urls in self?.dropped(at: p, urls: urls) }
        }
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
        // An app or files dragged to the notch: the agents' desktops open as drop targets.
        teleport.near = { [weak self] p in
            guard let self else { return false }
            let s = self.notch.window.screen?.frame ?? NSScreen.main?.frame ?? .zero
            return self.notch.contains(p, margin: 24) || (p.y >= s.maxY - 8 && abs(p.x - s.midX) < 320)
        }
        teleport.phase = { [weak self] phase, p, drag in self?.dragging = drag; self?.dragPhase(phase, p) }
        NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown]) { [weak self] e in
            guard let self else { return }
            // Another app's title bar right-clicked: "Send to Hover VM".
            if e.type == .rightMouseDown, !smoke { self.sendToVM.rightMouseDown(at: NSEvent.mouseLocation) }
            guard self.notch.expanded, !self.notch.contains(NSEvent.mouseLocation) else { return }
            self.expand(false)
        }
        sendToVM.enabled = { [weak self] in self?.latest?["spaces"] as? Bool == true }
        sendToVM.projects = { [weak self] in self?.vmProjects() ?? [] }
        sendToVM.send = { [weak self] app, project in self?.sendApp(app, to: project) }
        sendToVM.turnOn = { [weak self] in self?.presentSettings(.computerUse) }
    }
    func pollPointer(at point: NSPoint, buttons: Int) {
        teleport.poll(point, buttons: buttons)
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
            // The office's window is up: the notch stays an island (a click brings the window).
            if hoverOpens && !dashboardUp && buttons == 0 && notch.state.kind != .waiting && Date().timeIntervalSince(since) >= 0.12 {
                expand(true); openedByHover = true
            }
        } else if inside { lastInside = Date() }
        else if openedByHover && !notch.window.isKeyWindow && !teleport.dragging && dragging == nil && Date().timeIntervalSince(lastInside) > 0.35 { expand(false) }
    }
    /// Where a screen point is in the notch's office, in its view's points (the page
    /// scales them to CSS pixels with vw).
    func pagePoint(_ p: CGPoint) -> [String: Any] {
        let web = office.web, local = web.convert(notch.window.convertPoint(fromScreen: p), from: nil)
        return ["x": local.x, "y": web.isFlipped ? local.y : web.bounds.height - local.y, "vw": web.bounds.width]
    }
    /// A drag over the notch: open it on the agents' desktops, follow the pointer, and
    /// send what was dropped to the desktop under it.
    func dragPhase(_ phase: String, _ p: CGPoint) {
        guard let d = dragging else { DragTrace.log("\(phase) with no drag"); return }
        if phase != "over" { DragTrace.log("notch \(phase): expanded \(notch.expanded), page point \(pagePoint(p))") }
        var m: [String: Any] = ["type": "teleportDrag", "phase": phase, "app": d.app, "files": d.files.map(\.path)]
        if let b = d.bundle { m["bundle"] = b }
        if let p = d.path { m["path"] = p }
        m.merge(pagePoint(p)) { $1 }
        switch phase {
        case "start":
            if let path = d.path { lastDrag = (path, Date()) }
            // The drop targets are in the notch's office, window or not.
            if !notch.expanded { expand(true, force: true) }
            // A Finder or Dock drag drops on the drop view, not the page's own web view.
            dropView?.isHidden = d.pid != nil
            notch.takesDrops = d.pid == nil
            office.deliver(m)
        case "over": office.deliver(m)
        case "drop":
            // Finder drags are dropped by the drop view; a window drag ends here. A Finder
            // drag let go anywhere else is called off.
            if d.pid != nil { office.deliver(m) }
            else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
                    guard let self, self.dragging != nil else { return }
                    self.office.deliver(["type": "teleportDrag", "phase": "cancel"]); self.endDrag()
                }
                return
            }
            endDrag()
        default: office.deliver(m); endDrag()
        }
    }
    func dropped(at p: CGPoint, urls: [URL]) {
        DragTrace.log("drop view took \(urls.count) item(s) at \(Int(p.x)),\(Int(p.y)); drag \(dragging == nil ? "none" : "on")")
        if var d = dragging, d.pid == nil {
            if d.files.isEmpty && d.bundle == nil { d.files = urls }
            droppedFiles = Set(d.files.map(\.path))
            dragging = d; dragPhase("over", p)
            var m: [String: Any] = ["type": "teleportDrag", "phase": "drop", "app": d.app, "files": d.files.map(\.path)]
            if let b = d.bundle { m["bundle"] = b }
            if let p = d.path { m["path"] = p }
            m.merge(pagePoint(p)) { $1 }; office.deliver(m)
        }
        teleport.finishedByDropView(); endDrag()
    }
    func endDrag() {
        dropView?.isHidden = true; dragging = nil; notch.takesDrops = false
        // The result shows a moment in the office, then it folds unless the user stays.
        foldLater(2.2)
    }
    /// Folds the notch after a drop, unless the user is in it or a drop is still going.
    func foldLater(_ after: TimeInterval) {
        DispatchQueue.main.asyncAfter(deadline: .now() + after) { [weak self] in
            guard let self, self.notch.expanded, !self.notch.window.isKeyWindow, !self.notch.contains(NSEvent.mouseLocation, margin: 14) else { return }
            if self.stayOpenUntil > Date() { self.foldLater(1); return }
            self.expand(false)
        }
    }

    func expand(_ on: Bool, keyboard: Bool = false, restoreFocus: Bool = true, animated: Bool = true, force: Bool = false) {
        guard !on || settings?.window.isKeyWindow != true else { return }
        // One office at a time: with its window up, opening the notch brings the window.
        if on && !force && dashboardUp { focusDashboard(); return }
        guard notch.expanded != on else { if on && keyboard { openedByHover = false; focusOffice() }; return }
        if on { previous = NSWorkspace.shared.frontmostApplication; openedByHover = false }
        notch.hovered = false
        notch.setOpen(on, animated: animated && !smoke)
        office.deliver(["type": "visible", "on": on]); lastInside = Date()
        if on && keyboard { focusOffice() }
        if !on {
            browsers.detach(from: office.web); spaceViewers.detach(from: office.web)
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
        // A press while it is thought held means its release was missed (the key window
        // changed in between): let go first, so this press counts instead of being dropped.
        if down && voiceHeld { voiceHeld = false; voice?.keyUp() }
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
    @objc func showDashboard() { openDashboard(then: nil) }
    /// The office's window is up on this Space (not closed, minimised or on another).
    var dashboardUp: Bool { guard let d = dashboard else { return false }; return d.isVisible && !d.isMiniaturized && d.isOnActiveSpace }
    func focusDashboard() {
        guard let d = dashboard else { return }
        dashboardOffice?.deliver(["type": "visible", "on": true])
        NSApp.activate(ignoringOtherApps: true); d.makeKeyAndOrderFront(nil)
    }
    /// The window, made once: on the notch's display, where it was last left.
    func makeDashboard() {
        guard dashboard == nil else { return }
        let screen = NSScreen.notchScreen ?? NSScreen.main
        let area = screen?.visibleFrame ?? CGRect(x: 0, y: 0, width: 1440, height: 900)
        let size = CGSize(width: min(1480, area.width * 0.86), height: min(940, area.height * 0.88))
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView], backing: .buffered, defer: false)
        w.titlebarAppearsTransparent = true; w.titleVisibility = .hidden; w.collectionBehavior.insert(.fullScreenPrimary)
        w.minSize = CGSize(width: 900, height: 600)
        w.title = "Hover — Agent Office"; w.isReleasedWhenClosed = false; w.delegate = self
        // Hover draws its own opening (the notch growing into it), not AppKit's.
        w.animationBehavior = .none
        w.backgroundColor = Zoom.floor
        w.setFrame(CGRect(x: area.midX - size.width / 2, y: area.midY - size.height / 2, width: size.width, height: size.height), display: false)
        w.setFrameAutosaveName("HoverOffice")
        let view = Office(resources: resources, dataFolder: dataFolder, dashboard: true); view.message = { [weak self] m in self?.handle(m) }
        w.contentView = view.web; dashboardOffice = view; dashboard = w
        if let latest { view.deliver(latest) }
    }
    /// The office in a window of its own, bigger than the notch: what was open there
    /// (a chat, a desk's panel) opens again in it. The notch grows into it (or, shut,
    /// the island does), as an app opens from the Dock.
    func openDashboard(then restore: [String: Any]?) {
        let fromNotch = notch.expanded
        makeDashboard()
        guard let w = dashboard, let view = dashboardOffice else { return }
        if let restore { view.later(restore) }
        if dashboardUp || smoke || zooming {
            if fromNotch { expand(false, restoreFocus: false) }
            focusDashboard(); return
        }
        zooming = true
        let screen = notch.window.screen ?? NSScreen.notchScreen ?? NSScreen.main!
        let begin: (NSImage?) -> Void = { [weak self] image in
            guard let self else { return }
            let from = fromNotch ? self.notch.openFrame : self.notch.shapeFrame
            let z = Zoom(on: screen); self.zoom = z
            // The notch folds at once under the picture of itself, which then grows.
            if fromNotch { self.expand(false, restoreFocus: false, animated: false) }
            // Laid out at its size from the start; shown when the zoom lands on it.
            w.alphaValue = 0
            NSApp.activate(ignoringOtherApps: true); w.makeKeyAndOrderFront(nil)
            view.deliver(["type": "visible", "on": true])
            z.run(image: image, from: from, to: w.frame, radius: (fromNotch ? 26 : 10, 12), duration: fromNotch ? 0.44 : 0.4) { [weak self] in
                self?.whenShown(view, wait: 3) { w.alphaValue = 1; self?.whenShown(view, wait: 0.35) { z.finish(); self?.zoom = nil; self?.zooming = false } }
            }
        }
        guard fromNotch else { begin(nil); return }
        // The open office as it looks now, the notch's strip above it included.
        let web = office.web, strip = notch.notchHeight, size = notch.openFrame.size
        web.takeSnapshot(with: nil) { image, _ in
            guard let image else { begin(nil); return }
            begin(NSImage(size: size, flipped: false) { r in
                Zoom.floor.setFill(); r.fill()
                NSColor.black.setFill(); CGRect(x: 0, y: r.height - strip, width: r.width, height: strip).fill()
                image.draw(in: CGRect(x: 0, y: 0, width: r.width, height: r.height - strip)); return true
            })
        }
    }
    /// Calls back once the office's page has loaded and drawn a frame (a cold window
    /// loads it for a second or two), or after `wait` seconds whatever happens.
    func whenShown(_ view: Office, wait: TimeInterval, _ done: @escaping () -> Void) {
        var called = false
        let once = { if !called { called = true; done() } }
        DispatchQueue.main.asyncAfter(deadline: .now() + wait, execute: once)
        func poll() {
            guard !called else { return }
            guard view.ready else { DispatchQueue.main.asyncAfter(deadline: .now() + 0.05, execute: poll); return }
            view.web.callAsyncJavaScript("await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))", arguments: [:], in: nil, in: .page) { _ in once() }
        }
        poll()
    }
    /// Closing folds the window back into the notch.
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender === dashboard, !smoke, !zooming, !sender.styleMask.contains(.fullScreen), let web = dashboardOffice?.web, let screen = sender.screen else { return true }
        zooming = true
        web.takeSnapshot(with: nil) { [weak self] image, _ in
            guard let self else { return }
            let z = Zoom(on: screen); self.zoom = z
            z.run(image: image, from: sender.frame, to: self.notch.shapeFrame, radius: (12, 10), duration: 0.36, fade: true) { [weak self] in
                z.finish(fade: 0); self?.zoom = nil; self?.zooming = false
            }
            sender.close()
        }
        return false
    }
    func windowWillClose(_ notification: Notification) {
        if let web = dashboardOffice?.web { browsers.detach(from: web); spaceViewers.detach(from: web) }
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
    /// The projects an app can go to: the agents' folders in the office (one desktop
    /// each), the last folder used, and the voice projects; each once.
    func vmProjects() -> [SendToVM.Project] {
        var seen = Set<String>(), out: [SendToVM.Project] = []
        func add(_ folder: String?, _ name: String? = nil) {
            guard let folder, !folder.isEmpty, FileManager.default.fileExists(atPath: folder) else { return }
            let key = (folder as NSString).standardizingPath.lowercased()
            guard seen.insert(key).inserted else { return }
            out.append(.init(name: name ?? (folder as NSString).lastPathComponent, folder: folder))
        }
        for s in latest?["sessions"] as? [[String: Any]] ?? [] { add(s["folder"] as? String, (s["space"] as? [String: Any])?["project"] as? String) }
        add(latest?["folder"] as? String)
        for p in VoiceSettings.projects { add(p.path, p.name) }
        return out
    }
    /// Sends a running app to a project's desktop and opens the window on it, where the
    /// sending shows and the app lands (its review first, for an app Cua can teleport).
    func sendApp(_ app: NSRunningApplication, to project: SendToVM.Project) {
        guard let path = app.bundleURL?.path else { return }
        let name = app.localizedName ?? (path as NSString).lastPathComponent
        let session = (latest?["sessions"] as? [[String: Any]] ?? []).first { ($0["folder"] as? String).map { ($0 as NSString).standardizingPath.lowercased() } == (project.folder as NSString).standardizingPath.lowercased() }
        var m: [String: Any] = ["type": "teleport", "folder": project.folder, "app": name, "path": path]
        if let b = app.bundleIdentifier { m["bundle"] = b; moving[b] = Moved.Sent(bundle: b, pid: app.processIdentifier, tabs: [], at: Date()) }
        if let id = session?["id"] { m["id"] = id }
        openDashboard(then: session?["id"].map { ["type": "restore", "desk": $0, "tab": "screen"] })
        handle(m)
    }
    /// After a send has gone: the tabs that went close in the user's browser (a browser
    /// left with no window quits), or the app quits as if from its own menu, so it can
    /// ask to save first. Only what the user sent while it ran here.
    func takeOff(_ m: [String: Any]) {
        guard let b = m["bundle"] as? String, let sent = moving[b] else { return }
        // Over, gone or not: a later send is noted afresh.
        moving[b] = nil
        let app = NSRunningApplication(processIdentifier: sent.pid)
        let name = app?.localizedName ?? m["app"] as? String ?? "The app"
        switch Moved.after(m, sent: sent) {
        case .nothing: return
        case .quit:
            // terminate() is a request: the app may ask to save first, or stay if told to.
            if app?.terminate() == true { say("\(name) is on the agents’ desktop now, so it quits here.") }
        case .close(let tabs):
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let done = BrowserTabs.close(tabs, in: b)
                DispatchQueue.main.async {
                    guard let self else { return }
                    let count = done?.closed ?? tabs.count, n = count == 1 ? "The tab" : "The \(count) tabs"
                    switch done {
                    case nil: self.say("\(n) went to the agents’ desktop, but Hover couldn’t close \(count == 1 ? "it" : "them") in \(name) here.")
                    // Changed since they were read (or closed by hand): left alone.
                    case let d? where d.closed == 0: return
                    // Nothing of the user's is left in it: the browser goes too.
                    case let d? where d.windows == 0: app?.terminate(); self.say("\(n) went to the agents’ desktop. \(name) had nothing else open, so it quits here.")
                    default: self.say("\(n) went to the agents’ desktop and \(count == 1 ? "is" : "are") closed in \(name) here.")
                    }
                }
            }
        }
    }
    /// A line in both offices' toasts.
    func say(_ text: String) { office.deliver(["type": "toast", "text": text]); dashboardOffice?.deliver(["type": "toast", "text": text]) }
    /// The tools with a newer release out ("cua-driver" for Cua Driver).
    var updatesAvailable: [String] {
        let tools = (latest?["tools"] as? [[String: Any]] ?? []).compactMap { t -> String? in
            guard let u = t["update"] as? [String: Any], u["available"] as? Bool == true, u["busy"] as? Bool != true else { return nil }
            return t["id"] as? String
        }
        return tools + (settings.model.cua.update.map { $0.available && !$0.busy } == true ? ["cua-driver"] : [])
    }
    @objc func updateAll() { for id in updatesAvailable { backend.send(["type": "update", "tool": id]) } }
    @objc func quit() { NSApp.terminate(nil) }
    func handle(_ m: [String: Any]) {
        if DragTrace.on, let t = m["type"] as? String, ["teleport", "spaceFiles", "dragTrace"].contains(t) { DragTrace.log("page: \(m)") }
        switch m["type"] as? String {
        case "dragTrace": return
        // Files go to a desktop only as the user dropped them, whatever the page asks.
        case "spaceFiles":
            var out = m
            out["paths"] = (m["paths"] as? [String] ?? []).filter { droppedFiles.contains($0) }
            if (out["paths"] as? [String])?.isEmpty == false { backend.send(out) }
        // A drop being sent: the notch doesn't fold on its own until then.
        case "stay": stayOpenUntil = Date().addingTimeInterval(min(60, (m["seconds"] as? NSNumber)?.doubleValue ?? 10))
        // A browser's tabs are read here (macOS asks once to let Hover control it) for the
        // review to offer one by one, and kept, so only the tabs Hover read can be closed.
        case "teleport" where m["include"] == nil && m["tabs"] == nil && m["urls"] == nil:
            if let b = m["bundle"] as? String, let path = m["path"] as? String,
               let drag = lastDrag, drag.path == path, Date().timeIntervalSince(drag.at) < 120,
               let app = NSRunningApplication.runningApplications(withBundleIdentifier: b).first {
                // An app dragged to the notch from the Dock or Finder while it runs.
                moving[b] = Moved.Sent(bundle: b, pid: app.processIdentifier, tabs: [], at: Date())
            }
            guard let b = m["bundle"] as? String, BrowserTabs.handles(b), BrowserTabs.running(b) else { backend.send(m); break }
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let read = BrowserTabs.read(b)
                DispatchQueue.main.async {
                    guard let self else { return }
                    // Unread, its tabs could be neither picked nor closed here: the browser
                    // would go bare and stay. Say why, where it's fixed, and send nothing.
                    if read.denied {
                        self.moving[b] = nil
                        let name = m["app"] as? String ?? "the browser"
                        self.receive(["type": "error", "of": "teleport", "text": "Hover isn’t allowed to see \(name)’s tabs. Turn on \(name) under Hover in System Settings → Privacy & Security → Automation, then send it again."])
                        if !smoke { NSWorkspace.shared.open(BrowserTabs.automationSettings) }
                        return
                    }
                    self.moving[b]?.tabs = read.tabs
                    var out = m; out["tabs"] = read.tabs.map { ["id": $0.id, "title": $0.title, "url": $0.url] }
                    self.backend.send(out)
                }
            }
        case "ready":
            backend.send(m)
            // A page made again finds the browsers where they are.
            for b in browsers.states() { office.deliver(b); dashboardOffice?.deliver(b) }
            if smoke { DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in self?.checkSmoke() } }
        case "settings": if !smoke { showSettings() }
        case "setup":
            // A tool the office greyed out: its page, with the one-click button.
            if let id = m["tool"] as? String { if !smoke { presentSettings(.tool(id)) } }
        // Esc with nothing open: the notch folds; the window stays (it closes as windows do).
        case "fold": if m["dashboard"] as? Bool != true { expand(false) }
        // The full-screen button: from the notch, the office in a big window, with what
        // was open; in the window, full screen and back.
        case "window":
            guard !smoke else { return }
            if m["dashboard"] as? Bool == true { dashboard?.toggleFullScreen(nil); return }
            var restore: [String: Any] = ["type": "restore"]
            if let open = m["open"] as? [String: Any] { restore.merge(open) { $1 } }
            openDashboard(then: restore)
        // The agent's own desktop (a Cua Space): its viewer over the Screen panel.
        case "spaceOverlay":
            guard let id = m["space"] as? String else { return }
            let web = m["dashboard"] as? Bool == true ? dashboardOffice?.web : office.web
            guard let web else { return }
            var rect: CGRect?
            if let r = m["rect"] as? [String: Any], let x = (r["x"] as? NSNumber)?.doubleValue, let y = (r["y"] as? NSNumber)?.doubleValue,
               let w = (r["w"] as? NSNumber)?.doubleValue, let h = (r["h"] as? NSNumber)?.doubleValue {
                let k = (r["vw"] as? NSNumber).map { web.bounds.width / CGFloat(max(1, $0.doubleValue)) } ?? 1
                rect = CGRect(x: x * k, y: y * k, width: w * k, height: h * k)
            }
            spaceViewers.show(id, url: m["url"] as? String ?? "", rect: smoke ? nil : rect, in: web)
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
            // One feed for both offices: off only when neither wants it (a folding notch's
            // "off" doesn't stop the window's).
            let from = m["dashboard"] as? Bool == true, on = m["on"] as? Bool ?? false
            screenWants[from] = on ? Date().addingTimeInterval(8) : nil
            if on || !screenWants.values.contains(where: { $0 > Date() }) { screen.ask(on: on, live: m["live"] as? Bool ?? false, apps: m["apps"] as? [String: Any]) }
        case "screenAccess": if !smoke { screen.requestAccess() }
        // Control on the agent's desktop: the user's click or key, to the agent's own app,
        // in the background through Cua Driver. Never anywhere else.
        case "screenInput":
            guard !smoke else { return }
            let apps = ScreenFeed.Apps(m["apps"] as? [String: Any])
            let reply: (String?) -> Void = { error in self.office.deliver(["type": "screenInput", "error": error ?? NSNull()]); self.dashboardOffice?.deliver(["type": "screenInput", "error": error ?? NSNull()]) }
            guard let exe = ScreenControl.exe() else { reply("Install Cua Driver (Settings → Computer Use) to control the agent’s desktop."); return }
            let display = CGDisplayBounds(CGMainDisplayID()), scale = NSScreen.screens.first?.backingScaleFactor ?? 2
            guard let call = ScreenControl.call(m, apps: apps, display: display, scale: scale, targets: ScreenControl.targets(apps)) else {
                if m["kind"] as? String == "click" { reply("That isn’t one of the agent’s apps.") }
                return
            }
            ScreenControl.send(call.tool, call.args, exe: exe) { error in if let error { reply(error) } }
        // The desk's Browser panel: where the session's browser shows, and the user's
        // address, back and reload.
        case "browserView":
            guard let id = (m["id"] as? NSNumber)?.intValue else { return }
            let web = m["dashboard"] as? Bool == true ? dashboardOffice?.web : office.web
            guard let web else { return }
            var rect: CGRect?
            if let r = m["rect"] as? [String: Any], let x = (r["x"] as? NSNumber)?.doubleValue, let y = (r["y"] as? NSNumber)?.doubleValue,
               let w = (r["w"] as? NSNumber)?.doubleValue, let h = (r["h"] as? NSNumber)?.doubleValue {
                // CSS pixels to the web view's points, whatever the page's zoom.
                let k = (r["vw"] as? NSNumber).map { web.bounds.width / CGFloat(max(1, $0.doubleValue)) } ?? 1
                rect = CGRect(x: x * k, y: y * k, width: w * k, height: h * k)
            }
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
        case "initialized": backendUp = true; settings.model.request(["type": "getSettings"])
        case "state":
            latest = m
            let ids = Set((m["sessions"] as? [[String: Any]] ?? []).compactMap { ($0["id"] as? NSNumber)?.intValue })
            browsers.keep(ids)
            spaceViewers.keep(Set((m["sessions"] as? [[String: Any]] ?? []).compactMap { ($0["space"] as? [String: Any])?["name"] as? String }))
            office.deliver(m); dashboardOffice?.deliver(m)
            settings.model.receiveState(m)
            voice?.state = m
            updateIsland()
        case "preferences":
            receivePreferences(m)
        case "computerUse":
            settings.model.receiveComputerUse(m)
        case "spaces":
            settings.model.receiveSpaces(m)
        case "machine":
            settings.model.receiveMachine(m)
        // An agent's call to Hover's browser (BrowserTool in the backend).
        case "browser": browsers.handle(m)
        // A teleport to review: the notch stays open, with the keyboard, until it's answered.
        // To one office only, so it is answered once: the window when it is up.
        case "teleport" where m["phase"] as? String == "review":
            DragTrace.log("teleport review for \(m["app"] ?? "?")")
            if dashboardUp && !notch.expanded { focusDashboard(); dashboardOffice?.deliver(m) } else { expand(true, keyboard: true, force: true); office.deliver(m) }
        // Gone to the desktop: what went leaves this Mac.
        case "teleport" where m["phase"] as? String == "done":
            office.deliver(m); dashboardOffice?.deliver(m)
            takeOff(m)
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
                let content = UNMutableNotificationContent(); content.title = m["title"] as? String ?? "Agent finished"; content.body = m["error"] as? String ?? m["text"] as? String ?? ""; content.sound = .default
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
        // Something the user asked for failed: a popup in the office that shows, or Hover's
        // own alert when none does (a toast there was gone before it was read).
        case "error":
            let text = m["text"] as? String ?? "Something went wrong."
            if voice?.backendToast(text) == true { break }
            let showing: [Office] = (notch.expanded ? [office] : []) + (dashboardUp ? [dashboardOffice].compactMap { $0 } : [])
            if showing.isEmpty {
                // The office still puts a refused task's words back in its box, quietly.
                var quiet = m; quiet["quiet"] = true
                office.deliver(quiet); dashboardOffice?.deliver(quiet)
                alertError(text)
            } else { for o in showing { o.deliver(m) } }
        case "backendFailure": if !restartBackend(m["text"] as? String ?? "") { fatal(m["text"] as? String ?? "Backend stopped") }
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
    /// The backend died after it was up: started again (three times in five minutes at
    /// most), with a popup saying so, instead of Hover quitting. Its tasks were stopped with
    /// it; their chats are in the history. False when it isn't (Hover then quits).
    func restartBackend(_ why: String) -> Bool {
        guard backendUp, !smoke, let start = backendStart else { return false }
        backendRestarts = backendRestarts.filter { $0 > Date().addingTimeInterval(-300) }
        guard backendRestarts.count < 3 else { return false }
        backendRestarts.append(Date()); backendUp = false
        let next = BackendPipe()
        next.receive = { [weak self] m in self?.receive(m) }
        backend = next
        do { try next.start(resources: resources, dataFolder: dataFolder, key: start.key, env: start.env) } catch { return false }
        // The offices learn the new backend's state as a page that just loaded does.
        next.send(["type": "ready"])
        receive(["type": "error", "of": "backend", "text": "\(why.isEmpty ? "The agent backend stopped." : why.replacingOccurrences(of: " Quit and reopen Hover.", with: "")) Hover started it again. Tasks that were running were stopped; their chats are in the history."])
        return true
    }
    func fatal(_ text: String) {
        if smoke { finishSmoke(false, text); return }
        let alert = NSAlert(); alert.messageText = "Hover could not start"; alert.informativeText = text; alert.runModal(); NSApp.terminate(nil)
    }
    /// A failure with no office on screen to show it: Hover's alert, in front. One at a
    /// time; another while it shows is said in the log.
    private var alerting = false
    func alertError(_ text: String, title: String = "That didn’t work") {
        FileHandle.standardError.write(Data("Hover error: \(text)\n".utf8))
        if smoke || alerting { return }
        alerting = true
        DispatchQueue.main.async { [weak self] in
            NSApp.activate(ignoringOtherApps: true)
            let alert = NSAlert(); alert.alertStyle = .warning; alert.messageText = title; alert.informativeText = text; alert.addButton(withTitle: "OK")
            alert.runModal()
            self?.alerting = false
        }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool { if !smoke { showDashboard() }; return false }
    /// The projects' desktops are turned off as Hover quits, by a cua of their own that
    /// outlives Hover (a VM takes a while to stop), so none is left holding memory.
    func stopSpaces() {
        guard !smoke, let cua = ["\(NSHomeDirectory())/.local/bin/cua", "/usr/local/bin/cua", "/opt/homebrew/bin/cua"].first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else { return }
        let names = Set((latest?["sessions"] as? [[String: Any]] ?? []).compactMap { ($0["space"] as? [String: Any]).flatMap { $0["phase"] as? String == "ready" ? $0["name"] as? String : nil } })
        for n in names {
            let p = Process(); p.executableURL = URL(fileURLWithPath: cua); p.arguments = ["spaces", "stop", "local:" + n]
            var env = ProcessInfo.processInfo.environment; env["CUA_TELEMETRY"] = "0"; p.environment = env
            p.standardInput = FileHandle.nullDevice; p.standardOutput = FileHandle.nullDevice; p.standardError = FileHandle.nullDevice
            try? p.run()
        }
    }
    func applicationWillTerminate(_ note: Notification) { stopSpaces(); poll?.invalidate(); clock?.invalidate(); if let hotKey { UnregisterEventHotKey(hotKey) }; if let voiceKey { UnregisterEventHotKey(voiceKey) }; holdEscape(false); voice?.cancel(); backend.stop() }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = App(); app.delegate = delegate
app.run()
