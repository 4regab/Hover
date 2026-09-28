using System.Windows;
using System.Windows.Media;

namespace Hover.Owl;

/// Hover's Kiro mascot, drawn flat for the places WPF shows it (the first-use
/// note, the missing-WebView2 page): a boxy head, a dark visor with two pixel eyes, headphone pads,
/// and an antenna bulb in the colour of what it's doing. The office draws the same
/// bot in 3D (web/office).
///
/// Live, it works: it bobs, its eyes scan a line and drop to the keys, it blinks,
/// and its bulb pulses with a soft glow. Finished, it hops once with happy eyes and
/// a green bulb. Both run from Animator, and draw still with Windows' animations off.
internal sealed class BotGlyph : FrameworkElement
{
    /// The bots' colours, one per desk, in the office's order.
    public static readonly Color[] Colors =
    {
        Color.FromRgb(0x9B, 0x6B, 0xFF), Color.FromRgb(0x2F, 0xC9, 0xB0), Color.FromRgb(0xFF, 0x9A, 0x4A),
        Color.FromRgb(0xFF, 0x6F, 0xAE), Color.FromRgb(0x5A, 0xA8, 0xFF), Color.FromRgb(0xB4, 0xE0, 0x4A),
    };
    public static readonly string[] Names = { "Pip", "Juno", "Moss", "Nova", "Ada", "Rue" };
    public static readonly Color Purple = Colors[0];

    private static readonly Brush Visor = Freeze(new SolidColorBrush(Color.FromRgb(0x12, 0x10, 0x18)));
    private static readonly Brush Eye = Freeze(new SolidColorBrush(Color.FromRgb(0xAA, 0xF6, 0xFF)));
    private static readonly Color Working = Color.FromRgb(0xFF, 0xD2, 0x4A), Done = Color.FromRgb(0x4A, 0xDE, 0x80);

    private readonly Brush _body, _dark;
    private double _t, _since;

    public BotGlyph() : this(Purple) { }

    public BotGlyph(Color body)
    {
        _body = Freeze(new SolidColorBrush(body));
        _dark = Freeze(new SolidColorBrush(Shade(body, 0.55)));
        // A glow needs room around the head; it may spill past the bounds.
        ClipToBounds = false;
        Loaded += (_, _) => { if (Live || Finished) Animator.Add(this); };
        Unloaded += (_, _) => Animator.Remove(this);
    }

    /// At work: bobbing, reading, typing, pulsing.
    public bool Live { get; init; }
    /// A task has finished: happy eyes, a green bulb, one hop.
    public bool Finished { get; init; }

    /// Start the finished hop again, when a new task has ended.
    public void Cheer() => _since = 0;

    internal void Step(double dt)
    {
        _t += dt;
        _since += dt;
        InvalidateVisual();
    }

    protected override void OnRender(DrawingContext dc)
    {
        var w = ActualWidth;
        var h = ActualHeight;
        if (w <= 0 || h <= 0) return;
        var s = Math.Min(w, h / 0.9);
        var still = Animator.Still || !(Live || Finished);
        var t = still ? 0 : _t;

        // Motion: a bob while it works; a hop and a squash when it has finished.
        double dy = 0, squash = 0;
        if (Live) dy = Math.Sin(t * 5.2) * s * 0.035;
        if (Finished && !still && _since < 0.9)
        {
            var k = _since / 0.9;
            dy = -Math.Sin(k * Math.PI) * s * 0.18;
            squash = k > 0.8 ? Math.Sin((k - 0.8) / 0.2 * Math.PI) * 0.08 : 0;
        }
        var x = (w - s) / 2;
        var top = h - s * 0.78 + dy + s * squash * 0.78;
        var headH = s * 0.78 * (1 - squash);

        // The antenna, and its bulb pulsing inside a soft halo.
        var pulse = Live && !still ? 0.5 + 0.5 * Math.Sin(t * 6) : 1;
        var bulbColor = Finished ? Done : Working;
        var bulb = new Rect(x + s * 0.6, top - s * 0.2, s * 0.14, s * 0.14);
        if (Live || Finished)
        {
            var halo = new RadialGradientBrush(Color.FromArgb((byte)(90 + 110 * pulse), bulbColor.R, bulbColor.G, bulbColor.B), Color.FromArgb(0, bulbColor.R, bulbColor.G, bulbColor.B));
            halo.Freeze();
            var c = new Point(bulb.X + bulb.Width / 2, bulb.Y + bulb.Height / 2);
            dc.DrawEllipse(halo, null, c, s * 0.26, s * 0.26);
        }
        dc.DrawRectangle(_dark, null, new Rect(x + s * 0.64, top - s * 0.1, s * 0.06, s * 0.12));
        var bulbBrush = new SolidColorBrush(Mix(bulbColor, Colors[0], Live ? 0.25 * (1 - pulse) : 0));
        bulbBrush.Freeze();
        dc.DrawRoundedRectangle(bulbBrush, null, bulb, s * 0.03, s * 0.03);

        // Headphone pads, head and visor.
        dc.DrawRoundedRectangle(_dark, null, new Rect(x, top + headH * 0.28, s, headH * 0.44), s * 0.05, s * 0.05);
        dc.DrawRoundedRectangle(_body, null, new Rect(x + s * 0.07, top, s * 0.86, headH), s * 0.18, s * 0.18);
        var visor = new Rect(x + s * 0.18, top + headH * 0.26, s * 0.64, headH * 0.51);
        dc.DrawRoundedRectangle(Visor, null, visor, s * 0.1, s * 0.1);

        // Eyes. Working, they read along a line, then drop to the keys; now and then a blink.
        double look = 0, down = 0, blink = 1;
        if (Live && !still)
        {
            var c = t % 4.2;
            if (c < 2.4) look = -0.05 + 0.1 * (c / 2.4);          // reading along
            else if (c < 3.6) { look = 0.02; down = 0.05; }          // typing
            else look = 0.05 - 0.07 * ((c - 3.6) / 0.6);             // back to the start
            if (t % 3.3 < 0.12) blink = 0.15;
        }
        foreach (var ex in new[] { 0.34, 0.58 })
        {
            if (Finished)
            {
                // ^ ^
                var pen = new Pen(Eye, s * 0.07) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round };
                pen.Freeze();
                var cx = x + s * (ex + 0.045);
                var cy = visor.Y + visor.Height * 0.55;
                dc.DrawLine(pen, new Point(cx - s * 0.06, cy + s * 0.04), new Point(cx, cy - s * 0.03));
                dc.DrawLine(pen, new Point(cx, cy - s * 0.03), new Point(cx + s * 0.06, cy + s * 0.04));
                continue;
            }
            var eh = visor.Height * 0.5 * blink;
            var ey = visor.Y + visor.Height * 0.25 + (visor.Height * 0.5 - eh) / 2 + s * down;
            dc.DrawRoundedRectangle(Eye, null, new Rect(x + s * (ex + look), ey, s * 0.09, Math.Max(0.6, eh)), s * 0.02, s * 0.02);
        }
    }

    private static Color Shade(Color c, double k) => Color.FromRgb((byte)(c.R * k), (byte)(c.G * k), (byte)(c.B * k));
    private static Color Mix(Color a, Color b, double k) =>
        Color.FromRgb((byte)(a.R + (b.R - a.R) * k), (byte)(a.G + (b.G - a.G) * k), (byte)(a.B + (b.B - a.B) * k));

    internal static T Freeze<T>(T f) where T : Freezable
    {
        f.Freeze();
        return f;
    }
}

/// One 30 fps clock for the notch's little animations, hooked into rendering only
/// while one of them is on screen, and off entirely with Windows' animations off.
internal static class Animator
{
    private static readonly List<FrameworkElement> On = new();
    private static TimeSpan _last;
    private static double _acc;

    public static bool Still => !SystemParameters.ClientAreaAnimation;

    public static void Add(FrameworkElement e)
    {
        if (Still || On.Contains(e)) return;
        On.Add(e);
        if (On.Count == 1) { _last = TimeSpan.Zero; System.Windows.Media.CompositionTarget.Rendering += Tick; }
    }

    public static void Remove(FrameworkElement e)
    {
        if (!On.Remove(e) || On.Count > 0) return;
        System.Windows.Media.CompositionTarget.Rendering -= Tick;
    }

    private static void Tick(object? sender, EventArgs e)
    {
        var now = ((RenderingEventArgs)e).RenderingTime;
        var dt = _last == TimeSpan.Zero ? 0 : (now - _last).TotalSeconds;
        _last = now;
        _acc += Math.Min(dt, 0.1);
        if (_acc < 1 / 31.0) return;
        var step = _acc;
        _acc = 0;
        foreach (var el in On.ToArray())
        {
            // A notch folded away hides its pill; nothing there needs drawing.
            if (!el.IsVisible) continue;
            if (el is BotGlyph b) b.Step(step);
            else if (el is LiveMark m) m.Step(step);
            else if (el is MarkStack k) k.StepBy(step);
        }
    }
}
