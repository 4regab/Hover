using Avalonia;
using Avalonia.Media;

namespace Hover.Notes;

public static class TabShapes
{
    /// Rounded on the outward-facing side only, so the tab reads as docked to the edge
    /// it grew out of.
    public static Geometry EdgeTab(double w, double h, bool onRight, double r = 11)
    {
        r = Math.Min(r, Math.Min(w, h) / 2);
        var shape = new StreamGeometry();
        using (var draw = shape.Open())
        {
            var size = new Size(r, r);
            if (onRight)
            {
                // Rounded on the left, square against the right screen edge.
                draw.BeginFigure(new Point(r, 0), true);
                draw.LineTo(new Point(w, 0));
                draw.LineTo(new Point(w, h));
                draw.LineTo(new Point(r, h));
                draw.ArcTo(new Point(0, h - r), size, 0, false, SweepDirection.Clockwise);
                draw.LineTo(new Point(0, r));
                draw.ArcTo(new Point(r, 0), size, 0, false, SweepDirection.Clockwise);
                draw.EndFigure(true);
            }
            else
            {
                draw.BeginFigure(new Point(0, 0), true);
                draw.LineTo(new Point(w - r, 0));
                draw.ArcTo(new Point(w, r), size, 0, false, SweepDirection.Clockwise);
                draw.LineTo(new Point(w, h - r));
                draw.ArcTo(new Point(w - r, h), size, 0, false, SweepDirection.Clockwise);
                draw.LineTo(new Point(0, h));
                draw.EndFigure(true);
            }
        }
        return shape;
    }

    public static DropShadowEffect Shadow(double opacity, double radius, double dx, double dy) =>
        new()
        {
            Color = Colors.Black,
            Opacity = opacity,
            BlurRadius = radius,
            OffsetX = dx,
            OffsetY = dy,
        };
}
