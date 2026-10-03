import AppKit
import QuartzCore

/// The notch growing into Hover's window and the window folding back into the notch,
/// as an app opens from the Dock: one picture of the office (or the office's dark
/// floor while there is none), carried by Core Animation from one frame to the other
/// in a click-through window over everything, then handed over to the real view. The
/// office's web views are never resized frame by frame (that re-lays out the page).
final class Zoom {
    private let window: NSWindow
    private let shape = CALayer(), picture = CALayer()
    private var finished = false
    private let screenFrame: CGRect

    /// The office's floor: what shows before a picture is there.
    static let floor = NSColor(srgbRed: 0.027, green: 0.02, blue: 0.039, alpha: 1)

    init(on screen: NSScreen) {
        screenFrame = screen.frame
        window = NSWindow(contentRect: screen.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isOpaque = false; window.backgroundColor = .clear; window.hasShadow = false
        window.ignoresMouseEvents = true; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        // Over the notch (.statusBar) and the window it becomes.
        window.level = NSWindow.Level(rawValue: NSWindow.Level.statusBar.rawValue + 1)
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle, .transient]
        let root = NSView(frame: CGRect(origin: .zero, size: screen.frame.size)); root.wantsLayer = true
        window.contentView = root
        shape.backgroundColor = Zoom.floor.cgColor
        shape.masksToBounds = true
        shape.cornerCurve = .continuous
        picture.contentsGravity = .resizeAspectFill
        picture.masksToBounds = true
        shape.addSublayer(picture)
        // The window's shadow, drawn under the shape (not clipped by it).
        let shadow = CALayer()
        shadow.shadowColor = NSColor.black.cgColor; shadow.shadowOpacity = 0.45; shadow.shadowRadius = 28; shadow.shadowOffset = CGSize(width: 0, height: -14)
        root.layer?.addSublayer(shadow)
        root.layer?.addSublayer(shape)
        self.shadow = shadow
    }
    private let shadow: CALayer

    /// A screen rect (bottom-left origin) in the zoom window's own coordinates.
    private func local(_ r: CGRect) -> CGRect { r.offsetBy(dx: -screenFrame.minX, dy: -screenFrame.minY) }

    /// Runs from one screen rect to another. The picture fills the shape (cropped, never
    /// stretched out of shape); `fade` fades it out as it lands (folding into the notch).
    func run(image: NSImage?, from: CGRect, to: CGRect, radius: (CGFloat, CGFloat), duration: CFTimeInterval, fade: Bool = false, landed: @escaping () -> Void) {
        let a = local(from), b = local(to)
        CATransaction.begin(); CATransaction.setDisableActions(true)
        if let image, let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) { picture.contents = cg }
        set(a, radius: radius.0)
        CATransaction.commit()
        window.orderFrontRegardless()
        // Ease out, as AppKit's own window zoom: quick away, gentle landing.
        let curve = CAMediaTimingFunction(controlPoints: 0.2, 0.9, 0.22, 1)
        CATransaction.begin()
        CATransaction.setAnimationDuration(duration)
        CATransaction.setAnimationTimingFunction(curve)
        CATransaction.setCompletionBlock { landed() }
        for (layer, key, value) in [(shape, "bounds", NSValue(rect: CGRect(origin: .zero, size: b.size))), (shape, "position", NSValue(point: CGPoint(x: b.midX, y: b.midY))),
                                    (picture, "bounds", NSValue(rect: CGRect(origin: .zero, size: b.size))), (picture, "position", NSValue(point: CGPoint(x: b.width / 2, y: b.height / 2))),
                                    (shadow, "bounds", NSValue(rect: CGRect(origin: .zero, size: b.size))), (shadow, "position", NSValue(point: CGPoint(x: b.midX, y: b.midY)))] as [(CALayer, String, NSValue)] {
            let anim = CABasicAnimation(keyPath: key)
            anim.fromValue = layer.value(forKey: key); anim.toValue = value
            layer.add(anim, forKey: key)
            layer.setValue(value, forKey: key)
        }
        let corner = CABasicAnimation(keyPath: "cornerRadius"); corner.fromValue = radius.0; corner.toValue = radius.1
        shape.add(corner, forKey: "cornerRadius"); shape.cornerRadius = radius.1
        let path = CABasicAnimation(keyPath: "shadowPath")
        let toPath = CGPath(roundedRect: CGRect(origin: .zero, size: b.size), cornerWidth: radius.1, cornerHeight: radius.1, transform: nil)
        path.fromValue = shadow.shadowPath; path.toValue = toPath
        shadow.add(path, forKey: "shadowPath"); shadow.shadowPath = toPath
        if fade {
            let o = CAKeyframeAnimation(keyPath: "opacity"); o.values = [1, 1, 0]; o.keyTimes = [0, 0.55, 1]
            for l in [shape, shadow] { l.add(o, forKey: "opacity"); l.opacity = 0 }
        }
        CATransaction.commit()
    }

    private func set(_ r: CGRect, radius: CGFloat) {
        shape.frame = r; shadow.frame = r
        picture.frame = CGRect(origin: .zero, size: r.size)
        shape.cornerRadius = radius
        shadow.shadowPath = CGPath(roundedRect: CGRect(origin: .zero, size: r.size), cornerWidth: radius, cornerHeight: radius, transform: nil)
    }

    /// Hands over to what is under it: a short fade, then the zoom window goes.
    func finish(fade: CFTimeInterval = 0.16) {
        guard !finished else { return }
        finished = true
        NSAnimationContext.runAnimationGroup({ c in c.duration = fade; window.animator().alphaValue = 0 }, completionHandler: { [window] in window.orderOut(nil) })
    }
}
