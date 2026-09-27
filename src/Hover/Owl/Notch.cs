using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;
using System.Windows.Shapes;
using System.Windows.Threading;
using Hover.Core;
using Hover.Interop;

namespace Hover.Owl;

/// The black notch shape and what is inside it. Openness 0 is the resting shape —
/// a slim pill of glanceable items, or nothing — and 1 is the open
/// workspace. The
/// shape grows from one to the other and the workspace is revealed through it.
internal sealed class NotchShell : Canvas
{
    public static readonly DependencyProperty OpennessProperty = DependencyProperty.Register(
        nameof(Openness), typeof(double), typeof(NotchShell),
        new PropertyMetadata(0.0, (d, _) => ((NotchShell)d).Relayout()));

    public double Openness
    {
        get => (double)GetValue(OpennessProperty);
        set => SetValue(OpennessProperty, value);
    }

    private readonly Path _shape = new();
    /// The faint line around the open panel, so it keeps its edge over dark windows.
    private readonly Path _rim = new() { StrokeThickness = 1, IsHitTestVisible = false };
    private readonly SolidColorBrush _fill = new(Colors.Black);
    /// What the resting shape shows, centred in it.
    public Grid Mini { get; } = new();
    /// The workspace, always laid out at its full open size.
    public Border ViewHost { get; } = new();

    /// "Welcome back", said while the notch opens.
    public TextBlock Greeting { get; } = new()
    {
        Text = "Welcome back", FontFamily = Ui.Display, FontWeight = FontWeights.SemiBold, FontSize = 12,
        Foreground = Ui.White, IsHitTestVisible = false,
    };

    private Size _rest, _open = new(1120, 440);
    private bool _opening, _greet;

    /// Show the greeting for this opening. Cleared when the opening ends.
    public bool Greet
    {
        get => _greet;
        set { _greet = value; Relayout(); }
    }

    /// Set while opening or open. The workspace must be visible — clipped to the
    /// still-tiny shape, and transparent — from the very first frame, because WPF
    /// will not give keyboard focus to an element that is hidden, and the shortcut
    /// puts the caret in the task field straight away.
    public bool Opening
    {
        get => _opening;
        set { _opening = value; Relayout(); }
    }

    public NotchShell()
    {
        Background = null;
        _shape.Fill = _fill;
        Children.Add(_shape);
        Children.Add(_rim);
        Children.Add(Mini);
        Children.Add(ViewHost);
        Children.Add(Greeting);
        SizeChanged += (_, _) => Relayout();
        ApplyTheme();
    }

    /// The panel's colour, edge and shadow for the current appearance. The resting
    /// shape stays black either way.
    public void ApplyTheme()
    {
        _rim.Stroke = Ui.PanelEdge;
        _shape.Effect = new DropShadowEffect
        {
            BlurRadius = Theme.Dark ? 24 : 36, ShadowDepth = Theme.Dark ? 4 : 10, Direction = 270,
            Opacity = Theme.Dark ? 0.5 : 0.22, RenderingBias = RenderingBias.Performance,
        };
        Relayout();
    }

    public void SetSizes(Size rest, Size open)
    {
        _rest = rest;
        _open = open;
        Relayout();
    }

    private static double Lerp(double a, double b, double t) => a + (b - a) * t;

    private void Relayout()
    {
        var t = Openness;
        var w = Lerp(_rest.Width, _open.Width, t);
        var h = Lerp(_rest.Height, _open.Height, t);
        var (rr, re) = RestCorners(_rest);
        var r = Lerp(rr, 32, t);
        var ear = Lerp(re, 10, t);
        var cx = ActualWidth / 2;

        _shape.Visibility = w < 1 || h < 1 ? Visibility.Hidden : Visibility.Visible;
        _shape.Data = Outline(w, h, r, ear, 0);
        SetLeft(_shape, cx - w / 2);

        // Black while small, as a real notch is; the panel's own colour by the time
        // the cards are in (the same black, in dark mode). Late enough that white
        // text on the way out stays legible.
        var k = Math.Clamp((t - 0.2) / 0.6, 0, 1);
        k = k * k * (3 - 2 * k);
        _fill.Color = Mix(Colors.Black, Ui.Panel, k);
        _rim.Data = Outline(w, h, r, ear, 0, closed: false);
        _rim.Opacity = k;
        SetLeft(_rim, cx - w / 2);

        ViewHost.Width = _open.Width;
        ViewHost.Height = _open.Height;
        SetLeft(ViewHost, cx - _open.Width / 2);
        ViewHost.Clip = Outline(w, h, r, 0, (_open.Width - w) / 2);
        ViewHost.Opacity = Math.Clamp((t - 0.35) / 0.65, 0, 1);
        ViewHost.Visibility = t <= 0.001 && !_opening ? Visibility.Hidden : Visibility.Visible;
        ViewHost.IsHitTestVisible = t >= 0.999;

        Mini.Width = _rest.Width;
        Mini.Height = _rest.Height;
        SetLeft(Mini, cx - _rest.Width / 2);
        Mini.Opacity = Math.Clamp(1 - t * 3, 0, 1);
        Mini.Visibility = Mini.Opacity <= 0 ? Visibility.Hidden : Visibility.Visible;

        // The greeting sits low in the small notch, grows a little with it, and has
        // faded by the time the cards are in.
        Greeting.Visibility = _greet ? Visibility.Visible : Visibility.Collapsed;
        if (!_greet) return;
        var size = 12 + 10 * Math.Clamp(t / 0.6, 0, 1);
        if (Math.Abs(Greeting.FontSize - size) > 0.01) Greeting.FontSize = size;
        Greeting.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        var gs = Greeting.DesiredSize;
        var fits = w >= gs.Width + 24 && h >= gs.Height + 12;
        Greeting.Opacity = fits ? Math.Clamp((0.6 - t) / 0.3, 0, 1) : 0;
        SetLeft(Greeting, cx - gs.Width / 2);
        SetTop(Greeting, Math.Max(4, Math.Min(h - gs.Height - 9, 64)));
    }

    /// A fully round pill at rest, with a small flare into the screen edge.
    private static (double Radius, double Ear) RestCorners(Size s)
    {
        var r = Math.Min(s.Height / 2, 14);
        return (r, Math.Max(0, Math.Min(5, s.Height - r)));
    }

    private static Color Mix(Color a, Color b, double k) => Color.FromArgb(
        (byte)Math.Round(a.A + (b.A - a.A) * k), (byte)Math.Round(a.R + (b.R - a.R) * k),
        (byte)Math.Round(a.G + (b.G - a.G) * k), (byte)Math.Round(a.B + (b.B - a.B) * k));

    /// A notch: square top edge flush with the screen, rounded bottom corners, and a
    /// concave flare where each side meets the top edge. x0 shifts it right. Open
    /// (closed: false) it leaves out the top edge, for the rim.
    private static Geometry Outline(double w, double h, double r, double ear, double x0, bool closed = true)
    {
        var g = new StreamGeometry();
        if (w < 1 || h < 1) { g.Freeze(); return g; }
        r = Math.Min(r, Math.Min(w / 2, h));
        ear = Math.Max(0, Math.Min(ear, h - r));
        using (var c = g.Open())
        {
            c.BeginFigure(new Point(x0 - ear, 0), closed, closed);
            if (ear > 0) c.ArcTo(new Point(x0, ear), new Size(ear, ear), 0, false, SweepDirection.Clockwise, true, false);
            c.LineTo(new Point(x0, h - r), true, false);
            c.ArcTo(new Point(x0 + r, h), new Size(r, r), 0, false, SweepDirection.Counterclockwise, true, false);
            c.LineTo(new Point(x0 + w - r, h), true, false);
            c.ArcTo(new Point(x0 + w, h - r), new Size(r, r), 0, false, SweepDirection.Counterclockwise, true, false);
            c.LineTo(new Point(x0 + w, ear), true, false);
            if (ear > 0) c.ArcTo(new Point(x0 + w + ear, 0), new Size(ear, ear), 0, false, SweepDirection.Clockwise, true, false);
        }
        g.Freeze();
        return g;
    }
}

/// The notch on one display: resting at the top centre, opening into the workspace
/// when the pointer rests on it, when it is clicked, or on the shortcut.
internal sealed class NotchHost : IDisposable
{
    public enum Mode { Rest, Peek, Open }

    private enum RestKind { None, Pill, Alert }

    public string Device { get; }
    public Mode State { get; private set; } = Mode.Rest;
    public NotchManager? Manager { get; init; }

    private ScreenInfo _screen;
    private readonly HostWindow _window = new();
    private readonly NotchShell _shell = new();
    private WorkspaceView? _view;
    private IntPtr _hwnd;
    private IntPtr _previous;
    private Size _open;
    private RestKind _kind = (RestKind)(-1);
    /// The resting size last handed to the shell, so it is re-laid-out only when the
    /// shape really changes — not on every one-second tick.
    /// Null until the first pass. Not a negative Size: WPF's Size refuses those, and
    /// the throw from this initializer is what once stopped Hover starting at all.
    private Size? _restApplied;

    // Pointer bookkeeping for the hover trigger, in device pixels.
    private DateTime? _zoneSince, _outsideSince;
    private bool _armed = true;

    // The resting pill: one segment per item, built once and updated in place.
    private readonly StackPanel _pill = new() { Orientation = Orientation.Horizontal };
    private readonly TextBlock _time = Ui.Text("", 12.5, Ui.White, FontWeights.SemiBold);
    private readonly Ellipse _timerDot = new() { Width = 6, Height = 6 };
    private readonly FrameworkElement _timerSeg;
    private readonly Dictionary<string, (FrameworkElement Seg, Ring Ring, TextBlock Text)> _quotaSegs = new();
    private string _pillKey = "";

    private readonly TextBlock _alertTitle = Ui.Text("", 13.5, Ui.White, FontWeights.SemiBold);
    private readonly TextBlock _alertText = Ui.Text("", 11.5, Ui.WhiteDim);
    private readonly StackPanel _alertBox = new();
    private (string Title, string Text)? _alert;

    /// Heights of the resting shapes. Small on purpose: the notch at rest is a hint,
    /// not a panel — a slim pill when there is something to show, and nothing when
    /// there is not. The user knows where it is.
    private const double PillHeight = 24, PillPad = 12, PillGap = 12;
    private static readonly TimeSpan Dwell = TimeSpan.FromMilliseconds(120);
    private static readonly TimeSpan LeaveGrace = TimeSpan.FromMilliseconds(350);

    public NotchHost(ScreenInfo screen)
    {
        Device = screen.Device;
        _screen = screen;
        _window.Title = "Hover notch";
        AutomationProperties.SetAutomationId(_window, "HoverNotch");
        AutomationProperties.SetAutomationId(_time, "NotchTime");
        _timerSeg = Ui.Row(_timerDot, _time.Margin(6, 0));
        _time.Typography.NumeralAlignment = FontNumeralAlignment.Tabular;
        _window.Root.Children.Add(_shell);
        Theme.Changed += OnTheme;

        _window.SourceInitialized += (_, _) => _hwnd = new WindowInteropHelper(_window).Handle;
        _window.DpiChanged += (_, _) => _window.Dispatcher.BeginInvoke(Layout, DispatcherPriority.Background);
        // Esc closes — unless something inside (a rename box, a popover) used it first.
        _window.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Escape || State == Mode.Rest) return;
            e.Handled = true;
            Collapse();
        };
        _window.Deactivated += (_, _) =>
            _window.Dispatcher.BeginInvoke(ClickedAway, DispatcherPriority.Background);
        // A click inside means the user is working here: stop closing on pointer-leave.
        _shell.PreviewMouseDown += (_, _) => { if (State == Mode.Peek) State = Mode.Open; };
        _shell.MouseLeftButtonUp += (_, e) =>
        {
            if (State != Mode.Rest || _kind == RestKind.None) return;
            e.Handled = true;
            Manager?.OpenOn(this, focusInput: false);
        };

        BuildMini();
        _window.Show();
        Layout();
        UpdateRest();
    }

    public void UpdateScreen(ScreenInfo screen)
    {
        _screen = screen;
        Layout();
    }

    /// One window size for every state: resizing a layered window
    /// on each transition makes it blink. Everything outside the shape is
    /// click-through, so the empty part costs nothing.
    private void Layout()
    {
        var s = _screen;
        _open = OpenSize();
        const double Pad = 40;   // room for the flare and the shadow
        var w = (int)Math.Round((_open.Width + 2 * Pad) * s.Scale);
        var h = (int)Math.Round((_open.Height + Pad) * s.Scale);
        var x = s.Work.Left + (s.Work.Width - w) / 2;
        _window.PlaceDevice(x, s.Work.Top, w, h);
        _window.Root.Width = _shell.Width = w / s.Scale;
        _window.Root.Height = _shell.Height = h / s.Scale;
        _shell.SetSizes(RestSize, _open);
    }

    /// The open workspace's size from Settings → Notch, kept inside the display.
    private Size OpenSize()
    {
        var (w, h) = Settings.WorkspaceSize switch
        {
            // Three quarters of Default: the smallest that still fits all five cards
            // side by side and the timer's buttons under its ring.
            WorkspaceSize.Small => (840d, 340d),
            WorkspaceSize.Large => (1320d, 520d),
            WorkspaceSize.ExtraLarge => (1560d, 600d),
            _ => (1120d, 440d),
        };
        var work = _screen.WorkDips;
        return new Size(Math.Min(w, work.Width - 24), Math.Min(h, work.Height - 24));
    }

    // MARK: Resting shape

    private Size RestSize => _kind switch
    {
        // Rounded up to a few pixels so a clock ticking from 1:11 to 1:12 does not
        // make the pill twitch.
        RestKind.Pill => new Size(Math.Ceiling((_pill.DesiredSize.Width + 2 * PillPad) / 4) * 4, PillHeight),
        RestKind.Alert => new Size(Math.Clamp(Math.Max(_alertTitle.DesiredSize.Width, _alertText.DesiredSize.Width) + 40, 200, 420), 46),
        _ => new Size(0, 0),
    };

    /// The resting notch holds a slim row of glanceable items, or a short message
    /// spelled in dots.
    private void BuildMini()
    {
        var m = _shell.Mini;
        _time.VerticalAlignment = VerticalAlignment.Center;
        _timerDot.VerticalAlignment = VerticalAlignment.Center;
        _pill.HorizontalAlignment = HorizontalAlignment.Center;
        _pill.VerticalAlignment = VerticalAlignment.Center;
        m.Children.Add(_pill);

        _alertTitle.HorizontalAlignment = HorizontalAlignment.Center;
        _alertText.HorizontalAlignment = HorizontalAlignment.Center;
        _alertText.MaxWidth = 380;
        _alertText.Margin = new Thickness(0, 5, 0, 0);
        _alertBox.Children.Add(_alertTitle);
        _alertBox.Children.Add(_alertText);
        _alertBox.HorizontalAlignment = HorizontalAlignment.Center;
        _alertBox.VerticalAlignment = VerticalAlignment.Center;
        AutomationProperties.SetAutomationId(_alertTitle, "NotchAlert");
        m.Children.Add(_alertBox);
    }

    private (FrameworkElement Seg, Ring Ring, TextBlock Text) QuotaSeg(string id)
    {
        if (_quotaSegs.TryGetValue(id, out var q)) return q;
        var ring = new Ring
        {
            Width = 11, Height = 11, VerticalAlignment = VerticalAlignment.Center,
            TrackBrush = Ui.Frozen(Color.FromArgb(0x38, 0xFF, 0xFF, 0xFF)),
        };
        var name = Ui.Text(QuotaStrip.Short(id), 11.5, Ui.WhiteDim);
        var value = Ui.Text("—", 11.5, Ui.White, FontWeights.SemiBold);
        value.Typography.NumeralAlignment = FontNumeralAlignment.Tabular;
        var seg = Ui.Row(ring, name.Margin(6, 0), value.Margin(4, 0));
        // The row is a panel, which UI Automation does not see; the value is text.
        AutomationProperties.SetAutomationId(value, "NotchQuota" + char.ToUpperInvariant(id[0]) + id[1..]);
        return _quotaSegs[id] = (seg, ring, value);
    }

    public void ShowAlert((string Title, string Text)? alert)
    {
        _alert = alert;
        UpdateRest();
    }

    /// Which items the pill shows right now. The timer only while it runs; the quotas
    /// when they are set to stay on the notch.
    private List<string> PillItems()
    {
        var running = OwlApp.Timer.State != FocusTimer.Phase.Ready;
        return Settings.NotchItems.Where(id =>
            id == NotchItem.Timer ? running
            : NotchItem.Quotas.Contains(id) && Settings.QuotasOnNotch).ToList();
    }

    /// Pick the resting shape and refresh what it shows. Called every second.
    public void UpdateRest()
    {
        var t = OwlApp.Timer;
        var items = _alert is null ? PillItems() : new List<string>();
        var kind = _alert is not null ? RestKind.Alert
            : items.Count > 0 ? RestKind.Pill
            : RestKind.None;

        if (kind == RestKind.Alert)
        {
            _alertTitle.Text = _alert!.Value.Title;
            _alertText.Text = _alert.Value.Text;
            _alertTitle.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            _alertText.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }
        _alertBox.Visibility = kind == RestKind.Alert ? Visibility.Visible : Visibility.Collapsed;
        _pill.Visibility = kind == RestKind.Pill ? Visibility.Visible : Visibility.Collapsed;

        if (kind == RestKind.Pill)
        {
            var key = string.Join(",", items);
            if (key != _pillKey)
            {
                _pillKey = key;
                _pill.Children.Clear();
                foreach (var id in items)
                {
                    var seg = id == NotchItem.Timer ? _timerSeg : QuotaSeg(id).Seg;
                    seg.Margin = new Thickness(_pill.Children.Count == 0 ? 0 : PillGap, 0, 0, 0);
                    seg.VerticalAlignment = VerticalAlignment.Center;
                    _pill.Children.Add(seg);
                }
            }
            if (items.Contains(NotchItem.Timer))
            {
                var paused = t.State == FocusTimer.Phase.Paused;
                _time.Text = t.Text;
                _time.Opacity = paused ? 0.55 : 1;
                _timerDot.Fill = Ui.Accent(paused ? Ui.Gray : Ui.Orange);
            }
            foreach (var id in items.Where(NotchItem.Quotas.Contains))
            {
                var (_, ring, text) = QuotaSeg(id);
                var reading = OwlApp.Quotas.TryGetValue(id, out var q) ? q.Reading : null;
                ring.Value = reading?.Used;
                text.Text = reading?.Used is { } u ? $"{u:0}%" : "—";
                text.Foreground = reading is null || reading.Ok ? Ui.White : Ui.WhiteDim;
            }
            // A child's new text does not invalidate the panel's own measure.
            _pill.InvalidateMeasure();
            _pill.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }
        else _pillKey = "";

        _kind = kind;
        // Settings → Notch → Workspace size, or nothing: the comparison is cheap.
        if (OpenSize() != _open) { Layout(); _restApplied = RestSize; return; }
        var rest = RestSize;
        if (rest == _restApplied) return;
        _restApplied = rest;
        _shell.SetSizes(rest, _open);
    }

    // MARK: Opening and closing

    /// Device-pixel rectangle the pointer wakes the notch from: the resting shape,
    /// and never less than a strip across the top centre.
    private Win32.RECT Zone
    {
        get
        {
            var s = _screen;
            var rest = RestSize;
            var halfW = Math.Max(rest.Width / 2, 110) * s.Scale;
            var h = Math.Max(rest.Height, 6) * s.Scale;
            var cx = s.Work.Left + s.Work.Width / 2.0;
            return new Win32.RECT
            {
                Left = (int)(cx - halfW), Right = (int)(cx + halfW),
                Top = s.Work.Top, Bottom = s.Work.Top + (int)Math.Ceiling(h),
            };
        }
    }

    /// The open panel in device pixels, grown by a little slack.
    private Win32.RECT PanelZone
    {
        get
        {
            var s = _screen;
            var slack = 16 * s.Scale;
            var cx = s.Work.Left + s.Work.Width / 2.0;
            var halfW = _open.Width / 2 * s.Scale + slack;
            return new Win32.RECT
            {
                Left = (int)(cx - halfW), Right = (int)(cx + halfW),
                Top = s.Work.Top - 2, Bottom = s.Work.Top + (int)(_open.Height * s.Scale + slack),
            };
        }
    }

    public void Poll(Win32.POINT p, bool buttons, DateTime now)
    {
        switch (State)
        {
            case Mode.Rest:
                var inZone = Zone.Contains(p);
                if (!inZone) { _zoneSince = null; _armed = true; return; }
                // Arriving, not merely being there: a notch closed with Esc under a
                // resting pointer must not spring straight back open.
                if (!_armed || buttons || !Settings.HoverOpensWorkspace) return;
                _zoneSince ??= now;
                if (now - _zoneSince >= Dwell) Manager?.OpenOn(this, focusInput: false, peek: true);
                return;

            case Mode.Peek:
                if (PanelZone.Contains(p) || buttons || Popover.Open > 0) { _outsideSince = null; return; }
                _outsideSince ??= now;
                if (now - _outsideSince >= LeaveGrace) Collapse();
                return;
        }
    }

    public void Expand(bool peek, bool focusInput)
    {
        if (State == Mode.Rest)
        {
            _previous = Win32.GetForegroundWindow();
            _view ??= NewView();
            _window.SetAcceptsKeys(true);
            _window.Raise();
            _shell.Opening = true;
            if (Manager?.TakeGreeting() == true) Greet();
            else Animate(1, 300, new CubicEase { EasingMode = EasingMode.EaseOut });
        }
        State = peek && State != Mode.Open ? Mode.Peek : Mode.Open;
        _outsideSince = null;
        if (!focusInput) return;
        _window.Focus(foreground: true);
        // After the first layout pass, or the field is not in the tree yet.
        _window.Dispatcher.BeginInvoke(() =>
        {
            _view?.FocusTaskInput();
            if (!_window.IsActive || Keyboard.FocusedElement is not TextBox)
                Log.Line($"notch: shortcut focus fell short — active {_window.IsActive}, " +
                         $"focused {Keyboard.FocusedElement?.GetType().Name ?? "nothing"}");
        }, DispatcherPriority.Input);
    }

    /// The first opening after launch or a return to the PC: the notch grows a
    /// little and says hello, then opens the rest of the way.
    private void Greet()
    {
        _shell.Greet = true;
        var a = new DoubleAnimationUsingKeyFrames();
        a.KeyFrames.Add(new EasingDoubleKeyFrame(0.06, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(160)),
            new CubicEase { EasingMode = EasingMode.EaseOut }));
        a.KeyFrames.Add(new LinearDoubleKeyFrame(0.09, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(560))));
        a.KeyFrames.Add(new EasingDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(940)),
            new CubicEase { EasingMode = EasingMode.EaseOut }));
        a.Completed += (_, _) => _shell.Greet = false;
        _shell.BeginAnimation(NotchShell.OpennessProperty, a);
    }

    private WorkspaceView NewView()
    {
        var v = new WorkspaceView(dashboard: false);
        _shell.ViewHost.Child = v;
        return v;
    }

    /// Light and dark are baked into the cards when they are built, so a switch
    /// builds them again — straight away while open, on the next opening otherwise.
    private void OnTheme()
    {
        _shell.ApplyTheme();
        if (_view is null) return;
        _view.Flush();
        if (State == Mode.Rest)
        {
            _shell.ViewHost.Child = null;
            _view = null;
            return;
        }
        var tab = _view.CurrentTab;
        _view = NewView();
        if (tab == 2) _view.ShowSettings(SettingsPage.Last);
        else _view.ShowTab(tab);
    }

    public void Collapse()
    {
        if (State == Mode.Rest) return;
        State = Mode.Rest;
        _armed = false;
        _zoneSince = null;
        // Hand the keyboard back to whatever had it before the notch took it.
        if (_hwnd != IntPtr.Zero && Win32.GetForegroundWindow() == _hwnd &&
            _previous != IntPtr.Zero && Win32.IsWindow(_previous))
            Win32.SetForegroundWindow(_previous);
        _window.SetAcceptsKeys(false);
        _shell.Opening = false;
        _shell.Greet = false;
        Animate(0, 220, new CubicEase { EasingMode = EasingMode.EaseIn });
    }

    /// Focus left for another app: the notch closes. Our own popups, menus and
    /// dialogs (owned by this window) do not count.
    private void ClickedAway()
    {
        if (State == Mode.Rest) return;
        var fg = Win32.GetForegroundWindow();
        if (fg == _hwnd || (fg != IntPtr.Zero && Win32.GetAncestor(fg, Win32.GA_ROOTOWNER) == _hwnd)) return;
        Collapse();
    }

    private void Animate(double to, int ms, IEasingFunction ease)
    {
        var a = new DoubleAnimation(to, TimeSpan.FromMilliseconds(ms)) { EasingFunction = ease };
        _shell.BeginAnimation(NotchShell.OpennessProperty, a);
    }

    public void Dispose()
    {
        Theme.Changed -= OnTheme;
        _shell.BeginAnimation(NotchShell.OpennessProperty, null);
        _window.Close();
    }
}

/// The notch on the main display, the pointer poll that wakes it, and the shortcut.
public sealed class NotchManager : IDisposable
{
    private readonly Dictionary<string, NotchHost> _hosts = new();
    private readonly DispatcherTimer _poll;
    private readonly DispatcherTimer _alertEnd = new() { Interval = TimeSpan.FromSeconds(8) };
    private string _signature = "";
    private DateTime _lastDisplayCheck = DateTime.MinValue;
    private DashboardWindow? _dashboard;
    private bool _greet = true;

    internal bool TakeGreeting()
    {
        var g = _greet;
        _greet = false;
        return g;
    }

    private void OnPower(object? sender, Microsoft.Win32.PowerModeChangedEventArgs e)
    {
        if (e.Mode == Microsoft.Win32.PowerModes.Resume) _greet = true;
    }

    private void OnSession(object? sender, Microsoft.Win32.SessionSwitchEventArgs e)
    {
        if (e.Reason == Microsoft.Win32.SessionSwitchReason.SessionUnlock) _greet = true;
    }

    public NotchManager()
    {
        Microsoft.Win32.SystemEvents.PowerModeChanged += OnPower;
        Microsoft.Win32.SystemEvents.SessionSwitch += OnSession;
        Rebuild(Screens.All());
        // Normal priority for the same reason as the workspace clock: at Background it
        // starved for seconds under UI Automation traffic and the notch stopped
        // answering the pointer. The work is a cursor read and a few rectangles.
        _poll = new DispatcherTimer(DispatcherPriority.Normal) { Interval = TimeSpan.FromMilliseconds(50) };
        _poll.Tick += (_, _) => Tick();
        _poll.Start();

        OwlApp.Timer.Changed += UpdateRest;
        OwlApp.Tick += UpdateRest;
        OwlApp.QuotasChanged += UpdateRest;
        OwlApp.SettingsChanged = UpdateRest;
        OwlApp.Collapse = CollapseAll;
        OwlApp.OpenDashboard = () => OpenDashboard();
        OwlApp.OpenSettings = () => OpenDashboard(settings: true);
        OwlApp.ShowWorkspace = Toggle;
        Theme.Changed += OnTheme;
        _alertEnd.Tick += (_, _) =>
        {
            _alertEnd.Stop();
            foreach (var h in _hosts.Values) h.ShowAlert(null);
        };
    }

    private void UpdateRest()
    {
        foreach (var h in _hosts.Values) h.UpdateRest();
    }

    private void OnTheme()
    {
        if (_dashboard is { IsLoaded: true }) _dashboard.Rebuild();
    }

    private static string Signature(List<ScreenInfo> screens) =>
        string.Join("|", screens.Select(s =>
            $"{s.Device}{(s.Primary ? "*" : "")}:{s.Bounds.Left},{s.Bounds.Top},{s.Bounds.Right},{s.Bounds.Bottom}:" +
            $"{s.Work.Left},{s.Work.Top},{s.Work.Right},{s.Work.Bottom}@{s.Scale}"));

    /// One notch, on the main display: a second one on every other screen was more
    /// in the way than useful.
    private void Rebuild(List<ScreenInfo> screens)
    {
        _signature = Signature(screens);
        var main = screens.FirstOrDefault(s => s.Primary) ?? screens.FirstOrDefault();
        var live = main is null ? new Dictionary<string, ScreenInfo>() : new Dictionary<string, ScreenInfo> { [main.Device] = main };
        foreach (var gone in _hosts.Keys.Where(d => !live.ContainsKey(d)).ToList())
        {
            _hosts[gone].Dispose();
            _hosts.Remove(gone);
        }
        foreach (var (device, screen) in live)
        {
            if (_hosts.TryGetValue(device, out var h)) h.UpdateScreen(screen);
            else _hosts[device] = new NotchHost(screen) { Manager = this };
        }
    }

    private void Tick()
    {
        var now = DateTime.Now;
        // Enumerating displays costs a P/Invoke per monitor; they rarely change.
        if (now - _lastDisplayCheck > TimeSpan.FromSeconds(2))
        {
            _lastDisplayCheck = now;
            var screens = Screens.All();
            if (Signature(screens) != _signature) Rebuild(screens);
        }
        var p = Screens.Cursor;
        var buttons = Win32.AnyMouseButtonDown;
        foreach (var h in _hosts.Values.ToList()) h.Poll(p, buttons, now);
    }

    /// Only one notch is open at a time.
    internal void OpenOn(NotchHost host, bool focusInput, bool peek = false)
    {
        foreach (var h in _hosts.Values)
            if (!ReferenceEquals(h, host)) h.Collapse();
        host.Expand(peek, focusInput);
    }

    /// The shortcut: open the notch, or close it.
    public void Toggle()
    {
        if (_hosts.Values.FirstOrDefault() is not { } host) return;
        if (host.State != NotchHost.Mode.Rest) host.Collapse();
        else OpenOn(host, focusInput: true);
    }

    public void CollapseAll()
    {
        foreach (var h in _hosts.Values) h.Collapse();
    }

    /// A few seconds of message in the notch — the timer ending, a reminder — so it
    /// is seen even when Windows is holding notifications back.
    public void Alert(string title, string text)
    {
        foreach (var h in _hosts.Values) h.ShowAlert((title, text));
        _alertEnd.Stop();
        _alertEnd.Start();
    }

    public void OpenDashboard(bool settings = false)
    {
        if (_dashboard is null || !_dashboard.IsLoaded)
        {
            _dashboard = new DashboardWindow();
            _dashboard.Closed += (_, _) => _dashboard = null;
        }
        if (settings) _dashboard.View.ShowTab(2);
        _dashboard.Show();
        if (_dashboard.WindowState == WindowState.Minimized) _dashboard.WindowState = WindowState.Normal;
        // Activate while this process still holds the foreground; collapsing first
        // would hand it back to the previous app and Windows would refuse this.
        _dashboard.Activate();
        CollapseAll();
    }

    public void Dispose()
    {
        Microsoft.Win32.SystemEvents.PowerModeChanged -= OnPower;
        Microsoft.Win32.SystemEvents.SessionSwitch -= OnSession;
        Theme.Changed -= OnTheme;
        OwlApp.QuotasChanged -= UpdateRest;
        _poll.Stop();
        _alertEnd.Stop();
        _dashboard?.Close();
        foreach (var h in _hosts.Values) h.Dispose();
        _hosts.Clear();
    }
}

/// "Open app": the same workspace in an ordinary window, for when the notch is too
/// small a place to work. Its title bar takes the panel's colour (Windows 11), so
/// the bar and the window read as one surface.
public sealed class DashboardWindow : Window
{
    public WorkspaceView View { get; private set; } = NewView();

    private static WorkspaceView NewView() => new(dashboard: true);

    public DashboardWindow()
    {
        Title = "Hover";
        Width = 1200;
        Height = 620;
        MinWidth = 880;
        MinHeight = 480;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Content = View;
        AutomationProperties.SetAutomationId(this, "HoverDashboard");
        SourceInitialized += (_, _) => ApplyTheme();
    }

    /// Build the view again for a new appearance, on the tab it was showing.
    public void Rebuild()
    {
        var tab = View.CurrentTab;
        View.Flush();
        View = NewView();
        Content = View;
        if (tab == 2) View.ShowSettings(SettingsPage.Last);
        else View.ShowTab(tab);
        ApplyTheme();
    }

    private void ApplyTheme()
    {
        Background = new SolidColorBrush(Ui.Panel);
        var hwnd = new WindowInteropHelper(this).Handle;
        if (hwnd == IntPtr.Zero) return;
        var dark = Theme.Dark ? 1 : 0;
        Win32.DwmSetWindowAttribute(hwnd, Win32.DWMWA_USE_IMMERSIVE_DARK_MODE, ref dark, sizeof(int));
        // COLORREF is 0x00BBGGRR. Older Windows ignores these and keeps its own bar.
        var c = Ui.Panel;
        var caption = c.R | (c.G << 8) | (c.B << 16);
        Win32.DwmSetWindowAttribute(hwnd, Win32.DWMWA_CAPTION_COLOR, ref caption, sizeof(int));
        var text = Theme.Dark ? 0x00FFFFFF : 0x00000000;
        Win32.DwmSetWindowAttribute(hwnd, Win32.DWMWA_TEXT_COLOR, ref text, sizeof(int));
    }
}
