using Hover.Chat;

namespace Hover.Settings;

/// <summary>
/// Draws on another canvas with what has been pushed by <see cref="PushOpacity"/> applied to every colour, so a row that
/// is switched off looks faded whatever draws it. (The app's Direct2D canvas takes no layer opacity.)
/// </summary>
sealed class DimCanvas : ICanvas
{
    readonly Stack<float> stack = new();

    public ICanvas Inner { get; set; } = null!;

    /// <summary>The opacity now in force.</summary>
    public float Opacity { get; private set; } = 1;

    public void PushOpacity(float opacity)
    {
        stack.Push(Opacity);
        Opacity *= opacity;
    }

    public void PopOpacity() => Opacity = stack.Count > 0 ? stack.Pop() : 1;

    Rgba D(Rgba c) => Opacity >= 1 ? c : c with { A = (byte)Math.Clamp(c.A * Opacity + 0.5f, 0, 255) };

    public void PushClip(Rect4 r) => Inner.PushClip(r);
    public void PopClip() => Inner.PopClip();
    public void PushMove(float dx, float dy) => Inner.PushMove(dx, dy);
    public void PopMove() => Inner.PopMove();
    public void Fill(Rect4 r, float[] radius, Rgba c) => Inner.Fill(r, radius, D(c));
    public void Stroke(Rect4 r, float[] radius, float width, Rgba c) => Inner.Stroke(r, radius, width, D(c));
    public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) => Inner.Line(points, width, D(c));
    public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) => Inner.Polygon(points, D(c));
    public void Glow(float cx, float cy, float radius, Rgba c) => Inner.Glow(cx, cy, radius, D(c));

    // Text is shaped in faded colours by the pen that draws while the opacity is below 1 (see SettingsUi.Opacity): the colours are in the layout.
    public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y) => Inner.Text(layout, runs, x, y);

    public void Svg(string svg, Rect4 r) => Inner.Svg(svg, r);
    public void Image(string src, Rect4 r, float radius, bool cover) => Inner.Image(src, r, radius, cover);
    public void BrokenImage(float x, float y) => Inner.BrokenImage(x, y);
    public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash)
        => Inner.Path(path, fill is { } f ? D(f) : null, stroke is { } s ? D(s) : null, strokeWidth, evenOdd, dash);
    public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family) => Inner.Label(text, x, y, size, weight, D(c), anchor, family);
}
