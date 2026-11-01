using Hover.Chat;
using Hover.Chat.Ui;

namespace Hover.Settings;

/// <summary>What the page needs of the window that shows it.</summary>
public sealed class SettingsUiEnv
{
    /// <summary>The way back to the office (the "Agent office" pill, and Esc in the app window).</summary>
    public required Action Back { get; init; }

    /// <summary>The notch folds (its close button, and Esc in the notch).</summary>
    public Action Fold { get; init; } = () => { };

    public required IClipboard Clipboard { get; init; }

    /// <summary>The page is in the notch (rounded, with a close button) rather than the app window.</summary>
    public bool InNotch { get; init; }

    /// <summary>Opens an address in the browser.</summary>
    public Action<string>? OpenUrl { get; init; }

    /// <summary>The tool's logo, drawn by the canvas as <c>mark:id</c>.</summary>
    public Func<DateTime> Utc { get; init; } = () => DateTime.UtcNow;
}

enum TK { Back, Close, Section, Switch, Button, Shortcut, Seg, Slider, Picker, Field, FieldRemove, Tile, Link, Open, Hold, MenuItem, MenuOutside, Mini, Act, Bar, Pane }

/// <summary>A place that answers the pointer in the last frame. <see cref="Clip"/> is the part of it that is in view.</summary>
readonly record struct Hit(Rect4 R, Rect4 Clip, TK K, string Id, int N = 0, string S = "", bool On = false);

/// <summary>
/// Settings drawn on the panels' canvas: a sidebar of the sections beside one scrolling pane of grouped rows, the
/// picker's menu over it (<c>internal/ui/settings.go</c>, <c>overlay.go</c>, <c>mcp.go</c>). It draws the blocks the controller
/// builds, reads the pointer and the keys against the places the last frame drew, and hands what they mean to the controller.
/// </summary>
public sealed partial class SettingsUi
{
    // The pen in use: the plain one, or while a faded row draws, one that shapes text in faded colours.
    Pen pen;
    readonly Pen plainPen;
    readonly IShaper shaper;
    readonly Dictionary<int, Pen> fadedPens = [];
    readonly DimCanvas dim = new();
    readonly SettingsController ctl;
    readonly SettingsUiEnv env;
    readonly List<Hit> hits = [];
    float w, h;
    double now;
    AppPalette P = Palettes.HoverDark;

    // The pointer and what it holds.
    float px = -1, py = -1;
    Hit? hot, pressed;
    int sliderValue;
    string? sliderId;
    bool barDragging;
    string barDragId = "";
    float barGrab, barStartOff;
    bool pointerDown;

    // The two scroll areas.
    float sideOff, paneOff, sideContent, paneContent, sideView, paneView;
    Rect4 sideRect, paneRect;
    int lastSection = -1;
    string lastProject = "";

    public SettingsUi(IShaper shaper, SettingsController controller, SettingsUiEnv env)
    {
        this.shaper = shaper;
        pen = plainPen = new Pen(shaper);
        ctl = controller;
        this.env = env;
        ctl.Changed += () => Dirty = true;
    }

    /// <summary>Something changed since the last frame, so a new one is worth drawing.</summary>
    public bool Dirty { get; private set; } = true;

    /// <summary>A caret is blinking or the pointer is held on something that moves: frames keep coming.</summary>
    public bool Animating => FocusId is not null || barDragging || sliderId is not null;

    public double NextFrameMs => 250;

    public void Invalidate() => Dirty = true;

    /// <summary>The places the last frame drew that answer the pointer, for the checks of the real app: (kind, id, rectangle).</summary>
    public IEnumerable<(string Kind, string Id, Rect4 R)> Targets => hits.Select(t => (t.K.ToString(), t.Id, t.Clip));

    /// <summary>The lines of text the last frame drew.</summary>
    public IEnumerable<string> Texts => pen.Drawn;

    // MARK: Colours and text

    static Rgba C(uint argb) => new((byte)(argb >> 16), (byte)(argb >> 8), (byte)argb, (byte)(argb >> 24));

    static Rgba A(Rgba c, float f) => c with { A = (byte)Math.Clamp(c.A * f + 0.5f, 0, 255) };

    static readonly Rgba White = new(255, 255, 255);

    Rgba Ink => C(P.Ink);
    Rgba Dim => C(P.InkDim);
    Rgba Faint => C(P.InkFaint);

    Look F(float size, float weight = 400, Rgba? color = null) => new(size, 1.3f, color ?? Ink, weight);

    Look Mono(float size, Rgba color) => new(size, 1.3f, color, 400, FontFamily.Mono);

    float Tl(float size) => pen.LineHeight(F(size));

    float TextW(string s, float size, float weight = 400) => pen.Width(s, F(size, weight));

    float SubH(string s, float width) => s.Length == 0 ? 0 : pen.WrappedHeight(s, width, F(11.5f, 400, Dim));

    float Sub(string s, float x, float y, float width) => pen.Wrapped(s, x, y, width, F(11.5f, 400, Dim));

    void Box(float x, float y, float bw, float bh, float r, Rgba c) => pen.Box(x, y, bw, bh, r, c);

    void Frame(float x, float y, float bw, float bh, float r, float width, Rgba c) => pen.Border(x, y, bw, bh, r, width, c);

    void Icon(string name, float x, float y, float size, Rgba c, float stroke = 2)
        => pen.Icon(SettingsIcons.Body(name), x, y, size, pen.Cv is DimCanvas d && d.Opacity < 1 ? A(c, d.Opacity) : c, stroke);

    void Opacity(float a, Action draw)
    {
        if (a >= 1) { draw(); return; }
        var outer = pen;
        var cv = outer.Cv;
        cv.PushOpacity(a);
        // The app's canvas draws a text layout in the colours it was shaped with, so the faded text is shaped faded: by a pen of its
        // own for each opacity (it keeps its own cache of lines).
        var key = (int)Math.Round(dim.Opacity * 1000);
        if (!fadedPens.TryGetValue(key, out var faded)) fadedPens[key] = faded = new Pen(new FadedShaper(shaper, dim.Opacity));
        faded.Cv = dim;
        pen = faded;
        try { draw(); }
        finally
        {
            pen = outer;
            foreach (var t in faded.Drawn) if (outer.Drawn.Count < 2000) outer.Drawn.Add(t);
            faded.Drawn.Clear();
            cv.PopOpacity();
        }
    }

    // MARK: Hits

    Rect4 clip = new(-1e6f, -1e6f, 2e6f, 2e6f);

    void Add(Rect4 r, TK k, string id, int n = 0, string s = "", bool on = false)
    {
        var c = Intersect(r, clip);
        if (c.W <= 0 || c.H <= 0) return;
        hits.Add(new Hit(r, c, k, id, n, s, on));
    }

    static Rect4 Intersect(Rect4 a, Rect4 b)
    {
        var x = Math.Max(a.X, b.X);
        var y = Math.Max(a.Y, b.Y);
        var r = Math.Min(a.Right, b.Right);
        var bt = Math.Min(a.Bottom, b.Bottom);
        return new Rect4(x, y, Math.Max(0, r - x), Math.Max(0, bt - y));
    }

    Hit? HitAt(float x, float y)
    {
        // The last one drawn is on top.
        for (var i = hits.Count - 1; i >= 0; i--)
            if (hits[i].Clip.Contains(x, y)) return hits[i];
        return null;
    }

    bool Hot(TK k, string id, int n = 0) => hot is { } t && t.K == k && t.Id == id && t.N == n;

    bool Down(TK k, string id, int n = 0) => pointerDown && pressed is { } t && t.K == k && t.Id == id && t.N == n && Hot(k, id, n);

    void Clipped(Rect4 r, Action draw)
    {
        var old = clip;
        clip = Intersect(clip, r);
        pen.Cv.PushClip(r);
        try { draw(); }
        finally
        {
            pen.Cv.PopClip();
            clip = old;
        }
    }

    // MARK: A frame

    /// <summary>Draws a frame in a w x h box at the origin.</summary>
    public void Paint(ICanvas canvas, float width, float height, double nowMs)
    {
        dim.Inner = canvas;
        pen.Cv = dim;
        plainPen.Drawn.Clear();
        hits.Clear();
        sliders.Clear();
        w = width;
        h = height;
        now = nowMs;
        P = ctl.Palette;
        Dirty = false;
        clip = new Rect4(-1e6f, -1e6f, 2e6f, 2e6f);
        var radius = env.InNotch ? 32f : 0f;
        Box(0, 0, w, h, radius, C(P.Panel));

        var section = (int)ctl.Section;
        if (section != lastSection || ctl.Project != lastProject)
        {
            // Another section opens at its top.
            if (section != lastSection || ctl.Project != lastProject) paneOff = 0;
            if (section != lastSection) RevealSection(section);
            lastSection = section;
            lastProject = ctl.Project;
            Unfocus(commit: false);
        }

        var backSpec = new PillSpec("Agent office", "chevron-left", 9, 4, 13, 500, Ink, C(P.Fill), true);
        var (bw, bh) = PillSize(backSpec);
        DrawPill(10, 10, 0, backSpec, TK.Back, "back");
        var rowH = bh;
        if (env.InNotch)
        {
            var close = new PillSpec("", "close", 7, 7, 13, 500, Dim, C(P.Fill), true) { H = 28 };
            DrawPill(w - 12 - 28, 10, 28, close, TK.Close, "close");
            rowH = Math.Max(rowH, 28);
        }
        var top = 10 + rowH + 8;
        Page(0, top, w, h - top);
        if (ctl.Menu is { } menu) DrawMenu(menu);
        if (ctl.ToastText is { } toast && (DateTime.UtcNow - ctl.ToastAt).TotalSeconds < 5) Toast(toast);
    }

    void Toast(string text)
    {
        var look = F(12.5f, 400, White);
        var tw = Math.Min(pen.Width(text, look), w - 60);
        var bx = (w - tw - 28) / 2;
        var by = h - 44;
        Box(bx, by, tw + 28, 30, 15, new Rgba(30, 30, 34, 235));
        pen.TextIn(text, new Rect4(bx, by, tw + 28, 30), look, pad: 14);
    }
}
