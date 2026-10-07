using HoverAvalonia.Core;

namespace HoverAvalonia.Office;

public enum Blend { Normal, Additive }
public enum MatKind { Std, Basic, Glow, Points }

/// <summary>Port of scene.rs's Mat: MeshStandardMaterial / MeshBasicMaterial / glow sprite / points.</summary>
public sealed class Mat
{
    public MatKind Kind;
    public Rgb Color;
    public double Rough, Metal, Opacity = 1;
    public bool Vertex, Tone, DepthWrite = true, Double;
    public Blend Blend;
    public int Tex = -1;

    public static Mat Std(Rgb c, double rough, double metal = 0) => new() { Kind = MatKind.Std, Color = c, Rough = rough, Metal = metal };
    public static Mat StdVertex(double rough) => new() { Kind = MatKind.Std, Color = new(1, 1, 1), Rough = rough, Vertex = true };
    public static Mat Basic(uint hex, bool tone) => new() { Kind = MatKind.Basic, Color = Rgb.Hex(hex), Tone = tone };
    public static Mat BasicRgb(Rgb c, double opacity, bool tone = false) => new() { Kind = MatKind.Basic, Color = c, Opacity = opacity, Tone = tone };
    public static Mat Glow(Rgb c, double opacity) => new() { Kind = MatKind.Glow, Color = c, Opacity = opacity, Tone = true, DepthWrite = false, Blend = Blend.Additive };
    public static Mat Points(Rgb c, double opacity) => new() { Kind = MatKind.Points, Color = c, Opacity = opacity, Tone = true, DepthWrite = false, Blend = Blend.Additive };
    public bool Transparent => Kind switch { MatKind.Basic => Opacity < 1 || Blend == Blend.Additive, MatKind.Glow or MatKind.Points => true, _ => false };
}

public readonly record struct VBox(double X, double Y, double Z, double W, double H, double D, Rgb C);

public enum GeoKind { Unit, Merged, Cylinder, Ring, Plane, Quad, Sprite, Points }
public sealed class Geo
{
    public GeoKind Kind; public int Merged;
    public double Top, Bottom, H, Inner, Outer, W; public int Seg;
    public V3[] Pts = [];
    public static Geo Unit => new() { Kind = GeoKind.Unit };
    public static Geo Sprite => new() { Kind = GeoKind.Sprite };
}

public enum HitKind { None, Bot, Prop, Desk }

public sealed class Node
{
    public int Parent = -1;
    public V3 P, R, S = new(1, 1, 1);
    public bool Visible = true, Cast, Receive;
    public Geo? Geometry; public Mat? Material;
    public HitKind Hit; public int HitIndex;
    public string? MeshKey;
}

public sealed class Graph
{
    public const int Root = 0;
    public readonly List<Node> Nodes = [new Node()];
    public readonly List<List<VBox>> Merged = [];

    public int Add(int parent, V3 p) { Nodes.Add(new Node { Parent = parent, P = p }); return Nodes.Count - 1; }
    public int Pivot(int parent, double x, double y, double z) => Add(parent, new(x, y, z));

    public int Box(int parent, double w, double h, double d, double x, double y, double z, Mat mat, bool shadow)
    {
        int n = Add(parent, new(x, y, z));
        var node = Nodes[n];
        node.S = new(w, h, d); node.Geometry = Geo.Unit; node.Material = mat; node.Cast = shadow; node.Receive = true;
        return n;
    }
    public int Drawing(int parent, Geo geo, Mat mat) { int n = Add(parent, default); Nodes[n].Geometry = geo; Nodes[n].Material = mat; return n; }

    M4 Local(int i) { var n = Nodes[i]; return M4.Trs(n.P, n.R, n.S); }
    public M4[] World()
    {
        var w = new M4[Nodes.Count];
        for (int i = 0; i < Nodes.Count; i++) { var l = Local(i); w[i] = Nodes[i].Parent >= 0 ? w[Nodes[i].Parent].Mul(l) : l; }
        return w;
    }
    public bool[] Shown()
    {
        var v = new bool[Nodes.Count];
        for (int i = 0; i < Nodes.Count; i++) v[i] = Nodes[i].Visible && (Nodes[i].Parent < 0 || v[Nodes[i].Parent]);
        return v;
    }
}

/// <summary>class Vox: boxes gathered into one mesh with a colour per box; RNG drawn only when jittered.</summary>
public sealed class Vox(Rng r)
{
    public readonly List<VBox> B = [];
    public Vox Bx(double x, double y, double z, double w, double h, double d, uint c, double j = 0.04)
    {
        var col = Rgb.Hex(c);
        if (j != 0) col = col.Mul(1 + (r.Next() - 0.5) * j * 2);
        B.Add(new(x, y, z, w, h, d, col));
        return this;
    }
}

/// <summary>The room's handles: what main.js keeps in variables to change later.</summary>
public sealed class Room
{
    public int Door, ExitGlow, Sky, Tv, Board, Clock, TvGlow, ClockGlow, CoffeeLed, FloorShade, Vac, Patch, Beam, Dust;
    public int[] Steam = new int[3];
    public List<int> Shades = [], DeskGlows = [];
    public List<V3> Lamps = [];
}

/// <summary>Port of scene.rs::build — the room, in the page's order (the order matters: every jittered box draws from R).</summary>
public static class RoomBuilder
{
    public const double RW = 14, RD = 11, WH = 4, X0 = -7, Z0 = -5.5;
    public static readonly (double x, double z) Door = (-5.65, Z0 + 0.35);
    public static readonly (double x, double z)[] Desks = [(-3.2, -1.6), (0.6, -1.6), (4.4, -1.6), (-3.2, 2.1), (0.6, 2.1), (4.4, 2.1)];
    public static double Seat(int d) => Desks[d].x - 0.67;
    public const int TexSky = 0, TexTv = 1, TexBoard = 2, TexClock = 3, TexBeam = 4, TexPatch = 5, TexGlow = 6, TexWhite = 7;

    static int Glow(Graph g, int parent, uint hex, double size, double opacity, V3 p)
    {
        int n = g.Drawing(parent, Geo.Sprite, Mat.Glow(Rgb.Hex(hex), opacity));
        g.Nodes[n].S = new(size, size, size); g.Nodes[n].P = p; return n;
    }
    static int Screen(Graph g, int tex, double w, double h, V3 p, double ry)
    {
        var m = Mat.BasicRgb(new(1, 1, 1), 1); m.Tex = tex;
        int n = g.Drawing(Graph.Root, new Geo { Kind = GeoKind.Plane, W = w, H = h }, m);
        g.Nodes[n].P = p; g.Nodes[n].R = new(0, ry, 0); return n;
    }
    static int Quad(Graph g, V3[] pts, int tex)
    {
        var m = Mat.BasicRgb(new(1, 1, 1), 1); m.Tex = tex; m.Blend = Blend.Additive; m.DepthWrite = false; m.Double = true;
        return g.Drawing(Graph.Root, new Geo { Kind = GeoKind.Quad, Pts = pts }, m);
    }
    static void Plant(Vox v, double x, double z, double s, double y, int seed)
    {
        var r = new Rng(seed);
        v.Bx(x - 0.2 * s, y, z - 0.2 * s, 0.4 * s, 0.34 * s, 0.4 * s, 0xa4552e).Bx(x - 0.23 * s, y + 0.3 * s, z - 0.23 * s, 0.46 * s, 0.07 * s, 0.46 * s, 0xb8653a)
            .Bx(x - 0.03 * s, y + 0.34 * s, z - 0.03 * s, 0.06 * s, 0.55 * s, 0.06 * s, 0x4a3a22);
        uint[] leaf = [0x4f7a3a, 0x3e6630, 0x6a9a45, 0x5a8a3a];
        for (int i = 0; i < 11; i++)
        {
            double a = r.Next() * Rng.Tau, rr = r.Next() * 0.3 * s, h = (0.45 + r.Next() * 0.6) * s, w = (0.14 + r.Next() * 0.16) * s;
            v.Bx(x + Math.Cos(a) * rr - w / 2, y + h, z + Math.Sin(a) * rr - w / 2, w, w * 0.7, w, leaf[i % 4], 0.08);
        }
    }

    public static Room Build(Graph g, Rng r)
    {
        var walls = new Vox(r); var room = new Vox(r);
        var o = room; var w = walls;
        o.Bx(X0 - 0.3, -0.7, Z0 - 0.3, RW + 0.3, 0.6, RD + 0.3, 0x1c1215, 0);
        for (int i = 0; i < (int)RW; i++) for (int k = 0; k < (int)RD; k++) o.Bx(X0 + i, -0.1, Z0 + k, 1, 0.1, 1, (i + k) % 2 != 0 ? 0x5a3c34u : 0x48302bu, 0.05);
        o.Bx(X0 - 0.3, -0.7, Z0 - 0.3 + RD + 0.3 - 0.02, RW + 0.3, 0.6, 0.02, 0x140c0f, 0);
        for (double x = 0; x < RW; x += 0.5) w.Bx(X0 + x, -0.7, Z0 - 0.3, 0.5, WH + 0.7, 0.3, (x * 2) % 2 != 0 ? 0x6e4643u : 0x684240u, 0.03);
        for (double z = 0; z < RD; z += 0.5) w.Bx(X0 - 0.3, -0.7, Z0 + z, 0.3, WH + 0.7, 0.5, (z * 2) % 2 != 0 ? 0x633e3cu : 0x5e3a38u, 0.03);
        w.Bx(X0 - 0.3, -0.7, Z0 - 0.3, 0.3, WH + 0.7, 0.3, 0x5e3a38, 0);
        w.Bx(X0, 0, Z0, RW, 1.15, 0.035, 0x4b2f2c, 0.02).Bx(X0, 0, Z0, 0.035, 1.15, RD, 0x462b29, 0.02);
        w.Bx(X0, 1.15, Z0, RW, 0.07, 0.06, 0x80564d, 0).Bx(X0, 1.15, Z0, 0.06, 0.07, RD, 0x7a524a, 0);
        w.Bx(X0, 0, Z0, RW, 0.16, 0.07, 0x33201d, 0).Bx(X0, 0, Z0, 0.07, 0.16, RD, 0x301e1b, 0);
        w.Bx(X0 - 0.3, WH, Z0 - 0.3, RW + 0.3, 0.1, 0.3, 0x8d5f57, 0).Bx(X0 - 0.3, WH, Z0 - 0.3, 0.3, 0.1, RD + 0.3, 0x86594f, 0);
        w.Bx(X0 - 0.3, -0.7, Z0 + RD - 0.02, 0.3, WH + 0.8, 0.02, 0x3a2422, 0).Bx(X0 + RW - 0.02, -0.7, Z0 - 0.3, 0.02, WH + 0.8, 0.3, 0x3a2422, 0);

        // Door on the back wall; the panel swings.
        w.Bx(-6.3, 0, Z0, 1.3, 2.42, 0.08, 0x2c1b17, 0).Bx(-6.2, 0, Z0 + 0.01, 1.1, 2.3, 0.08, 0x0b0708, 0);
        var doorV = new Vox(r);
        doorV.Bx(0, 0, 0, 1.1, 2.3, 0.07, 0x5c3b2b, 0.02).Bx(0.14, 1.28, 0.07, 0.82, 0.82, 0.02, 0x6b4633, 0).Bx(0.14, 0.24, 0.07, 0.82, 0.86, 0.02, 0x6b4633, 0).Bx(0.9, 1.05, 0.07, 0.08, 0.08, 0.06, 0xe0ab4c, 0);
        int door = g.Pivot(Graph.Root, -6.2, 0, Z0 + 0.02);
        g.Merged.Add(doorV.B);
        int dm = g.Drawing(door, new Geo { Kind = GeoKind.Merged, Merged = g.Merged.Count - 1 }, Mat.StdVertex(0.88));
        g.Nodes[dm].Cast = true; g.Nodes[dm].Receive = true;
        o.Bx(-6.35, 0, Z0 + 0.12, 1.4, 0.02, 0.75, 0x6f3b2a, 0.02).Bx(-6.2, 0.02, Z0 + 0.22, 1.1, 0.005, 0.55, 0x8a4c34, 0);
        g.Box(Graph.Root, 0.34, 0.12, 0.08, Door.x, 2.62, Z0 + 0.05, Mat.Basic(0xffb35c, false), false);
        int exitGlow = Glow(g, Graph.Root, 0xffa24a, 1.4, 0.55, new(Door.x, 2.62, Z0 + 0.2));

        // Window with curtains; the sky behind it.
        w.Bx(0.1, 3.25, Z0, 2.8, 0.12, 0.12, 0x3a2620, 0).Bx(-0.05, 1.22, Z0, 3.1, 0.12, 0.26, 0x4a3026, 0)
            .Bx(0.1, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).Bx(2.78, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0)
            .Bx(1.46, 1.34, Z0, 0.08, 1.92, 0.1, 0x3a2620, 0).Bx(0.2, 2.26, Z0, 2.6, 0.07, 0.1, 0x3a2620, 0)
            .Bx(-0.5, 3.52, Z0 + 0.1, 4.0, 0.05, 0.05, 0x241612, 0);
        foreach (double cx in new[] { -0.45, 2.9 })
            for (int i = 0; i < 4; i++) w.Bx(cx + i * 0.13, 1.0, Z0 + 0.06 + (i % 2) * 0.05, 0.14, 2.52, 0.1, i % 2 != 0 ? 0x9b5230u : 0x8a4629u, 0);
        int sky = Screen(g, TexSky, 2.56, 1.9, new(1.5, 2.3, Z0 + 0.012), 0);

        // Wall TV and the cabinet under it.
        w.Bx(4.1, 1.62, Z0, 2.6, 1.52, 0.1, 0x0c0b10, 0);
        int tv = Screen(g, TexTv, 2.44, 1.38, new(5.4, 2.38, Z0 + 0.105), 0);
        int tvGlow = Glow(g, Graph.Root, 0x5aa8ff, 3.6, 0.22, new(5.4, 2.3, Z0 + 0.6));
        o.Bx(4.3, 0, Z0 + 0.02, 2.2, 0.55, 0.5, 0x4c3028).Bx(4.25, 0.55, Z0 + 0.02, 2.3, 0.05, 0.54, 0x5e3c30, 0)
            .Bx(4.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).Bx(5.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0)
            .Bx(4.45, 0.6, Z0 + 0.12, 0.26, 0.42, 0.26, 0x22212a).Bx(6.1, 0.6, Z0 + 0.1, 0.24, 0.1, 0.3, 0xc8a24a).Bx(6.12, 0.7, Z0 + 0.1, 0.2, 0.08, 0.3, 0x5a7aa0);
        // NOTE: Rust's `.b(...)` is `.bx(..., 0.04)`; Vox.Bx defaults to 0.04 too, but the 5-arg overloads above used `.bx(.., 0.0)` explicitly where Rust did.

        // Coffee counter.
        o.Bx(-4.6, 0, Z0 + 0.02, 1.9, 0.86, 0.62, 0x5a3a2c).Bx(-4.65, 0.86, Z0 + 0.02, 2.0, 0.06, 0.66, 0xd8cdbf, 0.02)
            .Bx(-4.5, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).Bx(-3.6, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0)
            .Bx(-4.45, 0.92, Z0 + 0.1, 0.42, 0.56, 0.4, 0x26252d).Bx(-4.4, 1.2, Z0 + 0.5, 0.32, 0.18, 0.02, 0x121118)
            .Bx(-3.8, 0.92, Z0 + 0.2, 0.12, 0.14, 0.12, 0xeeeeee).Bx(-3.6, 0.92, Z0 + 0.25, 0.12, 0.14, 0.12, 0x9b6bff)
            .Bx(-3.3, 0.92, Z0 + 0.12, 0.34, 0.4, 0.3, 0x7a8a95).Bx(-4.6, 1.85, Z0, 1.9, 0.05, 0.3, 0x4a2f24, 0);
        uint[] jars = [0xc8a24a, 0x9a4f2e, 0x7a9a6a, 0xdcd2c4, 0xb46a3a];
        for (int i = 0; i < 5; i++) o.Bx(-4.5 + i * 0.36, 1.9, Z0 + 0.06, 0.2, 0.22 + (i % 2) * 0.08, 0.18, jars[i]);
        int coffeeLed = Glow(g, Graph.Root, 0x7ee0ff, 0.35, 0.9, new(-4.24, 1.29, Z0 + 0.53));
        int[] steam = [Glow(g, Graph.Root, 0xffffff, 0.2, 0.25, default), Glow(g, Graph.Root, 0xffffff, 0.2, 0.25, default), Glow(g, Graph.Root, 0xffffff, 0.2, 0.25, default)];

        // Left wall: bookcase, board, clock, painting.
        o.Bx(X0, 0, -4.3, 0.46, 2.42, 0.06, 0x4a2e24).Bx(X0, 0, -2.76, 0.46, 2.42, 0.06, 0x4a2e24).Bx(X0, 0, -4.3, 0.05, 2.42, 1.6, 0x3a241c, 0);
        uint[] books = [0x8a3b2e, 0xc9a24a, 0x4a6b4a, 0x7b5aa6, 0xd07a3a, 0x3a5a8a, 0xb8b0a0, 0x9a4a5a];
        foreach (double y in new[] { 0.0, 0.6, 1.2, 1.8, 2.36 })
        {
            o.Bx(X0, y, -4.26, 0.46, 0.06, 1.52, 0x55352a, 0.02);
            if (y > 2.0) continue;
            double z = -4.2;
            while (z < -2.9)
            {
                double bw = 0.06 + r.Next() * 0.07, bh = 0.26 + r.Next() * 0.2;
                if (z + bw > -2.82) break;
                double depth = 0.32 + r.Next() * 0.06;
                uint book = books[(int)(r.Next() * books.Length)];
                o.Bx(X0 + 0.06, y + 0.06, z, depth, Math.Min(bh, 0.5), bw, book, 0.06);
                z += bw + (r.Next() < 0.12 ? 0.08 : 0.006);
            }
        }
        w.Bx(X0, 1.46, -2.2, 0.07, 1.74, 3.0, 0x3a2620, 0);
        int board = Screen(g, TexBoard, 2.84, 1.6, new(X0 + 0.075, 2.33, -0.7), Math.PI / 2);
        w.Bx(X0, 2.42, 1.22, 0.09, 0.6, 1.16, 0x0f0d12, 0);
        int clock = Screen(g, TexClock, 1.04, 0.48, new(X0 + 0.095, 2.72, 1.8), Math.PI / 2);
        int clockGlow = Glow(g, Graph.Root, 0xff7a2a, 1.6, 0.35, new(X0 + 0.3, 2.72, 1.8));
        w.Bx(X0, 1.6, 3.55, 0.06, 1.02, 1.42, 0x2e1c18, 0).Bx(X0 + 0.06, 1.68, 3.63, 0.01, 0.86, 1.26, 0x41628f, 0)
            .Bx(X0 + 0.07, 1.68, 3.63, 0.01, 0.3, 1.26, 0x3f6a44, 0).Bx(X0 + 0.075, 1.9, 3.75, 0.01, 0.3, 0.5, 0x5a7a5a, 0)
            .Bx(X0 + 0.075, 1.95, 4.2, 0.01, 0.42, 0.55, 0x6a8a6a, 0).Bx(X0 + 0.08, 2.24, 4.45, 0.01, 0.13, 0.13, 0xffd070, 0)
            .Bx(X0 + 0.075, 2.28, 4.3, 0.01, 0.09, 0.3, 0xe8f0ff, 0);

        // Lounge.
        o.Bx(-6.8, 0, 2.95, 2.8, 0.02, 2.5, 0x6f4430, 0.02).Bx(-6.5, 0.02, 3.25, 2.2, 0.01, 1.9, 0x8a5a3c, 0.02);
        o.Bx(X0 + 0.05, 0, 3.2, 0.95, 0.42, 2.1, 0x5b3a6a).Bx(X0 + 0.05, 0.42, 3.2, 0.26, 0.55, 2.1, 0x4f3160)
            .Bx(X0 + 0.05, 0.42, 3.02, 0.95, 0.24, 0.2, 0x553565).Bx(X0 + 0.05, 0.42, 5.28, 0.95, 0.24, 0.2, 0x553565)
            .Bx(X0 + 0.32, 0.42, 3.24, 0.66, 0.1, 0.98, 0x6b4a7a).Bx(X0 + 0.32, 0.42, 4.28, 0.66, 0.1, 0.98, 0x6b4a7a)
            .Bx(X0 + 0.33, 0.52, 3.4, 0.14, 0.34, 0.42, 0xd9a64a).Bx(X0 + 0.33, 0.52, 4.8, 0.14, 0.3, 0.36, 0x5aa8a0);
        o.Bx(-5.55, 0.32, 3.7, 0.8, 0.06, 1.25, 0x6b4a36).Bx(-5.5, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).Bx(-4.85, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c)
            .Bx(-5.5, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).Bx(-4.85, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c)
            .Bx(-5.3, 0.38, 3.9, 0.3, 0.05, 0.4, 0x3a5a8a).Bx(-5.0, 0.38, 4.5, 0.12, 0.14, 0.12, 0xeeeeee);
        o.Bx(-6.75, 0, 2.55, 0.26, 0.04, 0.26, 0x241a16).Bx(-6.64, 0.04, 2.66, 0.04, 1.6, 0.04, 0x241a16);
        int floorShade = g.Box(Graph.Root, 0.44, 0.3, 0.44, -6.62, 1.78, 2.68, Mat.Basic(0xffc27a, false), false);

        // Beanbags.
        o.Bx(4.7, 0, 3.8, 0.9, 0.3, 0.9, 0x7a4a9a).Bx(4.8, 0.3, 3.9, 0.7, 0.14, 0.7, 0x8a5aaa).Bx(4.75, 0.3, 3.82, 0.2, 0.34, 0.84, 0x6a3a8a)
            .Bx(5.9, 0, 4.4, 0.8, 0.28, 0.8, 0x2f8a7a).Bx(6.0, 0.28, 4.5, 0.6, 0.12, 0.6, 0x3a9a8a);

        Plant(room, 6.4, Z0 + 0.55, 1.6, 0, 3); Plant(room, -0.6, Z0 + 0.5, 1.2, 0, 5); Plant(room, 6.4, 5.0, 1.5, 0, 7);
        Plant(room, -3.3, 5.0, 1.0, 0, 9); Plant(room, -3.95, Z0 + 0.3, 0.55, 0.92, 4);

        // Desks.
        foreach (double z in new[] { -1.6, 2.1 })
            o.Bx(-4.9, 0, z - 1.15, 10.4, 0.015, 2.3, z < 0 ? 0x3f4a3au : 0x6e4a2au, 0.02).Bx(-4.7, 0.015, z - 0.95, 10.0, 0.006, 1.9, z < 0 ? 0x4a5846u : 0x7e5634u, 0.02);
        var shades = new List<int>(); var lamps = new List<V3>(); var deskGlows = new List<int>();
        uint[] mugs = [0xeeeeee, 0x9b6bff, 0xff9a4a];
        for (int di = 0; di < Desks.Length; di++)
        {
            var (x, z) = Desks[di];
            o.Bx(x - 0.42, 0.64, z - 0.78, 0.84, 0.07, 1.56, 0x6e4c37, 0.02);
            foreach (var (lx, lz) in new[] { (-0.38, -0.74), (0.3, -0.74), (-0.38, 0.68), (0.3, 0.68) }) o.Bx(x + lx, 0, z + lz, 0.07, 0.64, 0.07, 0x3e2a1f, 0);
            o.Bx(x - 0.3, 0.12, z + 0.3, 0.66, 0.5, 0.42, 0x5e4030).Bx(x - 0.31, 0.38, z + 0.36, 0.01, 0.04, 0.3, 0xc8a24a, 0);
            o.Bx(x + 0.02, 0.71, z - 0.14, 0.24, 0.03, 0.28, 0x1c1c24).Bx(x + 0.1, 0.74, z - 0.04, 0.06, 0.2, 0.08, 0x1c1c24)
                .Bx(x + 0.02, 0.88, z - 0.4, 0.09, 0.46, 0.8, 0x1a1d28).Bx(x + 0.11, 0.92, z - 0.12, 0.02, 0.26, 0.24, 0x2a2f3e, 0)
                .Bx(x - 0.36, 0.71, z - 0.26, 0.17, 0.025, 0.52, 0x2c3040).Bx(x - 0.34, 0.735, z - 0.24, 0.13, 0.008, 0.48, 0x454a5e, 0)
                .Bx(x - 0.34, 0.71, z + 0.36, 0.1, 0.03, 0.07, 0x2c3040).Bx(x - 0.1, 0.71, z + 0.3, 0.26, 0.04, 0.34, 0xece6da, 0.02)
                .Bx(x + 0.12, 0.71, z + 0.55, 0.11, 0.13, 0.11, mugs[di % 3]);
            o.Bx(x + 0.14, 0.71, z - 0.65, 0.18, 0.03, 0.18, 0x2a2a30).Bx(x + 0.21, 0.74, z - 0.58, 0.04, 0.4, 0.04, 0x2a2a30);
            shades.Add(g.Box(Graph.Root, 0.26, 0.14, 0.26, x + 0.23, 1.16, z - 0.56, Mat.Basic(0xffc27a, false), false));
            lamps.Add(new(x - 0.1, 1.1, z - 0.3));
            double s = x - 0.67;
            o.Bx(s - 0.24, 0.38, z - 0.24, 0.48, 0.07, 0.48, 0x3b2d4c).Bx(s - 0.29, 0.45, z - 0.22, 0.07, 0.54, 0.44, 0x33263f)
                .Bx(s - 0.03, 0.08, z - 0.03, 0.06, 0.3, 0.06, 0x1c1c22).Bx(s - 0.22, 0.04, z - 0.03, 0.44, 0.04, 0.06, 0x1c1c22).Bx(s - 0.03, 0.04, z - 0.22, 0.06, 0.04, 0.44, 0x1c1c22);
            deskGlows.Add(Glow(g, Graph.Root, 0x7fb8ff, 1.3, 0, new(x - 0.22, 1.02, z)));
        }

        g.Merged.Add(walls.B);
        int wm = g.Drawing(Graph.Root, new Geo { Kind = GeoKind.Merged, Merged = g.Merged.Count - 1 }, Mat.StdVertex(0.88));
        g.Nodes[wm].Receive = true;
        g.Merged.Add(room.B);
        int rm = g.Drawing(Graph.Root, new Geo { Kind = GeoKind.Merged, Merged = g.Merged.Count - 1 }, Mat.StdVertex(0.88));
        g.Nodes[rm].Receive = true; g.Nodes[rm].Cast = true;

        // The vacuum.
        int vac = g.Add(Graph.Root, default);
        int a = g.Drawing(vac, new Geo { Kind = GeoKind.Cylinder, Top = 0.27, Bottom = 0.28, H = 0.08, Seg = 24 }, Mat.Std(Rgb.Hex(0x2a2a33), 0.5));
        g.Nodes[a].P = new(0, 0.05, 0); g.Nodes[a].Cast = true; g.Nodes[a].Receive = true;
        int b = g.Drawing(vac, new Geo { Kind = GeoKind.Cylinder, Top = 0.17, Bottom = 0.17, H = 0.02, Seg = 20 }, Mat.Std(Rgb.Hex(0x4a4a58), 0.4));
        g.Nodes[b].P = new(0, 0.1, 0); g.Nodes[b].Receive = true;
        Glow(g, vac, 0x4ade80, 0.22, 0.9, new(0, 0.13, 0.2));

        // Light through the window.
        int patch = Quad(g, [new(0.3, 0.02, Z0 + 1.2), new(2.9, 0.02, Z0 + 1.2), new(3.9, 0.02, Z0 + 3.7), new(1.3, 0.02, Z0 + 3.7)], TexPatch);
        int beam = Quad(g, [new(0.2, 3.25, Z0 + 0.02), new(2.8, 3.25, Z0 + 0.02), new(3.9, 0.02, Z0 + 3.7), new(1.3, 0.02, Z0 + 3.7)], TexBeam);
        var dr = new Rng(4);
        var pts = new V3[46];
        for (int i = 0; i < 46; i++)
        {
            double f = dr.Next(), px = 0.4 + dr.Next() * 2.4 + f * 1.1, py = 3.1 * (1 - f) + dr.Next() * 0.3, pz = Z0 + 0.3 + f * 3.2;
            pts[i] = new(px, py, pz);
        }
        int dust = g.Drawing(Graph.Root, new Geo { Kind = GeoKind.Points, Pts = pts }, Mat.Points(Rgb.Hex(0xffe2a8), 0.8));

        return new Room
        {
            Door = door, ExitGlow = exitGlow, Sky = sky, Tv = tv, Board = board, Clock = clock, TvGlow = tvGlow, ClockGlow = clockGlow, CoffeeLed = coffeeLed,
            Steam = steam, Shades = shades, FloorShade = floorShade, Lamps = lamps, DeskGlows = deskGlows, Vac = vac, Patch = patch, Beam = beam, Dust = dust,
        };
    }
}
