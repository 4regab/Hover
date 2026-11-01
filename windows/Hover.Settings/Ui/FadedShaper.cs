using Hover.Chat;

namespace Hover.Settings;

/// <summary>Shapes text with every colour at a fraction of its opacity (a row that is switched off).</summary>
sealed class FadedShaper(IShaper inner, float opacity) : IShaper
{
    Rgba D(Rgba c) => c with { A = (byte)Math.Clamp(c.A * opacity + 0.5f, 0, 255) };

    IReadOnlyList<Span> Spans(IReadOnlyList<Span> spans) => spans.Select(s => s.Color is { } c
        ? new Span
        {
            Text = s.Text, Strong = s.Strong, Em = s.Em, Del = s.Del, Code = s.Code, Link = s.Link, Color = D(c), Family = s.Family,
            Size = s.Size, Weight = s.Weight, Gap = s.Gap, IsGap = s.IsGap, IsBreak = s.IsBreak,
        }
        : s).ToList();

    public ShapedText Shape(IReadOnlyList<Span> spans, Look look, float width, Align align) => inner.Shape(Spans(spans), look with { Color = D(look.Color) }, width, align);

    public (float Min, float Max) Widths(IReadOnlyList<Span> spans, Look look) => inner.Widths(spans, look);
}
