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
    public bool Animating => FocusId is not null || barDragging || sliderId is not null || ToastLive;

    /// <summary>A toast is showing, so a frame is due when it ends (it would stay on the screen until something else drew).</summary>
    bool ToastLive => ctl.ToastText is not null && (DateTime.UtcNow - ctl.ToastAt).TotalSeconds < ToastSeconds + 0.5;

    const double ToastSeconds = 5;

    public double NextFrameMs => 250;

    public void Invalidate() => Dirty = true;

    /// <summary>The places the last frame drew that answer the pointer, for the checks of the real app: (kind, id, rectangle).</summary>
    public IEnumerable<(string Kind, string Id, Rect4 R)> Targets => hits.Select(t => (t.K.ToString(), t.Id, t.Clip));

    /// <summary>The same places with their index (the section, the segment, the tile), for the checks.</summary>
    public IEnumerable<(string Kind, string Id, int N, Rect4 R)> IndexedTargets => hits.Select(t => (t.K.ToString(), t.Id, t.N, t.Clip));

    /// <summary>The lines of text the last frame drew.</summary>
    public IEnumerable<string> Texts => pen.Drawn;

    // MARK: Colours and text

    static Rgba C(uint argb) => new((byte)(argb >> 16), (byte)(argb >> 8), (byte)argb, (byte)(argb >> 24));

    static Rgba A(Rgba c, float f) => c with { A = (byte)Math.Clamp(c.A * f + 0.5f, 0, 255) };

    static readonly Rgba White = new(255, 255, 255);

    Rgba Ink => inkText;

    /// <summary>The lines of text the last frame drew faded (a row that is switched off), for the checks.</summary>
    public IEnumerable<string> FadedTexts => fadedLog;

    readonly List<string> fadedLog = [];
    Rgba Faint => C(P.InkFaint);

    // The secondary text colour, the link blue, the red of a message and the amber of a warning, each made readable (4.5 to 1) on the
    // surfaces it is drawn on, whatever the palette is: Apple's secondary label on white is 3.3 to 1, the accent colours of a
    // light theme are lighter still. A palette that already reads well keeps its own colour.
    Rgba inkText, dimInk, linkInk, redInk, amberInk, selectionFill, redFill;
    AppPalette? inkFor;

    Rgba Dim => dimInk;
    Rgba LinkInk => linkInk;
    Rgba RedInk => redInk;
    Rgba AmberInk => amberInk;

    /// <summary>The fill of the picked section and of the option the pointer is on: the accent, darkened until white words read on it.</summary>
    Rgba SelectionFill => selectionFill;

    void MakeInks()
    {
        if (ReferenceEquals(inkFor, P)) return;
        inkFor = P;
        var panel = C(P.Panel);
        var surface = C(P.Surface);
        var sheet = C(P.Sheet);
        var wash = Over(C(P.Wash), surface);
        var fill = Over(C(P.Fill), surface);
        var badge = Over(A(C(P.Ink), 0.07f), surface);
        // The places a button sits on: the page, a card, a menu, a box, a button (also lit by the pointer), the picked segment.
        var lit = Over(A(C(P.Ink), 0.16f), fill);
        var backs = new[] { panel, surface, sheet, wash, fill, badge, lit, Over(C(P.Fill), panel), Over(C(P.Wash), panel), Over(C(P.WashStrong), surface), C(P.Thumb), Over(C(P.RowHover), surface) };
        // A theme's own text colour is kept unless it is too close to its background (Solarized's is 4.1 to 1).
        inkText = Legible(C(P.Ink), 4.5, backs);
        dimInk = Legible(C(P.InkDim), 4.5, backs);
        linkInk = Legible(C(P.Blue), 4.5, panel, surface);
        var redTint = Over(A(C(P.Red), 0.07f), surface);
        redInk = Legible(C(P.Red), 4.5, panel, surface, fill, redTint);
        var amberTint = Over(Rgba.Of(0xff9f0a, 0.15), surface);
        amberInk = Legible(Rgba.Of(0xffb340), 4.5, panel, surface, amberTint);
        selectionFill = Legible(White, C(P.Blue), 4.5);
        redFill = Legible(White, C(P.Red), 4.5);
    }

    // MARK: Contrast (WCAG 2): the ratio of two opaque colours' luminance

    static double Channel(byte v)
    {
        var c = v / 255.0;
        return c <= 0.03928 ? c / 12.92 : Math.Pow((c + 0.055) / 1.055, 2.4);
    }

    internal static double Luminance(Rgba c) => 0.2126 * Channel(c.R) + 0.7152 * Channel(c.G) + 0.0722 * Channel(c.B);

    /// <summary>A colour laid over an opaque one, as the screen shows it.</summary>
    internal static Rgba Over(Rgba top, Rgba under)
    {
        var a = top.A / 255.0;
        byte Mix(byte t, byte u) => (byte)Math.Round(t * a + u * (1 - a));
        return new Rgba(Mix(top.R, under.R), Mix(top.G, under.G), Mix(top.B, under.B));
    }

    internal static double Ratio(Rgba a, Rgba b)
    {
        double la = Luminance(a), lb = Luminance(b);
        if (la < lb) (la, lb) = (lb, la);
        return (la + 0.05) / (lb + 0.05);
    }

    /// <summary>The text colour, made more opaque and then darker or lighter (away from the surfaces) until it reads on each of them.</summary>
    internal static Rgba Legible(Rgba fg, double need, params Rgba[] surfaces)
    {
        var c = fg;
        var lightSurface = surfaces.Average(s => Luminance(s)) > 0.4;
        for (var i = 0; i < 60; i++)
        {
            if (surfaces.All(s => Ratio(Over(c, s), s) >= need)) return c;
            if (c.A < 255) c = c with { A = (byte)Math.Min(255, c.A + 10) };
            else
            {
                var to = lightSurface ? 0 : 255;
                byte Step(byte v) => (byte)Math.Round(v + (to - v) * 0.06);
                c = new Rgba(Step(c.R), Step(c.G), Step(c.B));
            }
        }
        return c;
    }

    /// <summary>The background itself, moved away from white until white words read on it: for a fill that carries white text.</summary>
    internal static Rgba Legible(Rgba white, Rgba background, double need)
    {
        var c = background;
        for (var i = 0; i < 60 && Ratio(white, c) < need; i++)
        {
            byte Step(byte v) => (byte)Math.Round(v * 0.94);
            c = new Rgba(Step(c.R), Step(c.G), Step(c.B), background.A);
        }
        return c;
    }

    Look F(float size, float weight = 400, Rgba? color = null) => new(size, 1.3f, color ?? Ink, weight);

    Look Mono(float size, Rgba color) => new(size, 1.3f, color, 400, FontFamily.Mono);

    float Tl(float size) => pen.LineHeight(F(size));

    float TextW(string s, float size, float weight = 400) => pen.Width(s, F(size, weight));

    float SubH(string s, float width) => s.Length == 0 ? 0 : pen.HeightWhenWrapped(s, width, F(11.5f, 400, Dim));

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
            foreach (var t in faded.Drawn) if (outer.Drawn.Count < 2000) { outer.Drawn.Add(t); fadedLog.Add(t); }
            faded.Drawn.Clear();
            cv.PopOpacity();
        }
    }

    // MARK: Hits

    Rect4 clip = new(-1e6f, -1e6f, 2e6f, 2e6f);

    void Add(Rect4 r, TK k, string id, int n = 0, string s = "", bool on = false)
    {
        // Tab reaches it whether or not it is in view; the page scrolls to it.
        if (IsStop(k)) stops.Add(new Stop(k, id, n, r, s, on));
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
        fadedLog.Clear();
        hits.Clear();
        stops.Clear();
        drawnFields.Clear();
        sliders.Clear();
        w = width;
        h = height;
        now = nowMs;
        P = ctl.Palette;
        MakeInks();
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
            ctl.StopRecording();
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
        AfterPaint();
        if (ctl.ToastText is { } toast && (DateTime.UtcNow - ctl.ToastAt).TotalSeconds < ToastSeconds) Toast(toast);
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
