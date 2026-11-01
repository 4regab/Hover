using System.Globalization;

namespace Hover.Notch;

public enum PathOp { Move, Line, Cubic, Arc, Close }

/// <summary>
/// One drawing step with absolute coordinates. Move and Line use (X, Y). Cubic has two control
/// points (X1, Y1), (X2, Y2) and the end (X, Y). Arc is an SVG elliptical arc to (X, Y).
/// </summary>
public readonly record struct PathCmd(PathOp Op, double X1 = 0, double Y1 = 0, double X2 = 0, double Y2 = 0, double X = 0, double Y = 0,
    double Rx = 0, double Ry = 0, double Rotation = 0, bool Large = false, bool Sweep = false);

/// <summary>
/// Reads SVG path data (the <c>d</c> of a path) into absolute steps, so a drawing library can
/// build its own geometry from them. Handles M L H V C S A Z, upper and lower case, numbers
/// that run together ("-.5.25") and arc flags with no space after them ("00-.81").
/// </summary>
public static class PathData
{
    /// <summary>The steps, or null when the text is not path data this reader understands.</summary>
    public static List<PathCmd>? Parse(string d)
    {
        var s = new Scanner(d);
        var cmds = new List<PathCmd>();
        double cx = 0, cy = 0, sx = 0, sy = 0;
        // The last cubic's second control point, for S to mirror.
        double lx = 0, ly = 0;
        var lastCubic = false;
        char cmd = ' ';
        while (true)
        {
            s.PassSeparators();
            if (s.End) break;
            var c = s.Peek();
            if (char.IsLetter(c))
            {
                cmd = c;
                s.Next();
                if (cmd is 'Z' or 'z')
                {
                    cmds.Add(new PathCmd(PathOp.Close));
                    cx = sx; cy = sy;
                    lastCubic = false;
                    continue;
                }
            }
            else if (cmd == ' ' || cmd is 'Z' or 'z') return null;
            else if (cmd is 'M') cmd = 'L';
            else if (cmd is 'm') cmd = 'l';

            var rel = char.IsLower(cmd);
            double ox = rel ? cx : 0, oy = rel ? cy : 0;
            switch (char.ToUpperInvariant(cmd))
            {
                case 'M':
                {
                    if (!s.Number(out var x) || !s.Number(out var y)) return null;
                    cx = ox + x; cy = oy + y; sx = cx; sy = cy;
                    cmds.Add(new PathCmd(PathOp.Move, X: cx, Y: cy));
                    lastCubic = false;
                    break;
                }
                case 'L':
                {
                    if (!s.Number(out var x) || !s.Number(out var y)) return null;
                    cx = ox + x; cy = oy + y;
                    cmds.Add(new PathCmd(PathOp.Line, X: cx, Y: cy));
                    lastCubic = false;
                    break;
                }
                case 'H':
                {
                    if (!s.Number(out var x)) return null;
                    cx = ox + x;
                    cmds.Add(new PathCmd(PathOp.Line, X: cx, Y: cy));
                    lastCubic = false;
                    break;
                }
                case 'V':
                {
                    if (!s.Number(out var y)) return null;
                    cy = oy + y;
                    cmds.Add(new PathCmd(PathOp.Line, X: cx, Y: cy));
                    lastCubic = false;
                    break;
                }
                case 'C':
                {
                    if (!s.Number(out var x1) || !s.Number(out var y1) || !s.Number(out var x2) || !s.Number(out var y2)
                        || !s.Number(out var x) || !s.Number(out var y)) return null;
                    cmds.Add(new PathCmd(PathOp.Cubic, ox + x1, oy + y1, ox + x2, oy + y2, ox + x, oy + y));
                    lx = ox + x2; ly = oy + y2; cx = ox + x; cy = oy + y;
                    lastCubic = true;
                    break;
                }
                case 'S':
                {
                    if (!s.Number(out var x2) || !s.Number(out var y2) || !s.Number(out var x) || !s.Number(out var y)) return null;
                    // The first control point mirrors the last one about the current point.
                    var x1 = lastCubic ? 2 * cx - lx : cx;
                    var y1 = lastCubic ? 2 * cy - ly : cy;
                    cmds.Add(new PathCmd(PathOp.Cubic, x1, y1, ox + x2, oy + y2, ox + x, oy + y));
                    lx = ox + x2; ly = oy + y2; cx = ox + x; cy = oy + y;
                    lastCubic = true;
                    break;
                }
                case 'A':
                {
                    if (!s.Number(out var rx) || !s.Number(out var ry) || !s.Number(out var rot) || !s.Flag(out var large)
                        || !s.Flag(out var sweep) || !s.Number(out var x) || !s.Number(out var y)) return null;
                    cx = ox + x; cy = oy + y;
                    cmds.Add(new PathCmd(PathOp.Arc, X: cx, Y: cy, Rx: Math.Abs(rx), Ry: Math.Abs(ry), Rotation: rot, Large: large, Sweep: sweep));
                    lastCubic = false;
                    break;
                }
                default:
                    return null;
            }
        }
        return cmds.Count > 0 && cmds[0].Op == PathOp.Move ? cmds : null;
    }

    ref struct Scanner
    {
        readonly string text;
        int i;

        public Scanner(string text) { this.text = text; i = 0; }

        public readonly bool End => i >= text.Length;

        public readonly char Peek() => text[i];

        public void Next() => i++;

        public void PassSeparators()
        {
            while (i < text.Length && (char.IsWhiteSpace(text[i]) || text[i] == ',')) i++;
        }

        public bool Number(out double value)
        {
            PassSeparators();
            value = 0;
            var start = i;
            if (i < text.Length && (text[i] is '+' or '-')) i++;
            var digits = 0;
            while (i < text.Length && char.IsAsciiDigit(text[i])) { i++; digits++; }
            if (i < text.Length && text[i] == '.')
            {
                i++;
                while (i < text.Length && char.IsAsciiDigit(text[i])) { i++; digits++; }
            }
            if (digits == 0) return false;
            if (i < text.Length && (text[i] is 'e' or 'E'))
            {
                var save = i;
                i++;
                if (i < text.Length && (text[i] is '+' or '-')) i++;
                var exp = 0;
                while (i < text.Length && char.IsAsciiDigit(text[i])) { i++; exp++; }
                if (exp == 0) i = save;
            }
            return double.TryParse(text.AsSpan(start, i - start), NumberStyles.Float, CultureInfo.InvariantCulture, out value);
        }

        /// <summary>An arc flag: one character, 0 or 1, which may be followed at once by the next number.</summary>
        public bool Flag(out bool value)
        {
            PassSeparators();
            value = false;
            if (i >= text.Length || text[i] is not ('0' or '1')) return false;
            value = text[i] == '1';
            i++;
            return true;
        }
    }
}
