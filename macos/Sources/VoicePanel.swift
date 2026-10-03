import AppKit
import SwiftUI
import QuartzCore

// Voice tasks: hold ⌃⌥Space and speak, let go, and what was said becomes a task.
// A card of the office's drops in under the notch, with a desk bot that listens, a
// pixel meter of the voice and the words as they're heard. Let go and it opens into
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
    private var keptChoice: (target: VoiceTarget, folder: String, why: String?)?
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
            // Pressed again over the card: keep talking, added to what's there, to the
            // agent and folder the card has now (picked by hand, perhaps).
            keptChoice = (model.target, model.folder, model.why)
            panel.releaseKeyboard()
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
        let keep = keptChoice; keptChoice = nil
        model.target = picked ?? keep?.target ?? .new(model.tools.contains { $0.id == defaultTool } ? defaultTool : (model.tools.first?.id ?? "kiro"))
        // A reply goes to its session's folder; the card's own when it is talked to again;
        // a new task to the one it names, else the last one.
        if let s = model.replying, let f = sessionFolder(s.id) { model.folder = f; model.why = nil }
        else if let keep, picked == nil { model.folder = keep.folder; model.why = keep.why }
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
        // Only while the card still says the task went: other toasts are the office's.
        guard let at = awaitingBackend, Date().timeIntervalSince(at) < 3, case .started = model.stage else { return false }
        awaitingBackend = nil
        fail(text, nil)
        return true
    }

    func cancel() {
        generation += 1
        keptChoice = nil
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
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

final class VoicePanel {
    let window: VoiceWindow
    private let host: VoiceHostingView
    private let model: VoiceModel
    private(set) var hovered = false
    var level: NSWindow.Level { window.level }
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
        // Over the card keeps it up (read from the pointer poll: the window ignores the
        // mouse off the card, so a tracking area there never heard it leave).
        hovered = inside
        if window.ignoresMouseEvents == inside { window.ignoresMouseEvents = !inside }
    }

    /// Shows the panel (or keeps it), centred on the screen a little under the notch.
    func present(on screen: NSScreen?) {
        guard let screen = screen ?? NSScreen.main else { return }
        let f = screen.frame
        let height: CGFloat = 360
        // Under the menu bar, and under the notch when the menu bar hides itself.
        let top = f.maxY - max(f.maxY - screen.visibleFrame.maxY, screen.safeAreaInsets.top) - 10
        window.setFrame(NSRect(x: f.midX - Self.width / 2, y: top - height, width: Self.width, height: height), display: true)
        // Shown again while it faded out: the fade's end mustn't hide it after all.
        shown += 1
        window.alphaValue = 1
        if !window.isVisible { window.orderFrontRegardless() }
    }
    private var shown = 0

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
        let was = shown
        NSAnimationContext.runAnimationGroup({ c in c.duration = 0.18; window.animator().alphaValue = 0 }) { [weak self] in
            guard let self, self.shown == was else { return }
            self.window.orderOut(nil); self.window.alphaValue = 1; done()
        }
    }
}

// MARK: The screen's edge glow

/// A soft lamp-light around the screen's edges while listening, warmer as the voice
/// gets louder: the office's desk lamps, one flat colour (no turning rainbow). Drawn
/// by Core Animation, so the app does no work per frame.
final class ScreenGlow {
    private var window: NSWindow?
    private let container = CALayer(), mask = CAShapeLayer(), inner = CAShapeLayer()
    var level: CGFloat = 0 {
        didSet {
            CATransaction.begin(); CATransaction.setAnimationDuration(0.12)
            container.opacity = Float(0.35 + 0.55 * min(1, level * 1.4))
            CATransaction.commit()
        }
    }

    func show(on screen: NSScreen) {
        let w = window ?? make()
        w.setFrame(screen.frame, display: false)
        layout(screen.frame.size)
        container.opacity = 0.35
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
        container.backgroundColor = NSColor(srgbRed: 1, green: 0.66, blue: 0.38, alpha: 1).cgColor
        mask.fillColor = nil; mask.strokeColor = NSColor.black.cgColor
        mask.shadowColor = NSColor.black.cgColor; mask.shadowOpacity = 1; mask.shadowOffset = .zero; mask.shadowRadius = 16
        inner.fillColor = nil; inner.strokeColor = NSColor.black.withAlphaComponent(0.35).cgColor
        inner.shadowColor = NSColor.black.cgColor; inner.shadowOpacity = 0.6; inner.shadowOffset = .zero; inner.shadowRadius = 40
        let m = CALayer(); m.addSublayer(mask); m.addSublayer(inner)
        container.mask = m
        window = w
        return w
    }

    private func layout(_ size: CGSize) {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        let r = CGRect(origin: .zero, size: size)
        container.frame = r
        container.mask?.frame = r
        let path = CGPath(roundedRect: r.insetBy(dx: -2, dy: -2), cornerWidth: 14, cornerHeight: 14, transform: nil)
        mask.frame = r; mask.path = path; mask.lineWidth = 6
        inner.frame = r; inner.path = path; inner.lineWidth = 28
        CATransaction.commit()
    }
}

// MARK: Views

private struct CardFrame: PreferenceKey {
    static var defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) { value = nextValue() }
}

/// The office's look, for the voice card: its warm plum panels, peach rim, lamp amber,
/// purple and pixel lettering (docs/ART_DIRECTION.md: flat colours, no auroras).
enum VoiceStyle {
    static func rgb(_ hex: UInt32, _ a: Double = 1) -> Color { Color(.sRGB, red: Double(hex >> 16 & 255) / 255, green: Double(hex >> 8 & 255) / 255, blue: Double(hex & 255) / 255, opacity: a) }
    static let panelTop = rgb(0x1E1524), panelBottom = rgb(0x140E19)
    static let rim = rgb(0xFFBE96, 0.20), ink = rgb(0xF6F2FF), dim = rgb(0xF6F2FF, 0.62), faint = rgb(0xF6F2FF, 0.40)
    static let purple = rgb(0x9046FF), purpleDeep = rgb(0x5F25C2), lilac = rgb(0xC4A2FF)
    static let lamp = rgb(0xFFA860), amber = rgb(0xFFC46B), gold = rgb(0xFFD27A)
    static let ok = rgb(0x5DE37A), bad = rgb(0xFF7B72)
    static let visor = rgb(0x121018), eye = rgb(0xAAF6FF), antenna = rgb(0xFFD24A)
    static let well = rgb(0x0B0810, 0.65)
    /// The desk bots' colours, in the office's order (Pip, Juno, Moss, Nova, Ada, Rue).
    static let bots: [String: UInt32] = ["Pip": 0x9B6BFF, "Juno": 0x2FC9B0, "Moss": 0xFF9A4A, "Nova": 0xFF6FAE, "Ada": 0x5AA8FF, "Rue": 0xB4E04A]
    static func bot(_ name: String?) -> Color { rgb(bots[name ?? ""] ?? 0x9B6BFF) }

    /// The office's pixel font (Pixelify Sans, OFL), from the app's resources; the
    /// system's rounded face if it isn't there.
    static func pixel(_ size: CGFloat, _ weight: Font.Weight = .semibold) -> Font {
        registered ? .custom("Pixelify Sans", size: size).weight(weight) : .system(size: size, weight: weight, design: .rounded)
    }
    static let registered: Bool = {
        guard let url = Bundle.main.url(forResource: "PixelifySans", withExtension: "ttf") else { return false }
        var error: Unmanaged<CFError>?
        return CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) || NSFont(name: "Pixelify Sans", size: 12) != nil
    }()
}

struct VoiceView: View {
    @ObservedObject var model: VoiceModel
    var onCard: (CGRect) -> Void

    private var wide: Bool { model.stage == .compose || model.stage == .starting }
    private var live: Bool { switch model.stage { case .listening, .finishing, .preparing: return true; default: return false } }
    private var radius: CGFloat { wide ? 18 : 16 }

    var body: some View {
        VStack(spacing: 0) {
            if model.stage != .idle {
                card
                    .frame(width: wide ? 620 : 480)
                    .modifier(Panel(radius: radius, level: model.level, live: live))
                    .background(GeometryReader { g in Color.clear.preference(key: CardFrame.self, value: g.frame(in: .global)) })
                    .transition(.asymmetric(insertion: .scale(scale: 0.9, anchor: .top).combined(with: .opacity), removal: .opacity))
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onPreferenceChange(CardFrame.self) { onCard($0) }
        .environment(\.colorScheme, .dark)
    }

    /// The bot that will do it: the one replied to, else the office's first.
    private var bot: String? { model.replying?.bot }

    @ViewBuilder private var card: some View {
        switch model.stage {
        case .idle: EmptyView()
        case .preparing(let what):
            HStack(spacing: 14) {
                BotHead(color: VoiceStyle.bot(bot), level: 0.15, mood: .busy).frame(width: 40, height: 40)
                VStack(alignment: .leading, spacing: 3) {
                    Text(what).font(VoiceStyle.pixel(15)).foregroundStyle(VoiceStyle.ink)
                    Text("Once only. It stays on this Mac, and so does what you say.").font(.system(size: 11.5)).foregroundStyle(VoiceStyle.faint)
                }
                Spacer(minLength: 8)
                ProgressView().controlSize(.small).tint(VoiceStyle.amber)
            }
            .padding(.horizontal, 14).padding(.vertical, 13)
        case .listening, .finishing: listening
        case .compose, .starting: ComposeCard(model: model)
        case .started(let line):
            HStack(spacing: 12) {
                BotHead(color: VoiceStyle.bot(bot), level: 0, mood: model.trial ? .idle : .happy).frame(width: 34, height: 34)
                Text(line).font(VoiceStyle.pixel(14.5)).foregroundStyle(VoiceStyle.ink).lineLimit(2)
                Spacer(minLength: 8)
                if !model.trial { Button("Open Office") { model.onOpenOffice() }.buttonStyle(PillButton()) }
            }
            .padding(.horizontal, 16).padding(.vertical, 12)
        case .error(let message, let url):
            HStack(spacing: 12) {
                BotHead(color: VoiceStyle.bot(bot), level: 0, mood: .sad).frame(width: 34, height: 34)
                Text(message).font(.system(size: 13.5, weight: .medium)).foregroundStyle(VoiceStyle.ink).lineLimit(3).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 8)
                if url != nil { Button("Open Settings") { model.onOpenSettings(url) }.buttonStyle(PillButton()) }
                Button { model.onCancel() } label: { Image(systemName: "xmark").font(.system(size: 11, weight: .bold)) }
                    .buttonStyle(PillButton(round: true)).help("Dismiss")
            }
            .padding(.horizontal, 16).padding(.vertical, 12)
        }
    }

    private var listening: some View {
        let finishing = model.stage == .finishing
        let words = VoiceController.join(model.heard, model.guess)
        return HStack(spacing: 14) {
            BotHead(color: VoiceStyle.bot(bot), level: finishing ? 0 : model.level, mood: finishing ? .busy : .listening).frame(width: 42, height: 42)
            VStack(alignment: .leading, spacing: 4) {
                Group {
                    if words.isEmpty {
                        Text(finishing ? "One moment…" : model.handsFree ? "Listening hands-free…" : "Listening…").font(VoiceStyle.pixel(15.5)).foregroundStyle(VoiceStyle.dim)
                    } else {
                        (Text(model.heard) + Text(model.heard.isEmpty || model.guess.isEmpty ? "" : " ") + Text(model.guess).foregroundColor(VoiceStyle.faint))
                            .font(.system(size: 15, weight: .medium)).foregroundStyle(VoiceStyle.ink)
                    }
                }
                .lineLimit(2).truncationMode(.head)
                .frame(maxWidth: .infinity, alignment: .leading)
                .animation(.easeOut(duration: 0.15), value: words)
                Text(finishing ? "Writing it down" : model.handsFree ? "⌃⌥Space to finish · Esc to cancel" : "Let go to finish · tap for hands-free · Esc to cancel")
                    .font(.system(size: 11)).foregroundStyle(VoiceStyle.faint)
            }
            PixelMeter(levels: Array(model.levels.suffix(16)), muted: finishing).frame(width: 80, height: 28)
            if model.handsFree && !finishing {
                Button { model.onFinish() } label: { Image(systemName: "checkmark").font(.system(size: 12, weight: .heavy)) }
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
                BotHead(color: VoiceStyle.bot(model.replying?.bot), level: 0, mood: .idle).frame(width: 24, height: 24)
                Text(model.replying.map { "Reply to \($0.bot.isEmpty ? $0.title : $0.bot)" } ?? "New task")
                    .font(VoiceStyle.pixel(15)).foregroundStyle(VoiceStyle.ink)
                if model.trial {
                    Text("Try it").font(VoiceStyle.pixel(11)).foregroundStyle(VoiceStyle.gold)
                        .padding(.horizontal, 7).padding(.vertical, 2)
                        .background(RoundedRectangle(cornerRadius: 5, style: .continuous).fill(VoiceStyle.gold.opacity(0.14)))
                }
                Spacer()
                Group {
                    if let c = model.countdown { Text("Starting in \(Int(c.rounded(.up)))s · edit to wait") }
                    else if model.trial { Text("Nothing will start") }
                    else { Text("Return to \(model.replying == nil ? "start" : "send") · Esc to cancel") }
                }
                .font(.system(size: 11.5)).foregroundStyle(VoiceStyle.faint).monospacedDigit()
            }
            TextField("What should the agent do?", text: $model.text, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 16.5))
                .foregroundStyle(VoiceStyle.ink)
                .lineLimit(1...7)
                .focused($focused)
                .onSubmit { model.onStart() }
                .padding(.horizontal, 13).padding(.vertical, 11)
                .background(RoundedRectangle(cornerRadius: 11, style: .continuous).fill(VoiceStyle.well))
                // A well sunk into the panel: dark above, a lit lip below.
                .overlay(RoundedRectangle(cornerRadius: 11, style: .continuous).strokeBorder(focused ? VoiceStyle.lilac.opacity(0.55) : Color.white.opacity(0.07), lineWidth: focused ? 1.5 : 1))
                .overlay(alignment: .top) { RoundedRectangle(cornerRadius: 11, style: .continuous).fill(Color.black.opacity(0.35)).frame(height: 2).padding(.horizontal, 6).padding(.top, 1).allowsHitTesting(false) }
            HStack(spacing: 8) {
                AgentMenu(model: model)
                if model.replying == nil { FolderMenu(model: model) }
                if let a = accessLabel { Chip(icon: "shield.fill", text: a, tint: a == "Full access" ? VoiceStyle.amber : VoiceStyle.ok).help(accessHelp) }
                Spacer(minLength: 4)
                Button("Cancel") { model.onCancel() }.buttonStyle(PillButton()).keyboardShortcut(.cancelAction)
                StartButton(model: model)
            }
            if let p = model.problem {
                Label(p, systemImage: "exclamationmark.triangle.fill").font(.system(size: 12.5)).foregroundStyle(VoiceStyle.amber)
                    .fixedSize(horizontal: false, vertical: true)
            } else if let why = model.why {
                Label(why, systemImage: "folder.badge.questionmark").font(.system(size: 11.5)).foregroundStyle(VoiceStyle.faint)
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

/// A small control in the card's style: the office's chips, squarer than a capsule.
private struct Tile<Label: View>: View {
    @ViewBuilder var label: Label
    var body: some View {
        label
            .font(.system(size: 12.5, weight: .medium)).foregroundStyle(VoiceStyle.ink)
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(Bevel(fill: Color.white.opacity(0.08), radius: 8))
            .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
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
            Tile {
                HStack(spacing: 7) {
                    Image(nsImage: MenuBar.tileImage(model.tool)).resizable().frame(width: 16, height: 16)
                    Text(model.replying.map { $0.bot.isEmpty ? $0.title : $0.bot } ?? Marks.name(model.tool)).lineLimit(1)
                    Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).opacity(0.55)
                }
            }
        }
        // The plain style keeps the tile and its colours (the borderless one redraws the label as a template).
        .menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
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
            Tile {
                HStack(spacing: 6) {
                    Image(systemName: "folder.fill").font(.system(size: 11)).foregroundStyle(VoiceStyle.amber)
                    Text(model.folderName).lineLimit(1).truncationMode(.middle).frame(maxWidth: 170, alignment: .leading)
                    Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).opacity(0.55)
                }
            }
        }
        // The plain style keeps the tile and its colours (the borderless one redraws the label as a template).
        .menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
        .help(model.folder)
    }
}

private struct Chip: View {
    let icon: String, text: String, tint: Color
    var body: some View {
        HStack(spacing: 5) { Image(systemName: icon).font(.system(size: 9.5, weight: .semibold)); Text(text) }
            .font(.system(size: 11.5, weight: .medium)).foregroundStyle(tint)
            .padding(.horizontal, 8).padding(.vertical, 4)
            .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(tint.opacity(0.1)))
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
                Text(model.trial ? "Try" : model.replying == nil ? "Start" : "Send").font(VoiceStyle.pixel(13.5))
            }
            .padding(.leading, 9).padding(.trailing, 14).padding(.vertical, 6)
            .background(Bevel(fill: VoiceStyle.purple, radius: 9, lip: VoiceStyle.purpleDeep))
            .foregroundStyle(.white)
            .opacity(ready ? 1 : 0.45)
            .contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
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
            .background(Bevel(fill: prominent ? VoiceStyle.purple : Color.white.opacity(configuration.isPressed ? 0.18 : 0.09), radius: round ? 13 : 8, lip: prominent ? VoiceStyle.purpleDeep : nil))
            .offset(y: configuration.isPressed ? 1 : 0)
            .foregroundStyle(VoiceStyle.ink)
            .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

/// A chunky toy bevel, as the office's bot heads have: lit along the top, a darker lip
/// along the bottom.
private struct Bevel: View {
    let fill: Color, radius: CGFloat
    var lip: Color? = nil
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        ZStack {
            shape.fill(lip ?? Color.black.opacity(0.35)).offset(y: 2)
            shape.fill(fill)
            shape.strokeBorder(LinearGradient(colors: [Color.white.opacity(0.22), .clear], startPoint: .top, endPoint: .center), lineWidth: 1)
        }
    }
}

/// The card: an opaque panel of the office's (warm plum, a peach rim, a lit top edge and
/// a solid shadow), not glass. While it listens, the rim warms with the voice like a desk
/// lamp coming up; no colours turn.
private struct Panel: ViewModifier {
    let radius: CGFloat, level: CGFloat, live: Bool
    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        let glow = live ? Double(min(1, level * 1.6)) : 0
        content
            .background(shape.fill(LinearGradient(colors: [VoiceStyle.panelTop, VoiceStyle.panelBottom], startPoint: .top, endPoint: .bottom)))
            .overlay(shape.strokeBorder(live ? VoiceStyle.lamp.opacity(0.35 + 0.5 * glow) : VoiceStyle.rim, lineWidth: 1.5).animation(.easeOut(duration: 0.12), value: glow))
            .overlay(alignment: .top) {
                // The lit top edge.
                shape.strokeBorder(LinearGradient(colors: [Color.white.opacity(0.10), .clear], startPoint: .top, endPoint: UnitPoint(x: 0.5, y: 0.25)), lineWidth: 1).padding(1.5).allowsHitTesting(false)
            }
            .background(shape.fill(Color.black.opacity(0.55)).offset(y: 5))
            .shadow(color: VoiceStyle.lamp.opacity(0.28 * glow), radius: 14)
            .shadow(color: .black.opacity(0.45), radius: 18, y: 10)
    }
}

/// One of the office's desk bots, head only: a boxy toy head in its colour with a dark
/// visor, two cyan eyes and a yellow antenna. It listens (eyes up, the antenna's bulb
/// lit by the voice, a little bob), thinks (eyes side to side), is pleased or sorry, and
/// blinks now and then. It moves only while it has something to show.
struct BotHead: View {
    enum Mood { case idle, listening, busy, happy, sad }
    let color: Color, level: CGFloat, mood: Mood
    var body: some View {
        if mood == .listening || mood == .busy {
            TimelineView(.animation(minimumInterval: 1 / 30)) { tl in face(t: tl.date.timeIntervalSinceReferenceDate) }
        } else {
            TimelineView(.periodic(from: .now, by: 0.15)) { tl in face(t: tl.date.timeIntervalSinceReferenceDate) }
        }
    }

    private func face(t: Double) -> some View {
        GeometryReader { g in
            let s = min(g.size.width, g.size.height)
            let l = Double(level)
            let bob = mood == .listening ? -s * 0.04 * CGFloat(min(1, l * 1.8)) : 0
            // A blink every few seconds, for a tenth of one.
            let blink = t.truncatingRemainder(dividingBy: 3.7) < 0.12 && mood != .busy
            let look: CGFloat = mood == .busy ? CGFloat(sin(t * 5)) * s * 0.07 : 0
            ZStack {
                // Antenna: stalk and bulb, the bulb brighter as the voice is louder.
                let bulb = mood == .listening ? 0.45 + 0.55 * min(1, l * 1.8) : mood == .busy ? 0.5 + 0.5 * abs(sin(t * 4)) : 0.6
                RoundedRectangle(cornerRadius: s * 0.02).fill(color.opacity(0.75)).frame(width: s * 0.06, height: s * 0.16).offset(y: -s * 0.42)
                Circle().fill(VoiceStyle.antenna.opacity(bulb)).frame(width: s * 0.15, height: s * 0.15).offset(y: -s * 0.5)
                    .shadow(color: VoiceStyle.antenna.opacity(bulb * 0.9), radius: s * 0.08)
                // Head with the toy bevel.
                ZStack {
                    RoundedRectangle(cornerRadius: s * 0.2, style: .continuous).fill(color.opacity(0.7)).offset(y: s * 0.05)
                    RoundedRectangle(cornerRadius: s * 0.2, style: .continuous).fill(color)
                    RoundedRectangle(cornerRadius: s * 0.2, style: .continuous).strokeBorder(LinearGradient(colors: [Color.white.opacity(0.35), .clear], startPoint: .top, endPoint: .center), lineWidth: max(1, s * 0.05))
                    // Visor and eyes.
                    RoundedRectangle(cornerRadius: s * 0.1, style: .continuous).fill(VoiceStyle.visor).frame(width: s * 0.72, height: s * 0.4).offset(y: s * 0.02)
                    HStack(spacing: s * 0.16) {
                        ForEach(0..<2, id: \.self) { _ in eye(s: s, blink: blink) }
                    }
                    .offset(x: look, y: s * 0.02 + (mood == .listening ? -s * 0.02 : 0))
                }
                .frame(width: s * 0.86, height: s * 0.72)
                .offset(y: s * 0.1)
            }
            .frame(width: s, height: s)
            .offset(y: bob)
        }
    }

    @ViewBuilder private func eye(s: CGFloat, blink: Bool) -> some View {
        switch mood {
        case .happy:
            // ^ ^
            Path { p in p.move(to: CGPoint(x: 0, y: s * 0.07)); p.addLine(to: CGPoint(x: s * 0.06, y: 0)); p.addLine(to: CGPoint(x: s * 0.12, y: s * 0.07)) }
                .stroke(VoiceStyle.eye, style: StrokeStyle(lineWidth: max(1.5, s * 0.05), lineCap: .round, lineJoin: .round)).frame(width: s * 0.12, height: s * 0.08)
        case .sad:
            RoundedRectangle(cornerRadius: s * 0.02).fill(VoiceStyle.eye.opacity(0.8)).frame(width: s * 0.11, height: max(1.5, s * 0.04)).rotationEffect(.degrees(-12))
        default:
            RoundedRectangle(cornerRadius: s * 0.03, style: .continuous).fill(VoiceStyle.eye)
                .frame(width: s * 0.1, height: blink ? max(1.5, s * 0.025) : s * 0.15)
                .shadow(color: VoiceStyle.eye.opacity(0.6), radius: s * 0.04)
        }
    }
}

/// The voice as a pixel meter: columns of little square blocks, newest on the right,
/// lit lamp-amber from the bottom, lilac at the top.
private struct PixelMeter: View {
    let levels: [CGFloat], muted: Bool
    private let rows = 6
    var body: some View {
        GeometryReader { g in
            let cols = max(1, levels.count)
            let cell = min(g.size.height / CGFloat(rows), g.size.width / CGFloat(cols))
            let gap = max(1, cell * 0.22)
            Canvas { ctx, _ in
                for (i, v) in levels.enumerated() {
                    let lit = muted ? 1 : max(1, Int((min(1, v * 1.15) * CGFloat(rows)).rounded()))
                    let x = g.size.width - CGFloat(cols - i) * cell
                    for r in 0..<rows {
                        let rect = CGRect(x: x + gap / 2, y: g.size.height - CGFloat(r + 1) * cell + gap / 2, width: cell - gap, height: cell - gap)
                        let on = r < lit
                        let color = !on ? Color.white.opacity(0.06) : r >= rows - 2 ? VoiceStyle.lilac : VoiceStyle.amber
                        ctx.fill(Path(roundedRect: rect, cornerRadius: 1), with: .color(color.opacity(on ? 0.55 + 0.45 * Double(i) / Double(max(1, cols - 1)) : 1)))
                    }
                }
            }
        }
    }
}
