namespace Hover.Office;

// main.js's scene: a tree of nodes (Object3D), each maybe drawing something, and the room
// built into it line by line in the page's order (the order matters: every jittered box
// draws from the same generator). A port of internal/office/scene.go.

public enum Blend : byte { Normal, Additive }

public enum MatKind : byte
{
    /// <summary>MeshStandardMaterial: a colour (or the vertices'), roughness, metalness.</summary>
    Std,
    /// <summary>MeshBasicMaterial: unlit; tone mapped unless said.</summary>
    Basic,
    /// <summary>SpriteMaterial with the radial glow texture, additive (tone mapped by default).</summary>
    Glow,
    /// <summary>PointsMaterial: 1 px points (size 0.05, not attenuated by an orthographic camera).</summary>
    Points,
}

/// <summary>A material. Make one with <see cref="BasicHex"/>, <see cref="StdMat"/> or an initialiser that sets Tex (-1 for none).</summary>
public struct Mat
{
    public MatKind Kind;
    public Rgb Color;
    public double Rough, Metal;
    public bool Vertex, Tone;
    public double Opacity;
    public Blend Blend;
    public bool DepthWrite;
    public int Tex;
    public bool Double;

    public static Mat BasicHex(uint hex, bool tone) => new() { Kind = MatKind.Basic, Color = Rgb.Hex(hex), Tone = tone, Opacity = 1, DepthWrite = true, Tex = -1 };

    public static Mat StdMat(Rgb c, double rough, double metal) => new() { Kind = MatKind.Std, Color = c, Rough = rough, Metal = metal, Opacity = 1, DepthWrite = true, Tex = -1 };

    public static Mat GlowMat(Rgb c, double opacity) => new() { Kind = MatKind.Glow, Color = c, Opacity = opacity, Tex = -1 };

    public readonly bool Transparent => Kind switch
    {
        MatKind.Basic => Opacity < 1 || Blend == Blend.Additive,
        MatKind.Glow or MatKind.Points => true,
        _ => false,
    };
}

/// <summary>A voxel box of a merged mesh: its low corner, size and (linear) colour.</summary>
public readonly record struct VBox(double X, double Y, double Z, double W, double H, double D, Rgb C);

public enum GeoKind : byte
{
    /// <summary>BoxGeometry(1, 1, 1).</summary>
    Unit,
    /// <summary>Vox.mesh(): boxes merged, a colour per box (Index into Graph.Merged).</summary>
    Merged,
    Cylinder,
    Ring,
    Plane,
    /// <summary>Four points, uv (0,1)(1,1)(1,0)(0,0), indices 0 3 1 · 1 3 2.</summary>
    Quad,
    Sprite,
    Points,
}

public sealed class Geo
{
    public GeoKind Kind;
    /// <summary>A merged mesh's place in <see cref="Graph.Merged"/>.</summary>
    public int Index;
    /// <summary>Cylinder: Top, Bottom radii and H; Ring: Inner, Outer; Plane: W, H.</summary>
    public double Top, Bottom, H, Inner, Outer, W;
    public int Seg;
    public V3[] Quad = [];
    public V3[] Pts = [];
}

public sealed class Drawing(Geo geo, Mat mat)
{
    public Geo Geo = geo;
    public Mat Mat = mat;
}

public enum HitKind : byte { Bot = 1, Prop, Desk }

/// <summary>Marks a node that is in the raycast (hitMat: never drawn).</summary>
public sealed record Hit(HitKind Kind, int I);

public sealed class Node
{
    /// <summary>-1 for the root.</summary>
    public int Parent;
    public V3 P, R, S = new(1, 1, 1);
    public bool Visible = true;
    public Drawing? Draw;
    public bool Cast, Receive;
    public Hit? Hit;
}

public sealed class Graph
{
    public const int Root = 0;

    public readonly List<Node> Nodes = [];
    public readonly List<VBox[]> Merged = [];

    public Graph() => Nodes.Add(new Node { Parent = -1 });

    public int Add(int parent, V3 p)
    {
        Nodes.Add(new Node { Parent = parent, P = p });
        return Nodes.Count - 1;
    }

    /// <summary>`pivot(parent, x, y, z)`.</summary>
    public int Pivot(int parent, double x, double y, double z) => Add(parent, new V3(x, y, z));

    /// <summary>`box(parent, w, h, d, x, y, z, mat, shadow)`: the unit box scaled, centred at x, y, z.</summary>
    public int Boxm(int parent, double w, double h, double d, double x, double y, double z, Mat mat, bool shadow)
    {
        var n = Add(parent, new V3(x, y, z));
        var nd = Nodes[n];
        nd.S = new V3(w, h, d);
        nd.Draw = new Drawing(new Geo { Kind = GeoKind.Unit }, mat);
        nd.Cast = shadow;
        nd.Receive = true;
        return n;
    }

    public int Drawn(int parent, Geo geo, Mat mat)
    {
        var n = Add(parent, default);
        Nodes[n].Draw = new Drawing(geo, mat);
        return n;
    }

    public M4 Local(int i)
    {
        var n = Nodes[i];
        return M4.TRS(n.P, n.R, n.S);
    }

    /// <summary>matrixWorld of one node (getWorldPosition's matrix), without working out the rest.</summary>
    public M4 WorldAt(int i)
    {
        var m = Local(i);
        for (var p = Nodes[i].Parent; p >= 0; p = Nodes[p].Parent) m = Local(p) * m;
        return m;
    }

    /// <summary>matrixWorld of every node, parents before children (they are added so).</summary>
    public M4[] World()
    {
        var w = new M4[Nodes.Count];
        for (var i = 0; i < Nodes.Count; i++)
        {
            var l = Local(i);
            var p = Nodes[i].Parent;
            w[i] = p >= 0 ? w[p] * l : l;
        }
        return w;
    }

    /// <summary>Which nodes are drawn: visible itself and all the way up.</summary>
    public bool[] Shown()
    {
        var v = new bool[Nodes.Count];
        for (var i = 0; i < Nodes.Count; i++)
        {
            var n = Nodes[i];
            v[i] = n.Visible && (n.Parent < 0 || v[n.Parent]);
        }
        return v;
    }

    /// <summary>The material of a drawing node.</summary>
    public ref Mat MatMut(int i) => ref Nodes[i].Draw!.Mat;
}

/// <summary>`class Vox`: boxes gathered into one mesh with a colour per box.</summary>
public sealed class Vox(Rng r)
{
    public readonly List<VBox> B = [];

    /// <summary>`box(x, y, z, w, h, d, c, j = 0.04)`: the colour jittered by ±j (the generator is drawn only when j).</summary>
    public Vox Bx(double x, double y, double z, double w, double h, double d, uint c, double j)
    {
        var col = Rgb.Hex(c);
        if (j != 0) col = col.Mul(1 + (r.Next() - 0.5) * j * 2);
        B.Add(new VBox(x, y, z, w, h, d, col));
        return this;
    }

    /// <summary>Bx with the default jitter.</summary>
    public Vox Bd(double x, double y, double z, double w, double h, double d, uint c) => Bx(x, y, z, w, h, d, c, 0.04);
}

/// <summary>A lamp's place and how lit it is.</summary>
public readonly record struct Lamp(V3 P, double K);

/// <summary>The room's handles: what main.js keeps in variables to change later.</summary>
public sealed class Room
{
    public int Door, ExitGlow;
    public int Sky, TV, Board, Clock;
    public int TVGlow, ClockGlow, CoffeeLED;
    public int[] Steam = new int[3];
    public int[] Shades = [];
    public int FloorShade;
    public Lamp[] Lamps = [];
    public int[] DeskGlows = [];
    public int Vac;
    public int Patch, Beam, Dust;
}

/// <summary>The room: its size, the desks and the door.</summary>
public static class Layout
{
    public const double RW = 14.0, RD = 11.0, WH = 4.0, X0 = -7.0, Z0 = -5.5;

    public static readonly (double X, double Z) Door = (-5.65, Z0 + 0.35);

    /// <summary>Desks are two rows of three; a bot sits on the -x side facing +x.</summary>
    public static readonly (double X, double Z)[] Desks = [(-3.2, -1.6), (0.6, -1.6), (4.4, -1.6), (-3.2, 2.1), (0.6, 2.1), (4.4, 2.1)];

    public static double Seat(int d) => Desks[d].X - 0.67;

    // Canvas texture ids, in the order the renderer makes them.
    public const int TexSky = 0, TexTV = 1, TexBoard = 2, TexClock = 3, TexBeam = 4, TexPatch = 5;
}

public static class RoomBuilder
{
    static int Glow(Graph g, int parent, uint hex, double size, double opacity, V3 p)
    {
        var n = g.Drawn(parent, new Geo { Kind = GeoKind.Sprite }, Mat.GlowMat(Rgb.Hex(hex), opacity));
        g.Nodes[n].S = new V3(size, size, size);
        g.Nodes[n].P = p;
        return n;
    }

    static int Screen(Graph g, int tex, double w, double h, V3 p, double ry)
    {
        var m = new Mat { Kind = MatKind.Basic, Color = new Rgb(1, 1, 1), Opacity = 1, DepthWrite = true, Tex = tex };
        var n = g.Drawn(Graph.Root, new Geo { Kind = GeoKind.Plane, W = w, H = h }, m);
        g.Nodes[n].P = p;
        g.Nodes[n].R = new V3(0, ry, 0);
        return n;
    }

    static int Quad(Graph g, V3[] pts, int tex)
    {
        var m = new Mat { Kind = MatKind.Basic, Color = new Rgb(1, 1, 1), Opacity = 1, Blend = Blend.Additive, Tex = tex, Double = true };
        return g.Drawn(Graph.Root, new Geo { Kind = GeoKind.Quad, Quad = pts }, m);
    }

    static void Plant(Vox v, double x, double z, double s, double y, int seed)
    {
        var r = new Rng(seed);
        v.Bd(x - 0.2 * s, y, z - 0.2 * s, 0.4 * s, 0.34 * s, 0.4 * s, 0xa4552e).Bd(x - 0.23 * s, y + 0.3 * s, z - 0.23 * s, 0.46 * s, 0.07 * s, 0.46 * s, 0xb8653a).
            Bd(x - 0.03 * s, y + 0.34 * s, z - 0.03 * s, 0.06 * s, 0.55 * s, 0.06 * s, 0x4a3a22);
        uint[] leaves = [0x4f7a3a, 0x3e6630, 0x6a9a45, 0x5a8a3a];
        for (var i = 0; i < 11; i++)
        {
            var a = r.Next() * Js.Tau;
            var rr = r.Next() * 0.3 * s;
            var h = (0.45 + r.Next() * 0.6) * s;
            var w = (0.14 + r.Next() * 0.16) * s;
            v.Bx(x + Math.Cos(a) * rr - w / 2, y + h, z + Math.Sin(a) * rr - w / 2, w, w * 0.7, w, leaves[i % 4], 0.08);
        }
    }

    /// <summary>Builds the room, as main.js does from "The room" to the dust.</summary>
    public static Room Build(Graph g, Rng r)
    {
        const double RW = Layout.RW, RD = Layout.RD, WH = Layout.WH, X0 = Layout.X0, Z0 = Layout.Z0;
        var Door = Layout.Door;
        var Desks = Layout.Desks;

        // Two Voxes share the generator; the page interleaves their calls, so this does too.
        Vox walls = new(r), room = new(r);
        room.Bx(X0 - 0.3, -0.7, Z0 - 0.3, RW + 0.3, 0.6, RD + 0.3, 0x1c1215, 0);
        for (var i = 0; i < (int)RW; i++)
            for (var k = 0; k < (int)RD; k++)
            {
                uint c = 0x48302b;
                if ((i + k) % 2 != 0) c = 0x5a3c34;
                room.Bx(X0 + i, -0.1, Z0 + k, 1, 0.1, 1, c, 0.05);
            }
        room.Bx(X0 - 0.3, -0.7, Z0 - 0.3 + RD + 0.3 - 0.02, RW + 0.3, 0.6, 0.02, 0x140c0f, 0);
        for (double x = 0; x < RW; x += 0.5)
        {
            uint c = 0x684240;
            if ((x * 2) % 2 != 0) c = 0x6e4643;
            walls.Bx(X0 + x, -0.7, Z0 - 0.3, 0.5, WH + 0.7, 0.3, c, 0.03);
        }
        for (double z = 0; z < RD; z += 0.5)
        {
            uint c = 0x5e3a38;
            if ((z * 2) % 2 != 0) c = 0x633e3c;
            walls.Bx(X0 - 0.3, -0.7, Z0 + z, 0.3, WH + 0.7, 0.5, c, 0.03);
        }
        walls.Bx(X0 - 0.3, -0.7, Z0 - 0.3, 0.3, WH + 0.7, 0.3, 0x5e3a38, 0);
        walls.Bx(X0, 0, Z0, RW, 1.15, 0.035, 0x4b2f2c, 0.02).Bx(X0, 0, Z0, 0.035, 1.15, RD, 0x462b29, 0.02);
        walls.Bx(X0, 1.15, Z0, RW, 0.07, 0.06, 0x80564d, 0).Bx(X0, 1.15, Z0, 0.06, 0.07, RD, 0x7a524a, 0);
        walls.Bx(X0, 0, Z0, RW, 0.16, 0.07, 0x33201d, 0).Bx(X0, 0, Z0, 0.07, 0.16, RD, 0x301e1b, 0);
        walls.Bx(X0 - 0.3, WH, Z0 - 0.3, RW + 0.3, 0.1, 0.3, 0x8d5f57, 0).Bx(X0 - 0.3, WH, Z0 - 0.3, 0.3, 0.1, RD + 0.3, 0x86594f, 0);
        walls.Bx(X0 - 0.3, -0.7, Z0 + RD - 0.02, 0.3, WH + 0.8, 0.02, 0x3a2422, 0).Bx(X0 + RW - 0.02, -0.7, Z0 - 0.3, 0.02, WH + 0.8, 0.3, 0x3a2422, 0);

        // Door on the back wall; the panel swings.
        walls.Bx(-6.3, 0, Z0, 1.3, 2.42, 0.08, 0x2c1b17, 0).Bx(-6.2, 0, Z0 + 0.01, 1.1, 2.3, 0.08, 0x0b0708, 0);
        Vox doorV = new(r);
        doorV.Bx(0, 0, 0, 1.1, 2.3, 0.07, 0x5c3b2b, 0.02).Bx(0.14, 1.28, 0.07, 0.82, 0.82, 0.02, 0x6b4633, 0).Bx(0.14, 0.24, 0.07, 0.82, 0.86, 0.02, 0x6b4633, 0).Bx(0.9, 1.05, 0.07, 0.08, 0.08, 0.06, 0xe0ab4c, 0);
        var door = g.Pivot(Graph.Root, -6.2, 0, Z0 + 0.02);
        g.Merged.Add([.. doorV.B]);
        var dm = g.Drawn(door, new Geo { Kind = GeoKind.Merged, Index = g.Merged.Count - 1 },
            new Mat { Kind = MatKind.Std, Color = new Rgb(1, 1, 1), Rough = 0.88, Vertex = true, Opacity = 1, DepthWrite = true, Tex = -1 });
        g.Nodes[dm].Cast = true;
        g.Nodes[dm].Receive = true;
        room.Bx(-6.35, 0, Z0 + 0.12, 1.4, 0.02, 0.75, 0x6f3b2a, 0.02).Bx(-6.2, 0.02, Z0 + 0.22, 1.1, 0.005, 0.55, 0x8a4c34, 0);
        g.Boxm(Graph.Root, 0.34, 0.12, 0.08, Door.X, 2.62, Z0 + 0.05, Mat.BasicHex(0xffb35c, false), false);
        var exitGlow = Glow(g, Graph.Root, 0xffa24a, 1.4, 0.55, new V3(Door.X, 2.62, Z0 + 0.2));

        // Window with curtains; the sky behind it.
        walls.Bx(0.1, 3.25, Z0, 2.8, 0.12, 0.12, 0x3a2620, 0).Bx(-0.05, 1.22, Z0, 3.1, 0.12, 0.26, 0x4a3026, 0).
            Bx(0.1, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).Bx(2.78, 1.34, Z0, 0.12, 1.92, 0.12, 0x3a2620, 0).
            Bx(1.46, 1.34, Z0, 0.08, 1.92, 0.1, 0x3a2620, 0).Bx(0.2, 2.26, Z0, 2.6, 0.07, 0.1, 0x3a2620, 0).
            Bx(-0.5, 3.52, Z0 + 0.1, 4.0, 0.05, 0.05, 0x241612, 0);
        foreach (var cx in new[] { -0.45, 2.9 })
            for (var i = 0; i < 4; i++)
            {
                uint c = 0x8a4629;
                if (i % 2 != 0) c = 0x9b5230;
                walls.Bx(cx + i * 0.13, 1.0, Z0 + 0.06 + (i % 2) * 0.05, 0.14, 2.52, 0.1, c, 0);
            }
        var sky = Screen(g, Layout.TexSky, 2.56, 1.9, new V3(1.5, 2.3, Z0 + 0.012), 0);

        // Wall TV and the cabinet under it.
        walls.Bx(4.1, 1.62, Z0, 2.6, 1.52, 0.1, 0x0c0b10, 0);
        var tv = Screen(g, Layout.TexTV, 2.44, 1.38, new V3(5.4, 2.38, Z0 + 0.105), 0);
        var tvGlow = Glow(g, Graph.Root, 0x5aa8ff, 3.6, 0.22, new V3(5.4, 2.3, Z0 + 0.6));
        room.Bd(4.3, 0, Z0 + 0.02, 2.2, 0.55, 0.5, 0x4c3028).Bx(4.25, 0.55, Z0 + 0.02, 2.3, 0.05, 0.54, 0x5e3c30, 0).
            Bx(4.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).Bx(5.45, 0.12, Z0 + 0.52, 0.9, 0.34, 0.01, 0x3c261f, 0).
            Bd(4.45, 0.6, Z0 + 0.12, 0.26, 0.42, 0.26, 0x22212a).Bd(6.1, 0.6, Z0 + 0.1, 0.24, 0.1, 0.3, 0xc8a24a).Bd(6.12, 0.7, Z0 + 0.1, 0.2, 0.08, 0.3, 0x5a7aa0);

        // Coffee counter.
        room.Bd(-4.6, 0, Z0 + 0.02, 1.9, 0.86, 0.62, 0x5a3a2c).Bx(-4.65, 0.86, Z0 + 0.02, 2.0, 0.06, 0.66, 0xd8cdbf, 0.02).
            Bx(-4.5, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).Bx(-3.6, 0.12, Z0 + 0.64, 0.8, 0.62, 0.01, 0x4a2f24, 0).
            Bd(-4.45, 0.92, Z0 + 0.1, 0.42, 0.56, 0.4, 0x26252d).Bd(-4.4, 1.2, Z0 + 0.5, 0.32, 0.18, 0.02, 0x121118).
            Bd(-3.8, 0.92, Z0 + 0.2, 0.12, 0.14, 0.12, 0xeeeeee).Bd(-3.6, 0.92, Z0 + 0.25, 0.12, 0.14, 0.12, 0x9b6bff).
            Bd(-3.3, 0.92, Z0 + 0.12, 0.34, 0.4, 0.3, 0x7a8a95).Bx(-4.6, 1.85, Z0, 1.9, 0.05, 0.3, 0x4a2f24, 0);
        uint[] jars = [0xc8a24a, 0x9a4f2e, 0x7a9a6a, 0xdcd2c4, 0xb46a3a];
        for (var i = 0; i < 5; i++)
            room.Bd(-4.5 + i * 0.36, 1.9, Z0 + 0.06, 0.2, 0.22 + (i % 2) * 0.08, 0.18, jars[i]);
        var coffeeLed = Glow(g, Graph.Root, 0x7ee0ff, 0.35, 0.9, new V3(-4.24, 1.29, Z0 + 0.53));
        var steam = new int[3];
        for (var i = 0; i < steam.Length; i++) steam[i] = Glow(g, Graph.Root, 0xffffff, 0.2, 0.25, default);

        // Left wall: bookcase, board, clock, painting.
        room.Bd(X0, 0, -4.3, 0.46, 2.42, 0.06, 0x4a2e24).Bd(X0, 0, -2.76, 0.46, 2.42, 0.06, 0x4a2e24).Bx(X0, 0, -4.3, 0.05, 2.42, 1.6, 0x3a241c, 0);
        uint[] books = [0x8a3b2e, 0xc9a24a, 0x4a6b4a, 0x7b5aa6, 0xd07a3a, 0x3a5a8a, 0xb8b0a0, 0x9a4a5a];
        foreach (var y in new[] { 0, 0.6, 1.2, 1.8, 2.36 })
        {
            room.Bx(X0, y, -4.26, 0.46, 0.06, 1.52, 0x55352a, 0.02);
            if (y > 2.0) continue;
            var z = -4.2;
            while (z < -2.9)
            {
                var w = 0.06 + r.Next() * 0.07;
                var h = 0.26 + r.Next() * 0.2;
                if (z + w > -2.82) break;
                var depth = 0.32 + r.Next() * 0.06;
                var book = books[(int)(r.Next() * books.Length)];
                room.Bx(X0 + 0.06, y + 0.06, z, depth, Math.Min(h, 0.5), w, book, 0.06);
                var gap = 0.006;
                if (r.Next() < 0.12) gap = 0.08;
                z += w + gap;
            }
        }
        walls.Bx(X0, 1.46, -2.2, 0.07, 1.74, 3.0, 0x3a2620, 0);
        var board = Screen(g, Layout.TexBoard, 2.84, 1.6, new V3(X0 + 0.075, 2.33, -0.7), Math.PI / 2);
        walls.Bx(X0, 2.42, 1.22, 0.09, 0.6, 1.16, 0x0f0d12, 0);
        var clock = Screen(g, Layout.TexClock, 1.04, 0.48, new V3(X0 + 0.095, 2.72, 1.8), Math.PI / 2);
        var clockGlow = Glow(g, Graph.Root, 0xff7a2a, 1.6, 0.35, new V3(X0 + 0.3, 2.72, 1.8));
        walls.Bx(X0, 1.6, 3.55, 0.06, 1.02, 1.42, 0x2e1c18, 0).Bx(X0 + 0.06, 1.68, 3.63, 0.01, 0.86, 1.26, 0x41628f, 0).
            Bx(X0 + 0.07, 1.68, 3.63, 0.01, 0.3, 1.26, 0x3f6a44, 0).Bx(X0 + 0.075, 1.9, 3.75, 0.01, 0.3, 0.5, 0x5a7a5a, 0).
            Bx(X0 + 0.075, 1.95, 4.2, 0.01, 0.42, 0.55, 0x6a8a6a, 0).Bx(X0 + 0.08, 2.24, 4.45, 0.01, 0.13, 0.13, 0xffd070, 0).
            Bx(X0 + 0.075, 2.28, 4.3, 0.01, 0.09, 0.3, 0xe8f0ff, 0);

        // Lounge.
        room.Bx(-6.8, 0, 2.95, 2.8, 0.02, 2.5, 0x6f4430, 0.02).Bx(-6.5, 0.02, 3.25, 2.2, 0.01, 1.9, 0x8a5a3c, 0.02);
        room.Bd(X0 + 0.05, 0, 3.2, 0.95, 0.42, 2.1, 0x5b3a6a).Bd(X0 + 0.05, 0.42, 3.2, 0.26, 0.55, 2.1, 0x4f3160).
            Bd(X0 + 0.05, 0.42, 3.02, 0.95, 0.24, 0.2, 0x553565).Bd(X0 + 0.05, 0.42, 5.28, 0.95, 0.24, 0.2, 0x553565).
            Bd(X0 + 0.32, 0.42, 3.24, 0.66, 0.1, 0.98, 0x6b4a7a).Bd(X0 + 0.32, 0.42, 4.28, 0.66, 0.1, 0.98, 0x6b4a7a).
            Bd(X0 + 0.33, 0.52, 3.4, 0.14, 0.34, 0.42, 0xd9a64a).Bd(X0 + 0.33, 0.52, 4.8, 0.14, 0.3, 0.36, 0x5aa8a0);
        room.Bd(-5.55, 0.32, 3.7, 0.8, 0.06, 1.25, 0x6b4a36).Bd(-5.5, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).Bd(-4.85, 0, 3.75, 0.06, 0.32, 0.06, 0x3a261c).
            Bd(-5.5, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).Bd(-4.85, 0, 4.84, 0.06, 0.32, 0.06, 0x3a261c).
            Bd(-5.3, 0.38, 3.9, 0.3, 0.05, 0.4, 0x3a5a8a).Bd(-5.0, 0.38, 4.5, 0.12, 0.14, 0.12, 0xeeeeee);
        room.Bd(-6.75, 0, 2.55, 0.26, 0.04, 0.26, 0x241a16).Bd(-6.64, 0.04, 2.66, 0.04, 1.6, 0.04, 0x241a16);
        var floorShade = g.Boxm(Graph.Root, 0.44, 0.3, 0.44, -6.62, 1.78, 2.68, Mat.BasicHex(0xffc27a, false), false);

        // Beanbags.
        room.Bd(4.7, 0, 3.8, 0.9, 0.3, 0.9, 0x7a4a9a).Bd(4.8, 0.3, 3.9, 0.7, 0.14, 0.7, 0x8a5aaa).Bd(4.75, 0.3, 3.82, 0.2, 0.34, 0.84, 0x6a3a8a).
            Bd(5.9, 0, 4.4, 0.8, 0.28, 0.8, 0x2f8a7a).Bd(6.0, 0.28, 4.5, 0.6, 0.12, 0.6, 0x3a9a8a);

        Plant(room, 6.4, Z0 + 0.55, 1.6, 0.0, 3);
        Plant(room, -0.6, Z0 + 0.5, 1.2, 0.0, 5);
        Plant(room, 6.4, 5.0, 1.5, 0.0, 7);
        Plant(room, -3.3, 5.0, 1.0, 0.0, 9);
        Plant(room, -3.95, Z0 + 0.3, 0.55, 0.92, 4);

        // Desks.
        foreach (var z in new[] { -1.6, 2.1 })
        {
            uint a = 0x6e4a2a, b = 0x7e5634;
            if (z < 0) { a = 0x3f4a3a; b = 0x4a5846; }
            room.Bx(-4.9, 0, z - 1.15, 10.4, 0.015, 2.3, a, 0.02).Bx(-4.7, 0.015, z - 0.95, 10.0, 0.006, 1.9, b, 0.02);
        }
        var shades = new List<int>();
        var deskGlows = new List<int>();
        var lamps = new List<Lamp>();
        uint[] mugs = [0xeeeeee, 0x9b6bff, 0xff9a4a];
        for (var di = 0; di < Desks.Length; di++)
        {
            double x = Desks[di].X, z = Desks[di].Z;
            room.Bx(x - 0.42, 0.64, z - 0.78, 0.84, 0.07, 1.56, 0x6e4c37, 0.02);
            foreach (var (lx, lz) in new[] { (-0.38, -0.74), (0.3, -0.74), (-0.38, 0.68), (0.3, 0.68) })
                room.Bx(x + lx, 0, z + lz, 0.07, 0.64, 0.07, 0x3e2a1f, 0);
            room.Bd(x - 0.3, 0.12, z + 0.3, 0.66, 0.5, 0.42, 0x5e4030).Bx(x - 0.31, 0.38, z + 0.36, 0.01, 0.04, 0.3, 0xc8a24a, 0);
            room.Bd(x + 0.02, 0.71, z - 0.14, 0.24, 0.03, 0.28, 0x1c1c24).Bd(x + 0.1, 0.74, z - 0.04, 0.06, 0.2, 0.08, 0x1c1c24).
                Bd(x + 0.02, 0.88, z - 0.4, 0.09, 0.46, 0.8, 0x1a1d28).Bx(x + 0.11, 0.92, z - 0.12, 0.02, 0.26, 0.24, 0x2a2f3e, 0).
                Bd(x - 0.36, 0.71, z - 0.26, 0.17, 0.025, 0.52, 0x2c3040).Bx(x - 0.34, 0.735, z - 0.24, 0.13, 0.008, 0.48, 0x454a5e, 0).
                Bd(x - 0.34, 0.71, z + 0.36, 0.1, 0.03, 0.07, 0x2c3040).Bx(x - 0.1, 0.71, z + 0.3, 0.26, 0.04, 0.34, 0xece6da, 0.02).
                Bd(x + 0.12, 0.71, z + 0.55, 0.11, 0.13, 0.11, mugs[di % 3]);
            room.Bd(x + 0.14, 0.71, z - 0.65, 0.18, 0.03, 0.18, 0x2a2a30).Bd(x + 0.21, 0.74, z - 0.58, 0.04, 0.4, 0.04, 0x2a2a30);
            shades.Add(g.Boxm(Graph.Root, 0.26, 0.14, 0.26, x + 0.23, 1.16, z - 0.56, Mat.BasicHex(0xffc27a, false), false));
            lamps.Add(new Lamp(new V3(x - 0.1, 1.1, z - 0.3), 0));
            var s = x - 0.67;
            room.Bd(s - 0.24, 0.38, z - 0.24, 0.48, 0.07, 0.48, 0x3b2d4c).Bd(s - 0.29, 0.45, z - 0.22, 0.07, 0.54, 0.44, 0x33263f).
                Bd(s - 0.03, 0.08, z - 0.03, 0.06, 0.3, 0.06, 0x1c1c22).Bd(s - 0.22, 0.04, z - 0.03, 0.44, 0.04, 0.06, 0x1c1c22).Bd(s - 0.03, 0.04, z - 0.22, 0.06, 0.04, 0.44, 0x1c1c22);
            deskGlows.Add(Glow(g, Graph.Root, 0x7fb8ff, 1.3, 0.0, new V3(x - 0.22, 1.02, z)));
        }

        // scene.add(walls.mesh(false), room.mesh(true)).
        Mat Vert() => new() { Kind = MatKind.Std, Color = new Rgb(1, 1, 1), Rough = 0.88, Vertex = true, Opacity = 1, DepthWrite = true, Tex = -1 };
        g.Merged.Add([.. walls.B]);
        var wm = g.Drawn(Graph.Root, new Geo { Kind = GeoKind.Merged, Index = g.Merged.Count - 1 }, Vert());
        g.Nodes[wm].Receive = true;
        g.Merged.Add([.. room.B]);
        var rm = g.Drawn(Graph.Root, new Geo { Kind = GeoKind.Merged, Index = g.Merged.Count - 1 }, Vert());
        g.Nodes[rm].Receive = true;
        g.Nodes[rm].Cast = true;

        // The vacuum.
        var vac = g.Add(Graph.Root, default);
        var va = g.Drawn(vac, new Geo { Kind = GeoKind.Cylinder, Top = 0.27, Bottom = 0.28, H = 0.08, Seg = 24 }, Mat.StdMat(Rgb.Hex(0x2a2a33), 0.5, 0));
        g.Nodes[va].P = g.Nodes[va].P with { Y = 0.05 };
        g.Nodes[va].Cast = true;
        g.Nodes[va].Receive = true;
        var vb = g.Drawn(vac, new Geo { Kind = GeoKind.Cylinder, Top = 0.17, Bottom = 0.17, H = 0.02, Seg = 20 }, Mat.StdMat(Rgb.Hex(0x4a4a58), 0.4, 0));
        g.Nodes[vb].P = g.Nodes[vb].P with { Y = 0.1 };
        g.Nodes[vb].Receive = true;
        Glow(g, vac, 0x4ade80, 0.22, 0.9, new V3(0, 0.13, 0.2));

        // Light through the window.
        var patch = Quad(g, [new V3(0.3, 0.02, Z0 + 1.2), new V3(2.9, 0.02, Z0 + 1.2), new V3(3.9, 0.02, Z0 + 3.7), new V3(1.3, 0.02, Z0 + 3.7)], Layout.TexPatch);
        var beam = Quad(g, [new V3(0.2, 3.25, Z0 + 0.02), new V3(2.8, 3.25, Z0 + 0.02), new V3(3.9, 0.02, Z0 + 3.7), new V3(1.3, 0.02, Z0 + 3.7)], Layout.TexBeam);
        var dr = new Rng(4);
        var pts = new V3[46];
        for (var i = 0; i < pts.Length; i++)
        {
            var f = dr.Next();
            var x = 0.4 + dr.Next() * 2.4 + f * 1.1;
            var y = 3.1 * (1 - f) + dr.Next() * 0.3;
            var z = Z0 + 0.3 + f * 3.2;
            pts[i] = new V3(x, y, z);
        }
        var dust = g.Drawn(Graph.Root, new Geo { Kind = GeoKind.Points, Pts = pts }, new Mat { Kind = MatKind.Points, Color = Rgb.Hex(0xffe2a8), Opacity = 0.8, Tex = -1 });

        return new Room
        {
            Door = door, ExitGlow = exitGlow, Sky = sky, TV = tv, Board = board, Clock = clock, TVGlow = tvGlow, ClockGlow = clockGlow, CoffeeLED = coffeeLed,
            Steam = steam, Shades = [.. shades], FloorShade = floorShade, Lamps = [.. lamps], DeskGlows = [.. deskGlows], Vac = vac, Patch = patch, Beam = beam, Dust = dust,
        };
    }
}
