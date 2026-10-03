import AppKit
import SwiftUI
import QuartzCore

// Voice tasks: hold ⌃⌥Space and speak, let go, and what was said becomes a task.
// A glass capsule drops in under the notch, Siri-style, with a glow that moves with
// the voice, a live waveform and the words as they're heard. Let go and it opens into
// a card: the task (editable), the agent (a new chat with the default agent, or another
// agent, or a reply to one already at a desk), the folder it runs in, and a short
// countdown. Edit anything and the countdown stops; Return starts, Esc cancels.
// A quick tap instead of a hold listens hands-free until the next press.

// MARK: Settings

struct VoiceProject: Codable, Identifiable, Equatable, Hashable {
    var id = UUID()
    var name: String
    var path: String
    /// Other names it may be called, comma separated.
    var aliases: String = ""
}

enum VoiceSettings {
    private static let d = UserDefaults.standard
    static var enabled: Bool { get { d.object(forKey: "voice.enabled") as? Bool ?? true } set { d.set(newValue, forKey: "voice.enabled") } }
    /// Seconds the card waits before starting; 0 waits for Return.
    static var countdown: Int { get { d.object(forKey: "voice.countdown") as? Int ?? 3 } set { d.set(newValue, forKey: "voice.countdown") } }
    static var screenGlow: Bool { get { d.object(forKey: "voice.glow") as? Bool ?? true } set { d.set(newValue, forKey: "voice.glow") } }
    static var sounds: Bool { get { d.object(forKey: "voice.sounds") as? Bool ?? true } set { d.set(newValue, forKey: "voice.sounds") } }
    /// "last" (the agent last picked in the office) or a tool id.
    static var agent: String { get { d.string(forKey: "voice.agent") ?? "last" } set { d.set(newValue, forKey: "voice.agent") } }
    static var projects: [VoiceProject] {
        get { (d.data(forKey: "voice.projects")).flatMap { try? JSONDecoder().decode([VoiceProject].self, from: $0) } ?? [] }
        set { d.set(try? JSONEncoder().encode(newValue), forKey: "voice.projects") }
    }
    /// Where a task goes when nothing else fits: ~/Hover, made when first needed.
    static var workspace: String { FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Hover").path }
}

// MARK: Model

struct VoiceTool: Identifiable, Equatable { var id: String; var name: String; var ready: Bool; var hint: String; var access: String }
struct VoiceSession: Identifiable, Equatable { var id: Int; var title: String; var tool: String; var bot: String; var stage: String }
struct VoiceFolder: Identifiable, Equatable, Hashable { var id: String { path }; var name: String; var path: String; var project: Bool }
enum VoiceTarget: Hashable { case new(String), reply(Int) }

final class VoiceModel: ObservableObject {
    enum Stage: Equatable { case idle, preparing(String), listening, finishing, compose, starting, started(String), error(String, URL?) }
    @Published var stage: Stage = .idle
    @Published var handsFree = false
    @Published var levels: [CGFloat] = Array(repeating: 0, count: 36)
    @Published var level: CGFloat = 0
    @Published var heard = ""
    @Published var guess = ""
    @Published var text = ""
    @Published var target: VoiceTarget = .new("kiro")
    @Published var folder = ""
    @Published var why: String?
    @Published var countdown: Double?
    @Published var countdownTotal: Double = 3
    @Published var problem: String?
    @Published var tools: [VoiceTool] = []
    @Published var sessions: [VoiceSession] = []
    @Published var folders: [VoiceFolder] = []
    @Published var trial = false
    var original = ""
    var onStart: () -> Void = {}
    var onCancel: () -> Void = {}
    var onFinish: () -> Void = {}
    var onChooseFolder: () -> Void = {}
    var onOpenSettings: (URL?) -> Void = { _ in }
    var onOpenOffice: () -> Void = {}

    var tool: String {
        switch target {
        case .new(let t): return t
        case .reply(let id): return sessions.first { $0.id == id }?.tool ?? "kiro"
        }
    }
    var toolInfo: VoiceTool? { tools.first { $0.id == tool } }
    var replying: VoiceSession? { if case .reply(let id) = target { return sessions.first { $0.id == id } }; return nil }
    var folderName: String { folders.first { $0.path == folder }?.name ?? (folder as NSString).lastPathComponent }

    /// Why Start can't go now, if it can't.
    var blocker: String? {
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Say or type what the agent should do." }
        if replying != nil { return nil }
        if let t = toolInfo, !t.ready { return t.hint.isEmpty ? "\(t.name) isn’t set up yet." : t.hint }
        return nil
    }
    func stopCountdown() { if countdown != nil { countdown = nil } }
}

// MARK: Routing

enum VoiceRoute {
    static let bots = ["Pip", "Juno", "Moss", "Nova", "Ada", "Rue"]
    // Kiro is a new word to the speech models, which hear it as these.
    private static let tools: [(String, String)] = [("open ?code", "opencode"), ("kiro|kyro|keiro|keyro|kero|piro|pyro|kira", "kiro"), ("codex|codecs", "codex"), ("cursor", "cursor"), ("claude( code)?|cloud code", "claude")]

    /// A leading "Ask Codex to …", "Codex, …" or "Tell Pip …" picks the agent, and comes off the task.
    static func agent(in text: String, sessions: [VoiceSession]) -> (String, VoiceTarget?) {
        let lead = #"^\s*(?:(?:hey|ok|okay)[,\s]+)?(?:(?:ask|tell|have|get|use|with)\s+)?"#
        let tail = #"\b[,:]?\s*(?:to\s+|and\s+)?"#
        for (pattern, id) in tools {
            if let r = text.range(of: lead + "(" + pattern + ")" + tail, options: [.regularExpression, .caseInsensitive]) {
                return (tidy(String(text[r.upperBound...])), .new(id))
            }
        }
        for s in sessions where !s.bot.isEmpty {
            if let r = text.range(of: lead + "(" + NSRegularExpression.escapedPattern(for: s.bot) + ")" + tail, options: [.regularExpression, .caseInsensitive]) {
                return (tidy(String(text[r.upperBound...])), .reply(s.id))
            }
        }
        return (text, nil)
    }

    /// The folder a task names ("… in the Hover project"): the longest project name,
    /// alias or folder name said as whole words.
    static func folder(in text: String, among folders: [VoiceFolder], projects: [VoiceProject]) -> VoiceFolder? {
        let said = " " + words(text) + " "
        var best: (VoiceFolder, Int)?
        for f in folders {
            var names = [f.name, (f.path as NSString).lastPathComponent]
            if let p = projects.first(where: { $0.path == f.path }) { names += p.aliases.split(separator: ",").map(String.init) }
            for n in names {
                let w = words(n)
                guard w.count >= 3, said.contains(" " + w + " ") else { continue }
                if best == nil || w.count > best!.1 { best = (f, w.count) }
            }
        }
        return best?.0
    }

    /// Lower case, camelCase and separators split into words.
    static func words(_ s: String) -> String {
        let split = s.replacingOccurrences(of: #"([a-z0-9])([A-Z])"#, with: "$1 $2", options: .regularExpression)
        return split.lowercased().replacingOccurrences(of: #"[^a-z0-9]+"#, with: " ", options: .regularExpression).trimmingCharacters(in: .whitespaces)
    }

    static func tidy(_ s: String) -> String {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let f = t.first else { return t }
        return f.uppercased() + t.dropFirst()
    }
}

// MARK: Controller

final class VoiceController: NSObject, DictationSink {
    let model = VoiceModel()
    let panel: VoicePanel
    private let glow = ScreenGlow()
    private var dictation: Dictation?
    private var pressedAt: Date?
    private var tick: Timer?, hideWork: DispatchWorkItem?
    private var generation = 0
    private var awaitingBackend: Date?
    /// What the card held when the shortcut was pressed again over it.
    private var kept = ""
    /// The office's latest state, for tools, sessions and folders.
    var state: [String: Any] = [:]
    var noticeSeen: () -> Bool = { true }
    var send: ([String: Any]) -> Void = { _ in }
    var openSettings: (SettingsModel.Page) -> Void = { _ in }
    var openOffice: () -> Void = {}
    /// Takes or lets go of Esc while listening (the panel doesn't have the keyboard then).
    var holdEscape: (Bool) -> Void = { _ in }
    var screen: () -> NSScreen? = { NSScreen.notchScreen }

    var active: Bool { model.stage != .idle }

    override init() {
        panel = VoicePanel(model: model)
        super.init()
        model.onStart = { [weak self] in self?.start() }
        model.onCancel = { [weak self] in self?.cancel() }
        model.onFinish = { [weak self] in self?.finishListening() }
        model.onChooseFolder = { [weak self] in self?.chooseFolder() }
        model.onOpenSettings = { [weak self] url in
            guard let self else { return }
            self.hide()
            if let url { NSWorkspace.shared.open(url) } else { self.openSettings(.voice) }
        }
        model.onOpenOffice = { [weak self] in self?.hide(); self?.openOffice() }
    }

    // MARK: The shortcut

    func keyDown(trial: Bool = false) {
        voiceLog.info("key down: stage \(String(describing: self.model.stage), privacy: .public), enabled \(VoiceSettings.enabled, privacy: .public)")
        guard VoiceSettings.enabled || trial else { return }
        switch model.stage {
        case .idle, .started, .error: begin(trial: trial); pressedAt = Date()
        case .listening, .preparing: if model.handsFree { finishListening() }
        case .compose:
            // Pressed again over the card: keep talking, added to what's there.
            begin(trial: model.trial, keeping: model.text); pressedAt = Date()
        case .finishing, .starting: break
        }
    }

    func keyUp() {
        guard let at = pressedAt else { return }
        pressedAt = nil
        guard model.stage == .listening || isPreparing, !model.handsFree else { return }
        // A tap listens hands-free; a hold ends when it's let go.
        if Date().timeIntervalSince(at) < 0.32 { withAnimation(.snappy) { model.handsFree = true } }
        else { finishListening() }
    }

    func escape() { if active { cancel() } }

    private var isPreparing: Bool { if case .preparing = model.stage { return true }; return false }

    // MARK: Listening

    private func begin(trial: Bool, keeping: String = "") {
        generation += 1
        let gen = generation
        hideWork?.cancel()
        dictation?.cancel()
        refreshChoices()
        model.trial = trial
        model.handsFree = false
        kept = keeping
        model.heard = keeping; model.guess = ""; model.problem = nil
        model.levels = Array(repeating: 0, count: model.levels.count); model.level = 0
        model.stopCountdown()
        set(.listening)
        holdEscape(true)
        if VoiceSettings.sounds { NSSound(named: "Tink")?.play() }
        if VoiceSettings.screenGlow, let s = screen() { glow.show(on: s) }
        let d = Dictation(); d.sink = self; dictation = d
        Task { @MainActor in
            do { try await d.start(locale: Locale.autoupdatingCurrent) }
            catch {
                voiceLog.error("start failed: \(error.localizedDescription, privacy: .public)")
                guard gen == self.generation else { return }
                d.cancel(); self.dictation = nil
                self.fail(error.localizedDescription, (error as? DictationError)?.settingsURL)
            }
        }
    }

    func finishListening() {
        guard model.stage == .listening || isPreparing, let d = dictation else { return }
        let gen = generation
        set(.finishing)
        holdEscape(false)
        glow.hide()
        Task { @MainActor in
            let said = await d.finish()
            voiceLog.info("finished: \(said.count, privacy: .public) characters heard")
            guard gen == self.generation else { return }
            self.dictation = nil
            let text = Self.join(self.kept, said)
            if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                if VoiceSettings.sounds { NSSound(named: "Funk")?.play() }
                self.fail("Nothing was heard. Hold ⌃⌥Space, speak, then let go.", nil, quiet: true)
                return
            }
            self.compose(text)
        }
    }

    // DictationSink

    func dictationLevel(_ level: Float) {
        guard model.stage == .listening else { return }
        let l = CGFloat(level)
        model.level = model.level * 0.55 + l * 0.45
        var v = model.levels; v.removeFirst(); v.append(l); model.levels = v
        glow.level = model.level
    }

    func dictationText(final: String, partial: String) {
        guard model.stage == .listening || model.stage == .finishing else { return }
        model.heard = Self.join(kept, final); model.guess = partial
    }

    static func join(_ a: String, _ b: String) -> String {
        let a = a.trimmingCharacters(in: .whitespacesAndNewlines), b = b.trimmingCharacters(in: .whitespacesAndNewlines)
        return a.isEmpty ? b : b.isEmpty ? a : a + " " + b
    }

    func dictationPreparing(_ what: String?) {
        guard model.stage == .listening || isPreparing else { return }
        set(what.map { .preparing($0) } ?? .listening)
    }

    // MARK: The card

    private func compose(_ said: String) {
        refreshChoices()
        let defaultTool = VoiceSettings.agent == "last" ? (state["tool"] as? String ?? "kiro") : VoiceSettings.agent
        let (task, picked) = VoiceRoute.agent(in: said, sessions: model.sessions)
        model.text = task; model.original = task
        model.target = picked ?? .new(model.tools.contains { $0.id == defaultTool } ? defaultTool : (model.tools.first?.id ?? "kiro"))
        // A reply goes to its session's folder; a new task to the one it names, else the last one.
        if let s = model.replying, let f = sessionFolder(s.id) { model.folder = f; model.why = nil }
        else if let f = VoiceRoute.folder(in: task, among: model.folders, projects: VoiceSettings.projects) { model.folder = f.path; model.why = "Heard “\(f.name)”" }
        else { model.folder = defaultFolder(); model.why = nil }
        model.problem = nil
        set(.compose)
        panel.takeKeyboard()
        let total = Double(VoiceSettings.countdown)
        if total > 0 && model.blocker == nil && !model.trial { model.countdownTotal = total; model.countdown = total; startTicking() }
    }

    private func startTicking() {
        tick?.invalidate()
        let started = Date(), total = model.countdownTotal
        tick = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { [weak self] t in
            guard let self, self.model.stage == .compose, self.model.countdown != nil else { t.invalidate(); return }
            // A menu open on the card (the agent or the folder) stops the countdown.
            if RunLoop.current.currentMode == .eventTracking { t.invalidate(); self.model.stopCountdown(); return }
            let left = total - Date().timeIntervalSince(started)
            if left <= 0 { t.invalidate(); self.model.countdown = 0; self.start() } else { self.model.countdown = left }
        }
        RunLoop.main.add(tick!, forMode: .common)
    }

    func start() {
        guard model.stage == .compose else { return }
        tick?.invalidate(); model.stopCountdown()
        if let b = model.blocker { model.problem = b; return }
        let text = model.text.trimmingCharacters(in: .whitespacesAndNewlines)
        if model.trial {
            let where_ = model.replying.map { "reply to \($0.bot.isEmpty ? $0.title : $0.bot)" } ?? "start \(Marks.name(model.tool)) in \(model.folderName)"
            finishCard("Try it: this would \(where_). Nothing was started.")
            return
        }
        if let s = model.replying {
            send(["type": "reply", "id": s.id, "text": text])
            awaitingBackend = Date()
            finishCard("Sent to \(s.bot.isEmpty ? Marks.name(s.tool) : s.bot)")
            return
        }
        guard noticeSeen() else {
            model.problem = "Review what agents may do in Settings → Get Started before the first task."
            return
        }
        var folder = model.folder
        if folder.isEmpty { folder = VoiceSettings.workspace }
        var dir: ObjCBool = false
        if !FileManager.default.fileExists(atPath: folder, isDirectory: &dir) || !dir.boolValue {
            // The default workspace is made when it's first needed; any other folder must exist.
            guard folder == VoiceSettings.workspace, (try? FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)) != nil else {
                model.problem = "That folder no longer exists. Pick another."; return
            }
        }
        send(["type": "new", "tool": model.tool, "folder": folder, "prompt": text])
        awaitingBackend = Date()
        finishCard("\(Marks.name(model.tool)) is on it · \((folder as NSString).lastPathComponent)")
    }

    private func finishCard(_ line: String) {
        if VoiceSettings.sounds && !model.trial { NSSound(named: "Pop")?.play() }
        set(.started(line))
        panel.releaseKeyboard()
        scheduleHide(after: model.trial ? 3 : 1.8)
    }

    /// The backend turned the task down (a busy desk, a tool that stopped): say so here,
    /// where the user is looking, rather than in the folded office.
    func backendToast(_ text: String) -> Bool {
        guard let at = awaitingBackend, Date().timeIntervalSince(at) < 3 else { return false }
        awaitingBackend = nil
        fail(text, nil)
        return true
    }

    func cancel() {
        generation += 1
        tick?.invalidate(); model.stopCountdown()
        dictation?.cancel(); dictation = nil
        holdEscape(false)
        hide()
    }

    private func fail(_ message: String, _ url: URL?, quiet: Bool = false) {
        holdEscape(false)
        glow.hide()
        set(.error(message, url))
        panel.releaseKeyboard()
        scheduleHide(after: quiet ? 2.4 : 7)
    }

    private func scheduleHide(after seconds: Double) {
        hideWork?.cancel()
        let gen = generation
        let w = DispatchWorkItem { [weak self] in
            guard let self, gen == self.generation else { return }
            if self.panel.hovered { self.scheduleHide(after: 1.5); return }
            self.hide()
        }
        hideWork = w
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: w)
    }

    private func hide() {
        hideWork?.cancel()
        glow.hide()
        panel.dismiss { [weak self] in self?.model.stage = .idle }
    }

    private func set(_ stage: VoiceModel.Stage) {
        let s = screen()
        withAnimation(.spring(response: 0.42, dampingFraction: 0.82)) { model.stage = stage }
        panel.present(on: s)
    }

    // MARK: Choices

    private func chooseFolder() {
        model.stopCountdown()
        let picker = NSOpenPanel()
        picker.canChooseDirectories = true; picker.canChooseFiles = false; picker.allowsMultipleSelection = false
        picker.title = "Choose the folder the agent works in"
        if !model.folder.isEmpty { picker.directoryURL = URL(fileURLWithPath: model.folder) }
        picker.level = NSWindow.Level(rawValue: panel.level.rawValue + 1)
        NSApp.activate(ignoringOtherApps: true)
        if picker.runModal() == .OK, let url = picker.url {
            if !model.folders.contains(where: { $0.path == url.path }) { model.folders.insert(VoiceFolder(name: url.lastPathComponent, path: url.path, project: false), at: 0) }
            model.folder = url.path; model.why = nil
        }
        panel.takeKeyboard()
    }

    private func sessionFolder(_ id: Int) -> String? {
        (state["sessions"] as? [[String: Any]])?.first { $0["id"] as? Int == id }?["folder"] as? String
    }

    private func defaultFolder() -> String {
        let fm = FileManager.default
        if let f = state["folder"] as? String, !f.isEmpty, fm.fileExists(atPath: f) { return f }
        if let p = VoiceSettings.projects.first(where: { fm.fileExists(atPath: $0.path) }) { return p.path }
        return model.folders.first?.path ?? VoiceSettings.workspace
    }

    private func refreshChoices() {
        let tools = (state["tools"] as? [[String: Any]] ?? []).compactMap { t -> VoiceTool? in
            guard let id = t["id"] as? String else { return nil }
            return VoiceTool(id: id, name: t["name"] as? String ?? Marks.name(id), ready: t["ready"] as? Bool ?? false, hint: t["hint"] as? String ?? "", access: t["access"] as? String ?? "full")
        }
        model.tools = tools.isEmpty ? SettingsModel.order.map { VoiceTool(id: $0, name: Marks.name($0), ready: true, hint: "", access: "full") } : tools
        model.sessions = (state["sessions"] as? [[String: Any]] ?? []).compactMap { s in
            guard let id = s["id"] as? Int else { return nil }
            let bot: String
            if let i = s["bot"] as? Int { bot = VoiceRoute.bots.indices.contains(i) ? VoiceRoute.bots[i] : "" } else { bot = s["bot"] as? String ?? "" }
            return VoiceSession(id: id, title: s["title"] as? String ?? "Session", tool: s["tool"] as? String ?? "kiro", bot: bot, stage: s["stage"] as? String ?? "")
        }
        let fm = FileManager.default
        var seen = Set<String>(), list: [VoiceFolder] = []
        func add(_ path: String?, _ name: String? = nil, project: Bool = false) {
            guard let path, !path.isEmpty, !seen.contains(path), fm.fileExists(atPath: path) else { return }
            seen.insert(path); list.append(VoiceFolder(name: name ?? (path as NSString).lastPathComponent, path: path, project: project))
        }
        for p in VoiceSettings.projects { add(p.path, p.name, project: true) }
        add(state["folder"] as? String)
        for s in state["sessions"] as? [[String: Any]] ?? [] { add(s["folder"] as? String) }
        for h in (state["history"] as? [[String: Any]] ?? []).prefix(30) { add(h["folder"] as? String) }
        add(VoiceSettings.workspace, "Hover workspace")
        model.folders = Array(list.prefix(14))
    }
}

// MARK: The panel

final class VoiceWindow: NSPanel {
    var wantsKey = false
    override var canBecomeKey: Bool { wantsKey }
    override var canBecomeMain: Bool { false }
    override func cancelOperation(_ sender: Any?) { (contentView as? VoiceHostingView)?.model?.onCancel() }
}

final class VoiceHostingView: NSHostingView<VoiceView> {
    weak var model: VoiceModel?
    var onHover: (Bool) -> Void = { _ in }
    private var area: NSTrackingArea?
    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let area { removeTrackingArea(area) }
        let a = NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(a); area = a
    }
    override func mouseEntered(with event: NSEvent) { onHover(true) }
    override func mouseExited(with event: NSEvent) { onHover(false) }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class VoicePanel {
    let window: VoiceWindow
    private let host: VoiceHostingView
    private let model: VoiceModel
    private(set) var hovered = false
    var level: NSWindow.Level { window.level }
    /// Room around the card for its glow and shadow.
    static let margin: CGFloat = 36
    static let width: CGFloat = 680

    init(model: VoiceModel) {
        self.model = model
        window = VoiceWindow(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: 300), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        window.isOpaque = false; window.backgroundColor = .clear; window.hasShadow = false
        window.level = NSWindow.Level(rawValue: NSWindow.Level.statusBar.rawValue + 2)
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        window.hidesOnDeactivate = false; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        window.becomesKeyOnlyIfNeeded = false
        var report: (CGRect) -> Void = { _ in }
        host = VoiceHostingView(rootView: VoiceView(model: model, onCard: { report($0) }))
        host.model = model
        host.sizingOptions = []
        window.contentView = host
        host.onHover = { [weak self] in self?.hovered = $0 }
        window.appearance = NSAppearance(named: .darkAqua)
        report = { [weak self] r in self?.card = r; self?.track(NSEvent.mouseLocation) }
    }

    /// The card's frame in the window, top-left origin (from the SwiftUI layout).
    var card: CGRect = .zero

    /// Clicks go to the card; everywhere else in the panel falls through.
    func track(_ point: NSPoint) {
        guard window.isVisible else { return }
        let f = window.frame
        let local = CGPoint(x: point.x - f.minX, y: f.maxY - point.y)
        let inside = card.insetBy(dx: -4, dy: -4).contains(local)
        if window.ignoresMouseEvents == inside { window.ignoresMouseEvents = !inside }
    }

    /// Shows the panel (or keeps it), centred on the screen a little under the notch.
    func present(on screen: NSScreen?) {
        guard let screen = screen ?? NSScreen.main else { return }
        let f = screen.frame
        let height: CGFloat = 360
        let top = f.maxY - (f.maxY - screen.visibleFrame.maxY) - 10
        window.setFrame(NSRect(x: f.midX - Self.width / 2, y: top - height, width: Self.width, height: height), display: true)
        if !window.isVisible { window.alphaValue = 1; window.orderFrontRegardless() }
    }

    func takeKeyboard() {
        window.wantsKey = true
        window.makeKeyAndOrderFront(nil)
    }

    func releaseKeyboard() {
        window.wantsKey = false
        if window.isKeyWindow { window.resignKey() }
    }

    func dismiss(_ done: @escaping () -> Void) {
        releaseKeyboard()
        guard window.isVisible else { done(); return }
        NSAnimationContext.runAnimationGroup({ c in c.duration = 0.18; window.animator().alphaValue = 0 }) { [weak self] in
            self?.window.orderOut(nil); self?.window.alphaValue = 1; done()
        }
    }
}

// MARK: The screen's edge glow

/// A soft band of colour around the screen's edges while listening, as Siri does,
/// brighter as the voice gets louder. Drawn by Core Animation: the colours turn in the
/// render server, so the app does no work per frame.
final class ScreenGlow {
    private var window: NSWindow?
    private let container = CALayer(), gradient = CAGradientLayer(), mask = CAShapeLayer(), inner = CAShapeLayer()
    var level: CGFloat = 0 {
        didSet {
            CATransaction.begin(); CATransaction.setAnimationDuration(0.12)
            container.opacity = Float(0.55 + 0.45 * min(1, level * 1.4))
            CATransaction.commit()
        }
    }

    func show(on screen: NSScreen) {
        let w = window ?? make()
        w.setFrame(screen.frame, display: false)
        layout(screen.frame.size)
        container.opacity = 0.55
        w.alphaValue = 0
        w.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { c in c.duration = 0.35; w.animator().alphaValue = 1 }
    }

    func hide() {
        guard let w = window, w.isVisible else { return }
        NSAnimationContext.runAnimationGroup({ c in c.duration = 0.4; w.animator().alphaValue = 0 }) { w.orderOut(nil) }
    }

    private func make() -> NSWindow {
        let w = NSWindow(contentRect: .zero, styleMask: [.borderless], backing: .buffered, defer: false)
        w.isOpaque = false; w.backgroundColor = .clear; w.hasShadow = false; w.ignoresMouseEvents = true
        w.level = NSWindow.Level(rawValue: NSWindow.Level.statusBar.rawValue + 1)
        w.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        w.isReleasedWhenClosed = false
        let v = NSView(); v.wantsLayer = true
        w.contentView = v
        v.layer?.addSublayer(container)
        gradient.type = .conic
        gradient.colors = VoiceView.palette.map { NSColor($0).cgColor } + [NSColor(VoiceView.palette[0]).cgColor]
        gradient.startPoint = CGPoint(x: 0.5, y: 0.5); gradient.endPoint = CGPoint(x: 0.5, y: 0)
        container.addSublayer(gradient)
        mask.fillColor = nil; mask.strokeColor = NSColor.black.cgColor
        mask.shadowColor = NSColor.black.cgColor; mask.shadowOpacity = 1; mask.shadowOffset = .zero; mask.shadowRadius = 16
        inner.fillColor = nil; inner.strokeColor = NSColor.black.withAlphaComponent(0.35).cgColor
        inner.shadowColor = NSColor.black.cgColor; inner.shadowOpacity = 0.6; inner.shadowOffset = .zero; inner.shadowRadius = 40
        let m = CALayer(); m.addSublayer(mask); m.addSublayer(inner)
        container.mask = m
        let spin = CABasicAnimation(keyPath: "transform.rotation.z")
        spin.fromValue = 0; spin.toValue = -2 * Double.pi; spin.duration = 7; spin.repeatCount = .infinity
        gradient.add(spin, forKey: "spin")
        window = w
        return w
    }

    private func layout(_ size: CGSize) {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let r = CGRect(origin: .zero, size: size)
        container.frame = r
        let side = hypot(size.width, size.height)
        gradient.bounds = CGRect(x: 0, y: 0, width: side, height: side)
        gradient.position = CGPoint(x: r.midX, y: r.midY)
        container.mask?.frame = r
        let path = CGPath(roundedRect: r.insetBy(dx: -2, dy: -2), cornerWidth: 14, cornerHeight: 14, transform: nil)
        mask.frame = r; mask.path = path; mask.lineWidth = 10
        inner.frame = r; inner.path = path; inner.lineWidth = 34
        CATransaction.commit()
    }
}

// MARK: Views

private struct CardFrame: PreferenceKey {
    static var defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) { value = nextValue() }
}

struct VoiceView: View {
    @ObservedObject var model: VoiceModel
    var onCard: (CGRect) -> Void
    /// Apple Intelligence's colours.
    static let palette: [Color] = [
        Color(red: 0.74, green: 0.51, blue: 0.95), Color(red: 0.96, green: 0.73, blue: 0.92), Color(red: 0.55, green: 0.62, blue: 1.0),
        Color(red: 1.0, green: 0.40, blue: 0.47), Color(red: 1.0, green: 0.73, blue: 0.44), Color(red: 0.78, green: 0.53, blue: 1.0),
    ]

    private var wide: Bool { model.stage == .compose || model.stage == .starting }
    private var live: Bool { switch model.stage { case .listening, .finishing, .preparing: return true; default: return false } }
    private var radius: CGFloat { wide ? 26 : 30 }

    var body: some View {
        VStack(spacing: 0) {
            if model.stage != .idle {
                card
                    .frame(width: wide ? 620 : 480)
                    .modifier(Glass(radius: radius))
                    .overlay(GlowBorder(radius: radius, level: model.level, live: live))
                    .shadow(color: .black.opacity(0.4), radius: 22, y: 12)
                    .background(GeometryReader { g in Color.clear.preference(key: CardFrame.self, value: g.frame(in: .global)) })
                    .transition(.asymmetric(insertion: .scale(scale: 0.86, anchor: .top).combined(with: .opacity), removal: .opacity))
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onPreferenceChange(CardFrame.self) { onCard($0) }
        .environment(\.colorScheme, .dark)
    }

    @ViewBuilder private var card: some View {
        switch model.stage {
        case .idle: EmptyView()
        case .preparing(let what):
            HStack(spacing: 14) {
                Orb(level: 0.2, busy: true).frame(width: 38, height: 38)
                VStack(alignment: .leading, spacing: 3) {
                    Text(what).font(.system(size: 15, weight: .medium))
                    Text("Once only. It stays on this Mac, and so does what you say.").font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                ProgressView().controlSize(.small)
            }
            .padding(.horizontal, 14).padding(.vertical, 13)
        case .listening, .finishing: listening
        case .compose, .starting: ComposeCard(model: model)
        case .started(let line):
            HStack(spacing: 12) {
                Image(systemName: model.trial ? "eye.circle.fill" : "checkmark.circle.fill")
                    .font(.system(size: 26)).symbolRenderingMode(.palette)
                    .foregroundStyle(.white, model.trial ? Color.blue : Color.green)
                Text(line).font(.system(size: 15, weight: .medium)).lineLimit(2)
                Spacer(minLength: 8)
                if !model.trial {
                    Button("Open Office") { model.onOpenOffice() }.buttonStyle(PillButton())
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 13)
        case .error(let message, let url):
            HStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 20)).foregroundStyle(.orange)
                Text(message).font(.system(size: 14, weight: .medium)).lineLimit(3).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 8)
                if url != nil { Button("Open Settings") { model.onOpenSettings(url) }.buttonStyle(PillButton()) }
                Button { model.onCancel() } label: { Image(systemName: "xmark").font(.system(size: 11, weight: .bold)) }
                    .buttonStyle(PillButton(round: true)).help("Dismiss")
            }
            .padding(.horizontal, 16).padding(.vertical, 13)
        }
    }

    private var listening: some View {
        let finishing = model.stage == .finishing
        let words = VoiceController.join(model.heard, model.guess)
        return HStack(spacing: 14) {
            Orb(level: finishing ? 0.15 : model.level, busy: finishing).frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 3) {
                Group {
                    if words.isEmpty {
                        Text(finishing ? "One moment…" : model.handsFree ? "Listening hands-free…" : "Listening…").foregroundStyle(.secondary)
                    } else {
                        (Text(model.heard) + Text(model.heard.isEmpty || model.guess.isEmpty ? "" : " ") + Text(model.guess).foregroundColor(.white.opacity(0.55)))
                    }
                }
                .font(.system(size: 15.5, weight: .medium))
                .lineLimit(2).truncationMode(.head)
                .frame(maxWidth: .infinity, alignment: .leading)
                .animation(.easeOut(duration: 0.15), value: words)
                Text(finishing ? "Writing it down" : model.handsFree ? "⌃⌥Space to finish · Esc to cancel" : "Let go to finish · tap for hands-free · Esc to cancel")
                    .font(.system(size: 11)).foregroundStyle(.white.opacity(0.45))
            }
            Waveform(levels: Array(model.levels.suffix(22)), muted: finishing).frame(width: 84, height: 30)
            if model.handsFree && !finishing {
                Button { model.onFinish() } label: { Image(systemName: "checkmark").font(.system(size: 12, weight: .bold)) }
                    .buttonStyle(PillButton(round: true, prominent: true)).help("Done (⌃⌥Space)")
            }
        }
        .padding(.leading, 12).padding(.trailing, 16).padding(.vertical, 12)
    }
}

private struct ComposeCard: View {
    @ObservedObject var model: VoiceModel
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Orb(level: 0.1, busy: false).frame(width: 22, height: 22)
                Text(model.replying.map { "Reply to \($0.bot.isEmpty ? $0.title : $0.bot)" } ?? "New task")
                    .font(.system(size: 14, weight: .semibold))
                if model.trial {
                    Text("Try it").font(.system(size: 10.5, weight: .semibold)).padding(.horizontal, 7).padding(.vertical, 2)
                        .background(Capsule().fill(Color.blue.opacity(0.35)))
                }
                Spacer()
                Group {
                    if let c = model.countdown { Text("Starting in \(Int(c.rounded(.up)))s · edit to wait") }
                    else if model.trial { Text("Nothing will start") }
                    else { Text("Return to \(model.replying == nil ? "start" : "send") · Esc to cancel") }
                }
                .font(.system(size: 11.5)).foregroundStyle(.white.opacity(0.5)).monospacedDigit()
            }
            TextField("What should the agent do?", text: $model.text, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 17))
                .lineLimit(1...7)
                .focused($focused)
                .onSubmit { model.onStart() }
                .padding(.horizontal, 14).padding(.vertical, 11)
                .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color.white.opacity(0.07)))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Color.white.opacity(focused ? 0.18 : 0.08)))
            HStack(spacing: 8) {
                AgentMenu(model: model)
                if model.replying == nil { FolderMenu(model: model) }
                if let a = accessLabel { Chip(icon: "shield", text: a, tint: a == "Full access" ? .orange : .green).help(accessHelp) }
                Spacer(minLength: 4)
                Button("Cancel") { model.onCancel() }.buttonStyle(PillButton()).keyboardShortcut(.cancelAction)
                StartButton(model: model)
            }
            if let p = model.problem {
                Label(p, systemImage: "exclamationmark.triangle.fill").font(.system(size: 12.5)).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            } else if let why = model.why {
                Label(why, systemImage: "folder.badge.questionmark").font(.system(size: 11.5)).foregroundStyle(.white.opacity(0.5))
            }
        }
        .padding(16)
        .onAppear { DispatchQueue.main.async { focused = true } }
        .onChange(of: model.text) { _, new in if new != model.original { model.stopCountdown(); model.problem = nil } }
        .onChange(of: model.target) { _, _ in model.stopCountdown(); model.problem = nil }
        .onChange(of: model.folder) { _, _ in model.stopCountdown() }
    }

    private var accessLabel: String? {
        guard model.replying == nil else { return nil }
        switch model.toolInfo?.access { case "risky": return "Ask first"; case "always": return "Ask always"; case "read": return "Read only"; case "full": return "Full access"; default: return nil }
    }
    private var accessHelp: String { "The agent's tool access, from its page in Settings." }
}

private struct AgentMenu: View {
    @ObservedObject var model: VoiceModel
    var body: some View {
        Menu {
            Section("New chat") {
                ForEach(model.tools) { t in
                    Button { model.target = .new(t.id) } label: {
                        Label { Text(t.ready ? t.name : "\(t.name) — \(t.hint)") } icon: { Image(nsImage: MenuBar.tileImage(t.id)) }
                    }
                    .disabled(!t.ready)
                }
            }
            if !model.sessions.isEmpty {
                Section("Reply to") {
                    ForEach(model.sessions) { s in
                        Button { model.target = .reply(s.id) } label: {
                            Label { Text(s.bot.isEmpty ? s.title : "\(s.bot) · \(s.title)") } icon: { Image(nsImage: MenuBar.tileImage(s.tool)) }
                        }
                    }
                }
            }
        } label: {
            HStack(spacing: 7) {
                Image(nsImage: MenuBar.tileImage(model.tool)).resizable().frame(width: 16, height: 16)
                Text(model.replying.map { $0.bot.isEmpty ? $0.title : $0.bot } ?? Marks.name(model.tool)).lineLimit(1)
                Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).opacity(0.6)
            }
            .font(.system(size: 12.5, weight: .medium))
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(Capsule().fill(Color.white.opacity(0.1)))
            .contentShape(Capsule())
        }
        .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        .help("The agent: a new chat, or a reply to one at a desk")
    }
}

private struct FolderMenu: View {
    @ObservedObject var model: VoiceModel
    var body: some View {
        Menu {
            let projects = model.folders.filter(\.project), recent = model.folders.filter { !$0.project }
            if !projects.isEmpty {
                Section("Projects") { ForEach(projects) { f in Button(f.name) { model.folder = f.path; model.why = nil } } }
            }
            if !recent.isEmpty {
                Section("Recent") { ForEach(recent) { f in Button(f.name) { model.folder = f.path; model.why = nil }.help(f.path) } }
            }
            Divider()
            Button("Choose Folder…") { model.onChooseFolder() }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "folder.fill").font(.system(size: 11)).foregroundStyle(Color(red: 0.55, green: 0.7, blue: 1))
                Text(model.folderName).lineLimit(1).truncationMode(.middle).frame(maxWidth: 170, alignment: .leading)
                Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).opacity(0.6)
            }
            .font(.system(size: 12.5, weight: .medium))
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(Capsule().fill(Color.white.opacity(0.1)))
            .contentShape(Capsule())
        }
        .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        .help(model.folder)
    }
}

private struct Chip: View {
    let icon: String, text: String, tint: Color
    var body: some View {
        HStack(spacing: 5) { Image(systemName: icon).font(.system(size: 10, weight: .semibold)); Text(text) }
            .font(.system(size: 11.5, weight: .medium)).foregroundStyle(tint.opacity(0.95))
            .padding(.horizontal, 9).padding(.vertical, 5)
            .background(Capsule().fill(tint.opacity(0.14)))
    }
}

private struct StartButton: View {
    @ObservedObject var model: VoiceModel
    var body: some View {
        let ready = model.blocker == nil
        Button { model.onStart() } label: {
            HStack(spacing: 7) {
                ZStack {
                    if let c = model.countdown {
                        Circle().stroke(Color.white.opacity(0.25), lineWidth: 2)
                        Circle().trim(from: 0, to: max(0, min(1, c / model.countdownTotal))).stroke(Color.white, style: StrokeStyle(lineWidth: 2, lineCap: .round)).rotationEffect(.degrees(-90))
                        Text("\(Int(c.rounded(.up)))").font(.system(size: 9, weight: .bold)).monospacedDigit()
                    } else {
                        Image(systemName: "return").font(.system(size: 10, weight: .bold))
                    }
                }
                .frame(width: 17, height: 17)
                Text(model.trial ? "Try" : model.replying == nil ? "Start" : "Send").font(.system(size: 13, weight: .semibold))
            }
            .padding(.leading, 9).padding(.trailing, 14).padding(.vertical, 6)
            .background(Capsule().fill(LinearGradient(colors: [VoiceView.palette[2], VoiceView.palette[0]], startPoint: .leading, endPoint: .trailing)))
            .foregroundStyle(.white)
            .opacity(ready ? 1 : 0.45)
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .keyboardShortcut(.defaultAction)
        .help(model.blocker ?? "Start now (Return)")
    }
}

struct PillButton: ButtonStyle {
    var round = false, prominent = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12.5, weight: .medium))
            .padding(.horizontal, round ? 0 : 12).padding(.vertical, round ? 0 : 6)
            .frame(width: round ? 26 : nil, height: round ? 26 : nil)
            .background(Capsule().fill(prominent ? AnyShapeStyle(LinearGradient(colors: [VoiceView.palette[2], VoiceView.palette[0]], startPoint: .leading, endPoint: .trailing)) : AnyShapeStyle(Color.white.opacity(configuration.isPressed ? 0.22 : 0.12))))
            .foregroundStyle(.white)
            .contentShape(Capsule())
    }
}

/// Liquid Glass on macOS 26 and later, the HUD material before it; darkened a touch so
/// white words read over any wallpaper.
private struct Glass: ViewModifier {
    let radius: CGFloat
    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        if #available(macOS 26.0, *) {
            content.background(shape.fill(Color.black.opacity(0.42))).glassEffect(.regular, in: shape)
        } else {
            content.background(VisualEffect().clipShape(shape)).background(shape.fill(Color.black.opacity(0.3)))
                .overlay(shape.strokeBorder(Color.white.opacity(0.1)))
        }
    }
}

private struct VisualEffect: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView(); v.material = .hudWindow; v.blendingMode = .behindWindow; v.state = .active; return v
    }
    func updateNSView(_ v: NSVisualEffectView, context: Context) {}
}

/// The card's rim: Apple Intelligence's colours turning while it listens, glowing with
/// the voice; still and faint once it's a card.
private struct GlowBorder: View {
    let radius: CGFloat, level: CGFloat, live: Bool
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        if live {
            TimelineView(.animation) { tl in
                let a = Angle.radians(tl.date.timeIntervalSinceReferenceDate * 1.6)
                let g = AngularGradient(colors: VoiceView.palette + [VoiceView.palette[0]], center: .center, angle: a)
                ZStack {
                    shape.strokeBorder(g, lineWidth: 6).blur(radius: 10).opacity(0.35 + 0.65 * Double(min(1, level * 1.5)))
                    shape.strokeBorder(g, lineWidth: 2.5).blur(radius: 2.5).opacity(0.7)
                    shape.strokeBorder(g, lineWidth: 1)
                }
            }
            .allowsHitTesting(false)
        } else {
            shape.strokeBorder(AngularGradient(colors: VoiceView.palette + [VoiceView.palette[0]], center: .center), lineWidth: 1).opacity(0.55)
                .allowsHitTesting(false)
        }
    }
}

/// Siri's orb: three blurred gradient blobs turning against each other, swelling with the voice.
struct Orb: View {
    let level: CGFloat, busy: Bool
    var body: some View {
        TimelineView(.animation) { tl in
            let t = tl.date.timeIntervalSinceReferenceDate
            let l = Double(level)
            GeometryReader { g in
                let s = min(g.size.width, g.size.height)
                ZStack {
                    Circle().fill(Color(red: 0.12, green: 0.08, blue: 0.22))
                    ForEach(0..<3, id: \.self) { i in
                        let d = Double(i)
                        Circle()
                            .fill(AngularGradient(colors: VoiceView.palette + [VoiceView.palette[0]], center: .center,
                                                  angle: .radians(t * (busy ? 2.4 : 0.9 + d * 0.45) * (i % 2 == 0 ? 1 : -1))))
                            .frame(width: s * (0.62 + 0.38 * l + 0.05 * sin(t * 2 + d)), height: s * (0.62 + 0.38 * l + 0.05 * sin(t * 2 + d)))
                            .offset(x: cos(t * 1.3 + d * 2.1) * s * 0.09 * (1 + l * 2), y: sin(t * 1.1 + d * 2.1) * s * 0.09 * (1 + l * 2))
                            .blur(radius: s * 0.12)
                            .opacity(0.9)
                            .blendMode(.screen)
                    }
                    Circle().fill(RadialGradient(colors: [.white.opacity(0.55), .clear], center: UnitPoint(x: 0.35, y: 0.3), startRadius: 0, endRadius: s * 0.4))
                }
                .frame(width: s, height: s)
                .clipShape(Circle())
                .overlay(Circle().strokeBorder(Color.white.opacity(0.22), lineWidth: 0.5))
                .scaleEffect(1 + 0.08 * l)
            }
        }
    }
}

/// The voice as bars, newest on the right.
private struct Waveform: View {
    let levels: [CGFloat], muted: Bool
    var body: some View {
        GeometryReader { g in
            HStack(alignment: .center, spacing: 2) {
                ForEach(levels.indices, id: \.self) { i in
                    Capsule()
                        .fill(LinearGradient(colors: [VoiceView.palette[2], VoiceView.palette[0], VoiceView.palette[3]], startPoint: .bottom, endPoint: .top))
                        .frame(width: 2, height: max(3, (muted ? 0.05 : levels[i]) * g.size.height))
                        .opacity(0.45 + 0.55 * Double(i) / Double(max(1, levels.count - 1)))
                }
            }
            .frame(width: g.size.width, height: g.size.height, alignment: .trailing)
            .animation(.easeOut(duration: 0.12), value: levels)
        }
    }
}
