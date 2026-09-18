using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Rendering;

namespace Hover.Images;

/// Shows a picture and everything drawn on it, and turns drags into marks.
///
/// Everything is stored in the picture's own pixels. This control only ever scales for
/// display, so what is saved matches what is seen and neither depends on the size of
/// the window.
///
/// It answers hit tests across its whole area. A control that paints itself rather than
/// carrying a background is otherwise invisible to the pointer, and no drag would ever
/// reach it.
public sealed class MarkupCanvas : Control, ICustomHitTest
{
    private readonly Bitmap _picture;
    private readonly Markup _markup;

    private Point? _from;
    private Point _to;
    private bool _dragging;

    public Tool Tool { get; set; } = Tool.Arrow;
    public Color Colour { get; set; } = Markup.Red;

    /// Raised when a drag finishes, so the window can refresh its undo button.
    public event EventHandler? Drawn;

    public MarkupCanvas(Bitmap picture, Markup markup)
    {
        _picture = picture;
        _markup = markup;
        _markup.Changed += (_, _) => InvalidateVisual();
        Cursor = new Cursor(StandardCursorType.Cross);
        ClipToBounds = true;
    }

    public PixelSize PictureSize => _picture.PixelSize;

    /// Every point inside belongs to this control, painted or not.
    public bool HitTest(Point point) => true;

    /// Where the picture sits inside this control, and how much it was scaled by.
    ///
    /// Never scaled up past its own size: a snip is pixels, and blowing them up would
    /// make marks look placed differently from how they save.
    private (Rect Area, double Scale) Fit()
    {
        var size = _picture.PixelSize;
        if (size.Width <= 0 || size.Height <= 0) return (default, 1);

        var scale = Math.Min(Bounds.Width / size.Width, Bounds.Height / size.Height);
        if (scale <= 0) return (default, 1);
        scale = Math.Min(scale, 1);

        var width = size.Width * scale;
        var height = size.Height * scale;
        return (new Rect((Bounds.Width - width) / 2, (Bounds.Height - height) / 2, width, height),
                scale);
    }

    /// A point in this control turned into a point in the picture.
    private Point ToPicture(Point local)
    {
        var (area, scale) = Fit();
        if (scale <= 0) return default;
        var size = _picture.PixelSize;
        return new Point(
            Math.Clamp((local.X - area.X) / scale, 0, size.Width),
            Math.Clamp((local.Y - area.Y) / scale, 0, size.Height));
    }

    public override void Render(DrawingContext ctx)
    {
        var (area, scale) = Fit();
        if (area.Width <= 0) return;

        var size = _picture.PixelSize;
        ctx.DrawImage(_picture, new Rect(0, 0, size.Width, size.Height), area);

        // From here on, draw in the picture's own pixels.
        using (ctx.PushClip(area))
        using (ctx.PushTransform(Matrix.CreateScale(scale, scale) *
                                 Matrix.CreateTranslation(area.X, area.Y)))
        {
            var thickness = Mark.ThicknessFor(size);
            foreach (var mark in _markup.Marks) mark.Draw(ctx, thickness);

            if (_dragging && _from is { } start) Preview(ctx, start, _to, thickness);

            // Everything outside the crop is dimmed rather than hidden, so it is clear
            // what will be thrown away and it can still be undone.
            if (_markup.Crop is { } crop) DimOutside(ctx, Markup.Clamp(crop, size), size);
        }
    }

    private void Preview(DrawingContext ctx, Point from, Point to, double thickness)
    {
        switch (Tool)
        {
            case Tool.Arrow:
                new ArrowMark(from, to, Colour).Draw(ctx, thickness);
                break;
            case Tool.Box:
                new BoxMark(Between(from, to), Colour).Draw(ctx, thickness);
                break;
            case Tool.Highlight:
                new HighlightMark(Between(from, to), Colour).Draw(ctx, thickness);
                break;
            case Tool.Crop:
                var box = Between(from, to);
                ctx.DrawRectangle(null, new Pen(Brushes.White, thickness * 0.6)
                {
                    DashStyle = new DashStyle(new double[] { 4, 3 }, 0),
                }, box);
                break;
        }
    }

    private static void DimOutside(DrawingContext ctx, Rect keep, PixelSize size)
    {
        var shade = new SolidColorBrush(Colors.Black, 0.55);
        ctx.FillRectangle(shade, new Rect(0, 0, size.Width, keep.Y));
        ctx.FillRectangle(shade, new Rect(0, keep.Bottom, size.Width, size.Height - keep.Bottom));
        ctx.FillRectangle(shade, new Rect(0, keep.Y, keep.X, keep.Height));
        ctx.FillRectangle(shade, new Rect(keep.Right, keep.Y, size.Width - keep.Right, keep.Height));
    }

    /// The box two corners make, whichever way round they were dragged.
    internal static Rect Between(Point a, Point b) => new(
        Math.Min(a.X, b.X), Math.Min(a.Y, b.Y),
        Math.Abs(a.X - b.X), Math.Abs(a.Y - b.Y));

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed) return;
        _from = ToPicture(e.GetPosition(this));
        _to = _from.Value;
        _dragging = true;
        e.Pointer.Capture(this);
        InvalidateVisual();
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        base.OnPointerMoved(e);
        if (!_dragging) return;
        _to = ToPicture(e.GetPosition(this));
        InvalidateVisual();
    }

    protected override void OnPointerReleased(PointerReleasedEventArgs e)
    {
        base.OnPointerReleased(e);
        if (!_dragging || _from is not { } start) return;
        _dragging = false;
        e.Pointer.Capture(null);
        Commit(start, ToPicture(e.GetPosition(this)));
        _from = null;
        InvalidateVisual();
        Drawn?.Invoke(this, EventArgs.Empty);
    }

    /// Turns a finished drag into a mark. A drag of a few pixels is a stray click and is
    /// dropped, so a misplaced tap does not leave a dot on the picture.
    private void Commit(Point from, Point to)
    {
        var box = Between(from, to);
        var travelled = Math.Sqrt(Math.Pow(to.X - from.X, 2) + Math.Pow(to.Y - from.Y, 2));

        switch (Tool)
        {
            case Tool.Arrow when travelled >= 8:
                _markup.Add(new ArrowMark(from, to, Colour));
                break;
            case Tool.Box when box.Width >= 6 && box.Height >= 6:
                _markup.Add(new BoxMark(box, Colour));
                break;
            case Tool.Highlight when box.Width >= 6 && box.Height >= 6:
                _markup.Add(new HighlightMark(box, Colour));
                break;
            case Tool.Crop when box.Width >= 12 && box.Height >= 12:
                _markup.SetCrop(box);
                break;
        }
    }
}
