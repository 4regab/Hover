namespace Hover.Office;

// main.js's "Helpers: a session's subagents". While a session has subagents out, each is a small
// bot around its desk doing paperwork: writing on a clipboard, stamping it, turning the page and
// handing a sheet in to the desk's tray. They hop out of the session's bot and back into it when
// done. Each is the session bot's colour turned round the hue wheel, with its own eye colour and
// a cap, so they read as its team and not as sessions of their own (they can't be clicked; the
// desk and the bot still can). A port of internal/office/mini.go.

public enum Duty : byte { Write, Stamp, Flip, File }

/// <summary>A sheet a helper hands in: where from, and to the desk's tray.</summary>
public readonly record struct Toss(V3 From, V3 To);

public sealed class Mini
{
    /// <summary>The helper's scale (MINI).</summary>
    public const double Scale = 0.46;

    static readonly double[] Hue = [0.5, 0.17, -0.17, 0.33];
    static readonly uint[] EyeColors = [0xffe08a, 0xaaf6ff, 0xc8ffb0, 0xffc8ea];

    /// <summary>Where they stand, from the desk's centre: on the camera's sides of it, clear of the chair and of the walk between the rows.</summary>
    public static readonly (double X, double Z)[] Spots = [(0.72, -0.36), (0.72, 0.42), (0.06, 1.08), (-0.62, 1.02)];

    /// <summary>What a helper does, in turn, and for how long (s). Each starts at a different point.</summary>
    static readonly (Duty D, double S)[] Duties = [(Duty.Write, 2.6), (Duty.Stamp, 1.6), (Duty.Write, 2.2), (Duty.Flip, 0.9), (Duty.Write, 1.8), (Duty.File, 1.5)];

    static double Smooth(double f) => f * f * (3 - 2 * f);

    /// <summary>The three body colours of the helper in a slot for a session bot of colour <paramref name="bot"/>: the bot's hue turned, a little lighter.</summary>
    public static Rgb[] Palette(Rgb bot, int slot)
    {
        var (h, s, l) = bot.Hsl();
        var main = Rgb.FromHsl((h + Hue[slot] + 1) % 1, Math.Min(s * 0.9, 1), Math.Min(l + 0.06, 0.7));
        return [main, main.Mul(0.5), main.Lerp(new Rgb(1, 1, 1), 0.45)];
    }

    /// <summary>Where the helper in a slot stands at a desk.</summary>
    public static (double X, double Z) Spot(int desk, int slot)
    {
        var d = Layout.Desks[desk];
        return (d.X + Spots[slot].X, d.Z + Spots[slot].Z);
    }

    enum Tint : byte { Main, Dark, Pale, Eye }

    readonly record struct MiniTint(int Node, Tint T);

    struct MiniPose { public double Lean, Hx, Hy, Al, Ar, Sl, Sr, Leg; }

    /// <summary>The session it works for.</summary>
    public int Sid;
    public int Desk, Slot;
    double t;
    /// <summary>0 in the bot, 1 at its post: it hops between.</summary>
    public double Out;
    public bool Leaving, Gone;
    public int DutyIndex;
    double dutyT;
    bool tossed;
    MiniPose p;
    public double Yaw;
    /// <summary>Where its bot was last seen, for the hop back when the bot is gone.</summary>
    (double X, double Z) botAt;
    public readonly int Root;
    readonly int[] legs;
    readonly int upper, head;
    readonly int[] arms;
    readonly int pencil, stamp, board, page, mark;
    readonly List<MiniTint> tints;
    /// <summary>The main colour, as the office's tags and the desk card show it.</summary>
    public Rgb Color;

    /// <summary>`new Mini(s, slot)`: the helper's nodes, hidden until it hops out of its bot.</summary>
    public Mini(Graph g, Rng r, int sid, int desk, int slot, Rgb bot)
    {
        var pal = Palette(bot, slot);
        Mat Sd(Rgb c, double rough) => Mat.StdMat(c, rough, 0);
        Mat main = Sd(pal[0], 0.5), dark = Sd(pal[1], 0.6), pale = Sd(pal[2], 0.45);
        var eye = Mat.BasicHex(EyeColors[slot], false);
        var visor = Mat.StdMat(Rgb.Hex(0x111018), 0.22, 0.35);
        Mat Paper() => Mat.StdMat(Rgb.Hex(0xf4efe4), 0.9, 0);
        Mat Ink() => Mat.BasicHex(0xd8443a, true);
        var tl = new List<MiniTint>();
        // A box that takes the body's colour: remembered, to be coloured again when the helper is used for another session.
        int Bx(int pr, double w, double h, double d, double x, double y, double z, Tint tint, Mat m, bool s)
        {
            var n = g.Boxm(pr, w, h, d, x, y, z, m, s);
            tl.Add(new MiniTint(n, tint));
            return n;
        }
        Root = g.Add(Graph.Root, default);
        g.Nodes[Root].S = new V3(0.001, 0.001, 0.001);
        g.Nodes[Root].Visible = false;
        var hips = g.Pivot(Root, 0, 0.24, 0);
        legs = new int[2];
        double[] ks = [-1, 1];
        for (var i = 0; i < 2; i++)
        {
            var pv = g.Pivot(hips, ks[i] * 0.1, 0, 0);
            Bx(pv, 0.13, 0.2, 0.15, 0, -0.1, 0, Tint.Dark, dark, true);
            Bx(pv, 0.15, 0.06, 0.21, 0, -0.21, 0.03, Tint.Pale, pale, true);
            legs[i] = pv;
        }
        upper = g.Pivot(hips, 0, 0, 0);
        Bx(upper, 0.42, 0.3, 0.3, 0, 0.15, 0, Tint.Dark, dark, true);
        Bx(upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, Tint.Pale, pale, true);
        head = g.Pivot(upper, 0, 0.3, 0);
        Bx(head, 0.58, 0.44, 0.48, 0, 0.22, 0, Tint.Main, main, true);
        g.Boxm(head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, visor, false);
        foreach (var k in ks)
        {
            Bx(head, 0.07, 0.2, 0.22, k * 0.315, 0.22, 0, Tint.Dark, dark, true);
            Bx(head, 0.075, 0.11, 0.01, k * 0.1, 0.21, 0.258, Tint.Eye, eye, false);
        }
        // The cap and its brim.
        Bx(head, 0.62, 0.07, 0.52, 0, 0.47, 0, Tint.Pale, pale, true);
        Bx(head, 0.46, 0.03, 0.16, 0, 0.455, 0.32, Tint.Pale, pale, true);
        Bx(head, 0.03, 0.12, 0.03, -0.14, 0.56, -0.08, Tint.Dark, dark, true);
        Bx(head, 0.08, 0.08, 0.08, -0.14, 0.65, -0.08, Tint.Eye, eye, false);
        arms = new int[2];
        for (var i = 0; i < 2; i++)
        {
            var pv = g.Pivot(upper, ks[i] * 0.27, 0.27, 0);
            Bx(pv, 0.1, 0.22, 0.12, 0, -0.1, 0, Tint.Main, main, true);
            Bx(pv, 0.11, 0.07, 0.13, 0, -0.23, 0, Tint.Pale, pale, true);
            arms[i] = pv;
        }
        pencil = g.Boxm(arms[1], 0.035, 0.035, 0.18, 0, -0.26, 0.07, Mat.StdMat(Rgb.Hex(0xf2c14a), 0.6, 0), true);
        stamp = g.Pivot(arms[1], 0, -0.29, 0.02);
        Bx(stamp, 0.05, 0.1, 0.05, 0, 0, 0, Tint.Dark, dark, true);
        g.Boxm(stamp, 0.12, 0.04, 0.12, 0, -0.06, 0, Ink(), true);
        // The clipboard is held against the chest, tilted up towards the face. Its top sheet takes
        // the stamp's mark and turns over the clip, leaving a clean one.
        board = g.Pivot(upper, 0, 0.13, 0.27);
        g.Nodes[board].R = new V3(-0.55, 0, 0);
        g.Boxm(board, 0.34, 0.02, 0.42, 0, 0, 0, Mat.StdMat(Rgb.Hex(0x8a5a34), 0.8, 0), true);
        g.Boxm(board, 0.3, 0.012, 0.36, 0, 0.016, -0.01, Paper(), true);
        g.Boxm(board, 0.13, 0.035, 0.05, 0, 0.026, 0.19, Mat.StdMat(Rgb.Hex(0x9aa0aa), 0.35, 0.6), true);
        page = g.Pivot(board, 0, 0.025, 0.17);
        g.Boxm(page, 0.3, 0.006, 0.36, 0, 0, -0.18, Paper(), true);
        mark = g.Boxm(page, 0.09, 0.004, 0.07, 0.05, 0.005, -0.24, Ink(), false);
        g.Nodes[mark].Visible = false;
        tints = tl;
        Color = pal[0];
        Renew(g, r, sid, desk, slot, bot);
    }

    /// <summary>
    /// Makes a helper that was done work for another: its nodes are coloured again and it starts over
    /// inside its bot, so the scene doesn't grow with each subagent the office sees. The random draw
    /// is the one the constructor makes.
    /// </summary>
    public void Renew(Graph g, Rng r, int sid, int desk, int slot, Rgb bot)
    {
        Sid = sid;
        Desk = desk;
        Slot = slot;
        t = r.Next() * 10;
        Out = 0;
        Leaving = Gone = false;
        DutyIndex = slot * 2 % Duties.Length;
        dutyT = 0;
        tossed = false;
        p = default;
        Yaw = 0;
        var pal = Palette(bot, slot);
        Color = pal[0];
        foreach (var tn in tints)
        {
            var c = tn.T switch { Tint.Main => pal[0], Tint.Dark => pal[1], Tint.Pale => pal[2], _ => Rgb.Hex(EyeColors[slot]) };
            ref var mt = ref g.MatMut(tn.Node);
            if (mt.Kind is MatKind.Std or MatKind.Basic) mt.Color = c;
        }
        g.Nodes[page].R = g.Nodes[page].R with { X = 0 };
        g.Nodes[mark].Visible = false;
        g.Nodes[Root].S = new V3(0.001, 0.001, 0.001);
        g.Nodes[Root].Visible = true;
    }

    /// <summary>Takes it out of the scene; the nodes stay, for <see cref="Renew"/>.</summary>
    public void Dispose(Graph g) => g.Nodes[Root].Visible = false;

    public (double X, double Z) SpotNow => Spot(Desk, Slot);

    public Duty DutyNow => Duties[DutyIndex].D;

    static void EaseTo(ref double v, double to, double k, double dt) => v = Js.Ease(v, to, k, dt);

    /// <summary>One step. <paramref name="bot"/> is where its session's bot sits, if the session is still there. It returns a sheet it hands in this step.</summary>
    public Toss? Step(Graph g, double dt, bool still, (double X, double Z)? bot)
    {
        if (bot is { } b) botAt = b;
        t += dt;
        var tt = t;
        if (still)
        {
            Out = 1;
            if (Leaving) Out = 0;
        }
        else
        {
            var dir = Leaving ? -1.0 : 1.0;
            Out = Math.Min(Math.Max(Out + dt / 0.6 * dir, 0), 1);
        }
        if (Leaving && Out <= 0)
        {
            Gone = true;
            return null;
        }
        var (px, pz) = SpotNow;
        var d = Layout.Desks[Desk];
        double bx = botAt.X + 0.32, bz = botAt.Z;
        var f = Smooth(Out);
        var hop = Out < 1;
        g.Nodes[Root].P = new V3(bx + (px - bx) * f, Math.Sin(Math.PI * Out) * 0.42, bz + (pz - bz) * f);
        var sc = Scale * Math.Min(Math.Max(Out * 1.8, 0.001), 1);
        g.Nodes[Root].S = new V3(sc, sc, sc);
        // At work it turns three-quarters to the room (the camera looks from +x, +z), so its
        // clipboard, pencil and stamp show rather than its back.
        var work = Math.PI / 4 - 0.55;
        if (Slot % 2 != 0) work = Math.PI / 4 + 0.55;
        double face;
        if (!hop) face = work;
        else if (Leaving) face = Math.Atan2(bx - px, bz - pz);
        else face = Math.Atan2(px - bx, pz - bz);
        if (Out < 0.05 && !Leaving) Yaw = face;
        else Yaw = Js.AngTo(Yaw, face, 10, dt);

        double lean = 0, hx = 0.28, hy = 0, al = -1.0, sl = 0.35, sr = -0.3;
        // Every branch below sets these two.
        double ar, leg;
        Toss? toss = null;
        var duty = Duty.Write;
        if (hop)
        {
            al = -2.7; ar = -2.7; sl = -0.3; sr = 0.3; hx = -0.15;
            leg = -0.6 * Math.Sin(Math.PI * Out);
        }
        else
        {
            dutyT += dt;
            var (n, length) = Duties[DutyIndex];
            if (dutyT >= length)
            {
                // A new page after the turn, and a fresh hand-in after a file.
                if (n == Duty.Flip)
                {
                    g.Nodes[page].R = g.Nodes[page].R with { X = 0 };
                    g.Nodes[mark].Visible = false;
                }
                DutyIndex = (DutyIndex + 1) % Duties.Length;
                dutyT = 0;
                tossed = false;
                n = Duties[DutyIndex].D;
            }
            length = Duties[DutyIndex].S;
            duty = n;
            var u = dutyT;
            var w = still ? 0.0 : 1.0;
            ar = 0;
            switch (n)
            {
                case Duty.Write:
                    ar = -1.15 + Math.Sin(tt * 16) * 0.07 * w;
                    sr = -0.32 + Math.Sin(tt * 6.5) * 0.09 * w;
                    hy = Math.Sin(tt * 0.8) * 0.08 * w;
                    break;
                case Duty.Stamp:
                    // Up, down hard, a beat on the paper; twice.
                    var c = (u / 0.8) % 1;
                    if (c < 0.55) ar = -1.1 - Smooth(c / 0.55) * 1.1;
                    else if (c < 0.68) ar = -2.2 + (c - 0.55) / 0.13 * 1.15;
                    else ar = -1.05;
                    sr = -0.34;
                    if (c >= 0.68 && c < 0.8) lean = 0.06;
                    hx = 0.34;
                    if (c >= 0.68) g.Nodes[mark].Visible = true;
                    break;
                case Duty.Flip:
                    ar = -1.7; sr = -0.4; hx = 0.18;
                    g.Nodes[page].R = g.Nodes[page].R with { X = Smooth(Math.Min(u / (length * 0.75), 1)) * Math.PI };
                    break;
                case Duty.File:
                    // Lifts the board, sends the top sheet to the desk's tray, and bows a little.
                    al = -1.45; ar = -1.6; hx = 0.05;
                    if (u > 0.5 && u < 1.1) lean = -0.08;
                    if (!tossed && u > 0.35)
                    {
                        tossed = true;
                        // The pose is set below; the board is where last step left it.
                        if (!still) toss = new Toss(g.WorldAt(board).Point(default), new V3(d.X + 0.03, 0.77, d.Z + 0.47));
                    }
                    break;
            }
            leg = Math.Max(Math.Sin(tt * 2.2 + Slot), 0) * 0.12 * w;
        }
        var k = still ? 30.0 : 14.0;
        EaseTo(ref p.Lean, lean, k, dt);
        EaseTo(ref p.Hx, hx, k, dt);
        EaseTo(ref p.Hy, hy, k, dt);
        EaseTo(ref p.Al, al, k, dt);
        EaseTo(ref p.Ar, ar, k, dt);
        EaseTo(ref p.Sl, sl, k, dt);
        EaseTo(ref p.Sr, sr, k, dt);
        EaseTo(ref p.Leg, leg, k, dt);
        g.Nodes[Root].R = g.Nodes[Root].R with { Y = Yaw };
        if (!hop && !still) g.Nodes[Root].P = g.Nodes[Root].P with { Y = g.Nodes[Root].P.Y + Math.Sin(tt * 3 + Slot) * 0.008 };
        g.Nodes[legs[0]].R = g.Nodes[legs[0]].R with { X = p.Leg };
        g.Nodes[legs[1]].R = g.Nodes[legs[1]].R with { X = hop ? p.Leg : -p.Leg * 0.3 };
        g.Nodes[upper].R = g.Nodes[upper].R with { X = p.Lean };
        g.Nodes[head].R = new V3(p.Hx, p.Hy, 0);
        g.Nodes[arms[0]].R = new V3(p.Al, 0, p.Sl);
        g.Nodes[arms[1]].R = new V3(p.Ar, 0, p.Sr);
        // The clipboard comes out once it has landed, and goes away before it hops back.
        g.Nodes[board].Visible = !hop;
        g.Nodes[pencil].Visible = !hop && duty != Duty.Stamp;
        g.Nodes[stamp].Visible = !hop && duty == Duty.Stamp;
        var blink = tt % 4.1 < 0.12 ? 0.3 : 1.0;
        foreach (var tn in tints)
        {
            ref var mt = ref g.MatMut(tn.Node);
            if (tn.T == Tint.Eye && mt.Kind == MatKind.Basic) mt.Color = Rgb.Hex(EyeColors[Slot]).Mul(blink);
        }
        return toss;
    }
}

/// <summary>
/// Every helper of the office, and the sheets they hand in. Helpers and sheets that are done go to
/// a spare list and are used again, so the scene stops growing once the busiest moment has had its nodes.
/// </summary>
public sealed class Crew
{
    sealed class Sheet(int node, V3 from, V3 to)
    {
        public int Node = node;
        public V3 From = from, To = to;
        public double T;
    }

    public readonly List<Mini> Minis = [];
    readonly List<Mini> spare = [];
    readonly List<Sheet> sheets = [];
    readonly List<int> spareSheets = [];

    /// <summary>syncMinis: the session <paramref name="sid"/> has <paramref name="want"/> subagents out. The newest helpers past that hop back into their bot; missing ones hop out, each to the first free place.</summary>
    public void Sync(Graph g, Rng r, int sid, int desk, Rgb bot, int want)
    {
        want = Math.Min(want, Mini.Spots.Length);
        var mine = Minis.Where(m => m.Sid == sid && !m.Leaving).ToList();
        for (var i = want; i < mine.Count; i++) mine[i].Leaving = true;
        for (var n = mine.Count; n < want; n++)
        {
            var used = new HashSet<int>(Minis.Where(m => m.Sid == sid).Select(m => m.Slot));
            var slot = -1;
            for (var i = 0; i < Mini.Spots.Length; i++)
                if (!used.Contains(i)) { slot = i; break; }
            if (slot < 0) break;
            Mini m;
            if (spare.Count > 0)
            {
                m = spare[^1];
                spare.RemoveAt(spare.Count - 1);
                m.Renew(g, r, sid, desk, slot, bot);
            }
            else m = new Mini(g, r, sid, desk, slot, bot);
            Minis.Add(m);
        }
    }

    /// <summary>The session is gone: its helpers go back into its bot.</summary>
    public void Leave(int sid)
    {
        foreach (var m in Minis)
            if (m.Sid == sid) m.Leaving = true;
    }

    /// <summary>The frame loop's part: every helper one step, the ones that are back in their bot put away, the sheets handed in. <paramref name="botAt"/> says where a session's bot sits.</summary>
    public void Step(Graph g, double dt, bool still, Func<int, (double X, double Z)?> botAt)
    {
        var tosses = new List<Toss>();
        foreach (var m in Minis)
            if (m.Step(g, dt, still, botAt(m.Sid)) is { } t) tosses.Add(t);
        for (var i = Minis.Count - 1; i >= 0; i--)
        {
            if (!Minis[i].Gone) continue;
            var m = Minis[i];
            Minis.RemoveAt(i);
            m.Dispose(g);
            spare.Add(m);
        }
        foreach (var t in tosses) Hand(g, t.From, t.To);
        StepSheets(g, dt);
    }

    void Hand(Graph g, V3 from, V3 to)
    {
        int node;
        if (spareSheets.Count > 0)
        {
            node = spareSheets[^1];
            spareSheets.RemoveAt(spareSheets.Count - 1);
        }
        else
        {
            node = g.Boxm(Graph.Root, 0.13, 0.006, 0.16, 0, 0, 0, Mat.StdMat(Rgb.Hex(0xf4efe4), 0.9, 0), true);
            g.Nodes[node].Receive = false;
        }
        g.Nodes[node].Visible = true;
        g.Nodes[node].P = from;
        sheets.Add(new Sheet(node, from, to));
    }

    void StepSheets(Graph g, double dt)
    {
        for (var i = sheets.Count - 1; i >= 0; i--)
        {
            var s = sheets[i];
            s.T += dt / 0.7;
            var f = Math.Min(s.T, 1);
            var n = g.Nodes[s.Node];
            var pos = s.From + (s.To - s.From) * f;
            n.P = pos with { Y = pos.Y + Math.Sin(Math.PI * f) * 0.35 };
            n.R = new V3(Math.Sin(f * 9) * 0.4 * (1 - f), f * 4, 0);
            // It lies on the pile a moment, then is part of it.
            if (s.T >= 1.4)
            {
                n.Visible = false;
                spareSheets.Add(s.Node);
                sheets.RemoveAt(i);
            }
        }
    }

    /// <summary>Something is hopping or in the air: the shadows are redrawn every frame.</summary>
    public bool Moving => sheets.Count > 0 || Minis.Any(m => m.Out < 1);

    /// <summary>Sheets on their way to a tray (or lying on it for a moment).</summary>
    public int SheetsInAir => sheets.Count;

    /// <summary>There is a helper at all: they bob and write, so the office keeps its lively pace.</summary>
    public bool Any => Minis.Count > 0;

    /// <summary>The main colours of the helpers a session has out (those not on their way back).</summary>
    public List<(byte R, byte G, byte B)> Colors(int sid) => [.. Minis.Where(m => m.Sid == sid && !m.Leaving).Select(m => m.Color.Css())];
}
