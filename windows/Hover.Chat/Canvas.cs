namespace Hover.Chat;

/// <summary>
/// What the chat and the office panels draw with. Coordinates are device-independent pixels in
/// the current translation. The app implements it with Direct2D; the tests implement it by
/// writing down what was asked, so every drawing rule runs on any system.
/// </summary>
public interface ICanvas
{
    /// <summary>Everything drawn until <see cref="PopClip"/> is cut to the rectangle.</summary>
    void PushClip(Rect4 r);

    void PopClip();

    /// <summary>Everything drawn until <see cref="PopMove"/> is moved by (dx, dy).</summary>
    void PushMove(float dx, float dy);

    void PopMove();

    /// <summary>Multiplies the opacity of what is drawn until <see cref="PopOpacity"/>.</summary>
    void PushOpacity(float opacity);

    void PopOpacity();

    /// <summary>A filled box with a radius at each corner (top left, top right, bottom right, bottom left).</summary>
    void Fill(Rect4 r, float[] radius, Rgba c);

    /// <summary>A border of the given width, drawn inside the box.</summary>
    void Stroke(Rect4 r, float[] radius, float width, Rgba c);

    void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c);

    void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c);

    /// <summary>A soft round glow: full colour at the centre, nothing at the radius.</summary>
    void Glow(float cx, float cy, float radius, Rgba c);

    /// <summary>A shaped paragraph with its top left at (x, y). The runs say how it is coloured.</summary>
    void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y);

    /// <summary>An SVG document (or "mark:tool", a tool's logo) fitted to the box.</summary>
    void Svg(string svg, Rect4 r);

    /// <summary>A picture by its address, fitted to the box (cover: filled and cut), with rounded corners.</summary>
    void Image(string src, Rect4 r, float radius, bool cover);

    /// <summary>The broken-image icon, 14 wide, at the point.</summary>
    void BrokenImage(float x, float y);

    /// <summary>A path of the SVG renderer: filled and stroked (round caps and joins, dashed when dash is given), in the current translation.</summary>
    void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash);

    /// <summary>One line of text in a flowchart, centred on x (anchor middle) or starting at it.</summary>
    void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family);
}
