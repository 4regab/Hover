using System.Diagnostics;
using System.Numerics;
using System.Text.Json;
using Hover.Host;
using Hover.Host.Protocol;
using Hover.Office;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Vortice.Direct3D11;
using Vortice.DirectComposition;
using Vortice.DXGI;

namespace Hover.Windowing;

/// <summary>Where the office is being shown.</summary>
public enum OfficeTarget
{
    /// <summary>Nowhere: the notch is folded and the app window is not showing.</summary>
    None,
    /// <summary>The open notch, in the rectangle <c>NotchScene.OfficeArea</c> gives.</summary>
    Notch,
    /// <summary>The app window, in <c>DashboardWindow.OfficeArea</c>.</summary>
    Dashboard,
}

/// <summary>A rectangle in device-independent pixels.</summary>
public readonly record struct OfficeRect(double X, double Y, double W, double H);

/// <summary>What the pointer is on in the office: Kind 1 a bot, 2 a desk (Id is its session), 0 a prop or nothing; Hint is a prop's.</summary>
public readonly record struct OfficeTip(int Kind, int Id, string Hint);

/// <summary>What the user clicked on the wall or the floor, besides a bot or a desk.</summary>
public enum OfficeProp
{
    /// <summary>The TV on the right wall: the overview panel ("tv").</summary>
    Tv,
    /// <summary>The session board on the back wall ("board").</summary>
    Board,
    /// <summary>The shelf: the history panel ("history").</summary>
    History,
    /// <summary>The door: a new task.</summary>
    Door,
    /// <summary>The wall clock: the date toast.</summary>
    Clock,
}

/// <summary>
/// The 3D office as the app uses it: one office, shown in the open notch or, when the notch is
/// folded, in the app window. It follows the backend's <c>state</c> messages by itself and keeps
/// the user's camera and time of day in <c>office.json</c> (the file the Go and Mac builds share).
///
/// <para>The office draws the 3D scene only. The top bar, menus, chat, asks and cards are drawn over
/// it by their own code, which uses this class like so (everything on the UI thread):</para>
/// <list type="bullet">
/// <item><see cref="DeskPicked"/>, <see cref="BotPicked"/> and <see cref="PropPicked"/>: the user
/// clicked a desk with a session at it, a bot, or a prop. The first two give the session's <c>id</c>
/// from the backend's <c>state</c> message; a desk also gives where it was clicked.</item>
/// <item><see cref="BotScreenRect"/>: where a session's bot is on screen, to draw a prompt over it.</item>
/// <item><see cref="OpenDrawer"/>, <see cref="CloseDrawer"/>, <see cref="ShowPanel"/> and
/// <see cref="MarkDeskOpen"/> tell the office what is open over it: the camera goes to that bot or prop,
/// the bot shows as hot, and the wheel and drag stop moving the view while a drawer or panel is open.</item>
/// <item><see cref="Target"/>: which surface shows the office now.</item>
/// </list>
/// Rules: a click on the window prop (the sky) switches day and night itself. Frames are made only when
/// something moves, as the Go office does (10 a second when quiet, 60 when busy), and none while nothing shows it.
/// </summary>
public sealed class OfficeHost : IDisposable
{
    // Geometry.OpenR of the notch: the open shape's bottom corners, which the pictures cut the office to.
    const double OpenRadius = 32;
    // The top rows of the notch's office fade into the notch's black.
    const double TopFadeDips = 44;
    // The Go office thread works in DIPs, 16 less each way than the open notch (internal/shell/office.go), and its picture
    // is stretched over the whole view. The camera, the pointer and the tags are all in that size, so the framing is the same here.
    const double NotchInset = 16;

    /// <summary>The one office, for the pictures (<c>--shots</c>), which have no other way to reach it.</summary>
    internal static OfficeHost? Current { get; private set; }

    readonly DispatcherQueue queue;
    readonly BackendSession backend;
    readonly OfficeModel model;
    readonly OfficeController ctl;
    readonly OfficeStore store;
    readonly DispatcherQueueTimer frameTimer, slowTimer;
    readonly Stopwatch clock = Stopwatch.StartNew();
    readonly DirectWriteText? text;
    OfficeRenderer? renderer;
    DateTime nextRendererTry = DateTime.MinValue;
    DateTime nextErrorLog = DateTime.MinValue;
    bool disposed;
    bool settled;

    // The notch.
    NotchSlot? slot;
    IDCompositionDevice? notchDevice;
    IDCompositionVisual? notchRoot, notchVisual;
    Surface? notchSurface;
    bool notchAttached;
    (float X, float Y) notchOffset;
    (int X, int Y, int W, int H) notchPixels;
    // The office's own size (DIPs) per device pixel of its surface, each way.
    (double X, double Y) modelPerPixel = (1, 1);
    double lastPressMs = -1000;
    (int X, int Y) lastPress;
    bool secondPress;
    (int X, int Y, bool On) polled;

    // The app window.
    Border? dashboardArea;
    nint dashboardWindow;
    SwapChainPanel? panel;
    UserControl? host;
    Surface? dashboardSurface;
    (double X, double Y) dashboardScale = (1, 1);

    public OfficeHost(DispatcherQueue queue, BackendSession backend)
    {
        this.queue = queue;
        this.backend = backend;
        try
        {
            text = new DirectWriteText(Path.Combine(AppContext.BaseDirectory, "Assets", "Office"));
        }
        catch (Exception e)
        {
            // The room is drawn without the words on its walls rather than not at all.
            HostLog.Line($"office: the wall fonts could not be loaded ({e.Message}); the walls are drawn without text");
        }
        model = new OfficeModel(1120, 440, false, text);
        ctl = new OfficeController(model);
        store = new OfficeStore(Path.Combine(DataFolder.Resolve(), "office.json"));
        if (store.View() is { } v) ctl.SetView(v.X, v.Z, v.Zoom);
        ctl.SetTime(store.Time());
        ctl.Visible = false;
        ctl.Changed += () =>
        {
            store.SetView(model.User[0], model.User[1], model.User[2]);
            store.SetTime(model.ManualTime);
        };
        ctl.Clicked += OnClicked;

        frameTimer = queue.CreateTimer();
        frameTimer.Interval = TimeSpan.FromMilliseconds(16);
        frameTimer.IsRepeating = true;
        frameTimer.Tick += (_, _) => Guard("frame", OnFrame);
        slowTimer = queue.CreateTimer();
        slowTimer.Interval = TimeSpan.FromMilliseconds(400);
        slowTimer.IsRepeating = true;
        slowTimer.Tick += (_, _) => Guard("watch", Watch);
        slowTimer.Start();

        backend.Event += OnBackend;
        Current = this;
        HostLog.Line("office: ready");
    }

    // ---- what other code uses --------------------------------------------------------------

    /// <summary>A click landed on a desk that has a session at it: that session's id, and where the click was (as <see cref="BotScreenRect"/> counts).</summary>
    public event Action<int, double, double>? DeskPicked;

    /// <summary>
    /// Whether the panels drawn over the office have something at a place (in the office's own pixels). A press, a move or the wheel
    /// there belongs to the panels and not to the office behind them; an event that lands on the office anyway (the panels take the
    /// pointer only once it has moved) must not click it.
    /// </summary>
    public Func<double, double, bool>? Covered { get; set; }

    bool Under(double x, double y) => Covered?.Invoke(x, y) == true;

    /// <summary>What the pointer is on in the office changed (see <see cref="Tip"/>).</summary>
    public event Action? TipChanged;

    /// <summary>A click landed on a bot. The value is its session's id.</summary>
    public event Action<int>? BotPicked;

    /// <summary>A click landed on the TV, the board, the shelf, the door or the clock.</summary>
    public event Action<OfficeProp>? PropPicked;

    /// <summary>Which surface shows the office now.</summary>
    public OfficeTarget Target { get; private set; }

    /// <summary>True once the office draws into the notch, so the notch stops drawing its placeholder.</summary>
    internal bool DrawsInNotch => notchAttached;

    /// <summary>What the pointer is on: 1 a bot or 2 a desk with a session at it (with that session's id), or a prop's hint; none when it is on nothing.</summary>
    public OfficeTip Tip()
    {
        if (Target == OfficeTarget.None || !model.HasPointer) return default;
        var h = model.Hovered;
        return h.Kind switch
        {
            HoverKind.Bot => new OfficeTip(1, h.Id, ""),
            HoverKind.Desk => new OfficeTip(2, h.Id, ""),
            HoverKind.Prop => new OfficeTip(0, 0, model.Hint(h.Prop)),
            _ => default,
        };
    }

    OfficeTip lastTip;

    void NoteTip()
    {
        var now = Tip();
        // The name tags move with the bots and the camera, and their words are typed: the panels draw them again when they change.
        var tags = Target == OfficeTarget.None ? "" : string.Join('|', model.Tags().Select(t => $"{t.Id}:{t.X:0}:{t.Y:0}:{t.Text}:{(int)t.Stage}:{t.Hot}"));
        if (now == lastTip && tags == lastTags) return;
        lastTip = now;
        lastTags = tags;
        TipChanged?.Invoke();
    }

    /// <summary>The bots' name tags in the units <see cref="BotScreenRect"/> counts in (none when the office is not shown).</summary>
    public IReadOnlyList<Tag> Tags()
    {
        if (Target == OfficeTarget.None) return [];
        var tags = model.Tags();
        if (Target != OfficeTarget.Notch || slot is null) return tags;
        var (sx, sy) = (slot.Area.W / model.W, slot.Area.H / model.H);
        return [.. tags.Select(t => t with { X = t.X * sx, Y = t.Y * sy })];
    }

    string lastTags = "";

    /// <summary>The colours of the helpers a session's bot has out at its desk, as the office's tag says (none when the office is not shown).</summary>
    public IReadOnlyList<(byte R, byte G, byte B)> HelpersOf(int sessionId)
        => Target == OfficeTarget.None ? [] : model.Tags().FirstOrDefault(t => t.Id == sessionId)?.Helpers ?? [];

    /// <summary>
    /// Where a session's bot is on screen, in DIPs from the top left of the surface that shows the
    /// office (the rectangle <c>NotchScene.OfficeArea</c> gives in the notch, the top left of
    /// <c>DashboardWindow.OfficeArea</c> in the app window). Null when the office is not shown or
    /// the session has no bot in it.
    /// </summary>
    public OfficeRect? BotScreenRect(int sessionId)
    {
        if (Target == OfficeTarget.None || model.BotRect(sessionId) is not { } r) return null;
        // The office's picture covers the whole view, so its own size stretches to the view's.
        var (sx, sy) = Target == OfficeTarget.Notch ? (slot!.Area.W / model.W, slot.Area.H / model.H) : (1.0, 1.0);
        return new OfficeRect(r.X * sx, r.Y * sy, r.W * sx, r.H * sy);
    }

    /// <summary>
    /// The office beats switch, kept in office.json as <c>beats</c> ("on", "off"; off when the file has none), as the Go and Mac builds keep it.
    /// The office draws only; the audio phase plays the loop while this is on and follows <see cref="BeatsChanged"/>.
    /// </summary>
    public bool Beats
    {
        get => store.Beats();
        set
        {
            if (store.Beats() == value) return;
            store.SetBeats(value);
            BeatsChanged?.Invoke(value);
        }
    }

    /// <summary>The beats switch was turned on or off (the new value).</summary>
    public event Action<bool>? BeatsChanged;

    /// <summary>The drawer opened on a session: the camera goes to its bot and the wheel and drag stop moving the view.</summary>
    public void OpenDrawer(int sessionId) => ctl.Drawer(sessionId, true);

    /// <summary>The drawer closed.</summary>
    public void CloseDrawer() => ctl.Drawer(0, false);

    /// <summary>The panel open over the office: "board", "tv" or "history"; "" for none. The camera goes to its prop.</summary>
    public void ShowPanel(string panelName) => ctl.Panel(panelName);

    /// <summary>The session whose desk card is open (its bot shows as hot); null for none.</summary>
    public void MarkDeskOpen(int? sessionId) => ctl.DeskOpen(sessionId ?? 0, sessionId is not null);

    // ---- the backend's state -----------------------------------------------------------------

    void OnBackend(BackendEvent e)
    {
        // Raised on the backend reader's thread: the message is read into the office's own input there, applied on the UI thread.
        if (e is not StateEvent state || shotFrozen) return;
        var sessions = OfficeStateMap.Read(state);
        queue.TryEnqueue(() => Guard("state", () =>
        {
            if (!disposed) ctl.State(sessions);
        }));
    }

    void OnClicked(Click c)
    {
        switch (c.Kind)
        {
            case ClickKind.Open:
                BotPicked?.Invoke(c.Id);
                break;
            case ClickKind.Desk:
            {
                // Where the desk was clicked, in the same units as BotScreenRect: the desk's card sits there.
                var (sx, sy) = Target == OfficeTarget.Notch && slot is not null ? (slot.Area.W / model.W, slot.Area.H / model.H) : (1.0, 1.0);
                DeskPicked?.Invoke(c.Id, c.X * sx, c.Y * sy);
                break;
            }
            case ClickKind.Panel:
                PropPicked?.Invoke(c.Panel switch { "tv" => OfficeProp.Tv, "board" => OfficeProp.Board, _ => OfficeProp.History });
                break;
            case ClickKind.NewTask:
                PropPicked?.Invoke(OfficeProp.Door);
                break;
            case ClickKind.Toast:
                PropPicked?.Invoke(OfficeProp.Clock);
                break;
        }
    }

    // ---- the notch ------------------------------------------------------------------------------

    /// <summary>The open notch shows the office here. Called after every notch frame; cheap when nothing changed.</summary>
    internal void UpdateNotch(NotchSlot next)
    {
        if (disposed) return;
        var old = slot;
        slot = next;
        var open = NotchOpen;
        if (!open)
        {
            HideNotch();
            Retarget();
            return;
        }
        var same = old is not null && ReferenceEquals(old.Root, next.Root) && old.Area == next.Area && old.Scale == next.Scale &&
            old.Shape == next.Shape && old.ShapeRadius == next.ShapeRadius && old.Opacity == next.Opacity && old.Visible == next.Visible;
        var was = Target;
        Retarget();
        if (Target != OfficeTarget.Notch) return;
        // The shape and the fade move with the notch's own frames: only the page is drawn again, the room is left as it is.
        if (!same || was != OfficeTarget.Notch) Guard("notch", () => DrawNotch(scene: was != OfficeTarget.Notch));
    }

    bool NotchOpen => slot is { Visible: true, Opacity: > 0 } s && s.Area.W > 1 && s.Area.H > 1;

    /// <summary>The primary button went down in the notch window (device pixels, window coordinates).</summary>
    internal void NotchPressed(int x, int y)
    {
        if (Target != OfficeTarget.Notch || !InNotchArea(x, y)) return;
        var now = clock.Elapsed.TotalMilliseconds;
        secondPress = now - lastPressMs < 450 && Math.Abs(x - lastPress.X) <= 6 && Math.Abs(y - lastPress.Y) <= 6;
        lastPressMs = now;
        lastPress = (x, y);
        var (px, py) = InArea(x, y);
        ctl.Pointer(px, py, true);
        ctl.Down(px, py);
    }

    /// <summary>The primary button went up in the notch window.</summary>
    internal void NotchReleased(int x, int y)
    {
        if (Target != OfficeTarget.Notch || !ctl.IsDown) return;
        var (px, py) = InArea(x, y);
        ctl.Pointer(px, py, true);
        ctl.Up();
        if (secondPress)
        {
            secondPress = false;
            ctl.DoubleClick();
        }
    }

    /// <summary>The wheel turned over the notch window (120 is one notch of the wheel, positive away from the user).</summary>
    internal void NotchWheel(int delta)
    {
        if (Target != OfficeTarget.Notch || OfficeNative.CursorIn(slot!.Window) is not { } c || !InNotchArea(c.X, c.Y)) return;
        var (px, py) = InArea(c.X, c.Y);
        // A browser's deltaY: 100 for a notch of the wheel, negative when it turns away from the user.
        ctl.Wheel(-delta * 100.0 / 120, px, py);
    }

    /// <summary>A key went down in the notch window (virtual-key code).</summary>
    internal void NotchKey(int virtualKey)
    {
        if (Target == OfficeTarget.Notch && KeyChar(virtualKey) is { } c) ctl.Key(c);
    }

    /// <summary>The notch window is going away.</summary>
    internal void DetachNotch()
    {
        DropNotch();
        slot = null;
        Retarget();
    }

    bool InNotchArea(int x, int y) => x >= notchPixels.X && y >= notchPixels.Y && x < notchPixels.X + notchPixels.W && y < notchPixels.Y + notchPixels.H;

    /// <summary>A place in the notch window (device pixels) as the office reads it: from the area's top left, in the office's own size.</summary>
    (double X, double Y) InArea(int x, int y) => ((x - notchPixels.X) * modelPerPixel.X, (y - notchPixels.Y) * modelPerPixel.Y);

    static char? KeyChar(int vk) => vk switch
    {
        0x6B or 0xBB => '+',
        0x6D or 0xBD => '-',
        0x30 or 0x60 => '0',
        _ => null,
    };

    /// <summary>The pointer's place in the notch, read each frame: the notch window does not pass on moves. The office is told only when it moved.</summary>
    void PollNotchPointer()
    {
        if (Target != OfficeTarget.Notch || slot is null) return;
        var c = OfficeNative.CursorIn(slot.Window);
        (int X, int Y, bool On) now = c is { } p && InNotchArea(p.X, p.Y) ? (p.X, p.Y, true) : (0, 0, false);
        if (now == polled) return;
        polled = now;
        if (now.On)
        {
            var (x, y) = InArea(now.X, now.Y);
            ctl.Pointer(x, y, true);
        }
        else ctl.Pointer(0, 0, false);
    }

    void EnsureNotchVisual(OfficeRenderer r, int w, int h)
    {
        var s = slot!;
        if (notchSurface is not null && !ReferenceEquals(notchRoot, s.Root)) DropNotch();
        if (notchSurface is null)
        {
            notchSurface = new Surface(r, w, h, alpha: true);
            notchDevice = s.Composition;
            notchRoot = s.Root;
            notchDevice.CreateVisual(out notchVisual).CheckError();
            notchVisual!.SetContent(notchSurface.Swap).CheckError();
            notchOffset = (float.NaN, float.NaN);
        }
        else notchSurface.Resize(w, h);
        var off = ((float)(s.Area.X * s.Scale), (float)(s.Area.Y * s.Scale));
        if (off != notchOffset)
        {
            notchVisual!.SetOffsetX(off.Item1).CheckError();
            notchVisual.SetOffsetY(off.Item2).CheckError();
            notchOffset = off;
            notchDevice!.Commit().CheckError();
        }
        if (!notchAttached)
        {
            notchRoot!.AddVisual(notchVisual!, true, null!).CheckError();
            notchDevice!.Commit().CheckError();
            notchAttached = true;
            HostLog.Line($"office: drawing in the notch ({w} x {h})");
        }
    }

    void HideNotch()
    {
        if (!notchAttached) return;
        try
        {
            notchRoot!.RemoveVisual(notchVisual!).CheckError();
            notchDevice!.Commit().CheckError();
        }
        catch (Exception e)
        {
            HostLog.Line($"office: hiding the notch view failed ({e.Message})");
        }
        notchAttached = false;
    }

    void DropNotch()
    {
        HideNotch();
        notchVisual?.Dispose();
        notchSurface?.Dispose();
        notchVisual = null;
        notchSurface = null;
        notchRoot = null;
        notchDevice = null;
    }

    void DrawNotch(bool scene)
    {
        var s = slot!;
        var r = EnsureRenderer();
        if (r is null) return;
        var sc = s.Scale;
        int w = Math.Max((int)Math.Round(s.Area.W * sc), 1), h = Math.Max((int)Math.Round(s.Area.H * sc), 1);
        notchPixels = ((int)Math.Round(s.Area.X * sc), (int)Math.Round(s.Area.Y * sc), w, h);
        double mw = Math.Max(s.Area.W - NotchInset, 1), mh = Math.Max(s.Area.H - NotchInset, 1);
        if (model.W != mw || model.H != mh) ctl.Resize(mw, mh);
        modelPerPixel = (mw / w, mh / h);
        EnsureNotchVisual(r, w, h);
        var shape = new ShapeMask((s.Shape.X - s.Area.X) * sc, -s.Area.Y * sc, (s.Shape.X + s.Shape.W - s.Area.X) * sc, (s.Shape.H - s.Area.Y) * sc, s.ShapeRadius * sc);
        var page = new PageStyle(model.Time == TimeOfDay.Day, TopFadeDips * sc, shape, s.Opacity);
        var (sw, sh) = r.SceneSize(w, h);
        r.Render(model, notchSurface!.Rtv(), w, h, sw, sh, page, redrawScene: scene);
        notchSurface.Present();
    }

    // ---- the app window ----------------------------------------------------------------------------

    /// <summary>The app window has its office area; the office is shown there while the notch is folded.</summary>
    internal void AttachDashboard(Border area, nint window)
    {
        dashboardArea = area;
        dashboardWindow = window;
        panel = new SwapChainPanel
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            VerticalAlignment = VerticalAlignment.Stretch,
        };
        // A swap chain panel takes no Background, and an element without one takes no pointer events: the panel sits in a
        // transparent grid, which does, inside a control that can have the keyboard. The events bubble up to it.
        var surface = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        surface.Children.Add(panel);
        host = new UserControl { Content = surface, IsTabStop = true, UseSystemFocusVisuals = false };
        panel.SizeChanged += (_, _) => Guard("size", DashboardChanged);
        panel.CompositionScaleChanged += (_, _) => Guard("scale", DashboardChanged);
        host.PointerMoved += (_, e) => Guard("move", () =>
        {
            var p = Dips(e);
            if (Under(p.X, p.Y)) ctl.Pointer(0, 0, false);
            else ctl.Pointer(p.X, p.Y, true);
        });
        host.PointerExited += (_, _) => Guard("leave", () => ctl.Pointer(0, 0, false));
        host.PointerPressed += (_, e) => Guard("press", () =>
        {
            if (!e.GetCurrentPoint(host).Properties.IsLeftButtonPressed) return;
            var p = Dips(e);
            if (Under(p.X, p.Y)) return;
            host.CapturePointer(e.Pointer);
            host.Focus(FocusState.Pointer);
            ctl.Pointer(p.X, p.Y, true);
            ctl.Down(p.X, p.Y);
            if (Chatting.UiDump.On) HostLog.Line($"office: press at {p.X:0},{p.Y:0}");
        });
        host.PointerReleased += (_, e) => Guard("release", () =>
        {
            // The capture is let go after the click is read: letting it go raises PointerCaptureLost at once, and that cancels the press.
            try
            {
                if (!ctl.IsDown)
                {
                    if (Chatting.UiDump.On) HostLog.Line("office: a release without a press");
                    return;
                }
                var p = Dips(e);
                ctl.Pointer(p.X, p.Y, true);
                ctl.Up();
                if (Chatting.UiDump.On) HostLog.Line($"office: click at {p.X:0},{p.Y:0} found {model.Hovered.Kind} {model.Hovered.Id}");
            }
            finally { host.ReleasePointerCapture(e.Pointer); }
        });
        host.PointerCaptureLost += (_, _) => Guard("capture", ctl.Cancel);
        host.DoubleTapped += (_, _) => Guard("double", ctl.DoubleClick);
        host.PointerWheelChanged += (_, e) => Guard("wheel", () =>
        {
            var p = Dips(e);
            if (Under(p.X, p.Y)) return;
            ctl.Wheel(-e.GetCurrentPoint(host).Properties.MouseWheelDelta * 100.0 / 120, p.X, p.Y);
            e.Handled = true;
        });
        host.KeyDown += (_, e) => Guard("key", () =>
        {
            if (KeyChar((int)e.Key) is { } c)
            {
                ctl.Key(c);
                e.Handled = true;
            }
        });
        area.Child = host;
        Retarget();
    }

    /// <summary>A pointer event's place in the panel, in DIPs, which is the office's own size here.</summary>
    (double X, double Y) Dips(PointerRoutedEventArgs e)
    {
        var p = e.GetCurrentPoint(host!).Position;
        return (p.X, p.Y);
    }

    bool DashboardShown =>
        panel is { ActualWidth: > 1, ActualHeight: > 1, Visibility: Visibility.Visible } && dashboardArea?.XamlRoot is not null && OfficeNative.IsShown(dashboardWindow);

    void DashboardChanged()
    {
        if (panel is null) return;
        dashboardScale = (Math.Max(panel.CompositionScaleX, 0.01), Math.Max(panel.CompositionScaleY, 0.01));
        Retarget();
        if (Target == OfficeTarget.Dashboard) DrawDashboard(scene: true);
    }

    void DrawDashboard(bool scene)
    {
        var r = EnsureRenderer();
        if (r is null || panel is null) return;
        dashboardScale = (Math.Max(panel.CompositionScaleX, 0.01), Math.Max(panel.CompositionScaleY, 0.01));
        int w = Math.Max((int)Math.Round(panel.ActualWidth * dashboardScale.X), 1), h = Math.Max((int)Math.Round(panel.ActualHeight * dashboardScale.Y), 1);
        if (model.W != panel.ActualWidth || model.H != panel.ActualHeight) ctl.Resize(panel.ActualWidth, panel.ActualHeight);
        modelPerPixel = (panel.ActualWidth / w, panel.ActualHeight / h);
        if (dashboardSurface is null)
        {
            dashboardSurface = new Surface(r, w, h, alpha: false);
            OfficeNative.SetSwapChain(panel, dashboardSurface.Swap.NativePointer);
            HostLog.Line($"office: drawing in the app window ({w} x {h})");
        }
        else dashboardSurface.Resize(w, h);
        // The panel scales its swap chain by the window's scale; this undoes it so that one pixel is one pixel.
        using (var swap2 = dashboardSurface.Swap.QueryInterface<IDXGISwapChain2>())
            swap2.MatrixTransform = Matrix3x2.CreateScale((float)(1 / dashboardScale.X), (float)(1 / dashboardScale.Y));
        var (sw, sh) = r.SceneSize(w, h);
        r.Render(model, dashboardSurface.Rtv(), w, h, sw, sh, new PageStyle(model.Time == TimeOfDay.Day), redrawScene: scene);
        dashboardSurface.Present();
    }

    void DropDashboard()
    {
        if (dashboardSurface is null) return;
        try
        {
            if (panel is not null) OfficeNative.SetSwapChain(panel, 0);
        }
        catch (Exception e)
        {
            HostLog.Line($"office: letting go of the app window's view failed ({e.Message})");
        }
        dashboardSurface.Dispose();
        dashboardSurface = null;
    }

    // ---- which surface, and the frames -----------------------------------------------------------------

    void Retarget()
    {
        var next = NotchOpen ? OfficeTarget.Notch : DashboardShown ? OfficeTarget.Dashboard : OfficeTarget.None;
        if (next == Target) return;
        var from = Target;
        Target = next;
        HostLog.Line($"office: shown in {next.ToString().ToLowerInvariant()}");
        ctl.Visible = next != OfficeTarget.None;
        if (from == OfficeTarget.Dashboard) DropDashboard();
        if (next == OfficeTarget.None)
        {
            frameTimer.Stop();
            ctl.Pointer(0, 0, false);
            ctl.Cancel();
            return;
        }
        if (next == OfficeTarget.Dashboard) DashboardResize();
        model.Poke();
        if (!frameTimer.IsRunning) frameTimer.Start();
        if (next == OfficeTarget.Dashboard) DrawDashboard(scene: true);
    }

    void DashboardResize()
    {
        if (panel is null) return;
        dashboardScale = (Math.Max(panel.CompositionScaleX, 0.01), Math.Max(panel.CompositionScaleY, 0.01));
        ctl.Resize(Math.Max(panel.ActualWidth, 1), Math.Max(panel.ActualHeight, 1));
    }

    void OnFrame()
    {
        if (disposed || Target == OfficeTarget.None) return;
        PollNotchPointer();
        var drew = ctl.Tick(clock.Elapsed.TotalMilliseconds);
        NoteTip();
        if (!drew) return;
        if (Target == OfficeTarget.Notch) DrawNotch(scene: true);
        else DrawDashboard(scene: true);
    }

    /// <summary>Four times a second: whether the app window shows the office, and whether the graphics device is still there.</summary>
    void Watch()
    {
        if (disposed) return;
        Retarget();
        if (renderer is { } r && r.DeviceLost() is { } why)
        {
            // A driver reset or a GPU taken away: start again on a new device, and draw the room at once.
            HostLog.Line($"office: the graphics device was lost ({why}); making a new one");
            LoseRenderer();
            model.Poke();
        }
    }

    OfficeRenderer? EnsureRenderer()
    {
        if (renderer is not null) return renderer;
        if (DateTime.UtcNow < nextRendererTry) return null;
        try
        {
            // The notch makes its device the same way, so both are on the same adapter.
            renderer = OfficeRenderer.Create();
            HostLog.Line($"office: Direct3D 11 on {renderer.AdapterName}{(renderer.Software ? " (software, half-size room)" : "")}");
        }
        catch (Exception e)
        {
            nextRendererTry = DateTime.UtcNow.AddSeconds(5);
            HostLog.Line($"office: no graphics device ({e.Message}); trying again in 5 seconds");
        }
        return renderer;
    }

    void LoseRenderer()
    {
        DropNotch();
        DropDashboard();
        renderer?.Dispose();
        renderer = null;
        nextRendererTry = DateTime.UtcNow.AddSeconds(1);
        // The surfaces are made again by the next frame: from the notch's last slot, or from the app window.
        Target = OfficeTarget.None;
        Retarget();
    }

    void Guard(string what, Action action)
    {
        try
        {
            action();
        }
        catch (Exception e)
        {
            // A lost device shows up as an error from the next draw: the watch above makes a new one. Anything else is logged, not repeated every frame.
            if (DateTime.UtcNow >= nextErrorLog)
            {
                nextErrorLog = DateTime.UtcNow.AddSeconds(5);
                HostLog.Line($"office: {what} failed — {e}");
            }
        }
    }

    // ---- pictures ---------------------------------------------------------------------------------------

    /// <summary>Lets the camera and the room settle, as they would a few seconds after the office opens, so a picture is not taken mid-easing.</summary>
    void Settle()
    {
        if (settled) return;
        settled = true;
        var t = clock.Elapsed.TotalMilliseconds;
        for (var i = 0; i < 400; i++)
        {
            model.Frame(t, 16);
            t += 16;
        }
    }

    /// <summary>
    /// The office as a premultiplied BGRA picture of this size, for <c>--shots</c>. With <paramref name="cutToNotch"/> the picture is
    /// cut to the open notch (flat top, round bottom corners) and its top fades into the notch's black.
    /// </summary>
    internal byte[]? Shoot(int width, int height, double scale, bool cutToNotch)
    {
        var r = EnsureRenderer();
        if (r is null) return null;
        Settle();
        var inset = cutToNotch ? NotchInset : 0;
        ctl.Resize(Math.Max(width / scale - inset, 1), Math.Max(height / scale - inset, 1));
        var shape = cutToNotch ? new ShapeMask(0, -1, width, height, OpenRadius * scale) : (ShapeMask?)null;
        var (sw, sh) = r.SceneSize(width, height);
        return r.RenderToBytes(model, width, height, new PageStyle(model.Time == TimeOfDay.Day, cutToNotch ? TopFadeDips * scale : 0, shape), sw, sh);
    }

    // ---- for --shots ---------------------------------------------------------------------------------

    bool shotFrozen;

    /// <summary>For <c>--shots</c>: the office is this size and shows these sessions at their desks, and takes no other state.</summary>
    internal void ShowForShot(StateEvent state, int width, int height)
    {
        shotFrozen = true;
        ctl.Resize(width, height);
        ctl.State(OfficeStateMap.Read(state));
        RunForShot(600);
        settled = true;
    }

    /// <summary>For <c>--shots</c>: the office runs this many frames (the camera goes to a bot whose chat opened, the bots walk).</summary>
    internal void RunForShot(int frames)
    {
        var t = clock.Elapsed.TotalMilliseconds;
        for (var i = 0; i < frames; i++)
        {
            model.Frame(t, 16);
            t += 16;
        }
    }

    /// <summary>For <c>--shots</c>: where a session's name tag is, and its helpers' colours (in the office's own pixels).</summary>
    internal (double X, double Y, IReadOnlyList<(byte R, byte G, byte B)> Helpers)? TagForShot(int sessionId)
        => model.Tags().FirstOrDefault(t => t.Id == sessionId) is { } tag ? (tag.X, tag.Y, tag.Helpers) : null;

    /// <summary>For <c>--shots</c>: the name tags as the office now has them.</summary>
    internal IReadOnlyList<Tag> TagsForShot() => model.Tags();

    /// <summary>For <c>--shots</c>: where a session's bot is.</summary>
    internal OfficeRect? BotForShot(int sessionId) => model.BotRect(sessionId) is { } r ? new OfficeRect(r.X, r.Y, r.W, r.H) : null;

    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        if (ReferenceEquals(Current, this)) Current = null;
        backend.Event -= OnBackend;
        frameTimer.Stop();
        slowTimer.Stop();
        DropNotch();
        DropDashboard();
        renderer?.Dispose();
        renderer = null;
        text?.Dispose();
        HostLog.Line("office: stopped");
    }

    /// <summary>A swap chain and the view of its back buffer, resized with the surface.</summary>
    sealed class Surface : IDisposable
    {
        readonly OfficeRenderer renderer;
        ID3D11Texture2D? back;
        ID3D11RenderTargetView? view;

        public Surface(OfficeRenderer renderer, int width, int height, bool alpha)
        {
            this.renderer = renderer;
            Swap = renderer.CreateSwapChain(width, height, alpha);
            Width = width;
            Height = height;
        }

        public IDXGISwapChain1 Swap { get; }
        public int Width { get; private set; }
        public int Height { get; private set; }

        public ID3D11RenderTargetView Rtv()
        {
            if (view is not null) return view;
            back = Swap.GetBuffer<ID3D11Texture2D>(0);
            view = renderer.Device.CreateRenderTargetView(back);
            return view;
        }

        public void Resize(int width, int height)
        {
            if (width == Width && height == Height) return;
            view?.Dispose();
            back?.Dispose();
            view = null;
            back = null;
            Swap.ResizeBuffers(0, (uint)width, (uint)height, Format.Unknown, SwapChainFlags.None).CheckError();
            Width = width;
            Height = height;
        }

        public void Present() => Swap.Present(0, PresentFlags.None).CheckError();

        public void Dispose()
        {
            view?.Dispose();
            back?.Dispose();
            Swap.Dispose();
        }
    }
}

/// <summary>
/// Everything the office needs to know of the open notch for one frame: the window, the composition
/// device and root visual to add its swap chain to, and where the office goes.
/// </summary>
internal sealed record NotchSlot(
    nint Window,
    Vortice.Direct3D11.ID3D11Device Device,
    Vortice.DirectComposition.IDCompositionDevice Composition,
    Vortice.DirectComposition.IDCompositionVisual Root,
    OfficeRect Area,
    double Scale,
    OfficeRect Shape,
    double ShapeRadius,
    double Opacity,
    bool Visible);
