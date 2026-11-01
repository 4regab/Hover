using System.Globalization;
using System.Xml;
using Hover.Notch;

namespace Hover.Chat;

/// <summary>
/// Draws the SVG the chat shows: its own icons (a 24-unit grid of strokes) and the flowcharts the
/// backend writes. It reads svg, g, defs, marker, path, rect, circle, text and tspan with the
/// attributes those use, and applies the flowchart's stylesheet (page.html's `.flow` rules).
/// ponytail: no filters, masks, clip paths, transforms in markup, gradients, use or general CSS;
/// an unknown element is skipped. Upgrade path: a real SVG library if the chat ever shows foreign SVG.
/// </summary>
public static class SvgScene
{
    sealed class Node
    {
        public string Name = "";
        public Dictionary<string, string> Attr = [];
        public List<Node> Kids = [];
        public string Chars = "";
        public Node? Parent;
        public bool HasClass(string c) => Attr.TryGetValue("class", out var v) && v.Split(' ', StringSplitOptions.RemoveEmptyEntries).Contains(c);
    }

    /// <summary>A 2D similarity (scale, rotation, move) as the six numbers of an SVG matrix.</summary>
    readonly record struct Mat(double A, double B, double C, double D, double E, double F)
    {
        public static readonly Mat Identity = new(1, 0, 0, 1, 0, 0);

        /// <summary>Apply this, then <paramref name="n"/>.</summary>
        public Mat Then(Mat n) => new(A * n.A + B * n.C, A * n.B + B * n.D, C * n.A + D * n.C, C * n.B + D * n.D, E * n.A + F * n.C + n.E, E * n.B + F * n.D + n.F);

        public (double X, double Y) Pt(double x, double y) => (A * x + C * y + E, B * x + D * y + F);

        public double Scale => Math.Sqrt(Math.Abs(A * D - B * C));

        public double Angle => Math.Atan2(B, A) * 180 / Math.PI;
    }

    readonly record struct Paint(bool None, Rgba Color);

    sealed record Style
    {
        public Paint Fill = new(false, new Rgba(0, 0, 0));
        public Paint Stroke = new(true, default);
        public double Sw = 1, Fo = 1, So = 1, Op = 1, Size = 16;
        public bool Mid, EvenOdd;
        public string Marker = "";
        public double[] Dash = [];

        public Style Copy() => (Style)MemberwiseClone();
    }

    static Node? Parse(string src)
    {
        try
        {
            var settings = new XmlReaderSettings { DtdProcessing = DtdProcessing.Ignore, XmlResolver = null };
            using var reader = XmlReader.Create(new StringReader(src), settings);
            var root = new Node { Name = "#root" };
            var cur = root;
            while (reader.Read())
            {
                switch (reader.NodeType)
                {
                    case XmlNodeType.Element:
                        var n = new Node { Name = reader.LocalName, Parent = cur };
                        if (reader.HasAttributes)
                        {
                            while (reader.MoveToNextAttribute())
                                if (reader.Prefix.Length == 0) n.Attr[reader.LocalName] = reader.Value;
                            reader.MoveToElement();
                        }
                        cur.Kids.Add(n);
                        if (!reader.IsEmptyElement) cur = n;
                        break;
                    case XmlNodeType.EndElement:
                        if (cur.Parent is not null) cur = cur.Parent;
                        break;
                    case XmlNodeType.Text:
                    case XmlNodeType.CDATA:
                    case XmlNodeType.Whitespace:
                    case XmlNodeType.SignificantWhitespace:
                        cur.Chars += reader.Value;
                        break;
                }
            }
            return root.Kids.Count > 0 ? root.Kids[0] : null;
        }
        catch (XmlException)
        {
            return null;
        }
    }

    static double Num(string? s, double def)
    {
        if (s is null) return def;
        s = s.Trim();
        if (s.EndsWith("px", StringComparison.Ordinal)) s = s[..^2];
        return double.TryParse(s, NumberStyles.Float, CultureInfo.InvariantCulture, out var v) ? v : def;
    }

    static double[] Nums(string? s)
    {
        if (s is null) return [];
        return s.Split([' ', ',', '\t', '\n'], StringSplitOptions.RemoveEmptyEntries).Select(x => Num(x, 0)).ToArray();
    }

    /// <summary>Draws the document into the box (xMidYMid meet). Nothing is drawn for text that is not an SVG.</summary>
    public static void Draw(ICanvas cv, string svg, Rect4 box)
    {
        var root = Parse(svg);
        if (root is null || root.Name != "svg") return;
        var ids = new Dictionary<string, Node>();
        void Index(Node n)
        {
            if (n.Attr.TryGetValue("id", out var id) && id.Length > 0) ids[id] = n;
            foreach (var k in n.Kids) Index(k);
        }
        Index(root);
        var r = new Renderer(cv, ids);
        var st = new Style();
        r.Svg(root, r.Cascade(root, st), new Mat(1, 0, 0, 1, box.X, box.Y), box.W, box.H, 0);
    }

    sealed class Renderer(ICanvas cv, Dictionary<string, Node> ids)
    {
        public void Svg(Node n, Style st, Mat m, double w, double h, int depth)
        {
            var vb = Nums(n.Attr.GetValueOrDefault("viewBox"));
            if (vb.Length != 4) vb = [0, 0, Num(n.Attr.GetValueOrDefault("width"), w), Num(n.Attr.GetValueOrDefault("height"), h)];
            if (vb[2] <= 0 || vb[3] <= 0) return;
            var s = Math.Min(w / vb[2], h / vb[3]);
            var inner = new Mat(s, 0, 0, s, (w - vb[2] * s) / 2 - vb[0] * s, (h - vb[3] * s) / 2 - vb[1] * s);
            foreach (var k in n.Kids) Element(k, st, inner.Then(m), depth);
        }

        void Element(Node n, Style parent, Mat m, int depth)
        {
            if (depth > 8) return;
            var st = Cascade(n, parent);
            switch (n.Name)
            {
                case "g":
                    foreach (var k in n.Kids) Element(k, st, m, depth);
                    break;
                case "svg":
                    double x = Num(n.Attr.GetValueOrDefault("x"), 0), y = Num(n.Attr.GetValueOrDefault("y"), 0);
                    double w = Num(n.Attr.GetValueOrDefault("width"), 0), h = Num(n.Attr.GetValueOrDefault("height"), 0);
                    if (w > 0 && h > 0) Svg(n, st, new Mat(1, 0, 0, 1, x, y).Then(m), w, h, depth + 1);
                    break;
                case "rect":
                    {
                        double rx0 = Num(n.Attr.GetValueOrDefault("x"), 0), ry0 = Num(n.Attr.GetValueOrDefault("y"), 0);
                        double rw = Num(n.Attr.GetValueOrDefault("width"), 0), rh = Num(n.Attr.GetValueOrDefault("height"), 0);
                        var rx = Num(n.Attr.GetValueOrDefault("rx"), Num(n.Attr.GetValueOrDefault("ry"), 0));
                        var ry = Num(n.Attr.GetValueOrDefault("ry"), rx);
                        Shape(RoundRect(rx0, ry0, rw, rh, Math.Min(Math.Min(rx, ry), Math.Min(rw / 2, rh / 2))), true, st, m);
                        break;
                    }
                case "circle":
                    {
                        double cx = Num(n.Attr.GetValueOrDefault("cx"), 0), cy = Num(n.Attr.GetValueOrDefault("cy"), 0), rad = Num(n.Attr.GetValueOrDefault("r"), 0);
                        Shape(Ellipse(cx, cy, rad), true, st, m);
                        break;
                    }
                case "path":
                    {
                        var cmds = PathData.Parse(n.Attr.GetValueOrDefault("d") ?? "");
                        if (cmds is null) break;
                        Shape(cmds, true, st, m);
                        if (st.Marker.Length > 0) Marker(st, m, cmds);
                        break;
                    }
                case "text":
                    Text(n, st, m);
                    break;
            }
        }

        static List<PathCmd> RoundRect(double x, double y, double w, double h, double r)
        {
            if (w <= 0 || h <= 0) return [];
            if (r <= 0)
                return [new(PathOp.Move, X: x, Y: y), new(PathOp.Line, X: x + w, Y: y), new(PathOp.Line, X: x + w, Y: y + h), new(PathOp.Line, X: x, Y: y + h), new(PathOp.Close)];
            PathCmd Arc(double ex, double ey) => new(PathOp.Arc, X: ex, Y: ey, Rx: r, Ry: r, Sweep: true);
            return
            [
                new(PathOp.Move, X: x + r, Y: y),
                new(PathOp.Line, X: x + w - r, Y: y), Arc(x + w, y + r),
                new(PathOp.Line, X: x + w, Y: y + h - r), Arc(x + w - r, y + h),
                new(PathOp.Line, X: x + r, Y: y + h), Arc(x, y + h - r),
                new(PathOp.Line, X: x, Y: y + r), Arc(x + r, y),
                new(PathOp.Close),
            ];
        }

        static List<PathCmd> Ellipse(double cx, double cy, double r)
        {
            if (r <= 0) return [];
            return
            [
                new(PathOp.Move, X: cx + r, Y: cy),
                new(PathOp.Arc, X: cx - r, Y: cy, Rx: r, Ry: r, Sweep: true),
                new(PathOp.Arc, X: cx + r, Y: cy, Rx: r, Ry: r, Sweep: true),
                new(PathOp.Close),
            ];
        }

        static List<PathCmd> Moved(IReadOnlyList<PathCmd> path, Mat m)
        {
            var list = new List<PathCmd>(path.Count);
            var k = m.Scale;
            foreach (var c in path)
            {
                var (x, y) = m.Pt(c.X, c.Y);
                var (x1, y1) = m.Pt(c.X1, c.Y1);
                var (x2, y2) = m.Pt(c.X2, c.Y2);
                list.Add(c with { X = x, Y = y, X1 = x1, Y1 = y1, X2 = x2, Y2 = y2, Rx = c.Rx * k, Ry = c.Ry * k, Rotation = c.Rotation + m.Angle });
            }
            return list;
        }

        static Rgba With(Rgba c, double a) => c with { A = (byte)Math.Clamp(c.A * a + 0.5, 0, 255) };

        void Shape(IReadOnlyList<PathCmd> path, bool closed, Style st, Mat m)
        {
            if (path.Count == 0) return;
            var moved = Moved(path, m);
            Rgba? fill = st.Fill.None ? null : With(st.Fill.Color, st.Fo * st.Op);
            Rgba? stroke = st.Stroke.None || st.Sw <= 0 ? null : With(st.Stroke.Color, st.So * st.Op);
            if (fill is null && stroke is null) return;
            cv.Path(moved, fill, stroke, (float)(st.Sw * m.Scale), st.EvenOdd, st.Dash.Length >= 2 ? st.Dash.Select(d => (float)(d * m.Scale)).ToArray() : null);
        }

        void Marker(Style st, Mat m, List<PathCmd> path)
        {
            if (!ids.TryGetValue(st.Marker, out var mk) || path.Count < 2) return;
            // The last point and the direction the path arrives in.
            var end = path[^1];
            if (end.Op == PathOp.Close) return;
            double fromX, fromY;
            var prev = path[^2];
            if (end.Op == PathOp.Cubic) (fromX, fromY) = (end.X2, end.Y2);
            else (fromX, fromY) = (prev.X, prev.Y);
            if (Math.Abs(fromX - end.X) < 1e-6 && Math.Abs(fromY - end.Y) < 1e-6) (fromX, fromY) = (prev.X, prev.Y);
            var ang = Math.Atan2(end.Y - fromY, end.X - fromX);
            var vb = Nums(mk.Attr.GetValueOrDefault("viewBox"));
            if (vb.Length != 4) return;
            double mw = Num(mk.Attr.GetValueOrDefault("markerWidth"), 3), mh = Num(mk.Attr.GetValueOrDefault("markerHeight"), 3);
            var s = Math.Min(mw / vb[2], mh / vb[3]) * st.Sw;
            double rx = Num(mk.Attr.GetValueOrDefault("refX"), 0), ry = Num(mk.Attr.GetValueOrDefault("refY"), 0);
            double c = Math.Cos(ang), sn = Math.Sin(ang);
            var mm = new Mat(1, 0, 0, 1, -rx, -ry).Then(new Mat(s, 0, 0, s, 0, 0)).Then(new Mat(c, sn, -sn, c, end.X, end.Y));
            foreach (var k in mk.Kids)
            {
                var ms = Cascade(k, new Style());
                ms.Stroke = new Paint(true, default);
                Element(k, ms, mm.Then(m), 1);
            }
        }

        void Text(Node n, Style st, Mat m)
        {
            if (st.Fill.None) return;
            void One(string s, double x, double y, Style ts)
            {
                s = s.Trim();
                if (s.Length == 0) return;
                var (px, py) = m.Pt(x, y);
                var k = m.Scale;
                cv.Label(s, (float)px, (float)py, (float)(ts.Size * k), 400, With(ts.Fill.Color, ts.Fo * ts.Op), ts.Mid ? Align.Center : Align.Start, FontFamily.Sans);
            }
            double bx = Num(n.Attr.GetValueOrDefault("x"), 0), by = Num(n.Attr.GetValueOrDefault("y"), 0);
            var any = false;
            foreach (var k in n.Kids)
            {
                if (k.Name != "tspan") continue;
                any = true;
                One(k.Chars, Num(k.Attr.GetValueOrDefault("x"), bx), Num(k.Attr.GetValueOrDefault("y"), by), Cascade(k, st));
            }
            if (!any) One(n.Chars, bx, by, st);
        }

        public Style Cascade(Node n, Style p)
        {
            var st = p.Copy();
            st.Op = 1;
            st.Marker = "";
            var a = n.Attr;
            if (a.TryGetValue("fill", out var v)) st.Fill = PaintOf(v);
            if (a.TryGetValue("stroke", out v)) st.Stroke = PaintOf(v);
            if (a.TryGetValue("stroke-width", out v)) st.Sw = Num(v, st.Sw);
            if (a.TryGetValue("fill-opacity", out v)) st.Fo = Num(v, 1);
            if (a.TryGetValue("stroke-opacity", out v)) st.So = Num(v, 1);
            if (a.TryGetValue("opacity", out v)) st.Op = Num(v, 1);
            if (a.TryGetValue("fill-rule", out v)) st.EvenOdd = v == "evenodd";
            if (a.TryGetValue("font-size", out v)) st.Size = Num(v, st.Size);
            if (a.TryGetValue("text-anchor", out v)) st.Mid = v == "middle";
            if (a.TryGetValue("stroke-dasharray", out v)) st.Dash = Nums(v);
            if (a.TryGetValue("marker-end", out v)) st.Marker = v.Replace("url(#", "").TrimEnd(')');
            Flow(n, st);
            return st;
        }

        /// <summary>page.html's `.flow` rules for the elements of a flowchart.</summary>
        static void Flow(Node n, Style st)
        {
            var inside = false;
            for (var p = n.Parent; p is not null; p = p.Parent)
                if (p.HasClass("flow")) inside = true;
            if (n.HasClass("flow")) st.Size = 12;
            if (!inside) return;
            var pc = n.Parent?.Attr.GetValueOrDefault("class") ?? "";
            static Paint P(byte r, byte g, byte b, double a) => new(false, new Rgba(r, g, b, (byte)(a * 255)));
            var parent = n.Parent;
            if (parent is not null && parent.HasClass("n") && n.Name is "rect" or "path" or "circle")
            {
                st.Fill = P(144, 70, 255, .18);
                st.Stroke = P(0xC4, 0xA2, 0xFF, 1);
                st.Sw = 1.2;
                if (parent.HasClass("diamond") && n.Name == "path")
                {
                    st.Fill = P(255, 154, 74, .14);
                    st.Stroke = P(0xff, 0xb3, 0x6b, 1);
                }
            }
            else if (n.HasClass("e"))
            {
                st.Fill = new Paint(true, default);
                st.Stroke = P(246, 242, 255, .62);
                st.Sw = 1.4;
                if (n.HasClass("dot")) st.Dash = [4, 3];
                if (n.HasClass("thick")) st.Sw = 2.6;
            }
            else if (n.Name == "path" && parent is { Name: "marker" })
            {
                st.Fill = P(246, 242, 255, .62);
            }
            else if (parent is not null && parent.HasClass("el") && n.Name == "rect")
            {
                st.Fill = new Paint(false, new Rgba(0x1a, 0x12, 0x20));
                st.Stroke = P(255, 255, 255, .09);
            }
            else if (n.Name == "text")
            {
                st.Fill = P(0xf6, 0xf2, 0xff, 1);
                st.Mid = true;
                if (pc.Contains("el", StringComparison.Ordinal) && parent is not null && parent.HasClass("el"))
                {
                    st.Fill = P(246, 242, 255, .62);
                    st.Size = 11;
                }
            }
        }

        static Paint PaintOf(string v)
        {
            v = v.Trim();
            if (v is "none" or "") return new Paint(true, default);
            // Gradients are not drawn (the tools' marks have their own path).
            if (v.StartsWith("url(", StringComparison.Ordinal)) return new Paint(true, default);
            return new Paint(false, Color(v));
        }

        static Rgba Color(string v)
        {
            v = v.Trim().ToLowerInvariant();
            if (v.StartsWith('#'))
            {
                var h = v[1..];
                if (h.Length is 3 or 4) h = string.Concat(h.Select(c => $"{c}{c}"));
                if (!uint.TryParse(h, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var u)) return default;
                return h.Length == 8 ? new Rgba((byte)(u >> 24), (byte)(u >> 16), (byte)(u >> 8), (byte)u) : new Rgba((byte)(u >> 16), (byte)(u >> 8), (byte)u);
            }
            if (v.StartsWith("rgb", StringComparison.Ordinal))
            {
                var open = v.IndexOf('(');
                var close = v.LastIndexOf(')');
                if (open < 0 || close < open) return default;
                var f = Nums(v[(open + 1)..close].Replace('/', ' '));
                if (f.Length < 3) return default;
                var a = f.Length > 3 ? f[3] : 1;
                return new Rgba((byte)f[0], (byte)f[1], (byte)f[2], (byte)(a * 255 + 0.5));
            }
            return v switch { "white" => new Rgba(255, 255, 255), "black" => new Rgba(0, 0, 0), _ => default };
        }
    }
}
