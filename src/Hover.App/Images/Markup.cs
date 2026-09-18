using Avalonia;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Hover.Core;

namespace Hover.Images;

/// Which tool the pointer is holding.
public enum Tool
{
    Arrow,
    Box,
    Highlight,
    Crop,
}

/// One thing drawn on a picture.
///
/// Positions are in the picture's own pixels, never in screen or window units. That way
/// the saved file is exact whatever size the editor happened to be, and the drawing can
/// be checked without a window.
public abstract record Mark
{
    public abstract void Draw(DrawingContext ctx, double thickness);

    /// Marks thinner than this are invisible on a big screenshot and marks thicker
    /// swamp a small one, so thickness follows the picture's shorter side.
    public static double ThicknessFor(PixelSize picture) =>
        Math.Clamp(Math.Min(picture.Width, picture.Height) / 200.0, 2, 6);
}

public sealed record ArrowMark(Point From, Point To, Color Colour) : Mark
{
    public override void Draw(DrawingContext ctx, double thickness)
    {
        var pen = new Pen(new SolidColorBrush(Colour), thickness)
        {
            LineCap = PenLineCap.Round,
            LineJoin = PenLineJoin.Round,
        };
        ctx.DrawLine(pen, From, To);

        var dx = To.X - From.X;
        var dy = To.Y - From.Y;
        var length = Math.Sqrt(dx * dx + dy * dy);
        if (length < 1) return;

        // The head grows with the arrow but stops growing, so a long arrow does not end
        // in a huge triangle.
        var head = Math.Min(thickness * 6, length * 0.4);
        var angle = Math.Atan2(dy, dx);
        foreach (var spread in new[] { 0.42, -0.42 })
        {
            var a = angle + Math.PI - spread;
            ctx.DrawLine(pen, To, new Point(To.X + Math.Cos(a) * head, To.Y + Math.Sin(a) * head));
        }
    }
}

public sealed record BoxMark(Rect Area, Color Colour) : Mark
{
    public override void Draw(DrawingContext ctx, double thickness) =>
        ctx.DrawRectangle(null, new Pen(new SolidColorBrush(Colour), thickness)
        {
            LineJoin = PenLineJoin.Miter,
        }, Area);
}

public sealed record HighlightMark(Rect Area, Color Colour) : Mark
{
    public override void Draw(DrawingContext ctx, double thickness) =>
        ctx.FillRectangle(new SolidColorBrush(Colour, 0.35), Area);
}

/// Everything drawn on one picture, plus the crop, in the order it was done.
///
/// Undo walks back through that order, so cropping and then drawing undoes the drawing
/// first. Nothing here touches a window, which is why it can be tested on its own.
public sealed class Markup
{
    public static readonly Color Red = Color.Parse("#E03E3E");
    public static readonly Color Yellow = Color.Parse("#FFDC49");
    public static readonly Color Blue = Color.Parse("#2383E2");

    private abstract record Step;
    private sealed record MarkStep(Mark Mark) : Step;
    private sealed record CropStep(Rect? Before, Rect After) : Step;

    private readonly List<Step> _steps = new();

    /// Raised whenever anything was added, undone or cleared.
    public event EventHandler? Changed;

    public IReadOnlyList<Mark> Marks =>
        _steps.OfType<MarkStep>().Select(s => s.Mark).ToList();

    /// The area kept, in the picture's own pixels. Null means all of it.
    public Rect? Crop => _steps.OfType<CropStep>().LastOrDefault()?.After;

    public bool CanUndo => _steps.Count > 0;
    public bool IsEmpty => _steps.Count == 0;

    public void Add(Mark mark)
    {
        _steps.Add(new MarkStep(mark));
        Changed?.Invoke(this, EventArgs.Empty);
    }

    /// Crops to this area. A second crop replaces the first rather than stacking, so
    /// the area is always measured against the original picture.
    public void SetCrop(Rect area)
    {
        _steps.Add(new CropStep(Crop, area));
        Changed?.Invoke(this, EventArgs.Empty);
    }

    public void Undo()
    {
        if (_steps.Count == 0) return;
        _steps.RemoveAt(_steps.Count - 1);
        Changed?.Invoke(this, EventArgs.Empty);
    }

    public void Clear()
    {
        if (_steps.Count == 0) return;
        _steps.Clear();
        Changed?.Invoke(this, EventArgs.Empty);
    }

    /// The size the saved picture will be.
    public PixelSize SizeFor(PixelSize picture)
    {
        if (Crop is not { } crop) return picture;
        var clamped = Clamp(crop, picture);
        return new PixelSize(Math.Max(1, (int)Math.Round(clamped.Width)),
                             Math.Max(1, (int)Math.Round(clamped.Height)));
    }

    /// Keeps a crop inside the picture, so a box dragged off the edge cannot ask for
    /// pixels that are not there.
    public static Rect Clamp(Rect area, PixelSize picture)
    {
        var left = Math.Clamp(area.X, 0, picture.Width);
        var top = Math.Clamp(area.Y, 0, picture.Height);
        var right = Math.Clamp(area.Right, 0, picture.Width);
        var bottom = Math.Clamp(area.Bottom, 0, picture.Height);
        return new Rect(left, top, Math.Max(0, right - left), Math.Max(0, bottom - top));
    }

    /// Draws the picture, then everything on it, and returns a PNG. Null on failure.
    ///
    /// Rendered at the picture's own pixel size, so saving does not soften the
    /// screenshot or shift the marks.
    public byte[]? Export(Bitmap picture)
    {
        try
        {
            var source = picture.PixelSize;
            var crop = Crop is { } c ? Clamp(c, source) : new Rect(0, 0, source.Width, source.Height);
            if (crop.Width < 1 || crop.Height < 1) return null;

            var size = new PixelSize(Math.Max(1, (int)Math.Round(crop.Width)),
                                     Math.Max(1, (int)Math.Round(crop.Height)));
            using var target = new RenderTargetBitmap(size);
            using (var ctx = target.CreateDrawingContext())
            {
                // Shift everything so the crop's top-left becomes the origin.
                using (ctx.PushTransform(Matrix.CreateTranslation(-crop.X, -crop.Y)))
                {
                    ctx.DrawImage(picture, new Rect(0, 0, source.Width, source.Height));
                    var thickness = Mark.ThicknessFor(source);
                    foreach (var mark in Marks) mark.Draw(ctx, thickness);
                }
            }

            using var bytes = new MemoryStream();
            target.Save(bytes, new PngBitmapEncoderOptions());
            return bytes.ToArray();
        }
        catch (Exception e)
        {
            Log.Line($"saving an edited picture failed — {e.Message}");
            return null;
        }
    }
}
