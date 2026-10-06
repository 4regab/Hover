import AppKit
import SwiftUI
import ServiceManagement
import UserNotifications
import AVFoundation
import Speech

// Settings, as a Mac window: a sidebar of Get Started, General, Usage, Computer Use
// and one page per agent. Every change applies at once (no Save). The backend is the source of
// truth; replies that were asked for before the latest change are not shown, so a
// slow echo can't undo a click (the old window's feedback-loop bug).

struct ToolPrefs: Equatable { var access = "full"; var idle = 5; var hideSteps = false }

/// Cua Spaces, the agents' own desktops, as the backend's spaces message reports them.
struct SpacesStatus: Equatable {
    var on = false, supported = true, checked = false, installed = false, ready = false, busy = false
    var image = "macos", version = "", hint = "", line = "", running = 0
    var step: String?, error: String?, fraction: Double?
    init() {}
    init(_ m: [String: Any]) {
        on = m["on"] as? Bool ?? false; supported = m["supported"] as? Bool ?? true; checked = m["checked"] as? Bool ?? false
        installed = m["installed"] as? Bool ?? false; ready = m["ready"] as? Bool ?? false; busy = m["busy"] as? Bool ?? false
        image = m["image"] as? String ?? "macos"; version = m["version"] as? String ?? ""; hint = m["hint"] as? String ?? ""
        line = m["line"] as? String ?? ""; running = m["running"] as? Int ?? 0
        step = m["step"] as? String; error = m["error"] as? String; fraction = (m["fraction"] as? NSNumber)?.doubleValue
    }

    /// The one line under "Cua Spaces": progress, the last error, Ready, or what is missing.
    func statusLine(unsupported: String) -> String {
        if busy { return line }
        if let error { return error }
        if ready { return "Ready" + (version.isEmpty ? "" : " · \(version)") + (running > 0 ? " · \(running) running" : "") }
        if !supported && hint.isEmpty { return unsupported }
        return hint
    }
}

/// Cua Driver, as the backend's computerUse message reports it.
struct CuaStatus: Equatable {
    var checked = false, installed = false, ready = false, busy = false, canGrant = true
    var version = "", permissions = "unknown", hint = "", installHint = "", line = ""
    var step: String?, error: String?

    init() {}
    init(_ m: [String: Any]) {
        checked = m["checked"] as? Bool ?? false; installed = m["installed"] as? Bool ?? false
        ready = m["ready"] as? Bool ?? false; busy = m["busy"] as? Bool ?? false; canGrant = m["canGrant"] as? Bool ?? true
        version = m["version"] as? String ?? ""; permissions = m["permissions"] as? String ?? "unknown"
        hint = m["hint"] as? String ?? ""; installHint = m["installHint"] as? String ?? ""; line = m["line"] as? String ?? ""
        step = m["step"] as? String; error = m["error"] as? String
    }

    /// What the one button does now, if anything: install, grant or cancel.
    var action: (title: String, step: String)? {
        if busy || step != nil { return ("Cancel", "cancel") }
        if !checked { return nil }
        if !installed { return ("Install", "install") }
        if canGrant && permissions != "granted" { return (permissions == "partial" ? "Grant Screen Recording" : "Grant Access", "grant") }
        return nil
    }
    var summary: String {
        if busy || step != nil { return line.isEmpty ? (step == "granting" ? "Waiting for you to approve CuaDriver…" : "Installing…") : line }
        if let error, !error.isEmpty { return error }
        if !checked { return "Checking…" }
        if !installed { return "Not installed. Hover installs it with Cua's own installer." }
        if permissions == "granted" { return "Ready" + (version.isEmpty ? "" : " · " + version) }
        return hint
    }
}

/// One tool as the office's state reports it, with what one-click setup can do.
struct ToolStatus: Equatable {
    var id: String, name: String
    var checked = false, installed = false, signedIn = false, ready = false
    var hint = "", readOnly = true, canSetup = false
    var step: String?, line = "", error: String?, busy = false, needs: [String] = []

    init(id: String, name: String) { self.id = id; self.name = name }
    init(_ m: [String: Any]) {
        id = m["id"] as? String ?? ""; name = m["name"] as? String ?? id
        checked = m["checkedYet"] as? Bool ?? true
        installed = m["installed"] as? Bool ?? false; signedIn = m["signedIn"] as? Bool ?? false
        ready = m["ready"] as? Bool ?? false; hint = m["hint"] as? String ?? ""
        readOnly = m["readOnly"] as? Bool ?? true; canSetup = m["canSetup"] as? Bool ?? false
        let s = m["setup"] as? [String: Any] ?? [:]
        step = s["step"] as? String; line = s["line"] as? String ?? ""; error = s["error"] as? String
        busy = s["busy"] as? Bool ?? false; needs = s["needs"] as? [String] ?? []
    }

    enum Phase: Equatable { case checking, ready, installing, signingIn, failed, needsInstall, needsSignIn }
    var phase: Phase {
        if busy || step != nil { return step == "signing-in" ? .signingIn : .installing }
        if let error, !error.isEmpty { return .failed }
        if ready { return .ready }
        if !checked { return .checking }
        return installed ? .needsSignIn : .needsInstall
    }
    var action: String? {
        switch phase {
        case .ready, .checking: return nil
        case .installing, .signingIn: return "Cancel"
        case .failed: return "Try Again"
        case .needsInstall: return "Set Up"
        case .needsSignIn: return "Sign In"
        }
    }
    var summary: String {
        switch phase {
        case .checking: return "Checking…"
        case .ready: return "Ready"
        case .installing: return line.isEmpty ? "Installing…" : line
        case .signingIn: return line.isEmpty ? "Waiting for you to sign in…" : line
        case .failed: return error ?? "Setup stopped."
        case .needsInstall:
            return needs.isEmpty ? (hint.isEmpty ? "Not installed" : hint) : "Installs " + needs.map { $0.replacingOccurrences(of: "Installing ", with: "") }.joined(separator: ", then ")
        case .needsSignIn: return hint.isEmpty ? "Sign in to start tasks." : hint
        }
    }
}

final class SettingsModel: ObservableObject {
    enum Page: Hashable { case start, general, usage, computerUse, voice, tool(String) }
    @Published var page: Page = .start
    @Published var hover = true
    @Published var noticeSeen = false
    @Published var computerUse = false
    @Published var sandbox = true
    @Published var agentBrowser = true
    @Published var discordPresence = false
    /// Kiro compacts a long conversation by itself once it fills this share of its context window.
    @Published var kiroAutoCompact = false
    @Published var kiroCompactAt = 80
    @Published var spaces = SpacesStatus()
    @Published var cua = CuaStatus()
    @Published var maxRunning = 3
    @Published var quotaItems: [String] = []
    @Published var prefs: [String: ToolPrefs] = [:]
    @Published var tools: [ToolStatus] = SettingsModel.defaultTools
    @Published var quotas: [String: QuotaValue] = [:]
    @Published var login = false
    @Published var notifications = false
    @Published var voiceEnabled = VoiceSettings.enabled
    @Published var voiceCountdown = VoiceSettings.countdown
    @Published var voiceAgent = VoiceSettings.agent
    @Published var voiceGlow = VoiceSettings.screenGlow
    @Published var voiceSounds = VoiceSettings.sounds
    @Published var projects = VoiceSettings.projects
    @Published var micStatus = Permissions.microphone
    @Published var speechStatus = Permissions.speech
    var tryVoice: () -> Void = {}
    var send: ([String: Any]) -> Void = { _ in }
    var openOffice: () -> Void = {}
    var refreshUsage: () -> Void = {}
    /// Requests whose preferences reply hasn't come back yet.
    private(set) var outstanding = 0
    private(set) var hasPreferences = false

    static let order = ["codex", "kiro", "cursor", "opencode", "claude"]
    static let defaultTools = order.map { ToolStatus(id: $0, name: Marks.name($0)) }

    init() {
        login = !smoke && SMAppService.mainApp.status == .enabled
        notifications = UserDefaults.standard.bool(forKey: "notifications")
    }

    /// getSettings or saveSettings: each is answered by one preferences message.
    func request(_ m: [String: Any]) { outstanding += 1; send(m) }

    /// Takes a preferences reply. Returns true when it was applied: only the reply to
    /// the newest request is, as it reflects every change sent before it.
    @discardableResult func receive(_ m: [String: Any]) -> Bool {
        if outstanding > 0 { outstanding -= 1 }
        guard outstanding == 0 else { return false }
        hasPreferences = true
        hover = m["hover"] as? Bool ?? true
        noticeSeen = m["noticeSeen"] as? Bool ?? false
        maxRunning = m["maxRunning"] as? Int ?? 3
        quotaItems = m["quotaItems"] as? [String] ?? []
        computerUse = m["computerUse"] as? Bool ?? false
        sandbox = m["sandbox"] as? Bool ?? true
        agentBrowser = m["agentBrowser"] as? Bool ?? true
        discordPresence = m["discordPresence"] as? Bool ?? false
        // The backend says in the preferences whether this Mac can host agent desktops, so the switch is right before the Spaces message comes.
        if let ok = m["spacesSupported"] as? Bool { spaces.supported = ok }
        kiroAutoCompact = m["kiroAutoCompact"] as? Bool ?? false
        kiroCompactAt = m["kiroCompactAt"] as? Int ?? 80
        var p: [String: ToolPrefs] = [:]
        for t in m["tools"] as? [[String: Any]] ?? [] {
            guard let id = t["id"] as? String else { continue }
            p[id] = ToolPrefs(access: t["access"] as? String ?? "full", idle: t["idle"] as? Int ?? 5, hideSteps: t["hideSteps"] as? Bool ?? false)
        }
        prefs = p
        return true
    }

    func receiveState(_ m: [String: Any]) {
        let list = (m["tools"] as? [[String: Any]] ?? []).map(ToolStatus.init)
        guard !list.isEmpty else { return }
        let next = list.sorted { (Self.order.firstIndex(of: $0.id) ?? 99) < (Self.order.firstIndex(of: $1.id) ?? 99) }
        if next != tools { tools = next }
    }

    func status(_ id: String) -> ToolStatus { tools.first { $0.id == id } ?? ToolStatus(id: id, name: Marks.name(id)) }
    func pref(_ id: String) -> ToolPrefs { prefs[id] ?? ToolPrefs() }

    // MARK: Changes, each sent at once

    func setHover(_ on: Bool) { hover = on; request(["type": "saveSettings", "hover": on]) }
    func setNotice(_ on: Bool) { noticeSeen = on; if on { request(["type": "saveSettings", "noticeSeen": true]) } }
    func setMaxRunning(_ n: Int) { maxRunning = n; request(["type": "saveSettings", "maxRunning": n]) }
    func setComputerUse(_ on: Bool) { computerUse = on; request(["type": "saveSettings", "computerUse": on]) }
    func setAgentBrowser(_ on: Bool) { agentBrowser = on; request(["type": "saveSettings", "agentBrowser": on]) }
    func setDiscordPresence(_ on: Bool) { discordPresence = on; request(["type": "saveSettings", "discordPresence": on]) }
    func setSandbox(_ on: Bool) { sandbox = on; request(["type": "saveSettings", "sandbox": on]) }
    func setKiroAutoCompact(_ on: Bool) { kiroAutoCompact = on; request(["type": "saveSettings", "kiroAutoCompact": on]) }
    func setKiroCompactAt(_ n: Int) { kiroCompactAt = n; request(["type": "saveSettings", "kiroCompactAt": n]) }
    /// Asks the backend for Cua Driver's state (it answers with what it knows, then checks again).
    func checkComputerUse() { send(["type": "computerUse"]) }
    func cuaSetup(_ step: String) { send(["type": "computerUseSetup", "step": step]) }
    func receiveComputerUse(_ m: [String: Any]) { let next = CuaStatus(m); if next != cua { cua = next } }
    func receiveSpaces(_ m: [String: Any]) { let next = SpacesStatus(m); if next != spaces { spaces = next } }
    func setSpaces(_ on: Bool) { spaces.on = on; request(["type": "saveSettings", "agentSpaces": on]); send(["type": "spaces"]) }
    func setSpaceImage(_ image: String) { spaces.image = image; request(["type": "saveSettings", "spaceImage": image]); send(["type": "spaces"]) }
    func spacesSetup(_ step: String) { send(["type": "spaces", "step": step]) }
    func setQuota(_ id: String, _ on: Bool) {
        var items = quotaItems.filter { $0 != id }
        if on { items.append(id) }
        quotaItems = items
        request(["type": "saveSettings", "quotaItems": items])
    }
    func setPref(_ id: String, _ change: (inout ToolPrefs) -> Void) {
        var p = pref(id); change(&p); prefs[id] = p
        request(["type": "saveSettings", "tools": [["id": id, "access": p.access, "idle": p.idle, "hideSteps": p.hideSteps] as [String: Any]]])
    }
    func setLogin(_ on: Bool) {
        guard !smoke else { login = on; return }
        do { if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() } }
        catch { NSAlert(error: error).runModal() }
        login = SMAppService.mainApp.status == .enabled
    }
    func setNotifications(_ on: Bool) {
        notifications = on
        guard !smoke else { return }
        UserDefaults.standard.set(on, forKey: "notifications")
        if on { UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in } }
    }
    func setVoice(enabled: Bool? = nil, countdown: Int? = nil, agent: String? = nil, glow: Bool? = nil, sounds: Bool? = nil) {
        if let enabled { voiceEnabled = enabled; VoiceSettings.enabled = enabled }
        if let countdown { voiceCountdown = countdown; VoiceSettings.countdown = countdown }
        if let agent { voiceAgent = agent; VoiceSettings.agent = agent }
        if let glow { voiceGlow = glow; VoiceSettings.screenGlow = glow }
        if let sounds { voiceSounds = sounds; VoiceSettings.sounds = sounds }
    }
    func setProjects(_ list: [VoiceProject]) { projects = list; VoiceSettings.projects = list }
    func addProject() {
        let picker = NSOpenPanel(); picker.canChooseDirectories = true; picker.canChooseFiles = false; picker.allowsMultipleSelection = true
        picker.title = "Add a project voice tasks can run in"; picker.prompt = "Add"
        guard picker.runModal() == .OK else { return }
        var list = projects
        for url in picker.urls where !list.contains(where: { $0.path == url.path }) { list.append(VoiceProject(name: url.lastPathComponent, path: url.path)) }
        setProjects(list)
    }
    func refreshPermissions() { micStatus = Permissions.microphone; speechStatus = Permissions.speech }
    func askPermissions() {
        Task { @MainActor in
            try? await Permissions.ensure()
            refreshPermissions()
            if micStatus == .denied { NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")!) }
            else if speechStatus == .denied { NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition")!) }
        }
    }
    func setup(_ id: String) {
        let s = status(id)
        send(["type": "setup", "tool": id, "step": s.phase == .installing || s.phase == .signingIn ? "cancel" : "auto"])
    }
}

/// The Settings window; the SwiftUI view lives in it for the app's lifetime.
final class SettingsWindow: NSObject, NSWindowDelegate {
    let model = SettingsModel()
    let window: NSWindow
    override init() {
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 580), styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        super.init()
        window.title = "Hover Settings"
        window.titlebarAppearsTransparent = true
        window.toolbarStyle = .unified
        window.isReleasedWhenClosed = false
        window.minSize = NSSize(width: 700, height: 480)
        window.contentView = NSHostingView(rootView: SettingsView(model: model))
        window.delegate = self
        window.center()
        window.setFrameAutosaveName("HoverSettings")
    }
    func show(_ page: SettingsModel.Page? = nil) {
        if let page { model.page = page }
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }
}

// MARK: - Views

private struct Logo: View {
    let id: String
    var size: CGFloat = 20
    var body: some View {
        Image(nsImage: Self.image(id))
            .resizable().interpolation(.high)
            .frame(width: size, height: size)
            .accessibilityLabel(Marks.name(id))
    }
    /// Drawn on demand at whatever scale it lands on, so it is sharp on Retina.
    static func image(_ id: String) -> NSImage {
        NSImage(size: NSSize(width: 64, height: 64), flipped: false) { r in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            Marks.drawTile(id, in: r.insetBy(dx: 2, dy: 2), context: cg); return true
        }
    }
}

private struct StatusDot: View {
    let status: ToolStatus
    var body: some View {
        switch status.phase {
        case .ready: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .installing, .signingIn, .checking: ProgressView().controlSize(.small)
        case .failed: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        case .needsInstall: Image(systemName: "arrow.down.circle").foregroundStyle(.secondary)
        case .needsSignIn: Image(systemName: "person.crop.circle.badge.exclamationmark").foregroundStyle(.orange)
        }
    }
}

private struct SetupButton: View {
    @ObservedObject var model: SettingsModel
    let id: String
    var body: some View {
        let s = model.status(id)
        if let title = s.action, s.canSetup {
            Button(title) { model.setup(id) }
                .buttonStyle(.borderedProminent)
                .tint(title == "Cancel" ? .gray : .accentColor)
                .controlSize(.regular)
                .accessibilityLabel("\(title) \(s.name)")
        } else if s.phase == .ready {
            Label("Ready", systemImage: "checkmark").labelStyle(.titleAndIcon).foregroundStyle(.green).font(.callout.weight(.medium))
        }
    }
}

/// A tool's row in Get Started: logo, name, what it needs, and its one button.
private struct ToolCard: View {
    @ObservedObject var model: SettingsModel
    let id: String
    var body: some View {
        let s = model.status(id)
        HStack(spacing: 14) {
            Logo(id: id, size: 40)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) { Text(s.name).font(.headline); StatusDot(status: s) }
                Text(s.summary)
                    .font(.callout)
                    .foregroundStyle(s.phase == .failed ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                    .lineLimit(2).truncationMode(.tail)
                    .textSelection(.enabled)
            }
            Spacer(minLength: 12)
            SetupButton(model: model, id: id)
        }
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(.background.secondary))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(.separator.opacity(0.6)))
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { model.page = .tool(id) }
    }
}

struct SettingsView: View {
    @ObservedObject var model: SettingsModel
    var body: some View {
        NavigationSplitView {
            List(selection: Binding(get: { model.page }, set: { if let p = $0 { model.page = p } })) {
                Section {
                    Label("Get Started", systemImage: "sparkles").tag(SettingsModel.Page.start)
                    Label("General", systemImage: "gearshape").tag(SettingsModel.Page.general)
                    Label("Usage", systemImage: "gauge.with.dots.needle.33percent").tag(SettingsModel.Page.usage)
                    Label("Computer Use", systemImage: "cursorarrow.rays").tag(SettingsModel.Page.computerUse)
                    Label("Voice", systemImage: "waveform").tag(SettingsModel.Page.voice)
                }
                Section("Agents") {
                    ForEach(model.tools, id: \.id) { t in
                        HStack(spacing: 8) {
                            Logo(id: t.id, size: 18)
                            Text(t.name)
                            Spacer()
                            if t.phase != .ready { StatusDot(status: t).imageScale(.small).controlSize(.mini) }
                        }
                        .tag(SettingsModel.Page.tool(t.id))
                    }
                }
            }
            .navigationSplitViewColumnWidth(min: 190, ideal: 210, max: 260)
        } detail: {
            Group {
                switch model.page {
                case .start: StartPage(model: model)
                case .general: GeneralPage(model: model)
                case .usage: UsagePage(model: model)
                case .computerUse: ComputerUsePage(model: model)
                case .voice: VoicePage(model: model)
                case .tool(let id): ToolPage(model: model, id: id)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }
}

private struct StartPage: View {
    @ObservedObject var model: SettingsModel
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Set up your agents").font(.largeTitle.weight(.semibold))
                    Text("One click installs a tool with its maker's own installer, then opens its sign-in. Hover never sees your passwords: each tool keeps its own sign-in.")
                        .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                VStack(spacing: 10) {
                    ForEach(model.tools.filter { $0.id != "opencode" }, id: \.id) { ToolCard(model: model, id: $0.id) }
                }
                GroupBox {
                    VStack(alignment: .leading, spacing: 8) {
                        Toggle(isOn: Binding(get: { model.noticeSeen }, set: { model.setNotice($0) })) {
                            Text("I understand agents can edit files and run commands in the folder I pick").font(.body.weight(.medium))
                        }
                        .disabled(model.noticeSeen)
                        Text("With Full access (the default) they never ask. Pick a project under version control, or set an agent to Ask first on its page; the notch then shows what it wants to do before it does it.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }.padding(6)
                }
                HStack {
                    Spacer()
                    Button { model.openOffice() } label: { Label("Open the Office", systemImage: "rectangle.inset.filled.on.rectangle") }
                        .controlSize(.large)
                        .keyboardShortcut(.defaultAction)
                        .disabled(!model.noticeSeen)
                }
            }
            .padding(28)
            .frame(maxWidth: 680, alignment: .leading)
        }
    }
}

private struct GeneralPage: View {
    @ObservedObject var model: SettingsModel
    var body: some View {
        Form {
            Section {
                Toggle("Open the office when the pointer rests on the notch", isOn: Binding(get: { model.hover }, set: { model.setHover($0) }))
                LabeledContent("Keyboard shortcut") { Text("⌥ N").font(.body.monospaced()).foregroundStyle(.secondary) }
            } header: { Text("Notch") }
            Section {
                Picker("Tasks at once", selection: Binding(get: { model.maxRunning }, set: { model.setMaxRunning($0) })) {
                    ForEach(1...6, id: \.self) { Text("\($0)").tag($0) }
                }
                Toggle("Notify me when an agent finishes", isOn: Binding(get: { model.notifications }, set: { model.setNotifications($0) }))
                VStack(alignment: .leading, spacing: 3) {
                    Toggle("Run agents in a sandbox", isOn: Binding(get: { model.sandbox }, set: { model.setSandbox($0) }))
                    Text("Agents change only the folders they work in, can’t open windows or control your apps, and reach only their own service, package registries and GitHub. Computer use still works in the background.")
                        .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            } header: { Text("Agents") }
            Section {
                Toggle("Open Hover at login", isOn: Binding(get: { model.login }, set: { model.setLogin($0) }))
            } header: { Text("Startup") }
        }
        .formStyle(.grouped)
    }
}

private struct UsagePage: View {
    @ObservedObject var model: SettingsModel
    var body: some View {
        Form {
            Section {
                ForEach(MenuBar.quotaIds, id: \.self) { id in
                    let on = model.quotaItems.contains(id)
                    let q = model.quotas[id]
                    HStack(spacing: 12) {
                        Logo(id: id, size: 24)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Marks.name(id))
                            if on { Text(q?.detail ?? "Reading…").font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                        }
                        Spacer()
                        if on { Text(MenuBar.percent(q)).font(.body.monospacedDigit().weight(.medium)).foregroundStyle(q?.used.map { Color(Marks.quotaColor($0)) } ?? .secondary) }
                        Toggle("", isOn: Binding(get: { on }, set: { model.setQuota(id, $0) })).labelsHidden().toggleStyle(.switch)
                    }
                    .padding(.vertical, 2)
                }
            } header: {
                Text("Show in the menu bar")
            } footer: {
                Text("Each reader uses what that tool already keeps, read-only, every five minutes: Cursor's sign-in, Kiro's /usage, Codex's own logs. A reading that fails says why.")
                    .foregroundStyle(.secondary)
            }
            Section { Button("Read Again Now") { model.refreshUsage() } }
        }
        .formStyle(.grouped)
    }
}

/// Cua Driver for the agents: on or off, installed, and CuaDriver's grants.
private struct ComputerUsePage: View {
    /// hover_agents::spaces::UNSUPPORTED, for the moment before the backend's own line arrives.
    static let spacesUnsupported = "Agent desktops need macOS 26 or later on Apple silicon."
    @ObservedObject var model: SettingsModel
    var body: some View {
        let c = model.cua
        Form {
            Section {
                Toggle(isOn: Binding(get: { model.computerUse }, set: { model.setComputerUse($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Let agents see and use apps")
                        Text("Each agent gets Cua Driver's tools, so it can open the app it built, click through it and check what it shows.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .toggleStyle(.switch)
            } footer: {
                Text("It works in the background: your pointer doesn't move and the app you're in keeps the keyboard. Every action still follows each agent's tool access: Ask first asks in the notch before it clicks or types, and Read only turns them down. New tasks get it at once; a running agent from its next idle restart.")
                    .foregroundStyle(.secondary)
            }
            Section {
                let sp = model.spaces
                Toggle(isOn: Binding(get: { sp.on }, set: { model.setSpaces($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Give agents a desktop of their own")
                        Text("One Cua Space per project: a separate computer that the agents working in that folder share for computer use, each with its own cursor, instead of your screen. You watch it live in a desk’s Screen panel and can step in at any time. Drag an app or files onto the notch to send them to a project’s desktop.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .toggleStyle(.switch).disabled(!sp.supported)
                if sp.on || !sp.supported {
                    Picker("Desktop", selection: Binding(get: { sp.image }, set: { model.setSpaceImage($0) })) {
                        Text("macOS (two projects at a time, 8 GB of memory each)").tag("macos")
                        Text("Linux (needs Docker or Colima)").tag("linux")
                    }
                    HStack(spacing: 12) {
                        Image(systemName: "macwindow.on.rectangle").font(.system(size: 22)).foregroundStyle(.tint).frame(width: 32)
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Cua Spaces").font(.headline)
                            HStack(spacing: 5) {
                                if sp.busy || (!sp.checked && sp.supported) { ProgressView().controlSize(.mini) }
                                else if sp.ready { Image(systemName: "checkmark.circle.fill").foregroundStyle(.green) }
                                else { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange) }
                                Text(sp.statusLine(unsupported: Self.spacesUnsupported))
                                    .font(.callout).foregroundStyle(.secondary).lineLimit(2)
                            }
                            if sp.busy, let f = sp.fraction { ProgressView(value: f).frame(maxWidth: 260) }
                        }
                        Spacer()
                        if sp.busy { Button("Cancel") { model.spacesSetup("cancel") } }
                        else if sp.supported && !sp.ready { Button(sp.installed ? "Prepare" : "Set up") { model.spacesSetup("setup") }.buttonStyle(.borderedProminent) }
                    }
                }
            } footer: {
                Text("A desktop is not your own macOS: it is a separate macOS 26 virtual machine (Apple’s Virtualization, through Cua’s Lume), or a Linux container, with none of your apps, files or sign-ins until you send them. Spaces run on this Mac and are free; nothing goes through Cua’s servers. The first setup installs Cua’s command-line tool (not its app) and downloads the desktop image once (macOS is about 23 GB); each project’s desktop is then a quick copy of it. It is made when the project’s first task starts, turned off when none of its agents is left in the office, and deleted with the project’s last session. Needs macOS 26 or later on Apple silicon.")
                    .foregroundStyle(.secondary)
            }
            Section {
                Toggle(isOn: Binding(get: { model.agentBrowser }, set: { model.setAgentBrowser($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Give agents Hover's browser")
                        Text("Agents open pages, click, type and take screenshots in a browser of Hover's own, the one a desk's Browser panel shows, so you watch what they do.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .toggleStyle(.switch)
            } footer: {
                Text("It has no cookies or sign-ins of yours and opens web pages only. It runs outside the agents' sandbox, so it can reach any website; each step follows the agent's tool access like any other tool.")
                    .foregroundStyle(.secondary)
            }
            Section {
                Toggle(isOn: Binding(get: { model.discordPresence }, set: { model.setDiscordPresence($0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Show on Discord")
                        Text("Shows Hover on your Discord status, with how many agents are working and which ones.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .toggleStyle(.switch)
            } footer: {
                Text("Task names are never shared. The Discord app has to be open on this Mac, and “Share my activity” on in Discord's Activity Privacy.")
                    .foregroundStyle(.secondary)
            }
            Section {
                HStack(spacing: 12) {
                    Image(systemName: "cursorarrow.rays").font(.system(size: 22)).foregroundStyle(.tint).frame(width: 32)
                    VStack(alignment: .leading, spacing: 3) {
                        Text("Cua Driver").font(.headline)
                        HStack(spacing: 5) {
                            if c.busy || !c.checked { ProgressView().controlSize(.mini) }
                            else if c.ready && c.permissions == "granted" { Image(systemName: "checkmark.circle.fill").foregroundStyle(.green) }
                            else { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange) }
                            Text(c.summary).foregroundStyle(c.error?.isEmpty == false ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary))
                                .lineLimit(3).textSelection(.enabled)
                        }
                        .font(.callout)
                    }
                    Spacer()
                    if let a = c.action {
                        Button(a.title) { model.cuaSetup(a.step) }
                            .buttonStyle(.borderedProminent).tint(a.step == "cancel" ? .gray : .accentColor)
                    } else if c.checked {
                        Button("Check Again") { model.checkComputerUse() }
                    }
                }
                .padding(.vertical, 4)
                if c.installed && c.permissions != "granted" {
                    Button("Open Privacy & Security") {
                        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!)
                    }
                    .buttonStyle(.link)
                }
            } header: { Text("Driver") } footer: {
                Text("Cua Driver is open source (MIT) from github.com/trycua/cua. Accessibility and Screen Recording go to CuaDriver, not to Hover or the agents. Hover never passes Cua's approval-bypass flags.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onAppear { model.checkComputerUse(); model.send(["type": "spaces"]) }
    }
}

private struct ToolPage: View {
    @ObservedObject var model: SettingsModel
    let id: String
    static let access: [(String, String)] = [("full", "Full access"), ("risky", "Ask first"), ("always", "Ask always"), ("read", "Read only")]
    static func explain(_ access: String, _ id: String) -> String {
        switch access {
        case "risky": return "Asks in the notch before commands, deletes, moves, going online, and anything outside the folder. Edits inside the folder go ahead."
        case "always": return "Asks in the notch before every edit and command."
        case "read": return "Can read and search only; Hover refuses every change."
        default: return "Edits files and runs commands in the folder without asking."
        }
    }
    var body: some View {
        let s = model.status(id), p = model.pref(id)
        Form {
            Section {
                HStack(spacing: 14) {
                    Logo(id: id, size: 44)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(s.name).font(.title2.weight(.semibold))
                        HStack(spacing: 5) { StatusDot(status: s).controlSize(.mini); Text(s.summary).foregroundStyle(s.phase == .failed ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary)).lineLimit(3).textSelection(.enabled) }
                            .font(.callout)
                    }
                    Spacer()
                    SetupButton(model: model, id: id)
                }
                .padding(.vertical, 4)
            }
            Section {
                Picker("Tool access", selection: Binding(get: { p.access }, set: { v in model.setPref(id) { $0.access = v } })) {
                    ForEach(Self.access.filter { $0.0 != "read" || s.readOnly }, id: \.0) { Text($0.1).tag($0.0) }
                }
                Text(Self.explain(p.access, id)).font(.callout).foregroundStyle(.secondary)
                Picker("Stop the tool when idle for", selection: Binding(get: { p.idle }, set: { v in model.setPref(id) { $0.idle = v } })) {
                    Text("5 minutes").tag(5); Text("15 minutes").tag(15)
                }
                Toggle("Hide the tools it runs from the chat", isOn: Binding(get: { p.hideSteps }, set: { v in model.setPref(id) { $0.hideSteps = v } }))
            } header: { Text("Tasks") } footer: {
                Text("Model and effort are picked in the office, on the new task's model pill.").foregroundStyle(.secondary)
            }
            if id == "kiro" {
                Section {
                    Toggle("Compact automatically", isOn: Binding(get: { model.kiroAutoCompact }, set: { model.setKiroAutoCompact($0) }))
                        .toggleStyle(.switch)
                    Picker("at", selection: Binding(get: { model.kiroCompactAt }, set: { model.setKiroCompactAt($0) })) {
                        ForEach([50, 60, 70, 80, 90], id: \.self) { Text("\($0)% of the context window").tag($0) }
                    }
                    .disabled(!model.kiroAutoCompact)
                } header: { Text("Context") } footer: {
                    Text("A long conversation is summarised by Kiro before the next reply, so it keeps going instead of running out of room. Off by default.")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// Voice tasks: the shortcut, how the card behaves, speech on this Mac, and the
/// projects a spoken task can name.
private struct VoicePage: View {
    @ObservedObject var model: SettingsModel
    var body: some View {
        Form {
            Section {
                Toggle(isOn: Binding(get: { model.voiceEnabled }, set: { model.setVoice(enabled: $0) })) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Start tasks by voice")
                        Text("Hold ⌃⌥Space and say the task; let go to see it. A quick tap listens hands-free until you press it again. Esc cancels.")
                            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .toggleStyle(.switch)
                LabeledContent("Shortcut") { Text("⌃ ⌥ Space").font(.body.monospaced()).foregroundStyle(.secondary) }
                Picker("Start the task", selection: Binding(get: { model.voiceCountdown }, set: { model.setVoice(countdown: $0) })) {
                    Text("After 3 seconds").tag(3); Text("After 5 seconds").tag(5); Text("When I press Return").tag(0)
                }
                Picker("Agent", selection: Binding(get: { model.voiceAgent }, set: { model.setVoice(agent: $0) })) {
                    Text("The one last used in the office").tag("last")
                    Divider()
                    ForEach(SettingsModel.order, id: \.self) { Text(Marks.name($0)).tag($0) }
                }
                Toggle("Glow around the screen while listening", isOn: Binding(get: { model.voiceGlow }, set: { model.setVoice(glow: $0) }))
                Toggle("Play sounds", isOn: Binding(get: { model.voiceSounds }, set: { model.setVoice(sounds: $0) }))
            } header: { Text("Voice tasks") } footer: {
                Text("Start “Ask Codex to…” or “Cursor, …” to pick another agent, or a bot’s name (“Pip, …”) to reply to it. The card also has a menu for both.")
                    .foregroundStyle(.secondary)
            }
            Section {
                LabeledContent("Recognition") { Text(Dictation.engineDescription).foregroundStyle(.secondary).multilineTextAlignment(.trailing) }
                permission("Microphone", model.micStatus == .authorized, model.micStatus == .notDetermined)
                permission("Speech Recognition", model.speechStatus == .authorized, model.speechStatus == .notDetermined)
                HStack {
                    Button { model.tryVoice() } label: { Label("Try It", systemImage: "waveform") }
                    Text("Shows what would start, without starting anything.").font(.callout).foregroundStyle(.secondary)
                }
            } header: { Text("Speech") } footer: {
                Text("Speech is turned into text on this Mac by Apple’s own model, in your system language. No audio is saved or sent anywhere, and the agent gets only the task’s words.")
                    .foregroundStyle(.secondary)
            }
            Section {
                if model.projects.isEmpty {
                    Text("No projects yet. Voice tasks go to the folder you used last.").foregroundStyle(.secondary)
                }
                ForEach(model.projects) { p in ProjectRow(model: model, project: p) }
                Button { model.addProject() } label: { Label("Add Project…", systemImage: "plus") }
            } header: { Text("Projects") } footer: {
                Text("Say a project’s name or one of its other names (“…in the website”) and the task runs there. Otherwise it runs in the folder last used in the office, or in ~/Hover.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .onAppear { model.refreshPermissions() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in model.refreshPermissions() }
    }

    @ViewBuilder private func permission(_ name: String, _ ok: Bool, _ undecided: Bool) -> some View {
        LabeledContent(name) {
            if ok { Label("Allowed", systemImage: "checkmark.circle.fill").foregroundStyle(.green) }
            else { Button(undecided ? "Allow…" : "Open Privacy & Security") { model.askPermissions() } }
        }
    }
}

private struct ProjectRow: View {
    @ObservedObject var model: SettingsModel
    let project: VoiceProject
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: FileManager.default.fileExists(atPath: project.path) ? "folder.fill" : "questionmark.folder")
                .foregroundStyle(.tint).font(.system(size: 18)).frame(width: 24).padding(.top, 3)
            VStack(alignment: .leading, spacing: 4) {
                TextField("Name", text: binding(\.name)).textFieldStyle(.plain).font(.body.weight(.medium))
                Text((project.path as NSString).abbreviatingWithTildeInPath).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                TextField("Also called (comma separated)", text: binding(\.aliases)).textFieldStyle(.roundedBorder).font(.callout)
            }
            Spacer()
            Button { model.setProjects(model.projects.filter { $0.id != project.id }) } label: { Image(systemName: "minus.circle.fill").foregroundStyle(.secondary) }
                .buttonStyle(.borderless).help("Remove this project")
        }
        .padding(.vertical, 3)
    }
    private func binding(_ key: WritableKeyPath<VoiceProject, String>) -> Binding<String> {
        Binding(get: { model.projects.first { $0.id == project.id }?[keyPath: key] ?? "" },
                set: { v in var list = model.projects; if let i = list.firstIndex(where: { $0.id == project.id }) { list[i][keyPath: key] = v; model.setProjects(list) } })
    }
}
