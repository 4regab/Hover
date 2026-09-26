using System.Globalization;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;

namespace Hover.Owl;

/// The workspace's palette and the small builders every card uses. The look is
/// dark graphite cards on the black notch, as widgets are drawn on a Mac: one
/// surface colour, a hairline edge, white type in three strengths, and a single
/// system accent per card carried only by its icon.
internal static class Ui
{
    // Accents — the dark-mode system colours.
    public static readonly Color Green = Rgb(0x30, 0xD1, 0x58);
    public static readonly Color Purple = Rgb(0xBF, 0x5A, 0xF2);
    public static readonly Color Yellow = Rgb(0xFF, 0xD6, 0x0A);
    public static readonly Color Blue = Rgb(0x0A, 0x84, 0xFF);
    public static readonly Color Teal = Rgb(0x64, 0xD2, 0xFF);
    public static readonly Color Orange = Rgb(0xFF, 0x9F, 0x0A);
    public static readonly Color Red = Rgb(0xFF, 0x45, 0x3A);

    public static readonly Brush Surface = Frozen(Rgb(0x1C, 0x1C, 0x1E));
    public static readonly Brush SurfaceRaised = Frozen(Rgb(0x2C, 0x2C, 0x2E));
    public static readonly Brush Edge = Frozen(Color.FromArgb(0x14, 0xFF, 0xFF, 0xFF));

    public static readonly Brush Ink = Frozen(Color.FromArgb(0xEB, 0xFF, 0xFF, 0xFF));
    public static readonly Brush InkDim = Frozen(Color.FromArgb(0x8C, 0xFF, 0xFF, 0xFF));
    public static readonly Brush InkFaint = Frozen(Color.FromArgb(0x4D, 0xFF, 0xFF, 0xFF));
    public static readonly Brush Wash = Frozen(Color.FromArgb(0x0F, 0xFF, 0xFF, 0xFF));
    public static readonly Brush WashStrong = Frozen(Color.FromArgb(0x1F, 0xFF, 0xFF, 0xFF));
    public static readonly Brush White = Frozen(Colors.White);
    public static readonly Brush WhiteDim = Frozen(Color.FromArgb(0xA6, 0xFF, 0xFF, 0xFF));
    public static readonly Brush Black = Frozen(Colors.Black);

    public static readonly FontFamily Font = new("Segoe UI Variable Text, Segoe UI");
    public static readonly FontFamily Display = new("Segoe UI Variable Display, Segoe UI");
    public static readonly FontFamily Icons = new("Segoe Fluent Icons, Segoe MDL2 Assets");

    public static Brush Accent(Color c) => Frozen(c);

    /// Green, then amber, then red as a quota fills.
    public static Color Level(double used) => used < 70 ? Green : used < 90 ? Orange : Red;

    // Segoe Fluent Icons / MDL2 code points.
    public const string IcPlay = "\uE768", IcPause = "\uE769", IcCheck = "\uE73E", IcMore = "\uE712",
        IcAdd = "\uE710", IcCalendar = "\uE787", IcStopwatch = "\uE916",
        IcBell = "\uEA8F", IcRefresh = "\uE72C", IcDelete = "\uE74D", IcCopy = "\uE8C8",
        IcForward = "\uE72A", IcRename = "\uE8AC", IcReturn = "\uE751",
        IcLines = "\uE8E4", IcClose = "\uE711", IcWindow = "\uE78B", IcChevronDown = "\uE70D",
        IcClock = "\uE917", IcBolt = "\uE945", IcSliders = "\uE9E9", IcRing = "\uEA3A",
        IcDone = "\uE930", IcTarget = "\uF272", IcChecklist = "\uE9D5", IcCompose = "\uE70B",
        IcSettings = "\uE713", IcFolder = "\uE8B7", IcWarning = "\uE7BA", IcDoneSolid = "\uEC61",
        IcChevronUp = "\uE70E", IcPhoto = "\uE91B", IcLayout = "\uECA5", IcChevronLeft = "\uE76B",
        IcChevronRight = "\uE76C", IcGauge = "\uEC4A", IcNotch = "\uE7F4", IcView = "\uE890",
        IcHide = "\uED1A", IcReset = "\uE777";

    public static Color Rgb(byte r, byte g, byte b) => Color.FromRgb(r, g, b);

    public static Brush Frozen(Color c)
    {
        var b = new SolidColorBrush(c);
        b.Freeze();
        return b;
    }

    public static TextBlock Text(string text, double size = 13, Brush? fg = null,
        FontWeight? weight = null) => new()
    {
        Text = text,
        FontFamily = Font,
        FontSize = size,
        Foreground = fg ?? Ink,
        FontWeight = weight ?? FontWeights.Normal,
        TextTrimming = TextTrimming.CharacterEllipsis,
        VerticalAlignment = VerticalAlignment.Center,
    };

    public static FrameworkElement Icon(string glyph, double size = 13, Brush? fg = null)
    {
        if (glyph == IcPlay)
            return new Path
            {
                Data = PlayShape,
                Fill = fg ?? Ink,
                Width = size * 0.78,
                Height = size * 0.9,
                Stretch = Stretch.Fill,
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
        return new TextBlock
        {
            Text = glyph,
            FontFamily = Icons,
            FontSize = size,
            Foreground = fg ?? Ink,
            VerticalAlignment = VerticalAlignment.Center,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
    }

    private static readonly Geometry PlayShape = MakePlayShape();

    private static Geometry MakePlayShape()
    {
        var g = Geometry.Parse("M1,0.4 Q0,-0.2 0,1 L0,9 Q0,10.2 1,9.6 L8.4,5.6 Q9.4,5 8.4,4.4 Z");
        g.Freeze();
        return g;
    }

    public static StackPanel Row(params UIElement[] children)
    {
        var p = new StackPanel { Orientation = Orientation.Horizontal };
        foreach (var c in children) p.Children.Add(c);
        return p;
    }

    public static T Margin<T>(this T e, double l, double t = 0, double r = 0, double b = 0) where T : FrameworkElement
    {
        e.Margin = new Thickness(l, t, r, b);
        return e;
    }

    public static Style Style(string key) => (Style)Application.Current.FindResource(key);

    public static Button Button(string style, object content, string automationId, string name, Action onClick)
    {
        var b = new Button { Style = Style(style), Content = content };
        System.Windows.Automation.AutomationProperties.SetAutomationId(b, automationId);
        System.Windows.Automation.AutomationProperties.SetName(b, name);
        b.ToolTip = name;
        b.Click += (_, _) => onClick();
        return b;
    }

    public static Button IconButton(string glyph, string id, string name, Action onClick, double size = 12,
        Brush? fg = null)
    {
        var b = Button("OwlIconButton", Icon(glyph, size, fg ?? InkDim), id, name, onClick);
        b.Padding = new Thickness(5);
        return b;
    }

    /// A pill label: icon then text.
    public static StackPanel IconText(string glyph, string text, double size = 12, Brush? fg = null,
        FontWeight? weight = null) =>
        Row(Icon(glyph, size - 1, fg).Margin(0, 1, 6), Text(text, size, fg, weight));

    /// Corner radius of a card. The open notch is rounded at 24 and cards sit 10 in
    /// from its edge, so 14 keeps the corners concentric.
    public const double CardRadius = 14;

    /// A card: the graphite surface, a hairline edge and a faint top light, so it
    /// reads as a raised pane rather than a flat rectangle.
    public static Border Card(UIElement content, double radius = CardRadius)
    {
        var grid = new Grid();
        grid.Children.Add(new Border { Background = Sheen, CornerRadius = new CornerRadius(radius), IsHitTestVisible = false });
        grid.Children.Add(content);
        return new Border
        {
            Background = Surface,
            BorderBrush = Edge,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(radius),
            Child = grid,
            SnapsToDevicePixels = true,
        };
    }

    private static readonly Brush Sheen = MakeSheen();

    private static Brush MakeSheen()
    {
        var b = new LinearGradientBrush(Color.FromArgb(0x0A, 0xFF, 0xFF, 0xFF), Color.FromArgb(0, 0xFF, 0xFF, 0xFF), 90);
        b.Freeze();
        return b;
    }

    /// A card's title row: the accent-tinted glyph, then the title.
    public static StackPanel CardTitle(string glyph, string title, Color accent) =>
        Row(Icon(glyph, 13, Accent(accent)).Margin(0, 1, 8), Text(title, 13.5, Ink, FontWeights.SemiBold));

    /// The quiet capitals over a group: "TODAY", "REMINDERS".
    public static TextBlock Section(string text) =>
        Text(text.ToUpper(CultureInfo.CurrentCulture), 10.5, InkDim, FontWeights.SemiBold);

    /// A one-pixel rule between rows.
    public static Border Hairline() => Rule(Wash);

    public static Border Rule(Brush? fill = null) => new()
    {
        Height = 1,
        Background = fill ?? WashStrong,
        SnapsToDevicePixels = true,
    };

    public static MenuItem MenuItem(string glyph, string header, Action onClick)
    {
        var item = new MenuItem { Header = Row(Icon(glyph, 12, WhiteDim).Margin(0, 0, 9), new TextBlock { Text = header }) };
        System.Windows.Automation.AutomationProperties.SetName(item, header);
        item.Click += (_, _) => onClick();
        return item;
    }

    public static MenuItem MenuText(string header, Action onClick)
    {
        var item = new MenuItem { Header = new TextBlock { Text = header, Margin = new Thickness(0) } };
        System.Windows.Automation.AutomationProperties.SetName(item, header);
        item.Click += (_, _) => onClick();
        return item;
    }

    public static void Open(ContextMenu menu, FrameworkElement anchor)
    {
        Popover.Track(menu);
        menu.PlacementTarget = anchor;
        menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Bottom;
        menu.IsOpen = true;
    }

    public static string Clock(DateTime t) => t.ToString("h:mm tt", CultureInfo.CurrentCulture);
    public static string DayMonth(DateTime t) => t.ToString("d MMM", CultureInfo.CurrentCulture);

    /// "16 Sep at 5:24 PM", or "Tomorrow at 9:00 AM".
    public static string When(DateTime t, DateTime now) =>
        $"{((t.Date - now.Date).Days == 1 ? "Tomorrow" : DayMonth(t))} at {Clock(t)}";
}

/// Dot-matrix type: each character a 5×7 grid of dots — the timer's face, and the
/// capitals the notch spells its messages in ("WELCOME BACK").
internal sealed class DotMatrix : FrameworkElement
{
    private static readonly Dictionary<char, string[]> Glyphs = new()
    {
        ['A'] = new[] { "01110", "10001", "10001", "11111", "10001", "10001", "10001" },
        ['B'] = new[] { "11110", "10001", "10001", "11110", "10001", "10001", "11110" },
        ['C'] = new[] { "01110", "10001", "10000", "10000", "10000", "10001", "01110" },
        ['D'] = new[] { "11110", "10001", "10001", "10001", "10001", "10001", "11110" },
        ['E'] = new[] { "11111", "10000", "10000", "11110", "10000", "10000", "11111" },
        ['F'] = new[] { "11111", "10000", "10000", "11110", "10000", "10000", "10000" },
        ['G'] = new[] { "01110", "10001", "10000", "10111", "10001", "10001", "01111" },
        ['H'] = new[] { "10001", "10001", "10001", "11111", "10001", "10001", "10001" },
        ['I'] = new[] { "01110", "00100", "00100", "00100", "00100", "00100", "01110" },
        ['J'] = new[] { "00111", "00010", "00010", "00010", "00010", "10010", "01100" },
        ['K'] = new[] { "10001", "10010", "10100", "11000", "10100", "10010", "10001" },
        ['L'] = new[] { "10000", "10000", "10000", "10000", "10000", "10000", "11111" },
        ['M'] = new[] { "10001", "11011", "10101", "10101", "10001", "10001", "10001" },
        ['N'] = new[] { "10001", "10001", "11001", "10101", "10011", "10001", "10001" },
        ['O'] = new[] { "01110", "10001", "10001", "10001", "10001", "10001", "01110" },
        ['P'] = new[] { "11110", "10001", "10001", "11110", "10000", "10000", "10000" },
        ['Q'] = new[] { "01110", "10001", "10001", "10001", "10101", "10010", "01101" },
        ['R'] = new[] { "11110", "10001", "10001", "11110", "10100", "10010", "10001" },
        ['S'] = new[] { "01111", "10000", "10000", "01110", "00001", "00001", "11110" },
        ['T'] = new[] { "11111", "00100", "00100", "00100", "00100", "00100", "00100" },
        ['U'] = new[] { "10001", "10001", "10001", "10001", "10001", "10001", "01110" },
        ['V'] = new[] { "10001", "10001", "10001", "10001", "10001", "01010", "00100" },
        ['W'] = new[] { "10001", "10001", "10001", "10101", "10101", "10101", "01010" },
        ['X'] = new[] { "10001", "10001", "01010", "00100", "01010", "10001", "10001" },
        ['Y'] = new[] { "10001", "10001", "10001", "01010", "00100", "00100", "00100" },
        ['Z'] = new[] { "11111", "00001", "00010", "00100", "01000", "10000", "11111" },
        [' '] = new[] { "000", "000", "000", "000", "000", "000", "000" },
        ['\''] = new[] { "1", "1", "0", "0", "0", "0", "0" },
        ['.'] = new[] { "0", "0", "0", "0", "0", "0", "1" },
        ['-'] = new[] { "000", "000", "000", "111", "000", "000", "000" },
        ['0'] = new[] { "01110", "11011", "11011", "11011", "11011", "11011", "01110" },
        ['1'] = new[] { "00100", "01100", "00100", "00100", "00100", "00100", "01110" },
        ['2'] = new[] { "01110", "10001", "00001", "00010", "00100", "01000", "11111" },
        ['3'] = new[] { "01110", "10001", "00001", "00110", "00001", "10001", "01110" },
        ['4'] = new[] { "00010", "00110", "01010", "10010", "11111", "00010", "00010" },
        ['5'] = new[] { "11111", "10000", "10000", "11110", "00001", "00001", "11110" },
        ['6'] = new[] { "01110", "10001", "10000", "11110", "10001", "10001", "01110" },
        ['7'] = new[] { "11111", "00001", "00010", "00100", "01000", "01000", "01000" },
        ['8'] = new[] { "01110", "10001", "10001", "01110", "10001", "10001", "01110" },
        ['9'] = new[] { "01110", "10001", "10001", "01111", "00001", "10001", "01110" },
        [':'] = new[] { "0", "1", "1", "0", "1", "1", "0" },
    };

    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text), typeof(string), typeof(DotMatrix),
        new FrameworkPropertyMetadata("00:00",
            FrameworkPropertyMetadataOptions.AffectsMeasure | FrameworkPropertyMetadataOptions.AffectsRender));

    public string Text { get => (string)GetValue(TextProperty); set => SetValue(TextProperty, value); }

    /// Centre-to-centre distance between dots.
    public double Pitch { get; set; } = 5;
    public Brush Fill { get; set; } = Ui.Ink;

    /// Dot diameter as a share of the pitch.
    public double Weight { get; set; } = 0.66;

    private double GlyphWidth(char c) => (Glyphs.TryGetValue(char.ToUpperInvariant(c), out var g) ? g[0].Length : 5) * Pitch;
    private double Gap => Pitch * 1.4;

    protected override Size MeasureOverride(Size available)
    {
        var w = Text.Sum(GlyphWidth) + Gap * Math.Max(0, Text.Length - 1);
        return new Size(w, 7 * Pitch);
    }

    protected override void OnRender(DrawingContext dc)
    {
        var r = Pitch * Weight / 2;
        var x = 0.0;
        foreach (var ch in Text)
        {
            if (Glyphs.TryGetValue(char.ToUpperInvariant(ch), out var g))
            {
                for (var row = 0; row < 7; row++)
                    for (var col = 0; col < g[row].Length; col++)
                        if (g[row][col] == '1')
                            dc.DrawEllipse(Fill, null,
                                new Point(x + col * Pitch + Pitch / 2, row * Pitch + Pitch / 2), r, r);
            }
            x += GlyphWidth(ch) + Gap;
        }
    }

    protected override AutomationPeer OnCreateAutomationPeer() => new Peer(this);

    private sealed class Peer : FrameworkElementAutomationPeer
    {
        public Peer(DotMatrix owner) : base(owner) { }
        protected override string GetNameCore() => ((DotMatrix)Owner).Text;
        protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.Text;
        protected override string GetClassNameCore() => nameof(DotMatrix);
    }
}

/// Seven columns of bars with a right-hand axis. With two series the lighter one
/// (planned) stands behind the darker (completed). Clicking a column picks the day.
internal sealed class BarChart : FrameworkElement
{
    public IReadOnlyList<(string Top, string Bottom)> Labels { get; set; } = Array.Empty<(string, string)>();
    public IReadOnlyList<double> Front { get; set; } = Array.Empty<double>();
    public IReadOnlyList<double>? Back { get; set; }
    public int? Selected { get; set; }
    public event Action<int>? Picked;

    private int? _hover;
    public static readonly Brush FrontBrush = Ui.Accent(Ui.Purple);
    public static readonly Brush BackBrush = Ui.Frozen(Color.FromArgb(0x2E, 0xFF, 0xFF, 0xFF));
    private static readonly Pen GridPen = new(Ui.Frozen(Color.FromArgb(0x14, 0xFF, 0xFF, 0xFF)), 1);
    private static readonly Pen HoverPen = new(Ui.Frozen(Color.FromArgb(0x66, 0xFF, 0xFF, 0xFF)), 1);

    private const double Axis = 34, LabelBand = 40;

    public BarChart()
    {
        Cursor = Cursors.Hand;
        MouseMove += (_, e) => { var i = Column(e.GetPosition(this)); if (i != _hover) { _hover = i; InvalidateVisual(); } };
        MouseLeave += (_, _) => { _hover = null; InvalidateVisual(); };
        MouseLeftButtonUp += (_, e) => { if (Column(e.GetPosition(this)) is { } i) Picked?.Invoke(i); };
    }

    private int? Column(Point p)
    {
        var n = Front.Count;
        if (n == 0) return null;
        var w = (ActualWidth - Axis) / n;
        var i = (int)(p.X / w);
        return i >= 0 && i < n ? i : null;
    }

    public void Redraw() => InvalidateVisual();

    protected override void OnRender(DrawingContext dc)
    {
        dc.DrawRectangle(Brushes.Transparent, null, new Rect(RenderSize));   // hit-testable everywhere
        var n = Front.Count;
        if (n == 0) return;
        var max = Insights.NiceMax(Math.Max(Front.DefaultIfEmpty(0).Max(), Back?.DefaultIfEmpty(0).Max() ?? 0));
        var plotH = Math.Max(10, ActualHeight - LabelBand - 8);
        var top = 8.0;
        var colW = (ActualWidth - Axis) / n;
        var barW = Math.Min(40, colW * 0.36);
        var dpi = VisualTreeHelper.GetDpi(this).PixelsPerDip;

        // Axis ticks: three steps.
        for (var k = 0; k <= 3; k++)
        {
            var v = max * k / 3;
            var y = top + plotH - plotH * k / 3;
            dc.DrawLine(GridPen, new Point(0, y), new Point(ActualWidth - Axis, y));
            var label = new FormattedText(Math.Round(v).ToString(CultureInfo.CurrentCulture), CultureInfo.CurrentCulture,
                FlowDirection.LeftToRight, new Typeface(Ui.Font, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal),
                11, Ui.InkDim, dpi);
            dc.DrawText(label, new Point(ActualWidth - label.Width, y - label.Height / 2));
        }

        for (var i = 0; i < n; i++)
        {
            var cx = colW * i + colW / 2;
            var dim = Selected is { } s && s != i;
            dc.PushOpacity(dim ? 0.45 : 1);
            if (Back is { } back && i < back.Count && back[i] > 0) Bar(dc, cx, barW, back[i], max, plotH, top, BackBrush);
            if (Front[i] > 0) Bar(dc, cx, barW, Front[i], max, plotH, top, FrontBrush);
            else if (Back is null) dc.DrawRectangle(FrontBrush, null, new Rect(cx - barW / 2, top + plotH - 1.5, barW, 1.5));
            dc.Pop();

            // The pointer's column: a hairline from the top of the plot down to its bar.
            if (_hover == i || Selected == i)
            {
                var tallest = Math.Max(Front[i], Back is { } bk && i < bk.Count ? bk[i] : 0);
                var barTop = top + plotH - Math.Max(2, plotH * tallest / max);
                if (barTop > top + 2) dc.DrawLine(HoverPen, new Point(cx, top), new Point(cx, barTop - 3));
            }

            if (i < Labels.Count)
            {
                var (a, b) = Labels[i];
                var t1 = new FormattedText(a, CultureInfo.CurrentCulture, FlowDirection.LeftToRight,
                    new Typeface(Ui.Font, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal), 12.5, Ui.Ink, dpi);
                var t2 = new FormattedText(b, CultureInfo.CurrentCulture, FlowDirection.LeftToRight,
                    new Typeface(Ui.Font, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal), 12, Ui.InkDim, dpi);
                dc.DrawText(t1, new Point(cx - t1.Width / 2, top + plotH + 8));
                dc.DrawText(t2, new Point(cx - t2.Width / 2, top + plotH + 24));
            }
        }
    }

    private static void Bar(DrawingContext dc, double cx, double w, double v, double max, double plotH, double top, Brush fill)
    {
        var h = Math.Max(2, plotH * v / max);
        dc.DrawRoundedRectangle(fill, null, new Rect(cx - w / 2, top + plotH - h, w, h), 3, 3);
    }
}


/// A small ring gauge: a faint track and an arc for the share used, tinted green,
/// amber or red as it fills. Null draws the track alone — nothing read yet.
internal sealed class Ring : FrameworkElement
{
    private double? _value;
    public double? Value
    {
        get => _value;
        set { if (_value == value) return; _value = value; InvalidateVisual(); }
    }

    public double Stroke { get; set; } = 2;

    private static readonly Pen Track = MakeTrack();

    private static Pen MakeTrack()
    {
        var p = new Pen(Ui.Frozen(Color.FromArgb(0x38, 0xFF, 0xFF, 0xFF)), 2);
        p.Freeze();
        return p;
    }

    protected override Size MeasureOverride(Size available) => new(Width is > 0 ? Width : 12, Height is > 0 ? Height : 12);

    protected override void OnRender(DrawingContext dc)
    {
        var r = Math.Min(ActualWidth, ActualHeight) / 2 - Stroke / 2;
        if (r <= 0) return;
        var c = new Point(ActualWidth / 2, ActualHeight / 2);
        dc.DrawEllipse(null, Stroke == 2 ? Track : new Pen(Track.Brush, Stroke), c, r, r);
        if (_value is not { } v || v <= 0) return;
        var pen = new Pen(Ui.Accent(Ui.Level(v)), Stroke) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round };
        if (v >= 99.95) { dc.DrawEllipse(null, pen, c, r, r); return; }
        var a = v / 100 * 2 * Math.PI;
        var start = new Point(c.X, c.Y - r);
        var end = new Point(c.X + r * Math.Sin(a), c.Y - r * Math.Cos(a));
        var g = new StreamGeometry();
        using (var ctx = g.Open())
        {
            ctx.BeginFigure(start, false, false);
            ctx.ArcTo(end, new Size(r, r), 0, v > 50, SweepDirection.Clockwise, true, false);
        }
        g.Freeze();
        dc.DrawGeometry(null, pen, g);
    }
}
