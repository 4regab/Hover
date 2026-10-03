import AppKit
import ServiceManagement

// The menu bar shows the usage the Windows notch shows: each switched-on tool's
// logo in a ring of its used share, with the percentage beside it. The menu holds
// the details, the agents at work and Hover's actions.
struct QuotaValue: Equatable { var ok: Bool; var used: Double?; var detail: String }

final class MenuBar: NSObject, NSMenuDelegate {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    weak var app: App?
    var enabled: [String] = [] { didSet { if enabled != oldValue { render() } } }
    var quotas: [String: QuotaValue] = [:] { didSet { if quotas != oldValue { render() } } }
    var reading = false { didSet { if reading != oldValue { render() } } }
    var sessions: [[String: Any]] = []
    private var appearance: NSKeyValueObservation?, dark: Bool?
    // The tools this Mac build sets up. The backend's Claude Code reader (from the
    // Windows app) still works if settings switch it on, but isn't offered here.
    static let quotaIds = ["codex", "kiro", "cursor"]

    override init() {
        super.init()
        let menu = NSMenu(); menu.delegate = self; menu.autoenablesItems = false
        item.menu = menu
        item.button?.imagePosition = .imageOnly
        item.button?.setAccessibilityLabel("Hover")
        // Redraw in the menu bar's own appearance (it follows the wallpaper, not the app).
        // Setting the image re-announces the appearance, so redraw only when it really changed.
        appearance = item.button?.observe(\.effectiveAppearance) { [weak self] button, _ in
            let dark = button.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            DispatchQueue.main.async { guard let self, self.dark != dark else { return }; self.render() }
        }
        render()
    }

    static func percent(_ q: QuotaValue?) -> String {
        guard let q else { return "…" }
        guard q.ok, let used = q.used else { return "—" }
        return "\(Int(used.rounded()))%"
    }

    private func render() {
        guard let button = item.button else { return }
        let isDark = button.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        dark = isDark
        let shown = Self.quotaIds.filter(enabled.contains)
        if shown.isEmpty {
            let image = NSImage(systemSymbolName: "sparkles", accessibilityDescription: "Hover")
            image?.isTemplate = true
            button.image = image
            button.toolTip = "Hover"
            return
        }
        let font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
        let quotas = self.quotas
        let texts = shown.map { NSAttributedString(string: Self.percent(quotas[$0]), attributes: [.font: font]) }
        let ring: CGFloat = 16, gap: CGFloat = 4, between: CGFloat = 8
        let width = zip(shown, texts).reduce(CGFloat(0)) { $0 + ring + gap + ceil($1.1.size().width) } + between * CGFloat(shown.count - 1)
        let size = CGSize(width: ceil(width), height: 18)
        let label = isDark ? NSColor.white : NSColor.black
        let image = NSImage(size: size, flipped: false) { _ in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            var x: CGFloat = 0
            for (id, text) in zip(shown, texts) {
                let c = CGPoint(x: x + ring / 2, y: size.height / 2)
                let q = quotas[id]
                Marks.drawRing(center: c, radius: ring / 2 - 1.25, used: q?.ok == true ? q?.used : nil, track: label.withAlphaComponent(0.22), color: nil, width: 2, context: cg)
                Marks.drawGlyph(id, in: CGRect(x: c.x - 4, y: c.y - 4, width: 8, height: 8), context: cg, ink: label)
                x += ring + gap
                let s = NSAttributedString(string: text.string, attributes: [.font: font, .foregroundColor: label.withAlphaComponent(q?.ok == false ? 0.55 : 1)])
                s.draw(at: CGPoint(x: x, y: (size.height - s.size().height) / 2))
                x += ceil(s.size().width) + between
            }
            return true
        }
        image.isTemplate = false
        button.image = image
        button.toolTip = shown.map { "\(Marks.name($0)): \(quotas[$0]?.detail ?? "Reading…")" }.joined(separator: "\n")
        button.setAccessibilityLabel("Hover usage: " + shown.map { "\(Marks.name($0)) \(Self.percent(quotas[$0]))" }.joined(separator: ", "))
    }

    // MARK: Menu

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let app else { return }
        menu.addItem(.sectionHeader(title: "Usage"))
        let shown = Self.quotaIds.filter(enabled.contains)
        if shown.isEmpty {
            let hint = NSMenuItem(title: "Choose tools below to show their usage here", action: nil, keyEquivalent: ""); hint.isEnabled = false
            menu.addItem(hint)
        }
        for id in shown {
            let q = quotas[id]
            let title = NSMutableAttributedString(string: "\(Marks.name(id))  \(Self.percent(q))", attributes: [.font: NSFont.menuFont(ofSize: 13)])
            title.append(NSAttributedString(string: "\n" + (q?.detail ?? "Reading…"), attributes: [.font: NSFont.menuFont(ofSize: 11), .foregroundColor: NSColor.secondaryLabelColor]))
            let row = NSMenuItem(title: "", action: #selector(App.refresh), keyEquivalent: "")
            row.attributedTitle = title; row.target = app
            row.image = Self.ringImage(id, used: q?.ok == true ? q?.used : nil)
            row.toolTip = "Read again"
            menu.addItem(row)
        }
        let readers = NSMenuItem(title: "Show in menu bar", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        for id in Self.quotaIds {
            let toggle = NSMenuItem(title: Marks.name(id), action: #selector(App.toggleQuota(_:)), keyEquivalent: "")
            toggle.target = app; toggle.representedObject = id; toggle.state = enabled.contains(id) ? .on : .off
            toggle.image = Self.tileImage(id)
            sub.addItem(toggle)
        }
        sub.addItem(.separator())
        let note = NSMenuItem(title: "Readers use each tool's own sign-in, read-only", action: nil, keyEquivalent: ""); note.isEnabled = false
        sub.addItem(note)
        readers.submenu = sub
        menu.addItem(readers)

        menu.addItem(.separator())
        menu.addItem(.sectionHeader(title: "Agents"))
        let active = sessions.filter { ["working", "waking", "waiting"].contains($0["stage"] as? String ?? "") }
        if active.isEmpty {
            let quiet = NSMenuItem(title: "The office is quiet", action: nil, keyEquivalent: ""); quiet.isEnabled = false
            menu.addItem(quiet)
        }
        for s in active.prefix(6) {
            let tool = s["tool"] as? String ?? "kiro"
            let waiting = s["stage"] as? String == "waiting"
            let what = waiting ? "Needs your approval" : (s["act"] as? String ?? "Working")
            let row = NSMenuItem(title: "\(s["title"] as? String ?? Marks.name(tool)) — \(what)", action: #selector(App.openSession(_:)), keyEquivalent: "")
            row.target = app; row.representedObject = s["id"]; row.image = Self.tileImage(tool)
            menu.addItem(row)
        }
        // Newer releases of the tools: each one's own updater, all in one click.
        let updates = app.updatesAvailable
        if !updates.isEmpty {
            let names = updates.map { $0 == "cua-driver" ? "Cua Driver" : Marks.name($0) }
            let all = NSMenuItem(title: "Update \(ListFormatter.localizedString(byJoining: names))", action: #selector(App.updateAll), keyEquivalent: "")
            all.target = app; all.image = Self.badged(updates.first == "cua-driver" ? nil : updates.first)
            all.toolTip = "Runs each tool's own updater. A tool with a task at work is updated when it is done."
            menu.addItem(all)
        }
        // Any open app to a project's desktop (the same as right-clicking its title bar).
        let vm = NSMenuItem(title: "Send to Hover VM", action: nil, keyEquivalent: "")
        vm.image = SendToVM.icon; vm.submenu = app.sendToVM.appsMenu()
        menu.addItem(vm)
        let office = NSMenuItem(title: "Open Office", action: #selector(App.toggleOffice), keyEquivalent: "n")
        office.keyEquivalentModifierMask = [.option]; office.target = app
        menu.addItem(office)
        let dash = NSMenuItem(title: "Office in a Window", action: #selector(App.showDashboard), keyEquivalent: ""); dash.target = app
        menu.addItem(dash)
        let voice = NSMenuItem(title: "Start a Voice Task", action: #selector(App.startVoice), keyEquivalent: " ")
        voice.keyEquivalentModifierMask = [.control, .option]; voice.target = app
        voice.isEnabled = VoiceSettings.enabled
        voice.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: nil)
        voice.toolTip = "Hold ⌃⌥Space and speak; let go to see the task. A tap listens hands-free."
        menu.addItem(voice)

        menu.addItem(.separator())
        let hover = NSMenuItem(title: "Open on Hover", action: #selector(App.toggleHover), keyEquivalent: "")
        hover.target = app; hover.state = app.hoverOpens ? .on : .off
        menu.addItem(hover)
        let login = NSMenuItem(title: "Launch at Login", action: #selector(App.toggleLogin), keyEquivalent: "")
        login.target = app; login.state = SMAppService.mainApp.status == .enabled ? .on : .off
        menu.addItem(login)
        let settings = NSMenuItem(title: "Settings…", action: #selector(App.showSettings), keyEquivalent: ","); settings.target = app
        menu.addItem(settings)
        let refresh = NSMenuItem(title: reading ? "Reading usage…" : "Refresh Tools and Usage", action: #selector(App.refresh), keyEquivalent: "r")
        refresh.target = app; refresh.isEnabled = !reading
        menu.addItem(refresh)
        menu.addItem(.separator())
        let quit = NSMenuItem(title: "Quit Hover", action: #selector(App.quit), keyEquivalent: "q"); quit.target = app
        menu.addItem(quit)
    }

    static func tileImage(_ id: String) -> NSImage {
        NSImage(size: CGSize(width: 16, height: 16), flipped: false) { r in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            Marks.drawTile(id, in: r.insetBy(dx: 1, dy: 1), context: cg); return true
        }
    }

    /// A tool's tile (or a plain arrow) with the red "!" the office puts on it for an update.
    static func badged(_ id: String?) -> NSImage {
        NSImage(size: CGSize(width: 16, height: 16), flipped: false) { r in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            if let id { Marks.drawTile(id, in: r.insetBy(dx: 1, dy: 1), context: cg) }
            else { NSImage(systemSymbolName: "arrow.down.circle", accessibilityDescription: nil)?.draw(in: r.insetBy(dx: 1, dy: 1)) }
            let b = CGRect(x: r.maxX - 8, y: r.maxY - 8, width: 8, height: 8)
            cg.setFillColor(NSColor(srgbRed: 1, green: 0.27, blue: 0.23, alpha: 1).cgColor); cg.fillEllipse(in: b)
            return true
        }
    }

    static func ringImage(_ id: String, used: Double?) -> NSImage {
        NSImage(size: CGSize(width: 22, height: 22), flipped: false) { r in
            guard let cg = NSGraphicsContext.current?.cgContext else { return false }
            let c = CGPoint(x: r.midX, y: r.midY)
            Marks.drawRing(center: c, radius: 9.5, used: used, track: NSColor.labelColor.withAlphaComponent(0.18), color: nil, width: 2.2, context: cg)
            Marks.drawTile(id, in: CGRect(x: c.x - 6, y: c.y - 6, width: 12, height: 12), context: cg)
            return true
        }
    }
}
