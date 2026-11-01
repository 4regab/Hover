namespace Hover.Notch;

/// <summary>
/// What differs per system for the notch's window: where the main display is, where the pointer
/// is, who has the keyboard. The Windows app fills it in; tests use a fake.
/// </summary>
public interface INotchPlatform
{
    /// <summary>The main display's work area in device pixels, and its scale.</summary>
    (Rect Work, double Scale) Primary();

    /// <summary>Changes when the displays do.</summary>
    string Signature();

    (int X, int Y) Cursor();

    /// <summary>True while a mouse button is down.</summary>
    bool Buttons();

    void Place(Rect window);

    void Raise();

    /// <summary>The open office needs the keyboard; the resting notch must never take it.</summary>
    void SetAcceptsKeys(bool on);

    void RememberForeground();

    /// <summary>Hands the keyboard back to whatever had it before the notch took it.</summary>
    void RestoreForeground();

    void Focus();

    /// <summary>Everything outside the shape passes the pointer through; <paramref name="over"/> says the pointer is on the part that takes it.</summary>
    void SetHit(bool over);

    /// <summary>The foreground window is the notch or one it owns.</summary>
    bool ForegroundIsOurs();
}

/// <summary>The shape as it is drawn right now, in DIPs of the notch window.</summary>
public readonly record struct NotchShape(Frame Frame, double X0, double WindowWidth, double Openness, bool ShadowOn, bool ViewVisible, Size RestTarget);

/// <summary>
/// The notch's behaviour: one window at the top centre of the main display; the shape grows from
/// its resting size (island, card, or nothing) to the office by one openness value; hovering,
/// clicking and the shortcut open it. A port of the Go app's NotchCtl and the notch parts of its
/// shell. Run it from one thread (the UI thread).
/// </summary>
public sealed class NotchController
{
    /// <summary>The drop shadow's reach around the shape, for the pointer's hit test.</summary>
    public const double ShadowBlur = 24, ShadowDepth = 4;

    /// <summary>A display check at most every 2 s, as the Go app does.</summary>
    const long DisplayCheckMs = 2000;

    readonly INotchPlatform plat;
    readonly Func<double> clock;
    readonly HoverRules hover = new();
    readonly Openness open = new();
    readonly RestAnim restAnim = new();
    double lastDisplayCheck;
    bool hadFocus;

    public NotchController(INotchPlatform plat, Func<double> clockMs)
    {
        this.plat = plat;
        clock = clockMs;
        lastDisplayCheck = clockMs();
    }

    public NotchState State => hover.State;

    /// <summary>With animations off, sizes change at once.</summary>
    public bool Still { get; set; }

    /// <summary>A question waits (or voice's card is up): hovering does not open the office.</summary>
    public bool Asking { get; set; }

    public bool HoverOpens { get; set; } = true;

    /// <summary>A menu or popover is open over the office: a peek stays.</summary>
    public bool Popover { get; set; }

    public RestKind RestKind { get; set; }

    public OfficeSize Size { get; set; }

    public Size OpenSize { get; private set; } = new(1120, 440);

    public double Scale { get; private set; } = 1;

    public Rect Work { get; private set; } = new(0, 0, 1920, 1080);

    /// <summary>The window in device pixels.</summary>
    public Rect Window { get; private set; } = new(0, 0, 1, 1);

    /// <summary>The pointer is on the part of the window that takes it.</summary>
    public bool Over { get; private set; }

    public string Signature { get; private set; } = "";

    public double WindowWidth => OpenSize.W + 2 * Geometry.Pad;

    double Now => clock();

    public double OpennessNow => open.Value(Now);

    public bool Animating => open.Animating(Now) || restAnim.Animating(Now);

    /// <summary>The resting shape as drawn now.</summary>
    public Size Rest => restAnim.Value(Now);

    /// <summary>The size the resting shape is going to (where its content is laid out).</summary>
    public Size RestTarget => restAnim.Target;

    public void SetRest(Size to) => restAnim.Go(to, Now, Still);

    public Frame CurrentFrame => Geometry.FrameAt(OpennessNow, Rest, OpenSize, open.Closing);

    /// <summary>The shape, and tells the system whether the pointer is on it (click-through otherwise).</summary>
    public NotchShape Shape()
    {
        var t = OpennessNow;
        var f = Geometry.FrameAt(t, Rest, OpenSize, open.Closing);
        var winW = WindowWidth;
        // The shadow shows at rest and once fully open, not while the shape grows.
        var shadowOn = t <= 0.001 || !open.Animating(Now);
        plat.SetHit(Over);
        return new NotchShape(f, (winW - f.W) / 2, winW, t, shadowOn, t > 0.001 || hover.State != NotchState.Rest, RestTarget);
    }

    /// <summary>
    /// One window size for every state (resizing a layered window on each change makes it blink),
    /// at the size Settings asks for. Call it at start, when the displays change and when the size setting does.
    /// </summary>
    public void Layout()
    {
        var (work, scale) = plat.Primary();
        Work = work;
        Scale = scale;
        OpenSize = Geometry.OpenSize(Size, new Size(work.Width / scale, work.Height / scale));
        Window = Geometry.Placement(work, scale, OpenSize);
        plat.Place(Window);
        Signature = plat.Signature();
    }

    /// <summary>The pointer bookkeeping for one 50 ms poll, and what the hover rules say to do.</summary>
    public NotchAction Poll()
    {
        var (x, y) = plat.Cursor();
        var zone = Geometry.Zone(Work, Scale, Rest);
        var panel = Geometry.PanelZone(Work, Scale, OpenSize);
        var p = new Pointer(zone.Contains(x, y), panel.Contains(x, y), plat.Buttons(), Popover, HoverOpens && !Asking);
        var action = hover.Poll((long)Now, p);
        // The window takes the pointer only over the shape (and its shadow).
        var f = CurrentFrame;
        double dx = (x - Window.Left) / Scale, dy = (y - Window.Top) / Scale;
        Over = Window.Contains(x, y) && Geometry.Hittable(dx, dy, WindowWidth, f, ShadowBlur, ShadowDepth);
        return action;
    }

    /// <summary>
    /// The 50 ms tick: the pointer, click-away (the keyboard went to another app once the notch had
    /// it), and a look at the displays every 2 s. Returns what the host must do.
    /// </summary>
    public NotchAction Tick(out bool relayout)
    {
        var action = Poll();
        if (action == NotchAction.Peek) Expand(true, false);
        else if (action == NotchAction.Collapse) Collapse();

        if (hover.State == NotchState.Open)
        {
            if (plat.ForegroundIsOurs()) hadFocus = true;
            else if (hadFocus && !Popover) Collapse();
        }

        relayout = false;
        if (Now - lastDisplayCheck > DisplayCheckMs)
        {
            lastDisplayCheck = Now;
            if (hover.State == NotchState.Rest) plat.Raise();
            relayout = plat.Signature() != Signature;
        }
        return action;
    }

    /// <summary>A press anywhere in the notch: stop closing on pointer-leave.</summary>
    public void Stay()
    {
        if (hover.State == NotchState.Peek) hover.Opened(false);
    }

    /// <summary>The office comes in (560 ms, a touch past and back).</summary>
    public void Expand(bool peek, bool focus)
    {
        if (hover.State == NotchState.Rest)
        {
            plat.RememberForeground();
            plat.SetAcceptsKeys(true);
            plat.Raise();
            open.Go(1, Now);
            hadFocus = false;
        }
        hover.Opened(peek);
        if (focus) plat.Focus();
    }

    public void Collapse()
    {
        if (hover.State == NotchState.Rest) return;
        hover.Collapsed();
        plat.RestoreForeground();
        plat.SetAcceptsKeys(false);
        open.Go(0, Now);
    }

    /// <summary>The shortcut and the tray open the notch, or close it.</summary>
    public void Toggle()
    {
        if (hover.State == NotchState.Rest) Expand(false, true);
        else Collapse();
    }
}
