using HoverAvalonia.Core;

namespace HoverAvalonia.Office;

public enum TimeOfDay { Night, Day }

/// <summary>The three simulated agent states the brief asks for. Hover has no "idle" stage; Idle shows the settled
/// post-wake pose (<see cref="Stage.Waking"/> after its 1.6 s greeting: seated, bulb blinking, no task).</summary>
public enum SimState { Idle, Working, Completed }

public sealed class Session
{
    public int Id, Desk; public string Title = "", Tool = "kiro";
    public Bot B = null!;
    public SimState Sim; public string? Act; public string TagWant = ""; public double TagSince;
    public Stage Stage => Sim switch { SimState.Idle => Stage.Waking, SimState.Working => Stage.Working, _ => Stage.Done };
}

public sealed record Lights(Rgb HemiSky, Rgb HemiGround, V3 SunDir, Rgb Sun, M4 SunView, V3 FillDir, Rgb Fill, List<(V3 p, Rgb c, double dist, double decay)> Points, double Exposure);

/// <summary>Port of the parts of hover-office/office.rs the slice needs: lights per time of day, the iso camera with the user's
/// pan/zoom and focus-on-selection, slab-test picking of bots/props, and the frame step. Sessions come from a deterministic script.</summary>
public sealed class OfficeScene
{
    record struct Times(uint Hemi0, uint Hemi1, double HemiK, uint Sun, double SunK, uint Fill, double FillK, double Lamp, double Exposure, uint PatchC, double PatchO, uint BeamC, double BeamO, double Dust, uint Shade);
    static readonly Times Night = new(0x8a78b8, 0x2a1812, 1.05, 0x8fa2ff, 0.6, 0xffc8a0, 0.5, 3.4, 1.3, 0x6f86ff, 0.1, 0x6f86ff, 0.05, 0, 0xffc27a);
    static readonly Times Day = new(0xfff1de, 0x6a4a3a, 1.5, 0xffdcaa, 3.2, 0xfff0e0, 0.9, 0.0, 1.0, 0xffc070, 0.42, 0xffd79a, 0.13, 0.8, 0x8a7a66);

    public readonly Graph G = new();
    public readonly Room Room;
    readonly Rng r = new(11);
    public readonly List<Session> Sessions = [];
    public TimeOfDay Time = TimeOfDay.Night;
    public double W = 1120, H = 440, Aspect = 1120.0 / 440;
    public double[] Cam = [0, 1.7, 0, 1], CamTo = [0, 1.7, 0, 1];
    public double[] User = [0, 0, 1];
    public int? Sel;           // selected session id: the camera closes in on its bot
    public int? HoveredId;
    public (double x, double y)? Pointer;
    public bool Dragging, Still, ShadowDirty = true, Lively = true;
    public double ClockT, NowMs; double shadowAt = -1, acc;
    public ulong Frames;
    /// <summary>Which of the 8 canvas textures changed since the renderer took them.</summary>
    public readonly bool[] TexDirty = new bool[8];

    static readonly V3 Right = new(Math.Sqrt(0.5), 0, -Math.Sqrt(0.5)), Fwd = new(-Math.Sqrt(0.5), 0, -Math.Sqrt(0.5));
    static V3 IsoOffset => new V3(1.0, 0.86, 1.0).Norm() * 40.0;
    public static readonly V3 IsoDir = new V3(1.0, 0.86, 1.0).Norm();

    public OfficeScene()
    {
        Room = RoomBuilder.Build(G, r);
        // Prop hit boxes (the TV, board, clock, window, door, shelf) and desk hit boxes, as office.rs adds them.
        (double[] at, int idx)[] props = [([5.4, 2.38, RoomBuilder.Z0 + 0.2, 2.6, 1.55, 0.35], 0), ([RoomBuilder.X0 + 0.2, 2.33, -0.7, 0.35, 1.75, 3.0], 1), ([RoomBuilder.X0 + 0.2, 2.72, 1.8, 0.35, 0.62, 1.2], 2),
            ([1.5, 2.3, RoomBuilder.Z0 + 0.2, 2.9, 2.1, 0.35], 3), ([RoomBuilder.Door.x, 1.2, RoomBuilder.Z0 + 0.3, 1.3, 2.45, 0.5], 4), ([RoomBuilder.X0 + 0.25, 1.21, -3.53, 0.5, 2.42, 1.6], 5)];
        foreach (var (at, idx) in props)
        {
            int n = G.Add(Graph.Root, new(at[0], at[1], at[2])); G.Nodes[n].S = new(at[3], at[4], at[5]); G.Nodes[n].Hit = HitKind.Prop; G.Nodes[n].HitIndex = idx;
        }
        for (int i = 0; i < RoomBuilder.Desks.Length; i++)
        {
            var (dx, dz) = RoomBuilder.Desks[i];
            int n = G.Add(Graph.Root, new(dx + 0.08, 0.68, dz)); G.Nodes[n].S = new(0.78, 1.36, 1.66); G.Nodes[n].Hit = HitKind.Desk; G.Nodes[n].HitIndex = i;
        }
        Textures.Reset(this);
        ApplyTime(TimeOfDay.Night);
    }

    public void Resize(double w, double h) { W = w; H = h; Aspect = w / h; }
    Times T => Time == TimeOfDay.Day ? Day : Night;

    public void ApplyTime(TimeOfDay t)
    {
        Time = t; var tt = T;
        foreach (int s in Room.Shades.Append(Room.FloorShade)) G.Nodes[s].Material!.Color = Rgb.Hex(tt.Shade);
        var pm = G.Nodes[Room.Patch].Material!; pm.Color = Rgb.Hex(tt.PatchC); pm.Opacity = tt.PatchO;
        var bm = G.Nodes[Room.Beam].Material!; bm.Color = Rgb.Hex(tt.BeamC); bm.Opacity = tt.BeamO;
        G.Nodes[Room.Dust].Material!.Opacity = tt.Dust; G.Nodes[Room.Dust].Visible = tt.Dust > 0;
        Textures.Sky(this); Textures.Tv(this, 0);
        ShadowDirty = true;
    }

    public Lights Lights()
    {
        var t = T; var sunPos = new V3(-1.5, 10, -12); var target = new V3(1.5, 0, 1.5);
        var pts = Room.Lamps.Select(p => (p, Rgb.Hex(0xffa860).Mul(t.Lamp), 4.2, 1.6)).ToList();
        pts.Add((new V3(-6.5, 1.6, 2.8), Rgb.Hex(0xffa860).Mul(t.Lamp * 0.9), 5.0, 1.5));
        return new(Rgb.Hex(t.Hemi0).Mul(t.HemiK), Rgb.Hex(t.Hemi1).Mul(t.HemiK), (sunPos - target).Norm(), Rgb.Hex(t.Sun).Mul(t.SunK),
            M4.LookAt(sunPos, target, new(0, 1, 0)).RigidInverse(), new V3(8, 6, 10).Norm(), Rgb.Hex(t.Fill).Mul(t.FillK), pts, t.Exposure);
    }

    // MARK: sessions (deterministic script instead of Hover's `state` message)
    public Session AddSession(int bot, int desk, SimState sim, bool walkIn)
    {
        var (name, color) = Bot.Roster[bot % Bot.Roster.Length];
        int index = Sessions.Count;
        var b = new Bot(G, r, name, color, index);
        if (walkIn) { b.Place(RoomBuilder.Door.x, RoomBuilder.Z0 + 0.1, false); b.Go([(RoomBuilder.Door.x, 0.25), (RoomBuilder.Seat(desk) + 0.05, 0.25), (RoomBuilder.Seat(desk) + 0.05, RoomBuilder.Desks[desk].z)], Arrive.Sit); }
        else b.Place(RoomBuilder.Seat(desk) + 0.05, RoomBuilder.Desks[desk].z, true);
        var s = new Session { Id = index + 1, Desk = desk, B = b, Sim = sim, Title = $"{name}'s task" };
        s.B.Sync(s.Stage, s.Act);
        Sessions.Add(s); ShadowDirty = true; return s;
    }

    // MARK: camera
    /// <summary>Empirical: 1.0 is the Rust formula; --view-cal 1.115 matches the baseline screenshots' scale (cause in the Rust harness not found).</summary>
    public double ViewCal = 1.0;
    double HalfWidth(double zoom) => Math.Max(6.2 * Aspect, 9.2) * ViewCal / zoom;
    public (M4 view, M4 proj) Camera()
    {
        var t = new V3(Cam[0], Cam[1], Cam[2]);
        var world = M4.LookAt(t + IsoOffset, t, new(0, 1, 0));
        double w = HalfWidth(Cam[3]), h = w / Aspect;
        return (world.RigidInverse(), M4.Ortho(-w, w, h, -h, 1, 90));
    }
    (double, double) OverFloor(double dx, double dy, double zoom)
    {
        double w = 2 * HalfWidth(zoom) / W, sinE = IsoOffset.Y / IsoOffset.Len;
        return ((Right.X * dx + Fwd.X * dy / sinE) * w, (Right.Z * dx + Fwd.Z * dy / sinE) * w);
    }
    public void ZoomBy(double k, double dx, double dy)
    {
        double old = User[2], nz = Math.Clamp(old * k, 0.85, 2.8);
        if (nz == old) return;
        var a = OverFloor(dx, dy, old); var b = OverFloor(dx, dy, nz);
        User[0] += a.Item1 - b.Item1; User[1] += a.Item2 - b.Item2; User[2] = nz; ClampView();
    }
    void ClampView() { User[0] = Math.Clamp(User[0], -6, 6); User[1] = Math.Clamp(User[1], -5, 5); }
    public void Drag(double dx, double dy) { var g = OverFloor(dx, -dy, User[2]); User[0] -= g.Item1; User[1] -= g.Item2; ClampView(); }
    public void ResetView() { User = [0, 0, 1]; Sel = null; }

    // MARK: picking (Raycaster.setFromCamera for an orthographic camera, nearest hit box)
    static double? Slab(V3 o, V3 d, V3 lo, V3 hi)
    {
        double t0 = double.NegativeInfinity, t1 = double.PositiveInfinity;
        foreach (var (oo, dd, l, h) in new[] { (o.X, d.X, lo.X, hi.X), (o.Y, d.Y, lo.Y, hi.Y), (o.Z, d.Z, lo.Z, hi.Z) })
        {
            if (Math.Abs(dd) < 1e-12) { if (oo < l || oo > h) t1 = -1; continue; }
            double u = (l - oo) / dd, v = (h - oo) / dd;
            t0 = Math.Max(t0, Math.Min(u, v)); t1 = Math.Min(t1, Math.Max(u, v));
        }
        return t1 >= Math.Max(t0, 0) ? t0 : null;
    }

    /// <summary>The session id whose bot (or desk) is under the pointer, or null. Props are reported through <paramref name="prop"/>.</summary>
    public int? PickAt(double px, double py, out int prop)
    {
        prop = -1;
        var (view, proj) = Camera(); var inv = proj.Mul(view).Inverse();
        double nx = px / W * 2 - 1, ny = -(py / H) * 2 + 1;
        var a = inv.Point(new(nx, ny, -1)); var b = inv.Point(new(nx, ny, 1)); var dir = (b - a).Norm();
        var world = G.World(); var shown = G.Shown();
        var found = new List<(double t, HitKind kind, int idx)>();
        for (int i = 0; i < G.Nodes.Count; i++)
        {
            var n = G.Nodes[i]; if (n.Hit == HitKind.None || !shown[i]) continue;
            var m = world[i].Inverse();
            var t0 = Slab(m.Point(a), m.Dir(dir), new(-.5, -.5, -.5), new(.5, .5, .5)); if (t0 == null) continue;
            if (n.Hit == HitKind.Bot) { var s = Sessions.FirstOrDefault(s => s.B.HitNode == i); if (s == null) continue; found.Add((t0.Value, HitKind.Bot, s.Id)); }
            else if (n.Hit == HitKind.Desk) { var s = Sessions.FirstOrDefault(s => s.Desk == n.HitIndex); if (s == null) continue; found.Add((t0.Value, HitKind.Desk, s.Id)); }
            else found.Add((t0.Value, HitKind.Prop, n.HitIndex));
        }
        if (found.Count == 0) return null;
        found.Sort((x, y) => x.t.CompareTo(y.t));
        var first = found[0];
        if (first.kind == HitKind.Prop) { prop = first.idx; return null; }
        // A seated bot sits behind its monitor, so its box and its desk's overlap; prefer the bot (the Rust port refines this with a mesh test).
        var bot = found.FirstOrDefault(f => f.kind == HitKind.Bot);
        if (bot.kind == HitKind.Bot) return bot.idx;
        return found.First(f => f.kind == HitKind.Desk).idx;
    }

    // MARK: the frame
    /// <summary>One animation step at <paramref name="nowMs"/>. False when pacing skips it (60 Hz sim step, as office.rs).</summary>
    public bool Frame(double nowMs, double dtMs)
    {
        NowMs = nowMs; double dt = Math.Clamp(dtMs / 1000, 0, 0.1);
        acc += dt; if (acc < 1.0 / 60) return false;
        double step = acc; acc = 0; ClockT += step;
        foreach (var s in Sessions) { s.B.Sync(s.Stage, s.Act); s.B.Step(G, step, Still); }
        bool near = Sessions.Any(s => Math.Sqrt(Math.Pow(s.B.X - RoomBuilder.Door.x, 2) + Math.Pow(s.B.Z - RoomBuilder.Z0, 2)) < 1.5);
        var door = G.Nodes[Room.Door]; door.R = door.R with { Y = Rng.Ease(door.R.Y, near ? -1.3 : 0, 6, step) };
        for (int i = 0; i < RoomBuilder.Desks.Length; i++)
        {
            var s = Sessions.FirstOrDefault(x => x.Desk == i && x.B.Seated && !x.B.Walking);
            var (c, o) = s == null ? (0u, 0.0) : s.Stage.Screen();
            bool running = s?.Act == "Running" && Math.Sin(ClockT * 11) > 0;
            var gm = G.Nodes[Room.DeskGlows[i]].Material!; gm.Color = Rgb.Hex(c); gm.Opacity = Rng.Ease(gm.Opacity, o * (running ? 1.3 : 1), 8, step);
        }
        if (!Still)
        {
            double a = ClockT * 0.21;
            G.Nodes[Room.Vac].P = new(0.6 + Math.Sin(a) * 3, 0, 4.3 + Math.Sin(a * 2.3) * 0.7);
            G.Nodes[Room.Vac].R = G.Nodes[Room.Vac].R with { Y = Math.Atan2(Math.Cos(a) * 3, Math.Cos(a * 2.3) * 0.7 * 2.3) };
            for (int i = 0; i < 3; i++)
            {
                double f = (ClockT * 0.5 + i / 3.0) % 1.0; var n = G.Nodes[Room.Steam[i]];
                n.P = new(-4.3 + Math.Sin(f * 6 + i) * 0.04, 1.5 + f * 0.6, RoomBuilder.Z0 + 0.3);
                double sc = 0.15 + f * 0.25; n.S = new(sc, sc, sc); n.Material!.Opacity = 0.3 * (1 - f);
            }
            G.Nodes[Room.Dust].R = G.Nodes[Room.Dust].R with { Y = Math.Sin(ClockT * 0.1) * 0.02 };
            G.Nodes[Room.Dust].P = G.Nodes[Room.Dust].P with { Y = Math.Sin(ClockT * 0.4) * 0.05 };
            G.Nodes[Room.ExitGlow].Material!.Opacity = 0.5 + Math.Sin(ClockT * 2) * 0.05;
        }
        // Camera: the user's view; or close on the selected session's bot (office.rs: zoom 1.45, shifted for the chat drawer at the right).
        var sel = Sel is int id ? Sessions.FirstOrDefault(s => s.Id == id) : null;
        double side = W < 700 ? 0 : Math.Min(424, W * 0.42) / 2;
        if (sel != null) { double off = side * 2 * HalfWidth(1.45) / W; CamTo = [sel.B.X + Right.X * off, 0.9, sel.B.Z + Right.Z * off, 1.45]; }
        else CamTo = [User[0], 1.7, User[1], User[2]];
        double k = Still || Dragging ? 60 : 5;
        for (int i = 0; i < 4; i++) Cam[i] = Rng.Ease(Cam[i], CamTo[i], k, step);
        Pick();
        if ((int)(ClockT * 4) != tvAt) { tvAt = (int)(ClockT * 4); Textures.Tv(this, ClockT); }
        bool walking = Sessions.Any(s => s.B.Walking);
        if (walking || ShadowDirty || ClockT - shadowAt >= 0.1) { shadowAt = ClockT; ShadowDirty = true; }
        Lively = walking || Dragging || near || Sessions.Any(s => s.Stage.Busy() || s.B.Since < 2) || Enumerable.Range(0, 4).Any(i => Math.Abs(Cam[i] - CamTo[i]) > 0.002);
        Frames++; return true;
    }
    int tvAt = -1;

    void Pick()
    {
        int? hit = null; int prop = -1;
        if (Pointer is (double px, double py) && !Dragging) hit = PickAt(px, py, out prop);
        HoveredId = hit;
        foreach (var s in Sessions) s.B.Hot = hit == s.Id || Sel == s.Id;
        int[] panes = [Room.Tv, Room.Board, Room.Clock, Room.Sky];
        for (int k = 0; k < 4; k++) { double v = prop == k ? 1.35 : 1.0; G.Nodes[panes[k]].Material!.Color = new(v, v, v); }
    }
}
