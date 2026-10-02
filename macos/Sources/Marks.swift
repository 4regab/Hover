import AppKit

// The tools' logos, drawn from the same path data as the Windows notch (Owl/Marks.cs),
// so the Mac island, menu bar and office show one set of marks.
enum Marks {
    struct Mark { let path: CGPath; let evenOdd: Bool; let tile: NSColor; let ink: [NSColor]; let accent: NSColor; let pad: CGFloat; let edge: Bool }

    static let ids = ["claude", "kiro", "codex", "cursor", "opencode"]

    static func name(_ id: String) -> String {
        ["claude": "Claude Code", "kiro": "Kiro", "codex": "Codex", "cursor": "Cursor", "opencode": "OpenCode"][id] ?? id.capitalized
    }

    private static func rgb(_ v: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((v >> 16) & 255) / 255, green: CGFloat((v >> 8) & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: 1)
    }

    private static let all: [String: Mark] = [
        "kiro": Mark(path: parse(kiroPath).0, evenOdd: parse(kiroPath).1, tile: rgb(0x9046FF), ink: [.white], accent: rgb(0xB48CFF), pad: 0.17, edge: false),
        "codex": Mark(path: parse(codexPath).0, evenOdd: parse(codexPath).1, tile: .white, ink: [rgb(0xB1A7FF), rgb(0x7A9DFF), rgb(0x3941FF)], accent: rgb(0x7A9DFF), pad: 0.1, edge: false),
        "cursor": Mark(path: parse(cursorPath).0, evenOdd: parse(cursorPath).1, tile: rgb(0x18181C), ink: [rgb(0xECECF0)], accent: rgb(0xECECF0), pad: 0.2, edge: true),
        "claude": Mark(path: parse(claudePath).0, evenOdd: parse(claudePath).1, tile: rgb(0xD97757), ink: [.white], accent: rgb(0xD97757), pad: 0.19, edge: false),
        "opencode": Mark(path: parse(openCodePath).0, evenOdd: parse(openCodePath).1, tile: rgb(0x101012), ink: [rgb(0xF4F4F6)], accent: rgb(0xF4F4F6), pad: 0.2, edge: true),
    ]

    static func mark(_ id: String) -> Mark { all[id] ?? all["kiro"]! }
    static func accent(_ id: String) -> NSColor { mark(id).accent }

    /// Same thresholds as Windows: green below 70 %, amber below 90 %, then red.
    static func quotaColor(_ used: Double) -> NSColor {
        used < 70 ? rgb(0x32D74B) : used < 90 ? rgb(0xFFB340) : rgb(0xFF453A)
    }

    /// The glyph's path scaled into box; the source paths are y-down in a 24 unit box.
    static func glyph(_ id: String, in box: CGRect, flipped: Bool) -> CGPath {
        let m = mark(id), b = m.path.boundingBoxOfPath
        let k = min(box.width / b.width, box.height / b.height)
        let x = box.minX + (box.width - b.width * k) / 2 - b.minX * k
        var t: CGAffineTransform
        if flipped { t = CGAffineTransform(a: k, b: 0, c: 0, d: k, tx: x, ty: box.minY + (box.height - b.height * k) / 2 - b.minY * k) }
        else { t = CGAffineTransform(a: k, b: 0, c: 0, d: -k, tx: x, ty: box.maxY - (box.height - b.height * k) / 2 + b.minY * k) }
        return m.path.copy(using: &t) ?? m.path
    }

    /// The tool's tile and mark, as the Windows notch draws them.
    static func drawTile(_ id: String, in r: CGRect, context cg: CGContext, flipped: Bool = false) {
        let m = mark(id)
        let radius = r.width * 0.3
        let tile = CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
        cg.saveGState()
        cg.addPath(tile); cg.setFillColor(m.tile.cgColor); cg.fillPath()
        if m.edge { cg.addPath(tile); cg.setStrokeColor(NSColor(white: 1, alpha: 0.18).cgColor); cg.setLineWidth(1); cg.strokePath() }
        let pad = r.width * m.pad
        drawGlyph(id, in: r.insetBy(dx: pad, dy: pad), context: cg, flipped: flipped, ink: nil)
        cg.restoreGState()
    }

    /// The glyph alone; ink overrides the tool's own colour (the menu bar's label colour).
    static func drawGlyph(_ id: String, in box: CGRect, context cg: CGContext, flipped: Bool = false, ink: NSColor?) {
        let m = mark(id), path = glyph(id, in: box, flipped: flipped)
        cg.saveGState()
        cg.addPath(path)
        let colors = ink.map { [$0] } ?? m.ink
        if colors.count == 1 {
            cg.setFillColor(colors[0].cgColor); cg.fillPath(using: m.evenOdd ? .evenOdd : .winding)
        } else {
            cg.clip(using: m.evenOdd ? .evenOdd : .winding)
            let space = CGColorSpace(name: CGColorSpace.sRGB)!
            if let gradient = CGGradient(colorsSpace: space, colors: colors.map(\.cgColor) as CFArray, locations: nil) {
                let top = CGPoint(x: box.midX, y: flipped ? box.minY : box.maxY), bottom = CGPoint(x: box.midX, y: flipped ? box.maxY : box.minY)
                cg.drawLinearGradient(gradient, start: top, end: bottom, options: [])
            }
        }
        cg.restoreGState()
    }

    /// A quota ring: a faint track and the used share, clockwise from twelve o'clock.
    static func drawRing(center c: CGPoint, radius r: CGFloat, used: Double?, track: NSColor, color: NSColor?, width: CGFloat, context cg: CGContext, flipped: Bool = false) {
        cg.saveGState()
        cg.setLineWidth(width); cg.setLineCap(.round)
        cg.setStrokeColor(track.cgColor)
        cg.addEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)); cg.strokePath()
        if let used, used > 0 {
            let sweep = CGFloat(min(used, 100)) / 100 * 2 * .pi
            cg.setStrokeColor((color ?? quotaColor(used)).cgColor)
            let start: CGFloat = flipped ? -.pi / 2 : .pi / 2
            cg.addArc(center: c, radius: r, startAngle: start, endAngle: flipped ? start + sweep : start - sweep, clockwise: !flipped)
            cg.strokePath()
        }
        cg.restoreGState()
    }

    // MARK: - WPF path mini-language (M, L, C, A, Z and the F0/F1 fill rule)

    static func parse(_ data: String) -> (CGMutablePath, Bool) {
        let path = CGMutablePath()
        var evenOdd = true
        var tokens: [String] = []
        var number = ""
        func flush() { if !number.isEmpty { tokens.append(number); number = "" } }
        for ch in data {
            if ch.isLetter { flush(); tokens.append(String(ch)) }
            else if ch == "," || ch == " " { flush() }
            else if ch == "-" && !number.isEmpty && !number.hasSuffix("e") { flush(); number = "-" }
            else { number.append(ch) }
        }
        flush()
        var i = 0, command = "M"
        var current = CGPoint.zero, start = CGPoint.zero
        func next() -> CGFloat { defer { i += 1 }; return CGFloat(Double(tokens[i]) ?? 0) }
        func point() -> CGPoint { let x = next(); return CGPoint(x: x, y: next()) }
        while i < tokens.count {
            if let c = tokens[i].first, c.isLetter { command = String(c); i += 1 }
            switch command {
            case "F": evenOdd = next() == 0
            case "M": current = point(); start = current; path.move(to: current); command = "L"
            case "L": current = point(); path.addLine(to: current)
            case "C": let a = point(), b = point(); current = point(); path.addCurve(to: current, control1: a, control2: b)
            case "A":
                let rx = next(), ry = next(), rotation = next(), large = next() != 0, sweep = next() != 0, end = point()
                arc(path, from: current, to: end, rx: rx, ry: ry, rotation: rotation, large: large, sweep: sweep)
                current = end
            case "Z", "z": path.closeSubpath(); current = start
            default: i += 1
            }
        }
        return (path, evenOdd)
    }

    /// SVG endpoint arc to cubic Béziers (W3C SVG implementation notes, F.6.5).
    private static func arc(_ path: CGMutablePath, from p0: CGPoint, to p1: CGPoint, rx rxIn: CGFloat, ry ryIn: CGFloat, rotation: CGFloat, large: Bool, sweep: Bool) {
        var rx = abs(rxIn), ry = abs(ryIn)
        guard rx > 0, ry > 0, p0 != p1 else { path.addLine(to: p1); return }
        let phi = rotation * .pi / 180, cosP = cos(phi), sinP = sin(phi)
        let dx = (p0.x - p1.x) / 2, dy = (p0.y - p1.y) / 2
        let x1 = cosP * dx + sinP * dy, y1 = -sinP * dx + cosP * dy
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lambda > 1 { rx *= sqrt(lambda); ry *= sqrt(lambda) }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1
        var coef = sqrt(max(0, num / den)); if large == sweep { coef = -coef }
        let cx1 = coef * rx * y1 / ry, cy1 = -coef * ry * x1 / rx
        let cx = cosP * cx1 - sinP * cy1 + (p0.x + p1.x) / 2, cy = sinP * cx1 + cosP * cy1 + (p0.y + p1.y) / 2
        func angle(_ ux: CGFloat, _ uy: CGFloat, _ vx: CGFloat, _ vy: CGFloat) -> CGFloat {
            let a = atan2(ux * vy - uy * vx, ux * vx + uy * vy); return a
        }
        let theta = angle(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry)
        var delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry)
        if !sweep && delta > 0 { delta -= 2 * .pi } else if sweep && delta < 0 { delta += 2 * .pi }
        let segments = max(1, Int(ceil(abs(delta) / (.pi / 2))))
        let step = delta / CGFloat(segments), t = 4 / 3 * tan(step / 4)
        func at(_ a: CGFloat) -> (CGPoint, CGPoint) {
            let x = rx * cos(a), y = ry * sin(a), ex = -rx * sin(a), ey = ry * cos(a)
            return (CGPoint(x: cx + cosP * x - sinP * y, y: cy + sinP * x + cosP * y), CGPoint(x: cosP * ex - sinP * ey, y: sinP * ex + cosP * ey))
        }
        var a = theta
        for _ in 0..<segments {
            let (s, ds) = at(a), (e, de) = at(a + step)
            path.addCurve(to: e, control1: CGPoint(x: s.x + t * ds.x, y: s.y + t * ds.y), control2: CGPoint(x: e.x - t * de.x, y: e.y - t * de.y))
            a += step
        }
    }

    // Path data copied from src/Hover/Owl/Marks.cs (LobeHub icons, MIT; the marks belong to their owners).
    private static let kiroPath = "F0 M4.594,6.677 C6.670,-2.226 18.746,-2.211 21.160,6.632 C21.513,7.929 22.885,14.214 19.487,20.379 C17.942,23.176 13.646,25.869 12.497,22.262 C8.600,25.477 3.315,24.100 5.789,18.609 L5.471,18.752 C1.901,20.057 1.608,17.544 2.298,16.239 C2.748,15.399 3.025,14.904 3.235,14.342 C3.588,13.367 3.693,12.774 3.828,11.844 C4.098,10.007 4.105,8.237 4.593,6.677 L4.594,6.677 Z M12.964,6.687 A0.920,0.920 0.0 0 0 12.154,7.115 C11.937,7.438 11.824,7.940 11.824,8.577 C11.824,9.282 11.974,10.467 12.964,10.467 L12.972,10.467 C13.729,10.467 14.186,9.762 14.186,8.577 C14.186,7.955 14.059,7.452 13.819,7.122 A1.014,1.014 0.0 0 0 12.964,6.687 L12.964,6.687 Z M17.044,6.687 A0.920,0.920 0.0 0 0 16.234,7.115 C16.017,7.438 15.904,7.940 15.904,8.577 C15.904,9.282 16.054,10.467 17.044,10.467 L17.052,10.467 C17.809,10.467 18.267,9.762 18.267,8.577 C18.267,7.955 18.139,7.452 17.899,7.122 A1.014,1.014 0.0 0 0 17.044,6.687 L17.044,6.687 Z"
    private static let codexPath = "F1 M9.064,3.344 A4.578,4.578 0.0 0 1 11.349,3.032 C12.349,3.147 13.240,3.572 14.022,4.307 C14.032,4.317 14.046,4.324 14.059,4.328 A0.090,0.090 0.0 0 0 14.102,4.328 A4.550,4.550 0.0 0 1 17.148,4.603 L17.195,4.625 L17.311,4.682 A4.581,4.581 0.0 0 1 19.499,7.081 C19.708,7.591 19.812,8.122 19.814,8.676 A4.240,4.240 0.0 0 1 19.680,9.899 A0.123,0.123 0.0 0 0 19.710,10.014 C20.304,10.621 20.698,11.344 20.893,12.184 C21.182,13.609 20.886,14.894 20.006,16.038 L19.870,16.204 A4.548,4.548 0.0 0 1 17.669,17.592 A0.123,0.123 0.0 0 0 17.588,17.668 C17.397,18.219 17.205,18.691 16.848,19.162 C15.948,20.349 14.626,21.008 13.137,21 C11.950,20.994 10.898,20.560 9.980,19.698 A0.107,0.107 0.0 0 0 9.875,19.674 C9.487,19.799 9.095,19.817 8.671,19.812 A4.441,4.441 0.0 0 1 6.726,19.346 A4.544,4.544 0.0 0 1 5.116,18.011 C4.964,17.809 4.813,17.619 4.702,17.394 A5.810,5.810 0.0 0 1 4.332,16.433 A4.582,4.582 0.0 0 1 4.318,14.135 A0.124,0.124 0.0 0 0 4.324,14.079 A0.085,0.085 0.0 0 0 4.297,14.031 A4.467,4.467 0.0 0 1 3.263,12.380 A3.896,3.896 0.0 0 1 3.012,11.188 A5.189,5.189 0.0 0 1 3.153,9.588 C3.490,8.476 4.135,7.603 5.086,6.970 C5.298,6.829 5.499,6.719 5.687,6.640 C5.902,6.551 6.117,6.476 6.333,6.413 A0.098,0.098 0.0 0 0 6.398,6.347 A4.510,4.510 0.0 0 1 7.227,4.732 A4.535,4.535 0.0 0 1 9.064,3.344 L9.064,3.344 Z M12.546,13.909 A0.637,0.637 0.0 0 0 12.546,15.181 L16.182,15.181 A0.637,0.637 0.0 1 0 16.182,13.909 L12.546,13.909 Z M8.462,9.230 A0.637,0.637 0.0 0 0 7.356,9.861 L8.628,12.085 L7.362,14.221 A0.636,0.636 0.0 1 0 8.457,14.870 L9.911,12.415 A0.636,0.636 0.0 0 0 9.916,11.775 L8.462,9.230 Z"
    private static let cursorPath = "F0 M22.106,5.680 L12.500,0.135 A0.998,0.998 0.0 0 0 11.502,0.135 L1.893,5.680 A0.840,0.840 0.0 0 0 1.474,6.406 L1.474,17.592 C1.474,17.892 1.634,18.169 1.894,18.319 L11.501,23.866 A0.999,0.999 0.0 0 0 12.499,23.866 L22.107,18.319 A0.840,0.840 0.0 0 0 22.527,17.592 L22.527,6.407 A0.840,0.840 0.0 0 0 22.107,5.681 L22.106,5.680 Z M21.503,6.856 L12.228,22.920 C12.165,23.028 12,22.984 12,22.859 L12,12.340 A0.590,0.590 0.0 0 0 11.705,11.830 L2.595,6.570 C2.488,6.508 2.532,6.342 2.657,6.342 L21.207,6.342 C21.471,6.342 21.635,6.628 21.503,6.856 L21.503,6.856 Z"
    private static let claudePath = "F1 M4.709,15.955 L9.429,13.308 L9.509,13.078 L9.429,12.950 L9.200,12.950 L8.410,12.902 L5.712,12.829 L3.373,12.732 L1.107,12.610 L0.536,12.489 L0,11.784 L0.055,11.432 L0.535,11.111 L1.221,11.171 L2.741,11.274 L5.019,11.432 L6.671,11.529 L9.120,11.784 L9.509,11.784 L9.564,11.627 L9.430,11.529 L9.327,11.432 L6.969,9.836 L4.417,8.148 L3.081,7.176 L2.357,6.685 L1.993,6.223 L1.835,5.215 L2.491,4.493 L3.372,4.553 L3.597,4.614 L4.490,5.300 L6.398,6.776 L8.889,8.609 L9.254,8.913 L9.399,8.810 L9.418,8.737 L9.254,8.463 L7.899,6.017 L6.453,3.527 L5.809,2.495 L5.639,1.876 A2.970,2.970 0.0 0 1 5.535,1.147 L6.283,0.134 L6.696,0 L7.692,0.134 L8.112,0.498 L8.732,1.912 L9.734,4.141 L11.289,7.171 L11.745,8.069 L11.988,8.901 L12.079,9.156 L12.237,9.156 L12.237,9.010 L12.365,7.304 L12.602,5.209 L12.832,2.514 L12.912,1.754 L13.288,0.844 L14.035,0.352 L14.619,0.632 L15.099,1.317 L15.032,1.761 L14.746,3.612 L14.187,6.515 L13.823,8.457 L14.035,8.457 L14.278,8.215 L15.263,6.909 L16.915,4.845 L17.645,4.025 L18.495,3.121 L19.042,2.690 L20.075,2.690 L20.835,3.819 L20.495,4.985 L19.431,6.332 L18.550,7.474 L17.286,9.174 L16.496,10.534 L16.569,10.644 L16.757,10.624 L19.613,10.018 L21.156,9.738 L22.997,9.423 L23.830,9.811 L23.921,10.206 L23.593,11.013 L21.624,11.499 L19.315,11.961 L15.876,12.774 L15.834,12.804 L15.883,12.865 L17.432,13.011 L18.094,13.047 L19.716,13.047 L22.736,13.272 L23.526,13.794 L24,14.432 L23.921,14.917 L22.706,15.537 L21.066,15.148 L17.237,14.238 L15.925,13.909 L15.743,13.909 L15.743,14.019 L16.836,15.087 L18.842,16.897 L21.351,19.227 L21.478,19.805 L21.156,20.260 L20.816,20.211 L18.611,18.554 L17.760,17.807 L15.834,16.187 L15.706,16.187 L15.706,16.357 L16.150,17.006 L18.495,20.527 L18.617,21.607 L18.447,21.960 L17.839,22.173 L17.171,22.051 L15.797,20.126 L14.382,17.959 L13.239,16.016 L13.099,16.096 L12.425,23.350 L12.109,23.720 L11.380,24 L10.773,23.539 L10.451,22.792 L10.773,21.316 L11.162,19.392 L11.477,17.862 L11.763,15.962 L11.933,15.330 L11.921,15.288 L11.781,15.306 L10.347,17.273 L8.167,20.218 L6.441,22.063 L6.027,22.227 L5.310,21.857 L5.377,21.195 L5.778,20.606 L8.166,17.570 L9.606,15.688 L10.536,14.602 L10.530,14.444 L10.475,14.444 L4.132,18.560 L3.002,18.706 L2.515,18.250 L2.576,17.504 L2.807,17.261 L4.715,15.949 L4.709,15.955 L4.709,15.955 Z"
    private static let openCodePath = "F0 M4,2 L20,2 L20,22 L4,22 Z M8,6 L8,18 L16,18 L16,6 Z M8,12 L16,12 L16,18 L8,18 Z"
}
