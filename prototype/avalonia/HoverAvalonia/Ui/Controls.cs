using System.Globalization;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Media;
using HoverAvalonia.Core;
using HoverAvalonia.Office;

namespace HoverAvalonia.Ui;

/// <summary>A quota ring: the tool's mark in a ring that fills to its percentage, with the number beside it (the island's usage items).
/// The marks are letters, not Hover's logos (those are SVG/PNG assets this slice doesn't import).</summary>
public sealed class Ring : Control
{
    public static readonly StyledProperty<double> FractionProperty = AvaloniaProperty.Register<Ring, double>(nameof(Fraction));
    public static readonly StyledProperty<Color> RingColorProperty = AvaloniaProperty.Register<Ring, Color>(nameof(RingColor), Colors.White);
    public static readonly StyledProperty<string?> GlyphProperty = AvaloniaProperty.Register<Ring, string?>(nameof(Glyph));
    public static readonly StyledProperty<Color> GlyphColorProperty = AvaloniaProperty.Register<Ring, Color>(nameof(GlyphColor), Colors.White);
    public static readonly StyledProperty<string?> TextProperty = AvaloniaProperty.Register<Ring, string?>(nameof(Text));
    public static readonly StyledProperty<bool> SpinnerProperty = AvaloniaProperty.Register<Ring, bool>(nameof(Spinner));
    public double Fraction { get => GetValue(FractionProperty); set => SetValue(FractionProperty, value); }
    public Color RingColor { get => GetValue(RingColorProperty); set => SetValue(RingColorProperty, value); }
    public string? Glyph { get => GetValue(GlyphProperty); set => SetValue(GlyphProperty, value); }
    public Color GlyphColor { get => GetValue(GlyphColorProperty); set => SetValue(GlyphColorProperty, value); }
    public string? Text { get => GetValue(TextProperty); set => SetValue(TextProperty, value); }
    public bool Spinner { get => GetValue(SpinnerProperty); set => SetValue(SpinnerProperty, value); }

    public const double Dia = 22;
    static Ring() { AffectsRender<Ring>(FractionProperty, RingColorProperty, GlyphProperty, GlyphColorProperty, TextProperty); AffectsMeasure<Ring>(TextProperty); }

    FormattedText Fmt(string s, double size, IBrush b, FontWeight w = FontWeight.Medium)
        => new(s, CultureInfo.InvariantCulture, FlowDirection.LeftToRight, new Typeface(FontFamily.Default, FontStyle.Normal, w), size, b);

    protected override Size MeasureOverride(Size available)
    {
        double w = Dia;
        if (!string.IsNullOrEmpty(Text)) w += 6 + Fmt(Text + "%", 13, Brushes.White).Width + (Text == "—" ? 0 : 0);
        return new(w, 32);
    }

    public override void Render(DrawingContext ctx)
    {
        double d = Dia, r = d / 2, th = 2.5;
        var c = new Point(r, r); using var _ = ctx.PushTransform(Matrix.CreateTranslation(0, (32 - d) / 2));
        ctx.DrawEllipse(null, new Pen(new SolidColorBrush(Color.FromRgb(0x2A, 0x2A, 0x30)), th), c, r - th / 2, r - th / 2);
        if (Fraction > 0.001)
        {
            var pen = new Pen(new SolidColorBrush(RingColor), th, lineCap: PenLineCap.Round);
            if (Fraction >= 0.999) ctx.DrawEllipse(null, pen, c, r - th / 2, r - th / 2);
            else
            {
                double rad = r - th / 2, a0 = -Math.PI / 2, a1 = a0 + Fraction * Math.PI * 2;
                var g = new StreamGeometry();
                using (var sg = g.Open())
                {
                    sg.BeginFigure(new Point(c.X + rad * Math.Cos(a0), c.Y + rad * Math.Sin(a0)), false);
                    sg.ArcTo(new Point(c.X + rad * Math.Cos(a1), c.Y + rad * Math.Sin(a1)), new Size(rad, rad), 0, Fraction > 0.5, SweepDirection.Clockwise);
                    sg.EndFigure(false);
                }
                ctx.DrawGeometry(null, pen, g);
            }
        }
        var inner = new Rect(r - 8, r - 8, 16, 16);
        ctx.DrawRectangle(new SolidColorBrush(Color.FromRgb(0x1C, 0x1C, 0x22)), null, new RoundedRect(inner, 5));
        if (!string.IsNullOrEmpty(Glyph))
        {
            var t = Fmt(Glyph, 9.5, new SolidColorBrush(GlyphColor), FontWeight.Bold);
            ctx.DrawText(t, new Point(r - t.Width / 2, r - t.Height / 2));
        }
        if (!string.IsNullOrEmpty(Text))
        {
            var t = Fmt(Text, 13, Brushes.White, FontWeight.SemiBold);
            ctx.DrawText(t, new Point(d + 6, (32 - t.Height) / 2 - (32 - d) / 2));
            if (Text != "—") { var p = Fmt("%", 13, new SolidColorBrush(Color.FromRgb(0x66, 0x66, 0x6E))); ctx.DrawText(p, new Point(d + 6 + t.Width, (32 - p.Height) / 2 - (32 - d) / 2)); }
        }
    }
}

/// <summary>Name tags over the bots (the page's tags): a pixel-font chip with the bot's name and tool mark, and the bubble above it, typed out at 45 chars/s.</summary>
public sealed class TagLayer : Canvas
{
    sealed class Tag
    {
        public readonly StackPanel Root = new() { Orientation = Orientation.Vertical, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Left };
        public readonly Border Bubble = new() { CornerRadius = new(10), Padding = new(10, 5), Background = new SolidColorBrush(Color.FromArgb(0xE6, 0x16, 0x14, 0x1A)) };
        public readonly TextBlock BubbleText = new() { Foreground = Brushes.White, FontSize = 12, FontWeight = FontWeight.Medium };
        public readonly Border Chip = new() { CornerRadius = new(8), Padding = new(4, 3, 8, 3), HorizontalAlignment = HorizontalAlignment.Center };
        public readonly TextBlock Name = new() { Foreground = Brushes.White, FontSize = 11, FontWeight = FontWeight.Bold, FontFamily = new FontFamily("avares://HoverAvalonia/Assets#Pixelify Sans"), VerticalAlignment = VerticalAlignment.Center };
        public readonly Border Mark = new() { Width = 15, Height = 15, CornerRadius = new(4) };
        public readonly TextBlock MarkText = new() { FontSize = 9, FontWeight = FontWeight.Bold, Foreground = Brushes.White, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        public Tag()
        {
            Bubble.Child = BubbleText; Mark.Child = MarkText;
            Chip.Child = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 5, Children = { Mark, Name } };
            Root.Children.Add(Bubble); Root.Children.Add(Chip);
        }
    }
    readonly Dictionary<int, Tag> tags = [];

    public void Update(OfficeScene o)
    {
        var (view, proj) = o.Camera(); var vp = proj.Mul(view);
        foreach (var s in o.Sessions)
        {
            if (!tags.TryGetValue(s.Id, out var t)) { tags[s.Id] = t = new Tag(); Children.Add(t.Root); }
            var p = vp.Point(s.B.Head3(o.G));
            double x = (p.X + 1) / 2 * o.W, y = (1 - p.Y) / 2 * o.H;
            string want = Bubble(s); int n = Math.Min(want.Length, (int)(Math.Max(0, o.ClockT - Start(s)) * 45 + (o.Still ? 999 : 0)));
            if (s.TagWant != want) { s.TagWant = want; s.TagSince = o.ClockT; }
            n = Math.Min(want.Length, (int)((o.ClockT - s.TagSince) * 45));
            t.BubbleText.Text = want[..n]; t.Bubble.IsVisible = n > 0;
            bool done = s.Sim == SimState.Completed;
            t.Bubble.Background = new SolidColorBrush(done ? Color.FromArgb(0xE6, 0x10, 0x2A, 0x1E) : Color.FromArgb(0xE6, 0x16, 0x14, 0x1A));
            t.Bubble.BorderBrush = done ? new SolidColorBrush(Color.FromArgb(0x55, 0x4A, 0xDE, 0x80)) : null; t.Bubble.BorderThickness = new(done ? 1 : 0);
            var bc = Color.FromRgb((byte)(Rgb.ToSrgb(s.B.Color.R) * 255 + .5), (byte)(Rgb.ToSrgb(s.B.Color.G) * 255 + .5), (byte)(Rgb.ToSrgb(s.B.Color.B) * 255 + .5));
            t.Chip.Background = s.B.Hot ? new SolidColorBrush(Color.FromRgb(0x94, 0x66, 0xF5)) : new SolidColorBrush(Color.FromArgb(0xE6, 0x16, 0x14, 0x1A));
            t.Name.Text = s.B.Name; t.Mark.Background = new SolidColorBrush(bc); t.MarkText.Text = s.Tool[..1].ToUpperInvariant();
            t.Root.Measure(Size.Infinity);
            SetLeft(t.Root, x - t.Root.DesiredSize.Width / 2); SetTop(t.Root, y - t.Root.DesiredSize.Height - 14);
        }
    }
    static double Start(Session s) => 0;

    /// <summary>bubbleFor: what the bot says over its head (office.rs::bubble).</summary>
    public static string Bubble(Session s)
    {
        if (s.B.Walking) return "On my way…";
        return s.Sim switch
        {
            SimState.Idle => "Waking up…",
            SimState.Working => s.Act switch { "Thinking" => "Thinking…", "Writing" => "Writing it up…", { } a => $"{a} npm test", _ => "Working…" },
            _ => s.B.Since < 6 ? "Done! ✓" : "",
        };
    }
}
