using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

readonly record struct PillSpec(string Text, string Icon, float PadX, float PadY, float FontSize, float Weight, Rgba Fg, Rgba Bg, bool Enabled, bool Chevron = false, bool Bold = false)
{
    /// <summary>A height set on the button (0: its own).</summary>
    public float H { get; init; }
}

public sealed partial class SettingsUi
{
    // MARK: A pill button (OwlBase and its styles): capped at half its height, lifted on hover and press

    Look PillFont(PillSpec p) => F(p.FontSize, p.Bold ? 600 : p.Weight);

    (float W, float H) PillRow(PillSpec p)
    {
        var look = PillFont(p);
        var tw = pen.Width(p.Text, look);
        var th = pen.LineHeight(look);
        float rw = tw, rh = th;
        var n = 1;
        if (p.Icon != "") { rw += p.FontSize - 1; rh = Math.Max(rh, p.FontSize - 1); n++; }
        if (p.Chevron) { rw += 11; rh = Math.Max(rh, 11); n++; }
        return (rw + 6 * (n - 1), rh);
    }

    (float W, float H) PillSize(PillSpec p)
    {
        var (rw, rh) = PillRow(p);
        var ph = Math.Max(24, rh + 2 * p.PadY);
        if (p.H > 0) ph = p.H;
        return (rw + 4 * p.PadX, ph);
    }

    /// <summary>Draws the button at (x, y), <paramref name="width"/> wide (0: its own), and makes it answer the pointer. Returns its size.</summary>
    (float W, float H) DrawPill(float x, float y, float width, PillSpec p, TK kind, string id, int n = 0, string s = "")
    {
        var (pw, ph) = PillSize(p);
        if (width <= 0) width = pw;
        var hover = p.Enabled && Hot(kind, id, n);
        var held = p.Enabled && Down(kind, id, n);
        Opacity(p.Enabled ? 1 : 0.35f, () =>
        {
            Box(x, y, width, ph, Math.Min(ph, width) / 2, p.Bg);
            var lift = held ? 0.16f : hover ? 0.08f : 0f;
            if (lift > 0) Box(x, y, width, ph, Math.Min(ph, width) / 2, A(p.Fg, lift * p.Fg.A / 255f));
            var (rw, _) = PillRow(p);
            var cx = x + p.PadX + Math.Max(0, (width - 2 * p.PadX - rw) / 2);
            if (p.Icon != "")
            {
                var size = p.FontSize - 1;
                Icon(p.Icon, cx, y + (ph - size) / 2 + 1, size, p.Fg);
                cx += size + 6;
            }
            var look = PillFont(p) with { Color = p.Fg };
            var tw = pen.Width(p.Text, look);
            if (p.Text.Length > 0) pen.TextIn(p.Text, new Rect4(cx, y, tw + 2, ph), look, left: true);
            cx += tw + 6;
            if (p.Chevron) Icon("chevron-down", cx, y + (ph - 11) / 2, 11, Dim);
        });
        if (p.Enabled) Add(new Rect4(x, y, width, ph), kind, id, n, s);
        return (width, ph);
    }

    PillSpec Standard(string text, bool enabled = true) => new(text, "", 14, 5, 13, 500, Ink, C(P.Fill), enabled);

    // MARK: Switch: a 42 x 26 track (green when on), a 22 white knob

    const float SwitchW = 42, SwitchH = 26;

    void DrawSwitch(float x, float y, bool on, bool enabled, string id)
    {
        Opacity(enabled ? 1 : 0.4f, () =>
        {
            Box(x, y, SwitchW, SwitchH, 13, on ? C(P.Green) : C(P.SwitchOff));
            var kx = x + (on ? 18 : 2);
            Box(kx, y + 3, 22, 22, 11, new Rgba(0, 0, 0, 28));
            Box(kx, y + 2, 22, 22, 11, White);
        });
        if (enabled) Add(new Rect4(x, y, SwitchW, SwitchH), TK.Switch, id, 0, "", on);
    }

    // MARK: Segments: a track, the picked one on a thumb

    static string SegLongest(IReadOnlyList<string> labels)
    {
        string best = "";
        var bestN = -1;
        foreach (var l in labels)
        {
            var n = 0;
            foreach (var r in l.EnumerateRunes())
            {
                var s = r.ToString();
                n += s.ToUpperInvariant() == s && s.ToLowerInvariant() != s ? 3 : 2;
            }
            if (n >= bestN) { best = l; bestN = n; }
        }
        return best;
    }

    (float SegW, float SegH) SegDims(string longest)
    {
        var look = F(12.5f, 600);
        return (Math.Max(58, pen.Width(longest, look) + 20), pen.LineHeight(look) + 8);
    }

    (float W, float H) SegSize(int count, string longest)
    {
        var (sw, sh) = SegDims(longest);
        return (sw * count + 4, sh + 4);
    }

    void DrawSegments(float x, float y, IReadOnlyList<string> labels, string longest, int picked, bool enabled, string id)
    {
        var (sw, sh) = SegDims(longest);
        var (tw, th) = SegSize(labels.Count, longest);
        Opacity(enabled ? 1 : 0.4f, () =>
        {
            Box(x, y, tw, th, 9, C(P.Wash));
            for (var i = 1; i < labels.Count; i++)
                if (picked != i && picked != i - 1) Box(x + 2 + i * sw - 0.5f, y + 8, 1, Math.Max(0, th - 16), 0, C(P.Separator));
            if (picked >= 0)
            {
                Box(x + 2 + picked * sw, y + 3, sw, sh, 7, new Rgba(0, 0, 0, 24));
                Box(x + 2 + picked * sw, y + 2, sw, sh, 7, C(P.Thumb));
            }
            for (var i = 0; i < labels.Count; i++)
            {
                var a = Down(TK.Seg, id, i) && picked != i ? 0.45f : 1f;
                var look = F(12.5f, picked == i ? 600 : 400, A(Ink, a));
                pen.TextIn(labels[i], new Rect4(x + 2 + i * sw, y + 2, sw, sh), look);
            }
        });
        // The one picked (or the first, when none is) is where Tab stops; the Up and Down keys step inside the group.
        if (enabled)
            for (var i = 0; i < labels.Count; i++) Add(new Rect4(x + 2 + i * sw, y + 2, sw, sh), TK.Seg, id, i, "", i == picked || (picked < 0 && i == 0));
    }

    // MARK: Slider: a whole number from lo to hi, told when the pointer lets go

    const float SliderH = 28;

    readonly Dictionary<string, (float X, float W, int Lo, int Hi)> sliders = [];

    int SliderAt(float pointerX, float x, float width, int lo, int hi)
    {
        var track = width - 16;
        return lo + (int)Math.Round(Math.Clamp(pointerX - x - 8, 0, track) / track * (hi - lo), MidpointRounding.AwayFromZero);
    }

    void DrawSlider(float x, float y, float width, int lo, int hi, int value, bool enabled, string id)
    {
        var dragging = sliderId == id;
        value = SliderShown(id, value);
        var shown = dragging ? sliderValue : value;
        var track = width - 16;
        var knob = track * (shown - lo) / Math.Max(1, hi - lo);
        Opacity(enabled ? 1 : 0.4f, () =>
        {
            Box(x + 8, y + 12, track, 4, 2, C(P.WashStrong));
            Box(x + 8, y + 12, knob, 4, 2, C(P.Blue));
            Box(x + knob, y + 7, 16, 16, 8, new Rgba(0, 0, 0, 40));
            Box(x + knob, y + 6, 16, 16, 8, C(P.Thumb));
        });
        sliders[id] = (x, width, lo, hi);
        if (enabled) Add(new Rect4(x, y, width, SliderH), TK.Slider, id, 0, value.ToString());
    }

    // MARK: Rings, tiles, marks

    void Ring(float x, float y, float bw, float bh, double? value, float stroke)
    {
        var r = Math.Min(bw, bh) / 2 - stroke / 2;
        float cx = x + bw / 2, cy = y + bh / 2;
        void Arc(double sweep, Rgba c)
        {
            if (sweep <= 0) return;
            var steps = Math.Max(8, (int)(sweep / 6));
            var pts = new List<(float, float)>();
            for (var i = 0; i <= steps; i++)
            {
                var a = (sweep * i / steps) * Math.PI / 180;
                pts.Add((cx + r * (float)Math.Sin(a), cy - r * (float)Math.Cos(a)));
            }
            pen.Cv.Line(pts, stroke, c);
        }
        Arc(360, C(P.WashStrong));
        if (value is not { } v) return;
        var tint = v < 70 ? C(P.Green) : v < 90 ? C(P.Orange) : C(P.Red);
        Arc(v >= 99.95 ? 360 : Math.Min(v, 100) / 100 * 360, tint);
    }

    Rgba TintColor(Tint t) => t switch
    {
        Tint.Gray => Rgba.Of(0x8e8e93),
        Tint.Bot => Rgba.Of(0x9b6bff),
        Tint.Purple => C(P.Purple),
        Tint.Green => C(P.Green),
        Tint.Blue => C(P.Blue),
        Tint.Orange => C(P.Orange),
        Tint.Teal => C(P.Teal),
        _ => Rgba.Of(0xff375f),
    };

    void DrawTile(float x, float y, string icon, Tint tint)
    {
        Box(x, y, 24, 24, 7, TintColor(tint));
        Icon(icon, x + 5, y + 5, 14, White);
    }
}
