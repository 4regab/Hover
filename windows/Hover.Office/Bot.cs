namespace Hover.Office;

// class Bot: the boxy mascot, its walk along a path, sitting, and a pose per stage and act,
// eased as the page eases them. A port of internal/office/bot.go.

public readonly record struct BotDef(string Name, uint Color);

/// <summary>How far a session has got. Waiting is the agent asking the user first (its hand up, the bulb blinking amber).</summary>
public enum Stage : byte { Waking, Working, Done, Failed, Stopped, Waiting }

public static class StageExt
{
    public static Stage Parse(string? s) => s switch
    {
        "working" => Stage.Working,
        "done" => Stage.Done,
        "failed" => Stage.Failed,
        "stopped" => Stage.Stopped,
        "waiting" => Stage.Waiting,
        _ => Stage.Waking,
    };

    static readonly uint[] Bulbs = [0xffd24a, 0xc4a2ff, 0x4ade80, 0xff5b52, 0x55505f, 0xffb340];
    static readonly uint[] Lights = [0x7fb8ff, 0x7fb8ff, 0x4ade80, 0xff5b52, 0, 0xffb340];
    static readonly double[] LightK = [0.25, 0.6, 0.42, 0.5, 0, 0.6];
    static readonly string[] Words = ["Waking up", "Working", "Done", "Couldn’t finish", "Stopped", "Waiting for you"];

    public static uint Bulb(this Stage s) => Bulbs[(int)s];

    /// <summary>SCREEN: the desk screen's light on the bot's face.</summary>
    public static (uint Color, double Opacity) ScreenLight(this Stage s) => (Lights[(int)s], LightK[(int)s]);

    public static string Word(this Stage s) => Words[(int)s];

    public static bool Busy(this Stage s) => s is Stage.Waking or Stage.Working or Stage.Waiting;
}

/// <summary>What a bot does on reaching the end of its path (the page's `arrive` callbacks: sit down, or be gone).</summary>
public enum Arrive : byte { None, Sit, Gone }

public sealed class Bot
{
    public static readonly BotDef[] Defs =
        [new("Pip", 0x9b6bff), new("Juno", 0x2fc9b0), new("Moss", 0xff9a4a), new("Nova", 0xff6fae), new("Ada", 0x5aa8ff), new("Rue", 0xb4e04a)];

    struct Pose { public double Lean, Hx, Hy, Hz, Al, Ar, Sl, Sr, Lx, Ly; }

    readonly record struct Eye(int G, int Open, int Happy, int Shut, double S);

    public string Name;
    public Rgb Color;
    public (byte R, byte G, byte B) Css;
    public double T, Since, SinceSeat;
    public Stage Stage = Stage.Waking;
    public string Act = "";
    public bool HasAct;
    public double X, Z;
    double face;
    public double Yaw;
    public List<(double X, double Z)> Path = [];
    public bool Seated;
    public Arrive Arrive;
    double sit, walk, phase;
    public bool Hot;
    Pose p;
    public readonly int Root;
    readonly int[] legs;
    readonly int upper, head;
    readonly int[] arms;
    readonly Eye[] eyes;
    readonly int bulb, halo, ring;
    double ringOp;
    /// <summary>The node whose box is the bot's hit box.</summary>
    public readonly int Hit;

    public Bot(Graph g, Rng r, string name, uint color, int index)
    {
        var c = Rgb.Hex(color);
        T = r.Next() * 10;
        var main = Mat.StdMat(c, 0.5, 0);
        var dark = Mat.StdMat(c.Mul(0.5), 0.6, 0);
        var pale = Mat.StdMat(c.Lerp(new Rgb(1, 1, 1), 0.4), 0.45, 0);
        var visor = Mat.StdMat(Rgb.Hex(0x111018), 0.22, 0.35);
        var eyeM = Mat.BasicHex(0xaaf6ff, false);
        Root = g.Add(Graph.Root, default);
        var hips = g.Pivot(Root, 0, 0.24, 0);
        legs = new int[2];
        double[] sides = [-1, 1];
        for (var i = 0; i < 2; i++)
        {
            var pv = g.Pivot(hips, sides[i] * 0.1, 0, 0);
            g.Boxm(pv, 0.13, 0.2, 0.15, 0, -0.1, 0, dark, true);
            g.Boxm(pv, 0.15, 0.06, 0.21, 0, -0.21, 0.03, pale, true);
            legs[i] = pv;
        }
        upper = g.Pivot(hips, 0, 0, 0);
        g.Boxm(upper, 0.42, 0.3, 0.3, 0, 0.15, 0, dark, true);
        g.Boxm(upper, 0.22, 0.13, 0.02, 0, 0.17, 0.155, pale, true);
        head = g.Pivot(upper, 0, 0.3, 0);
        g.Boxm(head, 0.58, 0.44, 0.48, 0, 0.22, 0, main, true);
        g.Boxm(head, 0.5, 0.05, 0.4, 0, 0.465, 0, pale, true);
        var b = g.Boxm(head, 0.5, 0.36, 0.4, 0, 0.22, -0.02, pale, true);
        g.Nodes[b].S = new V3(0.5, 0.36, 0.49);
        g.Boxm(head, 0.46, 0.28, 0.02, 0, 0.21, 0.245, visor, false);
        foreach (var s in sides)
        {
            g.Boxm(head, 0.07, 0.2, 0.22, s * 0.315, 0.22, 0, dark, true);
            g.Boxm(head, 0.02, 0.1, 0.1, s * 0.355, 0.22, 0, pale, true);
        }
        g.Boxm(head, 0.03, 0.14, 0.03, 0.14, 0.53, -0.08, dark, true);
        bulb = g.Boxm(head, 0.1, 0.1, 0.1, 0.14, 0.64, -0.08, Mat.BasicHex(Stage.Waking.Bulb(), false), false);
        halo = g.Drawn(head, new Geo { Kind = GeoKind.Sprite }, Mat.GlowMat(Rgb.Hex(Stage.Waking.Bulb()), 0.8));
        g.Nodes[halo].S = new V3(0.55, 0.55, 0.55);
        g.Nodes[halo].P = new V3(0.14, 0.64, -0.08);
        var es = new List<Eye>();
        foreach (var s in sides)
        {
            var eg = g.Pivot(head, s * 0.1, 0.21, 0.258);
            var open = g.Boxm(eg, 0.075, 0.11, 0.01, 0, 0, 0, eyeM, false);
            var happy = g.Add(eg, default);
            var h1 = g.Boxm(happy, 0.055, 0.024, 0.01, -0.018, 0, 0, eyeM, false);
            g.Nodes[h1].R = g.Nodes[h1].R with { Z = 0.75 };
            var h2 = g.Boxm(happy, 0.055, 0.024, 0.01, 0.018, 0, 0, eyeM, false);
            g.Nodes[h2].R = g.Nodes[h2].R with { Z = -0.75 };
            var shut = g.Boxm(eg, 0.085, 0.022, 0.01, 0, -0.02, 0, eyeM, false);
            es.Add(new Eye(eg, open, happy, shut, s));
        }
        eyes = [.. es];
        arms = new int[2];
        for (var i = 0; i < 2; i++)
        {
            var pv = g.Pivot(upper, sides[i] * 0.27, 0.27, 0);
            g.Boxm(pv, 0.1, 0.22, 0.12, 0, -0.1, 0, main, true);
            g.Boxm(pv, 0.11, 0.07, 0.13, 0, -0.23, 0, pale, true);
            arms[i] = pv;
        }
        Hit = g.Add(Root, new V3(0, 0.65, 0));
        g.Nodes[Hit].S = new V3(0.8, 1.3, 0.8);
        g.Nodes[Hit].Hit = new Hit(HitKind.Bot, index);
        ring = g.Drawn(Root, new Geo { Kind = GeoKind.Ring, Inner = 0.42, Outer = 0.52, Seg = 40 },
            new Mat { Kind = MatKind.Basic, Color = c, Opacity = 0, Blend = Blend.Normal, Tex = -1 });
        g.Nodes[ring].R = new V3(-Math.PI / 2, 0, 0);
        Name = name;
        Color = c;
        Css = c.Css();
        SinceSeat = 99;
    }

    public void Place(double x, double z, bool seated)
    {
        X = x;
        Z = z;
        Seated = seated;
        sit = 0;
        Yaw = Math.PI;
        if (seated)
        {
            sit = 1;
            Yaw = Math.PI / 2;
        }
        face = Yaw;
        SinceSeat = 99;
    }

    public void Go(List<(double X, double Z)> path, Arrive arrive)
    {
        Path = path;
        Seated = false;
        Arrive = arrive;
    }

    public void Sync(Stage stage, string act, bool hasAct)
    {
        if (stage != Stage) Since = 0;
        Stage = stage;
        Act = act;
        HasAct = hasAct;
    }

    /// <summary>head3: where the name tag sits.</summary>
    public V3 Head3(Graph g) => new(X, g.Nodes[Root].P.Y + (Seated ? 1.28 : 1.22), Z);

    static void EaseTo(ref double v, double to, double k, double dt) => v = Js.Ease(v, to, k, dt);

    public bool Walking => Path.Count > 0;

    /// <summary>One step. It returns the arrival reached this step, if any (the caller acts on it).</summary>
    public Arrive Step(Graph g, double dt, bool still)
    {
        T += dt;
        Since += dt;
        SinceSeat += dt;
        var t = T;
        var walking = false;
        var arrived = Arrive.None;
        if (Path.Count > 0 && sit < 0.05)
        {
            var (px, pz) = Path[0];
            double dx = px - X, dz = pz - Z;
            var d = Math.Sqrt(dx * dx + dz * dz);
            var sp = 1.7 * dt;
            if (d <= sp)
            {
                X = px;
                Z = pz;
                Path.RemoveAt(0);
                if (Path.Count == 0)
                {
                    arrived = Arrive;
                    Arrive = Arrive.None;
                }
            }
            else
            {
                X += dx / d * sp;
                Z += dz / d * sp;
                face = Math.Atan2(dx, dz);
                walking = true;
            }
        }
        if (arrived == Arrive.Sit)
        {
            Seated = true;
            SinceSeat = 0;
        }
        EaseTo(ref walk, Js.B2F(walking), 10, dt);
        if (walking) phase += dt * 11;
        var seatNow = Seated && Path.Count == 0;
        if (seatNow) face = Math.PI / 2;
        EaseTo(ref sit, Js.B2F(seatNow), 7, dt);
        Yaw = Js.AngTo(Yaw, face, 9, dt);

        Pose q = default;
        string eyesNow = "open";
        bool blinkBulb = false;
        var haloK = 0.8;
        var sw = Math.Sin(phase);
        var bulbColor = Stage.Bulb();
        if (!seatNow)
        {
            q.Al = sw * 0.6 * walk;
            q.Ar = -sw * 0.6 * walk;
            q.Hx = 0.05;
            if (Stage == Stage.Done) eyesNow = "happy";
        }
        else
        {
            switch (Stage)
            {
                case Stage.Waking:
                    var w = SinceSeat;
                    if (w < 1.6)
                    {
                        q.Al = -2.9; q.Ar = -2.9; q.Sl = -0.35; q.Sr = 0.35; q.Lean = -0.14; q.Hx = -0.25;
                        eyesNow = "happy";
                        if (w < 0.6) eyesNow = "shut";
                    }
                    else
                    {
                        q.Al = -1.35; q.Ar = -1.35; q.Hx = 0.08;
                        q.Lx = Math.Sin(t * 1.3) * 0.02;
                    }
                    blinkBulb = true;
                    break;
                case Stage.Working:
                    haloK = 0.5 + 0.35 * Math.Sin(t * 3);
                    q.Al = -1.4; q.Ar = -1.4; q.Lean = 0.08; q.Hx = 0.12;
                    var act = HasAct ? Act : "";
                    switch (act)
                    {
                        case "Thinking":
                            q.Ar = -2.25; q.Sr = 0.55; q.Hx = -0.2; q.Hz = Math.Sin(t * 0.9) * 0.14; q.Ly = 0.02; q.Lx = 0.02;
                            break;
                        case "Reading":
                            q.Hy = Math.Sin(t * 1.5) * 0.14; q.Lx = Math.Sin(t * 1.5) * 0.022; q.Ly = -0.012;
                            break;
                        case "Editing":
                            q.Al = -1.4 + Math.Sin(t * 22) * 0.14; q.Ar = -1.4 + Math.Sin(t * 22 + 2) * 0.14; q.Hx = 0.16;
                            break;
                        case "Running":
                            q.Al = -1.15; q.Ar = -1.15; q.Lean = 0.2; q.Hx = 0.05;
                            haloK = 0.35;
                            if (Math.Sin(t * 11) > 0) haloK = 0.9;
                            break;
                    }
                    break;
                case Stage.Done:
                    eyesNow = "happy";
                    if (Since < 1.8)
                    {
                        q.Al = -3.0 + Math.Sin(t * 13) * 0.3; q.Ar = -3.0 - Math.Sin(t * 13) * 0.3;
                        q.Sl = -0.3; q.Sr = 0.3; q.Lean = -0.1; q.Hx = -0.2;
                    }
                    else
                    {
                        q.Al = -2.75; q.Ar = -2.75; q.Sl = 0.6; q.Sr = -0.6; q.Lean = -0.2; q.Hx = -0.12;
                        q.Hz = Math.Sin(t * 0.7) * 0.06;
                    }
                    break;
                case Stage.Waiting:
                    // Asking the user: a hand up and waving, the bulb blinking amber.
                    blinkBulb = true;
                    q.Al = -1.4; q.Ar = -2.95 + Math.Sin(t * 6) * 0.22;
                    q.Sr = 0.35 + Math.Sin(t * 6) * 0.12; q.Lean = -0.06; q.Hx = -0.12;
                    break;
                case Stage.Failed:
                    eyesNow = "sad";
                    blinkBulb = true;
                    q.Al = -1.5; q.Ar = -1.5; q.Lean = 0.25; q.Hx = 0.35;
                    break;
                case Stage.Stopped:
                    eyesNow = "shut";
                    haloK = 0;
                    q.Al = -1.55; q.Ar = -1.55; q.Lean = 0.45 + Math.Sin(t * 1.6) * 0.02; q.Hx = 0.42; q.Hz = 0.1;
                    break;
            }
        }
        if (eyesNow == "open" && t % 3.7 < 0.12) eyesNow = "shut";
        var k = still ? 30.0 : 14.0;
        EaseTo(ref p.Lean, q.Lean, k, dt);
        EaseTo(ref p.Hx, q.Hx, k, dt);
        EaseTo(ref p.Hy, q.Hy, k, dt);
        EaseTo(ref p.Hz, q.Hz, k, dt);
        EaseTo(ref p.Al, q.Al, k, dt);
        EaseTo(ref p.Ar, q.Ar, k, dt);
        EaseTo(ref p.Sl, q.Sl, k, dt);
        EaseTo(ref p.Sr, q.Sr, k, dt);
        EaseTo(ref p.Lx, q.Lx, k, dt);
        EaseTo(ref p.Ly, q.Ly, k, dt);

        double bob;
        if (seatNow) bob = Math.Sin(t * 2) * 0.006;
        else bob = Math.Abs(sw) * 0.035 * walk;
        var ry = sit * 0.21 + bob;
        g.Nodes[Root].P = new V3(X, ry, Z);
        g.Nodes[Root].R = g.Nodes[Root].R with { Y = Yaw };
        var legW = sw * 0.6 * walk;
        g.Nodes[legs[0]].R = g.Nodes[legs[0]].R with { X = -Math.PI / 2 * sit + legW };
        g.Nodes[legs[1]].R = g.Nodes[legs[1]].R with { X = -Math.PI / 2 * sit - legW };
        g.Nodes[upper].R = g.Nodes[upper].R with { X = p.Lean };
        g.Nodes[head].R = new V3(p.Hx, p.Hy, p.Hz);
        g.Nodes[arms[0]].R = new V3(p.Al, 0, p.Sl);
        g.Nodes[arms[1]].R = new V3(p.Ar, 0, p.Sr);
        foreach (var e in eyes)
        {
            g.Nodes[e.G].P = g.Nodes[e.G].P with { X = e.S * 0.1 + p.Lx, Y = 0.21 + p.Ly };
            g.Nodes[e.Open].Visible = eyesNow == "open" || eyesNow == "sad";
            if (eyesNow == "sad")
            {
                g.Nodes[e.Open].S = g.Nodes[e.Open].S with { Y = 0.065 };
                g.Nodes[e.Open].R = g.Nodes[e.Open].R with { Z = -e.S * 0.45 };
            }
            else
            {
                g.Nodes[e.Open].S = g.Nodes[e.Open].S with { Y = 0.11 };
                g.Nodes[e.Open].R = g.Nodes[e.Open].R with { Z = 0 };
            }
            g.Nodes[e.Happy].Visible = eyesNow == "happy";
            g.Nodes[e.Shut].Visible = eyesNow == "shut";
        }
        var on = !blinkBulb || Math.Sin(t * 9) > -0.2;
        ref var bm = ref g.MatMut(bulb);
        if (bm.Kind == MatKind.Basic) bm.Color = Rgb.Hex(bulbColor).Mul(on ? 1 : 0.35);
        ref var hm = ref g.MatMut(halo);
        if (hm.Kind == MatKind.Glow)
        {
            hm.Color = Rgb.Hex(bulbColor);
            hm.Opacity = 0.05;
            if (on) hm.Opacity = haloK;
        }
        g.Nodes[ring].P = g.Nodes[ring].P with { Y = 0.02 - ry };
        var hot = Hot ? 0.95 : 0.0;
        EaseTo(ref ringOp, hot, 12, dt);
        ref var rm = ref g.MatMut(ring);
        if (rm.Kind == MatKind.Basic) rm.Opacity = ringOp;
        g.Nodes[ring].Visible = ringOp > 0.02;
        return arrived;
    }

    /// <summary>Hides the bot and takes it out of the raycast; the nodes stay, for <see cref="Renew"/>.</summary>
    public void Dispose(Graph g)
    {
        g.Nodes[Root].Visible = false;
        g.Nodes[Hit].Hit = null;
    }

    /// <summary>
    /// Makes a bot that left come back as a new one of the same name: its nodes are used again (every
    /// step sets what they show), so the scene doesn't grow with each session the office sees. The
    /// random draw is the one the constructor makes.
    /// </summary>
    public Bot Renew(Graph g, Rng r, int index)
    {
        T = r.Next() * 10;
        Since = 0;
        SinceSeat = 99;
        Stage = Stage.Waking;
        Act = "";
        HasAct = false;
        X = Z = face = Yaw = 0;
        Path = [];
        Seated = false;
        Arrive = Arrive.None;
        sit = walk = phase = 0;
        Hot = false;
        p = default;
        ringOp = 0;
        g.Nodes[Root].Visible = true;
        g.Nodes[Hit].Hit = new Hit(HitKind.Bot, index);
        return this;
    }
}
