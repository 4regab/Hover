import AppKit
import QuartzCore

// The notch, Notchy-style: an always-on panel over the MacBook's camera housing.
// One window, sized for the open office, never resizes (resizing a WKWebView every
// frame re-lays out the office). A CAShapeLayer mask grows from the hardware notch's
// own outline to the island and to the office with a spring, as the Windows notch
// animates its one Openness value. Everything outside the mask ignores the mouse, so
// the menu bar under the window keeps working.

extension NSScreen {
    var displayID: CGDirectDisplayID { deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? CGDirectDisplayID ?? 0 }
    /// The built-in display (the one with the notch), else the menu-bar screen. Notchy's choice.
    static var notchScreen: NSScreen? {
        screens.first { CGDisplayIsBuiltin($0.displayID) != 0 && $0.safeAreaInsets.top > 0 } ?? screens.first ?? main
    }
}

struct NotchGeometry: Equatable {
    var screen: CGRect
    var hasNotch: Bool
    var notchWidth: CGFloat, notchHeight: CGFloat
    var open: CGSize

    static func current(for screen: NSScreen?) -> NotchGeometry {
        guard let screen else { return NotchGeometry(screen: CGRect(x: 0, y: 0, width: 1440, height: 900), hasNotch: false, notchWidth: 180, notchHeight: 24, open: CGSize(width: 1120, height: 464)) }
        var width: CGFloat = 180, height = screen.frame.maxY - screen.visibleFrame.maxY
        var notch = false
        if let left = screen.auxiliaryTopLeftArea, let right = screen.auxiliaryTopRightArea, screen.safeAreaInsets.top > 0 {
            // The notch is the gap between the two strips of menu bar either side of it.
            width = right.minX - left.maxX
            height = screen.safeAreaInsets.top
            notch = width > 40
        }
        height = max(height, 24)
        let work = screen.visibleFrame
        // Windows' Default office size (1120 × 440) plus the strip beside the notch.
        let open = CGSize(width: min(1120, work.width - 24), height: min(440 + height, work.height - 24 + height))
        return NotchGeometry(screen: screen.frame, hasNotch: notch, notchWidth: width, notchHeight: height, open: open)
    }
}

/// What the closed notch shows, from the office's state.
struct Island: Equatable {
    enum Kind: Equatable { case idle, working, waiting, ended(ok: Bool) }
    var kind: Kind = .idle
    var tools: [String] = []
    var text = ""
    var line = ""
    var sessionId: Int?
    var askId: String?
    var canAllow = true
    var danger = false
}

/// The mask's outline: the body's width and height hanging from the top edge,
/// concave "ears" where it meets the bezel, and round bottom corners.
struct Outline: Equatable {
    var width: CGFloat, height: CGFloat, ear: CGFloat, radius: CGFloat
    var values: [CGFloat] { [width, height, ear, radius] }
    init(width: CGFloat, height: CGFloat, ear: CGFloat, radius: CGFloat) { self.width = width; self.height = height; self.ear = ear; self.radius = radius }
    init(_ v: [CGFloat]) { self.init(width: v[0], height: v[1], ear: v[2], radius: v[3]) }

    func path(centerX: CGFloat, top: CGFloat) -> CGPath {
        let p = CGMutablePath()
        let w = max(width, 1), h = max(height, 1)
        let left = centerX - w / 2, right = centerX + w / 2, bottom = top - h
        let ear = max(0, min(self.ear, h / 2)), r = max(0, min(radius, (h - ear), w / 2))
        p.move(to: CGPoint(x: left - ear, y: top))
        p.addQuadCurve(to: CGPoint(x: left, y: top - ear), control: CGPoint(x: left, y: top))
        p.addLine(to: CGPoint(x: left, y: bottom + r))
        p.addQuadCurve(to: CGPoint(x: left + r, y: bottom), control: CGPoint(x: left, y: bottom))
        p.addLine(to: CGPoint(x: right - r, y: bottom))
        p.addQuadCurve(to: CGPoint(x: right, y: bottom + r), control: CGPoint(x: right, y: bottom))
        p.addLine(to: CGPoint(x: right, y: top - ear))
        p.addQuadCurve(to: CGPoint(x: right + ear, y: top), control: CGPoint(x: right, y: top))
        p.closeSubpath()
        return p
    }
}

final class NotchPanel: NSPanel {
    var expanded = false
    override var canBecomeKey: Bool { expanded }
    override var canBecomeMain: Bool { false }
}

final class Notch: NSObject {
    let window: NotchPanel
    private let root = NSView(), body = NSView(), mask = CAShapeLayer()
    let island = IslandView()
    private(set) var geometry: NotchGeometry
    private weak var web: NSView?
    private var current: [CGFloat] = [180, 32, 0, 10], velocity: [CGFloat] = [0, 0, 0, 0], target: [CGFloat] = [180, 32, 0, 10]
    private var link: CADisplayLink?, lastTime: CFTimeInterval = 0, bouncy = false
    private static let margin: CGFloat = 40

    var expanded: Bool { window.expanded }
    var hovered = false { didSet { if hovered != oldValue { retarget() } } }
    var state = Island() { didSet { if state != oldValue { island.state = state; retarget() } } }

    init(screen: NSScreen?) {
        geometry = NotchGeometry.current(for: screen)
        window = NotchPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        super.init()
        window.isOpaque = false; window.backgroundColor = .clear; window.hasShadow = false
        // Above the menu bar (Notchy uses the same level), on every Space and over full-screen apps.
        window.level = .statusBar
        window.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary, .ignoresCycle]
        window.hidesOnDeactivate = false; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        window.isMovable = false; window.ignoresMouseEvents = true; window.acceptsMouseMovedEvents = true
        root.wantsLayer = true
        body.wantsLayer = true; body.layer?.backgroundColor = NSColor.black.cgColor
        mask.fillColor = NSColor.black.cgColor
        body.layer?.mask = mask
        window.contentView = root
        root.addSubview(body)
        body.addSubview(island)
        island.notch = self
        layout()
        target = rest().values; current = target
        apply()
        window.orderFrontRegardless()
    }

    /// The office's web view, shown under the strip beside the notch.
    func attach(_ view: NSView) {
        web = view
        view.isHidden = true
        body.addSubview(view, positioned: .below, relativeTo: island)
        layout()
    }

    func screenChanged(_ screen: NSScreen?) {
        let g = NotchGeometry.current(for: screen)
        guard g != geometry || window.frame.isEmpty else { return }
        geometry = g
        layout(); retarget(animated: false)
    }

    private func layout() {
        let g = geometry
        let size = CGSize(width: g.open.width + 2 * Self.margin, height: g.open.height + 24)
        window.setFrame(CGRect(x: g.screen.midX - size.width / 2, y: g.screen.maxY - size.height, width: size.width, height: size.height), display: false)
        root.frame = CGRect(origin: .zero, size: size)
        body.frame = root.bounds
        // The island draws only in the band the closed shapes can reach.
        let band = min(size.height, g.notchHeight + 64)
        island.frame = CGRect(x: 0, y: size.height - band, width: size.width, height: band)
        island.geometry = g
        web?.frame = CGRect(x: (size.width - g.open.width) / 2, y: size.height - g.open.height, width: g.open.width, height: g.open.height - g.notchHeight)
    }

    // MARK: Shapes

    private func rest() -> Outline {
        let g = geometry, w = g.notchWidth, h = g.notchHeight
        let wing = island.wingWidth()
        switch state.kind {
        case .waiting:
            return Outline(width: max(w + 2 * wing, min(460, g.open.width)), height: h + 50, ear: 10, radius: 20)
        case .working, .ended:
            return Outline(width: w + 2 * wing + (hovered ? 8 : 0), height: h + (hovered ? 3 : 0), ear: hovered ? 8 : 6, radius: 10)
        case .idle:
            // The hardware outline; hovering grows Notchy's ears and a few points of body.
            return hovered ? Outline(width: w + 16, height: h + 3, ear: 8, radius: 10) : Outline(width: w, height: h, ear: g.hasNotch ? 4 : 0, radius: g.hasNotch ? 9 : h / 2)
        }
    }

    private func openOutline() -> Outline { Outline(width: geometry.open.width, height: geometry.open.height, ear: 14, radius: 26) }

    func setOpen(_ on: Bool, animated: Bool = true) {
        window.expanded = on
        if on { web?.isHidden = false }
        island.open = on
        retarget(animated: animated)
    }

    private func retarget(animated: Bool = true) {
        target = (window.expanded ? openOutline() : rest()).values
        bouncy = window.expanded
        if !animated { current = target; velocity = [0, 0, 0, 0]; apply(); settle(); return }
        if link == nil {
            let l = root.displayLink(target: self, selector: #selector(step(_:)))
            l.add(to: .main, forMode: .common)
            link = l; lastTime = 0
        }
    }

    @objc private func step(_ l: CADisplayLink) {
        let now = l.timestamp
        let dt = lastTime == 0 ? 1.0 / 60 : min(1.0 / 30, max(1.0 / 240, now - lastTime))
        lastTime = now
        // A spring per value: a little bounce opening, none folding (the Dynamic Island feel).
        let k: CGFloat = bouncy ? 260 : 340, c: CGFloat = 2 * sqrt(k) * (bouncy ? 0.72 : 1.0)
        var moving = false
        for i in 0..<4 {
            let a = -k * (current[i] - target[i]) - c * velocity[i]
            velocity[i] += a * CGFloat(dt); current[i] += velocity[i] * CGFloat(dt)
            if abs(current[i] - target[i]) > 0.3 || abs(velocity[i]) > 2 { moving = true }
        }
        if !moving { current = target; velocity = [0, 0, 0, 0] }
        apply()
        if !moving { link?.invalidate(); link = nil; settle() }
    }

    private func settle() {
        // Folded: stop drawing the office (WebKit pauses a hidden view's compositing).
        if !window.expanded { web?.isHidden = true }
    }

    private func apply() {
        CATransaction.begin(); CATransaction.setDisableActions(true)
        mask.frame = body.bounds
        mask.path = Outline(current).path(centerX: body.bounds.midX, top: body.bounds.maxY)
        CATransaction.commit()
        island.outline = Outline(current)
    }

    /// The open office's shape in screen coordinates (no ears): where the window grows from.
    var openFrame: CGRect {
        let o = openOutline(), f = window.frame
        return CGRect(x: f.midX - o.width / 2, y: f.maxY - o.height, width: o.width, height: o.height)
    }
    var notchHeight: CGFloat { geometry.notchHeight }

    /// The target shape in screen coordinates, ears included, for pointer tests.
    var shapeFrame: CGRect {
        let o = Outline(target), f = window.frame
        return CGRect(x: f.midX - o.width / 2 - o.ear, y: f.maxY - o.height, width: o.width + 2 * o.ear, height: o.height)
    }

    /// True over the visible shape. The top screen row counts, as in Notchy, so a
    /// pointer pushed against the top edge still opens it.
    func contains(_ point: CGPoint, margin: CGFloat = 0) -> Bool {
        var r = shapeFrame.insetBy(dx: -margin, dy: -margin)
        r.size.height += 1
        return r.contains(point)
    }

    /// Clicks go to the shape only; everywhere else falls through to what is beneath.
    /// While files are dragged over it, the whole window takes the drop.
    var takesDrops = false { didSet { if takesDrops { window.ignoresMouseEvents = false } } }
    func track(_ point: CGPoint) {
        if takesDrops { return }
        let inside = contains(point)
        if window.ignoresMouseEvents == inside { window.ignoresMouseEvents = !inside }
    }

    func refresh() { island.needsDisplay = true; if !window.expanded { let t = rest().values; if t != target { retarget() } } }
}

/// Draws the island beside and under the notch: the agents at work, a question
/// waiting on the user, or what just finished. Draws in flipped coordinates.
final class IslandView: NSView {
    weak var notch: Notch?
    var geometry = NotchGeometry.current(for: nil)
    var state = Island() { didSet { needsDisplay = true } }
    var outline = Outline(width: 0, height: 0, ear: 0, radius: 0) { didSet { needsDisplay = true } }
    var open = false { didSet { needsDisplay = true } }
    var onClick: (() -> Void)?
    var onAnswer: ((String) -> Void)?
    private var pressed: String?
    private var buttons: [(String, CGRect)] = []
    private let amber = NSColor(srgbRed: 1, green: 0.70, blue: 0.25, alpha: 1)
    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var cx: CGFloat { bounds.midX }
    private var stripH: CGFloat { geometry.notchHeight }
    private func font(_ size: CGFloat, _ weight: NSFont.Weight = .medium) -> NSFont { .systemFont(ofSize: size, weight: weight) }
    private func attributed(_ text: String, _ size: CGFloat, _ color: NSColor, _ weight: NSFont.Weight = .medium) -> NSAttributedString {
        let style = NSMutableParagraphStyle(); style.lineBreakMode = .byTruncatingTail
        return NSAttributedString(string: text, attributes: [.font: font(size, weight), .foregroundColor: color, .paragraphStyle: style])
    }

    private var rightText: String {
        switch state.kind {
        case .working: return state.text
        case .waiting: return "Needs you"
        case .ended(let ok): return state.text.isEmpty ? (ok ? "Done" : "Stopped") : state.text
        case .idle: return ""
        }
    }

    /// How far each side of the notch the island reaches; symmetric, so it stays centred.
    func wingWidth() -> CGFloat {
        if state.kind == .idle { return 0 }
        let tiles = CGFloat(min(3, max(1, state.tools.count)))
        let left = 12 + 18 + (tiles - 1) * 12 + 10
        let right = 10 + 14 + 6 + min(140, ceil(attributed(rightText, 12, .white).size().width)) + 12
        return min(170, max(left, right))
    }

    /// The question card's buttons, right to left, as last drawn.
    var buttonTitles: [String] { buttons.map(\.0) }
    /// Answers as a click on that button would (tests and accessibility).
    func press(_ id: String) { if buttons.contains(where: { $0.0 == id }) { onAnswer?(id) } }

    override func hitTest(_ point: NSPoint) -> NSView? {
        // Open, the office takes every click; folded, only the drawn shape does.
        guard !open, state.kind != .idle || notch?.hovered == true else { return nil }
        let local = convert(point, from: superview)
        let w = outline.width + 2 * outline.ear
        return CGRect(x: cx - w / 2, y: 0, width: w, height: outline.height).contains(local) ? self : nil
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        pressed = buttons.first { $0.1.contains(p) }?.0 ?? "body"
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        defer { pressed = nil; needsDisplay = true }
        guard let pressed else { return }
        if pressed == "body" { onClick?(); return }
        if buttons.first(where: { $0.0 == pressed })?.1.contains(p) == true { onAnswer?(pressed) }
    }

    override func draw(_ dirty: NSRect) {
        guard let cg = NSGraphicsContext.current?.cgContext else { return }
        buttons = []
        guard state.kind != .idle else { return }
        let half = geometry.notchWidth / 2, mid = stripH / 2
        let wing = wingWidth()
        // Left wing: the agents' logos, the front one on top.
        let tile: CGFloat = 18
        var x = cx - half - wing + 12
        for (i, tool) in state.tools.prefix(3).enumerated().reversed() {
            let r = CGRect(x: x + CGFloat(i) * 12, y: mid - tile / 2, width: tile, height: tile)
            if i > 0 { cg.setFillColor(NSColor.black.cgColor); cg.fillEllipse(in: r.insetBy(dx: -1.5, dy: -1.5)) }
            Marks.drawTile(tool, in: r, context: cg, flipped: true)
        }
        x = cx - half - wing + 12
        if case .ended(let ok) = state.kind {
            let b = CGRect(x: x + tile - 7, y: mid + tile / 2 - 8, width: 10, height: 10)
            cg.setFillColor(NSColor.black.cgColor); cg.fillEllipse(in: b.insetBy(dx: -1.5, dy: -1.5))
            cg.setFillColor((ok ? Marks.quotaColor(0) : Marks.quotaColor(100)).cgColor); cg.fillEllipse(in: b)
        }
        if state.kind == .waiting {
            let b = CGRect(x: x + tile - 7, y: mid - tile / 2 - 2, width: 9, height: 9)
            cg.setFillColor(NSColor.black.cgColor); cg.fillEllipse(in: b.insetBy(dx: -1.5, dy: -1.5))
            cg.setFillColor(amber.cgColor); cg.fillEllipse(in: b)
        }
        // Right wing: a spinner while working, then what the front agent is doing.
        let right = cx + half + wing - 12
        let text = attributed(rightText, 12, state.kind == .waiting ? amber : NSColor(white: 1, alpha: 0.88))
        let tw = min(140, ceil(text.size().width))
        text.draw(with: CGRect(x: right - tw, y: mid - 8, width: tw, height: 16), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        if state.kind == .working {
            let c = CGPoint(x: right - tw - 6 - 6, y: mid)
            let t = CACurrentMediaTime()
            cg.saveGState(); cg.setLineWidth(1.8); cg.setLineCap(.round)
            cg.setStrokeColor(NSColor(white: 1, alpha: 0.2).cgColor); cg.addEllipse(in: CGRect(x: c.x - 5.5, y: c.y - 5.5, width: 11, height: 11)); cg.strokePath()
            let a = CGFloat(t.truncatingRemainder(dividingBy: 1.1) / 1.1) * 2 * .pi
            cg.setStrokeColor(Marks.accent(state.tools.first ?? "kiro").cgColor)
            cg.addArc(center: c, radius: 5.5, startAngle: a, endAngle: a + 1.9, clockwise: false); cg.strokePath()
            cg.restoreGState()
            // Keep spinning while visible; the backend only reports changes.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0 / 30) { [weak self] in
                guard let self, self.state.kind == .working, !self.isHiddenOrHasHiddenAncestor, self.window?.isVisible == true else { return }
                self.setNeedsDisplay(CGRect(x: c.x - 8, y: c.y - 8, width: 16, height: 16))
            }
        }
        // The question card under the notch: what the agent asks, and the answers.
        guard state.kind == .waiting, !open, outline.height > stripH + 20 else { return }
        let left = cx - outline.width / 2 + 16, end = cx + outline.width / 2 - 14
        let rowY = stripH + 10, rowH: CGFloat = 26
        var bx = end
        let names: [(String, String)] = state.canAllow ? [("review", "Review"), ("allow", "Allow"), ("deny", "Deny")] : [("review", "Review")]
        for (id, title) in names {
            let label = attributed(title, 12, id == "allow" ? .black : .white, .semibold)
            let w = ceil(label.size().width) + 22
            bx -= w
            let r = CGRect(x: bx, y: rowY, width: w, height: rowH)
            let fill: NSColor = id == "allow" ? (state.danger ? Marks.quotaColor(100) : .white) : NSColor(white: 1, alpha: pressed == id ? 0.28 : 0.14)
            cg.setFillColor((pressed == id && id == "allow" ? fill.withAlphaComponent(0.8) : fill).cgColor)
            cg.addPath(CGPath(roundedRect: r, cornerWidth: rowH / 2, cornerHeight: rowH / 2, transform: nil)); cg.fillPath()
            label.draw(at: CGPoint(x: r.minX + 11, y: r.midY - label.size().height / 2))
            buttons.append((id, r))
            bx -= 6
        }
        let line = attributed(state.line, 12.5, NSColor(white: 1, alpha: 0.92))
        line.draw(with: CGRect(x: left, y: rowY + 4, width: max(0, bx - left - 6), height: 18), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}
