using HoverAvalonia.Core;

namespace HoverAvalonia.Office;

public enum Stage { Waking, Working, Done, Failed, Stopped, Waiting }
public enum Arrive { None, Sit, Gone }

public static class StageInfo
{
    static readonly uint[] Bulbs = [0xffd24a, 0xc4a2ff, 0x4ade80, 0xff5b52, 0x55505f, 0xffb340];
    static readonly (uint, double)[] Screens = [(0x7fb8ff, 0.25), (0x7fb8ff, 0.6), (0x4ade80, 0.42), (0xff5b52, 0.5), (0, 0), (0xffb340, 0.6)];
    static readonly string[] Words = ["Waking up", "Working", "Done", "Couldn’t finish", "Stopped", "Waiting for you"];
    public static uint Bulb(this Stage s) => Bulbs[(int)s];
    public static (uint, double) Screen(this Stage s) => Screens[(int)s];
    public static string Word(this Stage s) => Words[(int)s];
    public static bool Busy(this Stage s) => s is Stage.Waking or Stage.Working or Stage.Waiting;
}

/// <summary>Port of hover-office/bot.rs: the boxy mascot, its walk, sitting, and a pose per stage and act, eased as the page eases them.</summary>
public sealed class Bot
{
    public static readonly (string name, uint color)[] Roster = [("Pip", 0x9b6bff), ("Juno", 0x2fc9b0), ("Moss", 0xff9a4a), ("Nova", 0xff6fae), ("Ada", 0x5aa8ff), ("Rue", 0xb4e04a)];

    struct Pose { public double Lean, Hx, Hy, Hz, Al, Ar, Sl, Sr, Lx, Ly; }
    sealed class Eye { public int G, Open, Happy, Shut; public double S; }

    public string Name; public Rgb Color;
    public double T, Since, SinceSeat = 99;
    public Stage Stage = Stage.Waking; public string? Act;
    public double X, Z, Face, Yaw;
    public List<(double x, double z)> Path = [];
    public bool Seated, Hot;
    public Arrive ArriveAt;
    double sit, walk, phase;
    Pose p;
    public int Root, HitNode;
    readonly int[] legs = new int[2]; readonly int upper, head; readonly int[] arms = new int[2];
    readonly List<Eye> eyes = [];
    readonly int bulb, halo, ring; double ringOpacity;

    static Mat S(Rgb c, double rough) => Mat.Std(c, rough);

    public Bot(Graph g, Rng r, string name, uint color, int index)
    {
        var c = Rgb.Hex(color);
        T = r.Next() * 10; Name = name; Color = c;
        var main = S(c, 0.5); var dark = S(c.Mul(0.5), 0.6); var pale = S(c.Lerp(new(1, 1, 1), 0.4), 0.45);
        var visor = Mat.Std(Rgb.Hex(0x111018), 0.22, 0.35);
        Mat Eyem() => Mat.Basic(0xaaf6ff, false);
        Root = g.Add(Graph.Root, default);
        int hips = g.Pivot(Root, 0, 0.24, 0);
        foreach (double s in new[] { -1.0, 1.0 })
        {
            int pv = g.Pivot(hips, s * 0.1, 0, 0);
            g.Box(pv, 0.13, 0.2, 0.15, 0, -0.1, 0, dark, true);
            g.Box(pv, 0.15, 0.06, 0.21, 0, -0.21, 0.03, pale, true);
            legs[s < 0 ? 0 : 1] = pv;
        }
        upper = g.Pivot(hips, 0, 0, 0);
        g.Box(upper, 0.42, 0.3, 0.3, 0, 0.15, 0, dark, true);
        g.Box(upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, pale, true);
        head = g.Pivot(upper, 0, 0.3, 0);
        g.Box(head, 0.58, 0.44, 0.48, 0, 0.22, 0, main, true);
        g.Box(head, 0.5, 0.05, 0.4, 0, 0.465, 0, pale, true);
        int b = g.Box(head, 0.5, 0.36, 0.4, 0, 0.22, -0.02, pale, true);
        g.Nodes[b].S = new(0.5, 0.36, 0.49);
        g.Box(head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, visor, false);
        foreach (double s in new[] { -1.0, 1.0 })
        {
            g.Box(head, 0.07, 0.2, 0.22, s * 0.315, 0.22, 0, dark, true);
            g.Box(head, 0.02, 0.1, 0.1, s * 0.355, 0.22, 0, pale, true);
        }
        g.Box(head, 0.03, 0.14, 0.03, 0.14, 0.53, -0.08, dark, true);
        bulb = g.Box(head, 0.1, 0.1, 0.1, 0.14, 0.64, -0.08, Mat.Basic(Stage.Waking.Bulb(), false), false);
        halo = g.Drawing(head, Geo.Sprite, Mat.Glow(Rgb.Hex(Stage.Waking.Bulb()), 0.8));
        g.Nodes[halo].S = new(0.55, 0.55, 0.55); g.Nodes[halo].P = new(0.14, 0.64, -0.08);
        foreach (double s in new[] { -1.0, 1.0 })
        {
            int eg = g.Pivot(head, s * 0.1, 0.21, 0.258);
            int open = g.Box(eg, 0.075, 0.11, 0.01, 0, 0, 0, Eyem(), false);
            int happy = g.Add(eg, default);
            int h1 = g.Box(happy, 0.055, 0.024, 0.01, -0.018, 0, 0, Eyem(), false); g.Nodes[h1].R = g.Nodes[h1].R with { Z = 0.75 };
            int h2 = g.Box(happy, 0.055, 0.024, 0.01, 0.018, 0, 0, Eyem(), false); g.Nodes[h2].R = g.Nodes[h2].R with { Z = -0.75 };
            int shut = g.Box(eg, 0.085, 0.022, 0.01, 0, -0.02, 0, Eyem(), false);
            eyes.Add(new Eye { G = eg, Open = open, Happy = happy, Shut = shut, S = s });
        }
        foreach (double s in new[] { -1.0, 1.0 })
        {
            int pv = g.Pivot(upper, s * 0.27, 0.27, 0);
            g.Box(pv, 0.1, 0.22, 0.12, 0, -0.1, 0, main, true);
            g.Box(pv, 0.11, 0.07, 0.13, 0, -0.23, 0, pale, true);
            arms[s < 0 ? 0 : 1] = pv;
        }
        HitNode = g.Add(Root, new(0, 0.65, 0));
        g.Nodes[HitNode].S = new(0.8, 1.3, 0.8); g.Nodes[HitNode].Hit = HitKind.Bot; g.Nodes[HitNode].HitIndex = index;
        var rm = Mat.BasicRgb(c, 0); rm.DepthWrite = false;
        ring = g.Drawing(Root, new Geo { Kind = GeoKind.Ring, Inner = 0.42, Outer = 0.52, Seg = 40 }, rm);
        g.Nodes[ring].R = new(-Math.PI / 2, 0, 0);
    }

    public void Place(double x, double z, bool seated)
    {
        X = x; Z = z; Seated = seated; sit = seated ? 1 : 0;
        Yaw = seated ? Math.PI / 2 : Math.PI; Face = Yaw; SinceSeat = 99;
    }
    public void Go(List<(double, double)> path, Arrive a) { Path = path; Seated = false; ArriveAt = a; }
    public void Sync(Stage s, string? act) { if (s != Stage) Since = 0; Stage = s; Act = act; }
    public bool Walking => Path.Count > 0;
    public V3 Head3(Graph g) => new(X, g.Nodes[Root].P.Y + (Seated ? 1.28 : 1.22), Z);

    public Arrive Step(Graph g, double dt, bool still)
    {
        T += dt; Since += dt; SinceSeat += dt;
        double t = T; bool walking = false; var arrived = Arrive.None;
        if (Path.Count > 0 && sit < 0.05)
        {
            var (px, pz) = Path[0]; double dx = px - X, dz = pz - Z, d = Math.Sqrt(dx * dx + dz * dz), sp = 1.7 * dt;
            if (d <= sp) { X = px; Z = pz; Path.RemoveAt(0); if (Path.Count == 0) { arrived = ArriveAt; ArriveAt = Arrive.None; } }
            else { X += dx / d * sp; Z += dz / d * sp; Face = Math.Atan2(dx, dz); walking = true; }
        }
        if (arrived == Arrive.Sit) { Seated = true; SinceSeat = 0; }
        walk = Rng.Ease(walk, walking ? 1 : 0, 10, dt);
        if (walking) phase += dt * 11;
        bool seatNow = Seated && Path.Count == 0;
        if (seatNow) Face = Math.PI / 2;
        sit = Rng.Ease(sit, seatNow ? 1 : 0, 7, dt);
        Yaw = Rng.AngTo(Yaw, Face, 9, dt);

        var q = new Pose(); string eyesState = "open"; bool blinkBulb = false; double haloV = 0.8; double sw = Math.Sin(phase);
        uint bulbC = Stage.Bulb();
        if (!seatNow)
        {
            q.Al = sw * 0.6 * walk; q.Ar = -sw * 0.6 * walk; q.Hx = 0.05;
            if (Stage == Stage.Done) eyesState = "happy";
        }
        else switch (Stage)
        {
            case Stage.Waking:
                double w = SinceSeat;
                if (w < 1.6) { q.Al = -2.9; q.Ar = -2.9; q.Sl = -0.35; q.Sr = 0.35; q.Lean = -0.14; q.Hx = -0.25; eyesState = w < 0.6 ? "shut" : "happy"; }
                else { q.Al = -1.35; q.Ar = -1.35; q.Hx = 0.08; q.Lx = Math.Sin(t * 1.3) * 0.02; }
                blinkBulb = true; break;
            case Stage.Working:
                haloV = 0.5 + 0.35 * Math.Sin(t * 3);
                q.Al = -1.4; q.Ar = -1.4; q.Lean = 0.08; q.Hx = 0.12;
                switch (Act)
                {
                    case "Thinking": q.Ar = -2.25; q.Sr = 0.55; q.Hx = -0.2; q.Hz = Math.Sin(t * 0.9) * 0.14; q.Ly = 0.02; q.Lx = 0.02; break;
                    case "Reading": q.Hy = Math.Sin(t * 1.5) * 0.14; q.Lx = Math.Sin(t * 1.5) * 0.022; q.Ly = -0.012; break;
                    case "Editing": q.Al = -1.4 + Math.Sin(t * 22) * 0.14; q.Ar = -1.4 + Math.Sin(t * 22 + 2) * 0.14; q.Hx = 0.16; break;
                    case "Running": q.Al = -1.15; q.Ar = -1.15; q.Lean = 0.2; q.Hx = 0.05; haloV = Math.Sin(t * 11) > 0 ? 0.9 : 0.35; break;
                }
                break;
            case Stage.Done:
                eyesState = "happy";
                if (Since < 1.8) { q.Al = -3.0 + Math.Sin(t * 13) * 0.3; q.Ar = -3.0 - Math.Sin(t * 13) * 0.3; q.Sl = -0.3; q.Sr = 0.3; q.Lean = -0.1; q.Hx = -0.2; }
                else { q.Al = -2.75; q.Ar = -2.75; q.Sl = 0.6; q.Sr = -0.6; q.Lean = -0.2; q.Hx = -0.12; q.Hz = Math.Sin(t * 0.7) * 0.06; }
                break;
            case Stage.Waiting:
                blinkBulb = true; q.Al = -1.4; q.Ar = -2.95 + Math.Sin(t * 6) * 0.22; q.Sr = 0.35 + Math.Sin(t * 6) * 0.12; q.Lean = -0.06; q.Hx = -0.12; break;
            case Stage.Failed:
                eyesState = "sad"; blinkBulb = true; q.Al = -1.5; q.Ar = -1.5; q.Lean = 0.25; q.Hx = 0.35; break;
            case Stage.Stopped:
                eyesState = "shut"; haloV = 0; q.Al = -1.55; q.Ar = -1.55; q.Lean = 0.45 + Math.Sin(t * 1.6) * 0.02; q.Hx = 0.42; q.Hz = 0.1; break;
        }
        if (eyesState == "open" && t % 3.7 < 0.12) eyesState = "shut";
        double k = still ? 30 : 14;
        p.Lean = Rng.Ease(p.Lean, q.Lean, k, dt); p.Hx = Rng.Ease(p.Hx, q.Hx, k, dt); p.Hy = Rng.Ease(p.Hy, q.Hy, k, dt); p.Hz = Rng.Ease(p.Hz, q.Hz, k, dt);
        p.Al = Rng.Ease(p.Al, q.Al, k, dt); p.Ar = Rng.Ease(p.Ar, q.Ar, k, dt); p.Sl = Rng.Ease(p.Sl, q.Sl, k, dt); p.Sr = Rng.Ease(p.Sr, q.Sr, k, dt);
        p.Lx = Rng.Ease(p.Lx, q.Lx, k, dt); p.Ly = Rng.Ease(p.Ly, q.Ly, k, dt);

        double bob = seatNow ? Math.Sin(t * 2) * 0.006 : Math.Abs(sw) * 0.035 * walk;
        double ry = sit * 0.21 + bob;
        g.Nodes[Root].P = new(X, ry, Z); g.Nodes[Root].R = g.Nodes[Root].R with { Y = Yaw };
        double legW = sw * 0.6 * walk;
        g.Nodes[legs[0]].R = new(-Math.PI / 2 * sit + legW, 0, 0);
        g.Nodes[legs[1]].R = new(-Math.PI / 2 * sit - legW, 0, 0);
        g.Nodes[upper].R = new(p.Lean, 0, 0);
        g.Nodes[head].R = new(p.Hx, p.Hy, p.Hz);
        g.Nodes[arms[0]].R = new(p.Al, 0, p.Sl); g.Nodes[arms[1]].R = new(p.Ar, 0, p.Sr);
        foreach (var e in eyes)
        {
            var eg = g.Nodes[e.G]; eg.P = new(e.S * 0.1 + p.Lx, 0.21 + p.Ly, eg.P.Z);
            var op = g.Nodes[e.Open];
            op.Visible = eyesState is "open" or "sad";
            op.S = op.S with { Y = eyesState == "sad" ? 0.065 : 0.11 };
            op.R = op.R with { Z = eyesState == "sad" ? -e.S * 0.45 : 0 };
            g.Nodes[e.Happy].Visible = eyesState == "happy";
            g.Nodes[e.Shut].Visible = eyesState == "shut";
        }
        bool on = !blinkBulb || Math.Sin(t * 9) > -0.2;
        g.Nodes[bulb].Material!.Color = Rgb.Hex(bulbC).Mul(on ? 1 : 0.35);
        var hm = g.Nodes[halo].Material!; hm.Color = Rgb.Hex(bulbC); hm.Opacity = on ? haloV : 0.05;
        g.Nodes[ring].P = g.Nodes[ring].P with { Y = 0.02 - ry };
        ringOpacity = Rng.Ease(ringOpacity, Hot ? 0.95 : 0, 12, dt);
        g.Nodes[ring].Material!.Opacity = ringOpacity;
        g.Nodes[ring].Visible = ringOpacity > 0.02;
        return arrived;
    }
}
