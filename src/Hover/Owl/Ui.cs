using System.Globalization;
using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Effects;
using System.Windows.Media.Imaging;
using System.Windows.Shapes;

namespace Hover.Owl;

/// The workspace's palette and the small builders every card uses. The colours come
/// from Theme.Current: by default Apple's system colours for grouped content (in dark
/// mode a black panel holding #1C1C1E cards; in light mode a #F2F2F7 panel holding
/// white ones, as in iOS Settings), or a theme taken from VS Code. Surfaces are
/// solid, with no borders or gloss. Type comes in three strengths of one label
/// colour. Every brush here reads the current theme, so views are built after Theme
/// has settled.
internal static class Ui
{
    // Accents — Apple's system colours, a shade brighter in dark mode, or the theme's.
    public static Color Green => Argb(Theme.Current.Green);
    public static Color Purple => Argb(Theme.Current.Purple);
    public static Color Yellow => Argb(Theme.Current.Yellow);
    public static Color Blue => Argb(Theme.Current.Blue);
    public static Color Teal => Argb(Theme.Current.Teal);
    public static Color Orange => Argb(Theme.Current.Orange);
    public static Color Red => Argb(Theme.Current.Red);
    public static Color Gray => Rgb(0x8E, 0x8E, 0x93);

    /// The accents a command button can take, by the name its settings keep.
    public static readonly string[] AccentNames = { "blue", "teal", "green", "yellow", "orange", "red", "purple", "gray" };

    public static Color AccentNamed(string name) => name switch
    {
        "teal" => Teal, "green" => Green, "yellow" => Yellow, "orange" => Orange,
        "red" => Red, "purple" => Purple, "gray" => Gray, _ => Blue,
    };

    /// The theme's colours as frozen brushes, made once per theme.
    private sealed class Paint(Core.Palette p)
    {
        public readonly Core.Palette For = p;
        public readonly Brush Ink = Frozen(Argb(p.Ink)), InkDim = Frozen(Argb(p.InkDim)), InkFaint = Frozen(Argb(p.InkFaint)),
            Fill = Frozen(Argb(p.Fill)), Wash = Frozen(Argb(p.Wash)), WashStrong = Frozen(Argb(p.WashStrong)),
            Separator = Frozen(Argb(p.Separator)), Surface = Frozen(Argb(p.Surface)), Sheet = Frozen(Argb(p.Sheet)),
            SheetEdge = Frozen(Argb(p.SheetEdge)), PanelEdge = Frozen(Argb(p.PanelEdge));
    }

    private static Paint? _paint;
    private static Paint P => _paint is { } x && ReferenceEquals(x.For, Theme.Current) ? x : _paint = new Paint(Theme.Current);

    public static Brush Ink => P.Ink;
    public static Brush InkDim => P.InkDim;
    public static Brush InkFaint => P.InkFaint;
    /// The grey behind a plain button.
    public static Brush Fill => P.Fill;
    /// Fields, tiles and segmented-control tracks.
    public static Brush Wash => P.Wash;
    /// Empty tracks: the timer ring, progress bars, quota rings.
    public static Brush WashStrong => P.WashStrong;
    /// Hairlines between rows.
    public static Brush Separator => P.Separator;
    /// A card.
    public static Brush Surface => P.Surface;
    /// Menus and popovers, and the hairline around them.
    public static Brush Sheet => P.Sheet;
    public static Brush SheetEdge => P.SheetEdge;
    /// The open notch and the app window, and the faint line around the notch.
    public static Color Panel => Argb(Theme.Current.Panel);
    public static Brush PanelEdge => P.PanelEdge;

    public static Color Argb(uint c) => Color.FromArgb((byte)(c >> 24), (byte)(c >> 16), (byte)(c >> 8), (byte)c);

    // The resting notch is black in both appearances, as a real notch is.
    public static readonly Brush White = Frozen(Colors.White);
    public static readonly Brush WhiteDim = Frozen(Color.FromArgb(0xA6, 0xFF, 0xFF, 0xFF));
    public static readonly Brush Black = Frozen(Colors.Black);

    private static readonly Uri FontBase = new("pack://application:,,,/Hover;component/Assets/Fonts/");
    public static readonly FontFamily Font = new(FontBase, "./#Inter");
    public static readonly FontFamily Display = new(FontBase, "./#Inter Display");

    public static Brush Accent(Color c) => Frozen(c);

    /// An accent at a fraction of its strength: the fill of a tinted button.
    public static Brush Tint(Color c, byte alpha = 0x33) => Frozen(Color.FromArgb(alpha, c.R, c.G, c.B));

    /// Green, then amber, then red as a quota fills.
    public static Color Level(double used) => used < 70 ? Green : used < 90 ? Orange : Red;

    // Icon names: line icons in IconPaths, except play, pause and the ticked circle,
    // which are drawn solid as Apple draws them.
    public const string IcPlay = "play", IcPause = "pause", IcCheck = "check", IcMore = "more",
        IcAdd = "add", IcCalendar = "calendar", IcStopwatch = "stopwatch",
        IcBell = "bell", IcRefresh = "refresh", IcDelete = "delete", IcCopy = "copy",
        IcForward = "forward", IcRename = "rename", IcReturn = "return",
        IcLines = "lines", IcClose = "close", IcChevronDown = "chevron-down",
        IcClock = "clock", IcBolt = "bolt", IcSliders = "sliders", IcRing = "ring",
        IcDone = "done", IcTarget = "target", IcChecklist = "checklist", IcCompose = "compose",
        IcSettings = "settings", IcFolder = "folder", IcWarning = "warning", IcDoneSolid = "done-solid",
        IcChevronUp = "chevron-up", IcPhoto = "photo", IcLayout = "layout", IcChevronLeft = "chevron-left",
        IcChevronRight = "chevron-right", IcGauge = "gauge", IcNotch = "notch", IcView = "view",
        IcHide = "hide", IcReset = "reset", IcCut = "cut", IcSparkles = "sparkles",
        IcTerminal = "terminal", IcPalette = "palette", IcImport = "import";

    /// The icons a command button can wear.
    public static readonly string[] ButtonIcons =
    {
        "terminal", "bot", "sparkles", "code", "rocket", "bolt", "brain", "command", "globe", "git", "database", "server",
        "cloud", "cpu", "bug", "flask", "book", "compose", "music", "coffee", "star", "heart", "flame", "wrench", "package", "folder",
    };

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
        var ink = fg ?? Ink;
        if (glyph is IcPlay or IcPause or IcDoneSolid)
            return new Path
            {
                Data = glyph switch { IcPlay => PlayShape, IcPause => PauseShape, _ => DoneShape },
                Fill = ink,
                Width = glyph == IcDoneSolid ? size : size * 0.78,
                Height = glyph == IcDoneSolid ? size : size * 0.9,
                Stretch = glyph == IcDoneSolid ? Stretch.Uniform : Stretch.Fill,
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
        return new Path
        {
            Data = LineShape(glyph, size),
            Stroke = ink,
            // About the weight of a regular symbol beside text of the same size.
            StrokeThickness = Math.Clamp(size * 0.09 + 0.15, 1.1, 2),
            StrokeStartLineCap = PenLineCap.Round,
            StrokeEndLineCap = PenLineCap.Round,
            StrokeLineJoin = PenLineJoin.Round,
            Width = size,
            Height = size,
            VerticalAlignment = VerticalAlignment.Center,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
    }

    private static readonly Dictionary<(string, double), Geometry> Shapes = new();

    /// The icon's 24-unit drawing scaled to the size asked for. The pen is applied
    /// after the scale, so the stroke stays the same weight at any size.
    private static Geometry LineShape(string glyph, double size)
    {
        if (Shapes.TryGetValue((glyph, size), out var cached)) return cached;
        Geometry g;
        if (IconPaths.Data.TryGetValue(glyph, out var data))
        {
            g = Geometry.Parse(data).CloneCurrentValue();
            g.Transform = new ScaleTransform(size / 24, size / 24);
        }
        else
        {
            Core.Log.Line($"no icon named {glyph}");
            g = Geometry.Empty;
        }
        g.Freeze();
        return Shapes[(glyph, size)] = g;
    }

    private static readonly Geometry PlayShape = Solid("M1,0.4 Q0,-0.2 0,1 L0,9 Q0,10.2 1,9.6 L8.4,5.6 Q9.4,5 8.4,4.4 Z");
    private static readonly Geometry PauseShape = Solid(
        "M1.2,0 L2.8,0 Q4,0 4,1.2 L4,8.8 Q4,10 2.8,10 L1.2,10 Q0,10 0,8.8 L0,1.2 Q0,0 1.2,0 Z " +
        "M6.8,0 L8.4,0 Q9.6,0 9.6,1.2 L9.6,8.8 Q9.6,10 8.4,10 L6.8,10 Q5.6,10 5.6,8.8 L5.6,1.2 Q5.6,0 6.8,0 Z");

    /// A filled circle with the tick cut out of it, so the card shows through.
    private static readonly Geometry DoneShape = MakeDone();

    private static Geometry MakeDone()
    {
        var circle = new EllipseGeometry(new Point(12, 12), 10, 10);
        var tick = Geometry.Parse("M 7.6 12.3 L 10.6 15.2 L 16.4 9.2")
            .GetWidenedPathGeometry(new Pen(Brushes.Black, 2.4) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round });
        var g = new CombinedGeometry(GeometryCombineMode.Exclude, circle, tick).GetFlattenedPathGeometry();
        g.Freeze();
        return g;
    }

    private static Geometry Solid(string data)
    {
        var g = Geometry.Parse(data);
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

    /// Corner radius of a card. The open notch is rounded at 32 and cards sit 10 in
    /// from its edge, so 22 keeps the corners concentric.
    public const double CardRadius = 22;

    /// A card: one solid rounded surface, as Apple draws widgets and grouped lists.
    public static Border Card(UIElement content, double radius = CardRadius) => new()
    {
        Background = Surface,
        CornerRadius = new CornerRadius(radius),
        Child = content,
    };

    /// A card's title row: the icon in white on a filled accent circle, as a Reminders
    /// list is marked, then the title, which ends in "…" rather than clipping when the
    /// card is narrow.
    public static Grid CardTitle(string glyph, string title, Color accent)
    {
        var g = new Grid { VerticalAlignment = VerticalAlignment.Center };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.Children.Add(new Border
        {
            Width = 24, Height = 24, CornerRadius = new CornerRadius(12), Background = Accent(accent),
            Child = Icon(glyph, 13, White), Margin = new Thickness(0, 0, 9, 0),
        });
        var text = Text(title, 15, Ink, FontWeights.SemiBold);
        Grid.SetColumn(text, 1);
        g.Children.Add(text);
        return g;
    }
    /// The quiet capitals over a group: "TODAY", "REMINDERS".
    public static TextBlock Section(string text) =>
        Text(text.ToUpper(CultureInfo.CurrentCulture), 10.5, InkDim, FontWeights.SemiBold);

    /// A one-pixel rule between rows.
    public static Border Hairline() => Rule();

    public static Border Rule(Brush? fill = null) => new()
    {
        Height = 1,
        Background = fill ?? Separator,
        SnapsToDevicePixels = true,
    };

    public static MenuItem MenuItem(string glyph, string header, Action onClick)
    {
        var item = new MenuItem { Header = Row(Icon(glyph, 13, InkDim).Margin(0, 0, 9), new TextBlock { Text = header }) };
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
    public static Brush FrontBrush => Ui.Accent(Ui.Purple);
    public static Brush BackBrush => Ui.WashStrong;
    private static Pen GridPen => new(Ui.Wash, 1);
    private static Pen HoverPen => new(Ui.InkFaint, 1);

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

    /// The empty track: faint white on the black notch, the card's ink elsewhere.
    public Brush? TrackBrush { get; set; }

    /// A fixed colour for the arc. Without one it goes green, amber, red as it fills.
    public Color? Tint { get; set; }

    protected override Size MeasureOverride(Size available) => new(Width is > 0 ? Width : 12, Height is > 0 ? Height : 12);

    protected override void OnRender(DrawingContext dc)
    {
        var r = Math.Min(ActualWidth, ActualHeight) / 2 - Stroke / 2;
        if (r <= 0) return;
        var c = new Point(ActualWidth / 2, ActualHeight / 2);
        dc.DrawEllipse(null, new Pen(TrackBrush ?? Ui.WashStrong, Stroke), c, r, r);
        if (_value is not { } v || v <= 0) return;
        var pen = new Pen(Ui.Accent(Tint ?? Ui.Level(v)), Stroke) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round };
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
