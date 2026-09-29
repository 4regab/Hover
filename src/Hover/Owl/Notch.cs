using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;
using System.Windows.Threading;
using Hover.Core;
using Hover.Interop;
using Hover.Services;

namespace Hover.Owl;

/// The black notch shape and what is inside it. Openness 0 is the resting shape —
/// a slim island of glanceable items, a question from an agent, or nothing — and 1
/// is the open office. The shape grows from one to the other and the office is
/// revealed through it. The office fills the shape edge to edge: the shape is its
/// only frame.
internal sealed class NotchShell : Canvas
{
    public static readonly DependencyProperty OpennessProperty = DependencyProperty.Register(
        nameof(Openness), typeof(double), typeof(NotchShell),
        new PropertyMetadata(0.0, (d, _) => ((NotchShell)d).Relayout()));

    /// The resting shape's size as drawn: it springs to each new size rather than
    /// jumping, so the island breathes as its words change.
    private static readonly DependencyProperty RestWidthProperty = DependencyProperty.Register(
        "RestWidth", typeof(double), typeof(NotchShell), new PropertyMetadata(0.0, (d, _) => ((NotchShell)d).Relayout()));
    private static readonly DependencyProperty RestHeightProperty = DependencyProperty.Register(
        "RestHeight", typeof(double), typeof(NotchShell), new PropertyMetadata(0.0, (d, _) => ((NotchShell)d).Relayout()));
    /// How far the resting content has faded in after it changed, 0 to 1.
    private static readonly DependencyProperty FadeProperty = DependencyProperty.Register(
        "Fade", typeof(double), typeof(NotchShell), new PropertyMetadata(1.0, (d, _) => ((NotchShell)d).Relayout()));

    public double Openness
    {
        get => (double)GetValue(OpennessProperty);
        set => SetValue(OpennessProperty, value);
    }

    private readonly Path _shape = new();
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
    private bool _opening, _greet, _sized;
    private Color? _glow;
    // What the resting notch showed before it changed, as a picture fading out.
    private readonly Image _ghost = new() { IsHitTestVisible = false, Stretch = Stretch.None, Visibility = Visibility.Hidden };
    private double _ghostW;

    /// The resting content is about to change (an agent starts or asks, a task ends,
    /// the card opens). What shows now stays a moment as a picture and fades out, and
    /// the new content fades in a beat later, while the shape springs to its new size.
    /// Without it the words swapped at once inside a shape still on its way.
    public void Crossfade()
    {
        if (Animator.Still || Openness > 0.01) return;
        var w = Mini.ActualWidth;
        var h = Mini.ActualHeight;
        if (Mini.Visibility == Visibility.Visible && RestW >= 8 && w >= 1 && h >= 1)
        {
            var dpi = VisualTreeHelper.GetDpi(this);
            var bmp = new RenderTargetBitmap((int)Math.Ceiling(w * dpi.DpiScaleX), (int)Math.Ceiling(h * dpi.DpiScaleY),
                dpi.PixelsPerInchX, dpi.PixelsPerInchY, PixelFormats.Pbgra32);
            // Drawn through a brush, so the picture doesn't carry the Mini's offset.
            var dv = new DrawingVisual();
            using (var c = dv.RenderOpen())
                c.DrawRectangle(new VisualBrush(Mini) { Stretch = Stretch.None, AlignmentX = AlignmentX.Left, AlignmentY = AlignmentY.Top }, null, new Rect(0, 0, w, h));
            bmp.Render(dv);
            bmp.Freeze();
            _ghost.Source = bmp;
            _ghost.Width = _ghostW = w;
            _ghost.Height = h;
            _ghost.Visibility = Visibility.Visible;
            var out_ = new DoubleAnimation(1, 0, TimeSpan.FromMilliseconds(170)) { EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut } };
            out_.Completed += (_, _) => { _ghost.Visibility = Visibility.Hidden; _ghost.Source = null; };
            _ghost.BeginAnimation(OpacityProperty, out_);
        }
        var fade = new DoubleAnimationUsingKeyFrames();
        fade.KeyFrames.Add(new DiscreteDoubleKeyFrame(0, KeyTime.FromTimeSpan(TimeSpan.Zero)));
        fade.KeyFrames.Add(new LinearDoubleKeyFrame(0, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(90))));
        fade.KeyFrames.Add(new EasingDoubleKeyFrame(1, KeyTime.FromTimeSpan(TimeSpan.FromMilliseconds(320)), new QuadraticEase { EasingMode = EasingMode.EaseOut }));
        BeginAnimation(FadeProperty, fade);
        Relayout();
    }

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
        Children.Add(Mini);
        Children.Add(_ghost);
        Children.Add(ViewHost);
        Children.Add(Greeting);
        SizeChanged += (_, _) => Relayout();
        ApplyTheme();
    }

    /// The shadow under the shape: a soft one, or while resting a glow in a colour
    /// that means something (amber: an agent is asking; green or red: a task ended).
    public void ApplyTheme()
    {
        _shape.Effect = _glow is { } g
            ? new DropShadowEffect { Color = g, BlurRadius = 26, ShadowDepth = 0, Opacity = 0.62, RenderingBias = RenderingBias.Performance }
            : new DropShadowEffect
            {
                BlurRadius = Theme.Dark ? 24 : 36, ShadowDepth = Theme.Dark ? 4 : 10, Direction = 270,
                Opacity = Theme.Dark ? 0.5 : 0.22, RenderingBias = RenderingBias.Performance,
            };
        Relayout();
    }

    public void SetGlow(Color? glow)
    {
        if (_glow == glow) return;
        _glow = glow;
        ApplyTheme();
    }

    public void SetSizes(Size rest, Size open)
    {
        // The first size is set, not sprung to: that is the window being made.
        var first = !_sized;
        _sized = true;
        _rest = rest;
        _open = open;
        if (first || Animator.Still)
        {
            BeginAnimation(RestWidthProperty, null);
            BeginAnimation(RestHeightProperty, null);
            SetValue(RestWidthProperty, rest.Width);
            SetValue(RestHeightProperty, rest.Height);
        }
        else
        {
            // A little overshoot, as a spring settles.
            var d = new Duration(TimeSpan.FromMilliseconds(rest.Height > RestH + 40 ? 560 : 500));
            var ease = new BackEase { Amplitude = 0.22, EasingMode = EasingMode.EaseOut };
            BeginAnimation(RestWidthProperty, new DoubleAnimation(rest.Width, d) { EasingFunction = ease });
            BeginAnimation(RestHeightProperty, new DoubleAnimation(rest.Height, d) { EasingFunction = ease });
        }
        Relayout();
    }

    private double RestW => (double)GetValue(RestWidthProperty);
    private double RestH => (double)GetValue(RestHeightProperty);

    private static double Lerp(double a, double b, double t) => a + (b - a) * t;

    private void Relayout()
    {
        var t = Openness;
        var rest = new Size(Math.Max(0, RestW), Math.Max(0, RestH));
        var w = Lerp(rest.Width, _open.Width, t);
        var h = Lerp(rest.Height, _open.Height, t);
        var (rr, re) = RestCorners(rest);
        var r = Lerp(rr, 32, t);
        var ear = Lerp(re, 10, t);
        var cx = ActualWidth / 2;

        _shape.Visibility = w < 1 || h < 1 ? Visibility.Hidden : Visibility.Visible;
        _shape.Data = Outline(w, h, r, ear, 0);
        SetLeft(_shape, cx - w / 2);

        // Black while small, as a real notch is; the panel's own colour by the time
        // the office is in (the same black, in dark mode). Late enough that white
        // text on the way out stays legible.
        var k = Math.Clamp((t - 0.2) / 0.6, 0, 1);
        k = k * k * (3 - 2 * k);
        _fill.Color = Mix(Colors.Black, Ui.Panel, k);

        ViewHost.Width = _open.Width;
        ViewHost.Height = _open.Height;
        SetLeft(ViewHost, cx - _open.Width / 2);
        ViewHost.Clip = Outline(w, h, r, 0, (_open.Width - w) / 2);
        // Opening, the office comes in once the shape is well on its way; closing, it
        // goes first and the shape folds after it.
        ViewHost.Opacity = _opening ? Math.Clamp((t - 0.35) / 0.65, 0, 1) : Math.Clamp((t - 0.55) / 0.45, 0, 1);
        ViewHost.Visibility = t <= 0.001 && !_opening ? Visibility.Hidden : Visibility.Visible;
        ViewHost.IsHitTestVisible = t >= 0.999;

        // Laid out at the size it is going to, and shown through the size it is: the
        // words don't reflow while the shape springs.
        Mini.Width = _rest.Width;
        Mini.Height = _rest.Height;
        SetLeft(Mini, cx - _rest.Width / 2);
        Mini.Clip = new RectangleGeometry(new Rect((_rest.Width - rest.Width) / 2, 0, rest.Width, rest.Height), rr, rr);
        Mini.Opacity = Math.Clamp(1 - t * 3, 0, 1) * (double)GetValue(FadeProperty);
        Mini.Visibility = Mini.Opacity <= 0 ? Visibility.Hidden : Visibility.Visible;
        if (_ghost.Visibility == Visibility.Visible)
        {
            SetLeft(_ghost, cx - _ghostW / 2);
            _ghost.Clip = new RectangleGeometry(new Rect((_ghostW - rest.Width) / 2, 0, Math.Max(0, rest.Width), rest.Height), rr, rr);
        }

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

    /// A fully round island at rest, a card rounded as the open panel is, and a small
    /// flare into the screen edge either way.
    private static (double Radius, double Ear) RestCorners(Size s)
    {
        var r = s.Height > 60 ? 24 : Math.Min(s.Height / 2, 16);
        return (r, Math.Max(0, Math.Min(s.Height > 60 ? 10 : 7, s.Height - r)));
    }

    private static Color Mix(Color a, Color b, double k) => Color.FromArgb(
        (byte)Math.Round(a.A + (b.A - a.A) * k), (byte)Math.Round(a.R + (b.R - a.R) * k),
        (byte)Math.Round(a.G + (b.G - a.G) * k), (byte)Math.Round(a.B + (b.B - a.B) * k));

    /// A notch: square top edge flush with the screen, rounded bottom corners, and a
    /// concave flare where each side meets the top edge. x0 shifts it right.
    private static Geometry Outline(double w, double h, double r, double ear, double x0)
    {
        var g = new StreamGeometry();
        if (w < 1 || h < 1) { g.Freeze(); return g; }
        r = Math.Min(r, Math.Min(w / 2, h));
        ear = Math.Max(0, Math.Min(ear, h - r));
        using (var c = g.Open())
        {
            c.BeginFigure(new Point(x0 - ear, 0), true, true);
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

    private enum RestKind { None, Pill, Card }

    public string Device { get; }
    public Mode State { get; private set; } = Mode.Rest;
    public NotchManager? Manager { get; init; }

    private ScreenInfo _screen;
    private readonly HostWindow _window = new();
    private readonly NotchShell _shell = new();
    private OfficeView? _view;
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

    // The resting island: one segment per item, built once and updated in place.
    private readonly StackPanel _pill = new() { Orientation = Orientation.Horizontal };

    // The agents at work: their marks (the one spoken of in front, its ring turning),
    // what it is doing now, a verb and a file or command whose words rise in when
    // they change, and how long it has been at it.
    private readonly MarkStack _stack = new() { VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock _activity = Ui.Text("", 12.5, Ui.White, FontWeights.SemiBold);
    private readonly TextBlock _timer = Ui.Text("", 11.5, Dim, FontWeights.Medium);
    private readonly FrameworkElement _workSeg;
    private int _speaker, _ticks;
    private readonly DispatcherTimer _clock = new(DispatcherPriority.Normal) { Interval = TimeSpan.FromSeconds(1) };

    // An agent waiting on the user: its mark in a breathing amber ring, what it wants
    // in a few words, Deny, and Review, which opens the card.
    private readonly LiveMark _askMark = new() { Width = 26, Height = 26, Ring = LiveMark.Rings.Breathe, RingColor = Amber, VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock _askText = Ui.Text("", 12.5, Ui.White, FontWeights.SemiBold);
    private readonly FrameworkElement _askSeg;
    private (KiroSession Session, AgentAsk Ask)? _asked;

    // A task ended and nobody has looked yet: its mark with a badge, and the task.
    private readonly LiveMark _doneMark = new() { Width = 18, Height = 18, TileSize = 18, VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock _doneText = Ui.Text("", 12.5, Ui.White, FontWeights.SemiBold);
    private readonly TextBlock _doneTook = Ui.Text("", 11.5, Dim, FontWeights.Medium);
    private readonly FrameworkElement _doneSeg;
    private int _doneShown;
    /// The green or red glow of an ending lasts a moment; the words stay until seen.
    private readonly DispatcherTimer _endGlow = new(DispatcherPriority.Normal) { Interval = TimeSpan.FromSeconds(6) };

    private readonly Border _divider = new() { Width = 1, Height = 14, Background = Ui.Frozen(Color.FromArgb(0x2E, 0xFF, 0xFF, 0xFF)), VerticalAlignment = VerticalAlignment.Center };
    private readonly Dictionary<string, (FrameworkElement Seg, LiveMark Mark, TextBlock Text)> _quotaSegs = new();
    private string _pillKey = "";

    // The question in full, in the notch: open from Review, answered there.
    private readonly Border _card = new() { Visibility = Visibility.Collapsed, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Top };
    private bool _cardOpen;
    private string _cardKey = "";
    private IntPtr _cardPrevious;


    /// Heights of the resting shapes. Small on purpose: the notch at rest is a hint,
    /// not a panel — a slim island when there is something to show, and nothing when
    /// there is not. The user knows where it is.
    private const double PillHeight = 32, PillPadLeft = 4, PillPadRight = 7, PillGap = 12;
    private static readonly TimeSpan Dwell = TimeSpan.FromMilliseconds(120);
    private static readonly TimeSpan LeaveGrace = TimeSpan.FromMilliseconds(350);

    private static readonly Color Amber = Color.FromRgb(0xFF, 0xB3, 0x40);
    private static readonly Brush Dim = Ui.Frozen(Color.FromArgb(0x8C, 0xFF, 0xFF, 0xFF));
    private static readonly Brush Faint = Ui.Frozen(Color.FromArgb(0x5C, 0xFF, 0xFF, 0xFF));
    private static readonly FontFamily Mono = new("Cascadia Code, Cascadia Mono, Consolas, Courier New");

    public NotchHost(ScreenInfo screen)
    {
        Device = screen.Device;
        _screen = screen;
        _window.Title = "Hover notch";
        AutomationProperties.SetAutomationId(_window, "HoverNotch");
        AutomationProperties.SetAutomationId(_activity, "NotchKiro");
        AutomationProperties.SetAutomationId(_doneText, "NotchKiroDone");
        AutomationProperties.SetAutomationId(_askText, "NotchAsk");
        foreach (var t in new[] { _timer, _doneTook }) t.Typography.NumeralAlignment = FontNumeralAlignment.Tabular;
        _activity.RenderTransform = new TranslateTransform();
        _activity.MaxWidth = 300;
        _doneText.MaxWidth = 280;
        _askText.MaxWidth = 260;
        _workSeg = Ui.Row(_stack, _activity.Margin(9, 0), _timer.Margin(9, 0));
        _askSeg = Ui.Row(_askMark, _askText.Margin(9, 0), Rule().Margin(11, 0),
            Pressable(Ui.Text("Deny", 11.5, Ui.White, FontWeights.SemiBold), "NotchAskDeny", "Deny", () => AnswerAsked(AskAnswer.Deny), small: true).Margin(10, 0),
            Pressable(Ui.Text("Review", 11.5, Ui.Black, FontWeights.SemiBold), "NotchAskReview", "Review", OpenCard, primary: true, small: true).Margin(6, 0, 3));
        _doneSeg = Ui.Row(_doneMark, _doneText.Margin(9, 0), _doneTook.Margin(9, 0));
        foreach (var t in new[] { _activity, _timer, _askText, _doneText, _doneTook }) t.VerticalAlignment = VerticalAlignment.Center;
        _clock.Tick += (_, _) =>
        {
            // The speaker changes every three seconds when several are at work.
            if (++_ticks % 3 == 0) _speaker++;
            UpdateRest();
        };
        _endGlow.Tick += (_, _) => { _endGlow.Stop(); Glow(); };
        _window.Root.Children.Add(_shell);
        Theme.Changed += OnTheme;

        _window.SourceInitialized += (_, _) => _hwnd = new WindowInteropHelper(_window).Handle;
        _window.DpiChanged += (_, _) => _window.Dispatcher.BeginInvoke(Layout, DispatcherPriority.Background);
        _window.KeyDown += (_, e) =>
        {
            // The question's card has the keyboard while it is open.
            if (State == Mode.Rest && _kind == RestKind.Card)
            {
                if (e.Key == Key.Enter) { e.Handled = true; AnswerAsked(Keyboard.Modifiers.HasFlag(ModifierKeys.Shift) ? AskAnswer.Trust : AskAnswer.Allow); }
                else if (e.Key == Key.Escape) { e.Handled = true; AnswerAsked(AskAnswer.Deny); }
                return;
            }
            // Esc closes — unless something inside (a rename box, a popover) used it first.
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
            // The card is answered where it is; a click on it elsewhere does nothing.
            if (_kind == RestKind.Card) return;
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

    /// The open workspace's size from Settings → Workspace, kept inside the display.
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
        // make the island twitch.
        // DesiredSize already holds the row's left margin (PillPadLeft); adding it
        // again left that much extra room after the last item.
        RestKind.Pill => new Size(Math.Ceiling((_pill.DesiredSize.Width + PillPadRight) / 2) * 2, PillHeight),
        RestKind.Card => new Size(Math.Ceiling(_card.DesiredSize.Width), Math.Ceiling(_card.DesiredSize.Height)),
        _ => new Size(0, 0),
    };

    /// The resting notch holds a slim row of glanceable items, a short message, or
    /// an agent's question in full.
    private void BuildMini()
    {
        var m = _shell.Mini;
        _pill.HorizontalAlignment = HorizontalAlignment.Left;
        _pill.VerticalAlignment = VerticalAlignment.Center;
        _pill.Margin = new Thickness(PillPadLeft, 0, 0, 0);
        m.Children.Add(_pill);

        m.Children.Add(_card);
    }

    /// A quota: the tool's mark inside its ring, and the share used. No name; it is in
    /// the tooltip and for UI Automation.
    private (FrameworkElement Seg, LiveMark Mark, TextBlock Text) QuotaSeg(string id)
    {
        if (_quotaSegs.TryGetValue(id, out var q)) return q;
        var mark = new LiveMark { Tool = id, Width = 24, Height = 24, TileSize = 14, Ring = LiveMark.Rings.Value, VerticalAlignment = VerticalAlignment.Center };
        var value = Ui.Text("—", 11.5, Ui.White, FontWeights.SemiBold);
        value.Typography.NumeralAlignment = FontNumeralAlignment.Tabular;
        var seg = Ui.Row(mark, value.Margin(5, 0));
        // The row is a panel, which UI Automation does not see; the value is text.
        AutomationProperties.SetAutomationId(value, "NotchQuota" + char.ToUpperInvariant(id[0]) + id[1..]);
        return _quotaSegs[id] = (seg, mark, value);
    }

    private const string Work = "work", Asked = "ask", Ended = "done", Divider = "|";

    private FrameworkElement Segment(string id) => id switch
    {
        Work => _workSeg,
        Asked => _askSeg,
        Ended => _doneSeg,
        Divider => _divider,
        _ => QuotaSeg(id).Seg,
    };

    private static Border Rule() => new() { Width = 1, Height = 14, Background = Ui.Frozen(Color.FromArgb(0x2E, 0xFF, 0xFF, 0xFF)), VerticalAlignment = VerticalAlignment.Center };

    /// Pick the resting shape and refresh what it shows. Most urgent first: an agent's
    /// question, then the agents at work, then a task that ended unseen; the quotas
    /// sit after them.
    public void UpdateRest()
    {
        var all = OwlApp.Kiro.All;
        var waiting = all.Where(s => s.Waiting).ToList();
        var working = all.Where(s => s.Busy && !s.Waiting).ToList();
        var quotas = Settings.NotchItems.Where(NotchItem.Quotas.Contains).ToList();

        var items = new List<string>();
        if (waiting.Count > 0) items.Add(Asked);
        else if (working.Count > 0) items.Add(Work);
        else if (OwlApp.KiroUnseen > 0 && OwlApp.KiroUnseenLast is not null) items.Add(Ended);
        if (items.Count > 0 && quotas.Count > 0) items.Add(Divider);
        items.AddRange(quotas);
        if (waiting.Count == 0 && _cardOpen) CloseCard();
        var kind = _cardOpen ? RestKind.Card
            : items.Count > 0 ? RestKind.Pill
            : RestKind.None;
        // A new shape or new segments cross-fade; new words in the same segments rise in.
        if (_kind != (RestKind)(-1) && State == Mode.Rest && (kind != _kind || (kind == RestKind.Pill && string.Join(",", items) != _pillKey)))
            _shell.Crossfade();

        _pill.Visibility = kind == RestKind.Pill ? Visibility.Visible : Visibility.Collapsed;
        _card.Visibility = kind == RestKind.Card ? Visibility.Visible : Visibility.Collapsed;

        _asked = waiting.Count > 0 && waiting[0].Asking is { } first ? (waiting[0], first) : null;
        if (kind == RestKind.Card && _asked is { } q)
        {
            var total = waiting.Sum(s => s.Asks.Count);
            var key = $"{q.Session.Id}:{q.Ask.Id}:{total}";
            if (key != _cardKey)
            {
                _cardKey = key;
                _card.Child = Card(q.Session, q.Ask, total);
            }
            _card.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }

        if (kind == RestKind.Pill)
        {
            var key = string.Join(",", items);
            if (key != _pillKey)
            {
                _pillKey = key;
                _pill.Children.Clear();
                foreach (var id in items)
                {
                    var seg = Segment(id);
                    seg.Margin = new Thickness(_pill.Children.Count == 0 ? 0 : PillGap, 0, 0, 0);
                    seg.VerticalAlignment = VerticalAlignment.Center;
                    _pill.Children.Add(seg);
                }
            }
            if (items.Contains(Work)) ShowWork(working);
            if (items.Contains(Asked) && _asked is { } a) ShowAsk(a.Session, a.Ask, waiting.Sum(s => s.Asks.Count));
            if (items.Contains(Ended)) ShowEnded();
            else _doneShown = 0;
            foreach (var id in quotas)
            {
                var (_, mark, text) = QuotaSeg(id);
                var reading = OwlApp.Quotas.TryGetValue(id, out var r) ? r.Reading : null;
                mark.Value = reading?.Used;
                text.Inlines.Clear();
                if (reading?.Used is { } u) { text.Inlines.Add(new Run($"{u:0}")); text.Inlines.Add(new Run("%") { Foreground = Faint, FontSize = 10.5 }); }
                else text.Inlines.Add(new Run("—"));
                text.Foreground = reading is null || reading.Ok ? Ui.White : Ui.WhiteDim;
                var name = NotchItem.Short(id);
                var said = reading?.Used is { } used ? $"{name} {used:0}% used" : $"{name} quota";
                AutomationProperties.SetName(text, said);
                text.ToolTip = said;
            }
            // New text in a segment marks only that text for measuring; its row and the
            // island would answer from their last measure, too small, and cut the words
            // off (the island stays short until the next tick). So all of it is measured
            // again.
            Remeasure(_pill);
            _pill.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
        }
        else _pillKey = "";

        // The clock runs while there is a timer to tick or a speaker to change.
        if (working.Count > 0 || waiting.Count > 0) { if (!_clock.IsEnabled) _clock.Start(); }
        else { _clock.Stop(); _ticks = 0; }

        _kind = kind;
        Glow();
        // Settings → Workspace → Workspace size, or nothing: the comparison is cheap.
        if (OpenSize() != _open) { Layout(); _restApplied = RestSize; return; }
        var rest = RestSize;
        if (rest == _restApplied) return;
        _restApplied = rest;
        _shell.SetSizes(rest, _open);
    }

    private static void Remeasure(DependencyObject d)
    {
        if (d is UIElement e) e.InvalidateMeasure();
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(d); i++) Remeasure(VisualTreeHelper.GetChild(d, i));
    }

    /// Amber while an agent asks, green or red for a moment when a task ends unseen;
    /// only at rest.
    private void Glow()
    {
        Color? glow = null;
        if (State == Mode.Rest)
        {
            if (_kind == RestKind.Card || (_kind == RestKind.Pill && _pillKey.Contains(Asked))) glow = Amber;
            else if (_kind == RestKind.Pill && _pillKey.Contains(Ended) && _endGlow.IsEnabled && OwlApp.KiroUnseenLast is { } last)
                glow = last.State == KiroState.Completed ? Color.FromRgb(0x32, 0xD7, 0x4B) : last.State == KiroState.Failed ? Color.FromRgb(0xFF, 0x45, 0x3A) : null;
        }
        _shell.SetGlow(glow);
    }

    private void ShowWork(List<KiroSession> working)
    {
        var speaker = working[_speaker % working.Count];
        _stack.Show(working.Select(s => Agents.Id(s.Tool)).ToList(), working.IndexOf(speaker));
        var (verb, obj) = AgentWords.Activity(speaker);
        SetActivity(_activity, $"{speaker.Id}:{verb}:{obj}", verb, obj);
        _timer.Text = Clock(speaker.Elapsed);
        var others = working.Count > 1 ? $", and {working.Count - 1} more at work" : "";
        AutomationProperties.SetName(_activity, $"{Agents.Name(speaker.Tool)}: {verb} {obj}".Trim() + others);
    }

    private void ShowAsk(KiroSession s, AgentAsk a, int total)
    {
        _askMark.Tool = Agents.Id(s.Tool);
        var (verb, obj) = AgentWords.AskLine(a);
        _askText.Inlines.Clear();
        _askText.Inlines.Add(new Run(verb) { Foreground = Dim, FontWeight = FontWeights.Medium });
        if (obj.Length > 0) _askText.Inlines.Add(new Run(" " + obj) { FontFamily = a.Command is not null ? Mono : Ui.Font, FontSize = a.Command is not null ? 12 : 12.5 });
        if (total > 1) _askText.Inlines.Add(new Run($"  +{total - 1}") { Foreground = Faint, FontWeight = FontWeights.Medium });
        AutomationProperties.SetName(_askText, $"{Agents.Name(s.Tool)} {verb.ToLowerInvariant()} {obj}".Trim());
    }

    private void ShowEnded()
    {
        if (OwlApp.KiroUnseenLast is not { } last) return;
        _doneMark.Tool = Agents.Id(last.Tool);
        _doneMark.Badge = last.State switch
        {
            KiroState.Completed => LiveMark.Badges.Done,
            KiroState.Failed => LiveMark.Badges.Failed,
            _ => LiveMark.Badges.None,
        };
        var n = OwlApp.KiroUnseen;
        var verb = last.State switch { KiroState.Completed => "Done", KiroState.Failed => "Couldn’t finish", _ => "Stopped" };
        _doneText.Inlines.Clear();
        _doneText.Inlines.Add(new Run(verb + " · ") { Foreground = Dim, FontWeight = FontWeights.Medium });
        _doneText.Inlines.Add(new Run(last.Title.Length > 0 ? last.Title : "the task"));
        _doneTook.Text = n > 1 ? $"+{n - 1}" : Took(last.Took);
        if (n != _doneShown)
        {
            _doneShown = n;
            Rise(_doneText);
            _endGlow.Stop();
            _endGlow.Start();
        }
        var who = OwlApp.KiroUnseenTool;
        AutomationProperties.SetName(_doneText, n > 1 ? (who is null ? $"{n} tasks ended" : $"{n} {who} tasks ended") : $"{Agents.Name(last.Tool)} {verb.ToLowerInvariant()}: {last.Title}");
    }

    /// New words rise into place; the same words are left alone.
    private static void SetActivity(TextBlock t, string key, string verb, string obj)
    {
        if (t.Tag as string == key) return;
        t.Tag = key;
        t.Inlines.Clear();
        t.Inlines.Add(new Run(verb) { Foreground = Dim, FontWeight = FontWeights.Medium });
        if (obj.Length > 0) t.Inlines.Add(new Run(" " + obj));
        Rise(t);
    }

    private static void Rise(TextBlock t)
    {
        if (Animator.Still) return;
        if (t.RenderTransform is not TranslateTransform) t.RenderTransform = new TranslateTransform();
        var d = new Duration(TimeSpan.FromMilliseconds(320));
        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        t.BeginAnimation(UIElement.OpacityProperty, new DoubleAnimation(0, 1, d));
        t.RenderTransform.BeginAnimation(TranslateTransform.YProperty, new DoubleAnimation(7, 0, d) { EasingFunction = ease });
    }

    private static string Clock(TimeSpan t) => t.TotalHours >= 1 ? $"{(int)t.TotalHours}:{t.Minutes:00}:{t.Seconds:00}" : $"{(int)t.TotalMinutes}:{t.Seconds:00}";

    private static string Took(TimeSpan t) => t.TotalSeconds < 60 ? $"{Math.Max(1, (int)Math.Round(t.TotalSeconds))} s"
        : t.TotalMinutes < 60 ? $"{(int)t.TotalMinutes}m {t.Seconds:00}s" : $"{(int)t.TotalHours}h {t.Minutes:00}m";

    // MARK: The question's card

    /// Review: the question in full, in the notch, which takes the keyboard (Enter
    /// allows, Shift+Enter trusts, Esc denies) until it is answered.
    private void OpenCard()
    {
        if (_asked is null || State != Mode.Rest) return;
        _cardOpen = true;
        _cardKey = "";
        _cardPrevious = Win32.GetForegroundWindow();
        _window.SetAcceptsKeys(true);
        _window.Focus(foreground: true);
        UpdateRest();
    }

    private void CloseCard(bool giveBack = true)
    {
        if (!_cardOpen) return;
        _cardOpen = false;
        _cardKey = "";
        _card.Child = null;
        if (!giveBack || State != Mode.Rest) return;
        // The keyboard goes back to whatever had it before Review.
        if (_hwnd != IntPtr.Zero && Win32.GetForegroundWindow() == _hwnd &&
            _cardPrevious != IntPtr.Zero && Win32.IsWindow(_cardPrevious))
            Win32.SetForegroundWindow(_cardPrevious);
        _window.SetAcceptsKeys(false);
    }

    private void AnswerAsked(AskAnswer answer)
    {
        if (_asked is not { } a) return;
        Log.Line($"{Agents.Id(a.Session.Tool)} run {a.Session.Id}: {answer.ToString().ToLowerInvariant()} from the notch");
        a.Session.Answer(a.Ask.Id, answer);
        UpdateRest();
    }

    private FrameworkElement Card(KiroSession s, AgentAsk a, int total)
    {
        var root = new StackPanel { Width = 472, Margin = new Thickness(14, 12, 14, 14) };

        var head = new DockPanel();
        var mark = new LiveMark { Tool = Agents.Id(s.Tool), Width = 26, Height = 26, TileSize = 26, VerticalAlignment = VerticalAlignment.Center };
        DockPanel.SetDock(mark, Dock.Left);
        head.Children.Add(mark);
        if (total > 1)
        {
            var count = new Border
            {
                Background = Ui.Frozen(Color.FromArgb(0x17, 0xFF, 0xFF, 0xFF)), CornerRadius = new CornerRadius(99),
                Padding = new Thickness(8, 2, 8, 3), VerticalAlignment = VerticalAlignment.Center,
                Child = Ui.Text($"1 of {total}", 11, Dim, FontWeights.SemiBold),
            };
            DockPanel.SetDock(count, Dock.Right);
            head.Children.Add(count);
        }
        var titles = new StackPanel { Margin = new Thickness(10, 0, 8, 0), VerticalAlignment = VerticalAlignment.Center };
        var title = Ui.Text(AgentWords.AskTitle(a), 13.5, Ui.White, FontWeights.SemiBold);
        AutomationProperties.SetAutomationId(title, "NotchAskTitle");
        titles.Children.Add(title);
        var folder = System.IO.Path.GetFileName(s.Folder.TrimEnd('\\', '/'));
        titles.Children.Add(Ui.Text($"{folder} · {s.Title}", 11.5, Dim).Margin(0, 1, 0, 0));
        head.Children.Add(titles);
        root.Children.Add(head);

        var code = new TextBlock { FontFamily = Mono, FontSize = 12.5, Foreground = Ui.White, TextWrapping = TextWrapping.Wrap, LineHeight = 19 };
        if (a.Command is { } cmd)
        {
            code.Inlines.Add(new Run("$  ") { Foreground = Ui.Frozen(Amber) });
            code.Inlines.Add(new Run(cmd));
        }
        else
        {
            if (a.Path is { } path) code.Inlines.Add(new Run(path) { Foreground = a.Preview is null ? Ui.White : Dim, FontSize = a.Preview is null ? 12.5 : 11.5 });
            if (a.Preview is { } preview)
                foreach (var line in preview.Split('\n'))
                {
                    if (code.Inlines.Count > 0) code.Inlines.Add(new LineBreak());
                    code.Inlines.Add(new Run(line)
                    {
                        FontSize = 11.5,
                        Foreground = Ui.Frozen(line.StartsWith('+') ? Color.FromRgb(0x9D, 0xF0, 0xAE) : line.StartsWith('-') ? Color.FromRgb(0xFF, 0xAA, 0xA4) : Colors.White),
                    });
                }
            if (code.Inlines.Count == 0) code.Inlines.Add(new Run(a.Title));
        }
        root.Children.Add(new Border
        {
            Background = Ui.Frozen(Color.FromRgb(0x0F, 0x0F, 0x12)), CornerRadius = new CornerRadius(12),
            BorderBrush = Ui.Frozen(a.Danger ? Color.FromArgb(0x66, 0xFF, 0x45, 0x3A) : Color.FromArgb(0x17, 0xFF, 0xFF, 0xFF)), BorderThickness = new Thickness(1),
            Padding = new Thickness(12, 9, 12, 10), Margin = new Thickness(0, 11, 0, 10), MaxHeight = 150, Child = code,
        });

        var why = a.Reason + (a.Added + a.Removed > 0 && a.Kind != "edit" ? $" · +{a.Added} −{a.Removed}" : "");
        root.Children.Add(Ui.Row(new Ellipse { Width = 6, Height = 6, Fill = Ui.Frozen(a.Danger ? Color.FromRgb(0xFF, 0x45, 0x3A) : Amber), VerticalAlignment = VerticalAlignment.Center },
            Ui.Text(why, 11.5, Dim).Margin(8, 0)));

        var buttons = new DockPanel { Margin = new Thickness(0, 12, 0, 0), LastChildFill = false };
        var deny = Pressable(Label("Deny", "Esc", Ui.White), "NotchCardDeny", "Deny", () => AnswerAsked(AskAnswer.Deny));
        DockPanel.SetDock(deny, Dock.Left);
        buttons.Children.Add(deny);
        var go = AgentWords.AskAllow(a);
        var allow = Pressable(Label(go, "Enter", a.Danger ? Ui.White : Ui.Black), "NotchCardAllow", go, () => AnswerAsked(AskAnswer.Allow), primary: !a.Danger, danger: a.Danger);
        allow.Margin = new Thickness(8, 0, 0, 0);
        Border? more = null;
        more = Pressable(Ui.Icon(Ui.IcChevronDown, 11, Ui.White), "NotchCardTrustMore", "More ways to trust", () =>
        {
            var m = new ContextMenu();
            m.Items.Add(Ui.MenuText("Trust this for the rest of the session", () => AnswerAsked(AskAnswer.Trust)));
            m.Items.Add(Ui.MenuText("Allow everything this session", () => AnswerAsked(AskAnswer.TrustAll)));
            Ui.Open(m, more!);
        }, corners: new CornerRadius(0, 9, 9, 0));
        more.Padding = new Thickness(8, 0, 8, 0);
        var trust = Pressable(Label("Trust", "Shift+Enter", Ui.White), "NotchCardTrust", "Trust for the rest of the session", () => AnswerAsked(AskAnswer.Trust),
            corners: new CornerRadius(9, 0, 0, 9));
        trust.Margin = new Thickness(0, 0, 1, 0);
        foreach (var b in new FrameworkElement[] { allow, more, trust })
        {
            DockPanel.SetDock(b, Dock.Right);
            buttons.Children.Add(b);
        }
        root.Children.Add(buttons);
        return root;
    }

    /// A button's words and the key that does the same.
    private static FrameworkElement Label(string text, string key, Brush ink) => Ui.Row(
        Ui.Text(text, 12.5, ink, FontWeights.SemiBold),
        new Border
        {
            Margin = new Thickness(8, 0, 0, 0), Padding = new Thickness(5, 0, 5, 1), CornerRadius = new CornerRadius(5),
            BorderThickness = new Thickness(1), VerticalAlignment = VerticalAlignment.Center,
            BorderBrush = Ui.Frozen(ReferenceEquals(ink, Ui.Black) ? Color.FromArgb(0x29, 0, 0, 0) : Color.FromArgb(0x24, 0xFF, 0xFF, 0xFF)),
            Child = new TextBlock { Text = key, FontFamily = Mono, FontSize = 10, Foreground = ink, Opacity = 0.7 },
        });

    /// A button drawn for the black notch: a rounded fill that brightens under the
    /// pointer. It acts on release over it, and keeps the click from opening the office.
    private static Border Pressable(FrameworkElement content, string id, string name, Action click,
        bool primary = false, bool danger = false, bool small = false, CornerRadius? corners = null)
    {
        Color Of(bool hot) => primary ? (hot ? Colors.White : Color.FromRgb(0xF2, 0xF2, 0xF5))
            : danger ? (hot ? Color.FromRgb(0xFF, 0x5B, 0x51) : Color.FromRgb(0xFF, 0x45, 0x3A))
            : Color.FromArgb(hot ? (byte)0x2E : (byte)0x1C, 0xFF, 0xFF, 0xFF);
        var fill = new SolidColorBrush(Of(false));
        var b = new Border
        {
            Background = fill, CornerRadius = corners ?? new CornerRadius(small ? 7 : 9), Height = small ? 22 : 30,
            Padding = new Thickness(small ? 9 : 12, 0, small ? 9 : 12, 0), Child = content, Cursor = Cursors.Hand,
            VerticalAlignment = VerticalAlignment.Center, ToolTip = name,
        };
        content.VerticalAlignment = VerticalAlignment.Center;
        if (content is TextBlock tb) { AutomationProperties.SetAutomationId(tb, id); AutomationProperties.SetName(tb, name); }
        else AutomationProperties.SetName(content, name);
        b.MouseEnter += (_, _) => fill.Color = Of(true);
        b.MouseLeave += (_, _) => fill.Color = Of(false);
        b.MouseLeftButtonDown += (_, e) => { e.Handled = true; b.CaptureMouse(); };
        b.MouseLeftButtonUp += (_, e) =>
        {
            e.Handled = true;
            if (!b.IsMouseCaptured) return;
            b.ReleaseMouseCapture();
            var p = e.GetPosition(b);
            if (p.X >= 0 && p.Y >= 0 && p.X <= b.ActualWidth && p.Y <= b.ActualHeight) click();
        };
        return b;
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
                // A question waits to be answered here, not covered by the office.
                if (!_armed || buttons || !Settings.HoverOpensWorkspace || _kind == RestKind.Card || _pillKey.Contains(Asked)) return;
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
            // The office shows the question over the agent's head; the card goes.
            if (_cardOpen) CloseCard(giveBack: false);
            _shell.SetGlow(null);
            _previous = Win32.GetForegroundWindow();
            _view ??= NewView();
            _window.SetAcceptsKeys(true);
            _window.Raise();
            _shell.Opening = true;
            if (Manager?.TakeGreeting() == true) Greet();
            else Animate(1, 560, new BackEase { Amplitude = 0.16, EasingMode = EasingMode.EaseOut });
        }
        State = peek && State != Mode.Open ? Mode.Peek : Mode.Open;
        _outsideSince = null;
        if (!focusInput) return;
        _window.Focus(foreground: true);
        // After the first layout pass, or the office is not in the tree yet.
        _window.Dispatcher.BeginInvoke(() => _view?.FocusOffice(), DispatcherPriority.Input);
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

    private OfficeView NewView()
    {
        var v = new OfficeView(dashboard: false);
        _shell.ViewHost.Child = v;
        return v;
    }

    /// Light and dark are baked into the views when they are built, so a switch
    /// builds them again — straight away while open, on the next opening otherwise.
    private void OnTheme()
    {
        _shell.ApplyTheme();
        if (_view is null) return;
        if (State == Mode.Rest)
        {
            _shell.ViewHost.Child = null;
            _view = null;
            return;
        }
        var settings = _view.InSettings;
        _view = NewView();
        if (settings) _view.ShowSettings(SettingsPage.Last);
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
        Animate(0, 340, new SineEase { EasingMode = EasingMode.EaseInOut });
        UpdateRest();
    }

    /// Focus left for another app: the notch closes. Our own popups, menus and
    /// dialogs (owned by this window) do not count.
    private void ClickedAway()
    {
        // The question's card had the keyboard; clicking elsewhere folds it back to
        // the amber island, still waiting.
        if (State == Mode.Rest && _cardOpen)
        {
            CloseCard(giveBack: false);
            _window.SetAcceptsKeys(false);
            UpdateRest();
            return;
        }
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

        OwlApp.QuotasChanged += UpdateRest;
        OwlApp.Kiro.Changed += UpdateRest;
        OwlApp.SettingsChanged = UpdateRest;
        OwlApp.Collapse = CollapseAll;
        OwlApp.OpenDashboard = () => OpenDashboard();
        OwlApp.OpenSettings = () => OpenDashboard(settings: true);
        OwlApp.ShowOffice = Toggle;
        Theme.Changed += OnTheme;
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

    public void OpenDashboard(bool settings = false)
    {
        if (_dashboard is null || !_dashboard.IsLoaded)
        {
            _dashboard = new DashboardWindow();
            _dashboard.Closed += (_, _) => _dashboard = null;
        }
        if (settings) _dashboard.View.ShowSettings(SettingsPage.Section.General);
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
        OwlApp.Kiro.Changed -= UpdateRest;
        _poll.Stop();
        _dashboard?.Close();
        foreach (var h in _hosts.Values) h.Dispose();
        _hosts.Clear();
    }
}

/// "Open app": the same office in an ordinary window, for when the notch is too
/// small a place to work. It draws its own title bar in the office's colour (logo,
/// name, minimize, maximize, close), so the bar and the office read as one surface.
/// WindowChrome keeps Windows' own dragging, snapping, resizing and shadow.
public sealed class DashboardWindow : Window
{
    public OfficeView View { get; private set; } = NewView();

    private static OfficeView NewView() => new(dashboard: true);

    private const double BarHeight = 34;
    private static readonly Color BarColor = Color.FromRgb(0x0B, 0x08, 0x10);
    private readonly Grid _frame = new();
    private readonly Button _max;

    public DashboardWindow()
    {
        Title = "Hover";
        Width = 1200;
        Height = 620;
        MinWidth = 880;
        MinHeight = 480;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        Background = new SolidColorBrush(BarColor);
        System.Windows.Shell.WindowChrome.SetWindowChrome(this, new System.Windows.Shell.WindowChrome
        {
            CaptionHeight = BarHeight,
            ResizeBorderThickness = new Thickness(6),
            GlassFrameThickness = new Thickness(0, 0, 0, 1),
            CornerRadius = new CornerRadius(0),
            UseAeroCaptionButtons = false,
        });

        var bar = new Grid { Height = BarHeight, Background = Background };
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var logo = new Image
        {
            Source = new System.Windows.Media.Imaging.BitmapImage(new Uri("pack://application:,,,/Hover;component/Assets/hover-mark.png")),
            Width = 16, Height = 16, Margin = new Thickness(12, 0, 8, 0), VerticalAlignment = VerticalAlignment.Center,
        };
        RenderOptions.SetBitmapScalingMode(logo, BitmapScalingMode.HighQuality);
        var name = Ui.Text("Hover", 12, Ui.Frozen(Color.FromArgb(0xD9, 0xFF, 0xFF, 0xFF)), FontWeights.Medium);
        name.VerticalAlignment = VerticalAlignment.Center;
        var left = new StackPanel { Orientation = Orientation.Horizontal, Children = { logo, name } };
        bar.Children.Add(left);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal };
        buttons.Children.Add(Caption("\uE921", "Minimize", "OwlCaptionButton", () => WindowState = WindowState.Minimized));
        _max = Caption("\uE922", "Maximize", "OwlCaptionButton",
            () => WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized);
        buttons.Children.Add(_max);
        buttons.Children.Add(Caption("\uE8BB", "Close", "OwlCaptionClose", Close));
        Grid.SetColumn(buttons, 1);
        bar.Children.Add(buttons);

        _frame.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        _frame.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        _frame.Children.Add(bar);
        Place(View);
        Content = _frame;
        AutomationProperties.SetAutomationId(this, "HoverDashboard");
        StateChanged += (_, _) => OnState();
    }

    private Button Caption(string glyph, string name, string style, Action click)
    {
        var b = new Button { Content = glyph, Style = (Style)Application.Current.FindResource(style), ToolTip = name };
        AutomationProperties.SetName(b, name);
        System.Windows.Shell.WindowChrome.SetIsHitTestVisibleInChrome(b, true);
        b.Click += (_, _) => click();
        return b;
    }

    // Maximized, Windows lays the window a resize border past each screen edge; the
    // padding keeps the bar and the office on the screen.
    private void OnState()
    {
        var max = WindowState == WindowState.Maximized;
        _frame.Margin = max ? SystemParameters.WindowResizeBorderThickness : new Thickness(0);
        _max.Content = max ? "\uE923" : "\uE922";
        _max.ToolTip = max ? "Restore" : "Maximize";
        AutomationProperties.SetName(_max, (string)_max.ToolTip);
    }

    private void Place(OfficeView v)
    {
        if (_frame.Children.Count > 1) _frame.Children.RemoveAt(1);
        Grid.SetRow(v, 1);
        _frame.Children.Add(v);
    }

    /// Build the view again for a new appearance, on the page it was showing.
    public void Rebuild()
    {
        var settings = View.InSettings;
        View = NewView();
        Place(View);
        if (settings) View.ShowSettings(SettingsPage.Last);
    }
}
