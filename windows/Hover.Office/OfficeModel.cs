using System.Text;

namespace Hover.Office;

// The page's model (main.js from "Sessions" to "Frame loop"): the sessions Hover's `state` message
// lists, a bot each at its desk (walking in and out), the time of day, the camera and its user
// view, picking, the name tags, the wall pictures, and the frame pacing. The page's DOM parts
// (drawer, panels, HUD) are the app's; what they need is here as data. A port of
// internal/office/office.go.

/// <summary>One step of a turn, as far as the office reads it: its kind ("agent" is a subagent) and status.</summary>
public sealed record StepInput(string K, string Status);

/// <summary>One turn of a session, as the backend's `state` message lists it.</summary>
public sealed class TurnInput
{
    public string Prompt = "";
    /// <summary>waking, working, waiting, done, failed or stopped; anything else, or none, is waking.</summary>
    public string? Stage;
    public bool Queued;
    public double? T0;
    public double? Took;
    public IReadOnlyList<StepInput> Steps = [];
}

/// <summary>One session, as the backend's `state` message lists it (the part the office reads).</summary>
public sealed class SessionInput
{
    public int Id;
    public string Key = "", Tool = "";
    public int Bot, Seat;
    public string Title = "", Folder = "";
    public double? Ctx;
    public string? Act, Pose;
    public string File = "";
    public IReadOnlyList<TurnInput> Turns = [];
}

public sealed class Turn
{
    public string Prompt = "";
    public Stage Stage;
    public int Steps;
    public bool Queued;
    public double T0, Took;
    public bool HasTook;
    /// <summary>The subagent steps of the turn not yet completed or failed (the helpers it has out).</summary>
    public int Agents;
}

public sealed class Session
{
    public int Id;
    public string Key = "", Tool = "";
    public int Bot, Desk;
    public string Title = "", Folder = "";
    public double Ctx;
    public bool HasCtx;
    public List<Turn> Turns = [];
    public string Act = "";
    public bool HasAct;
    public string Pose = "";
    public bool HasPose;
    public string File = "";
    public Bot B = null!;
    public string TagText = "";
    public double TagShown;

    /// <summary>
    /// last(s): the newest turn that isn't waiting in the queue. A session with no turns returns an idle
    /// one: indexing [0] here would fail, and this runs while a task streams state.
    /// </summary>
    public Turn Last()
    {
        for (var i = Turns.Count - 1; i >= 0; i--)
            if (!Turns[i].Queued) return Turns[i];
        return Turns.Count == 0 ? new Turn() : Turns[0];
    }

    public bool Busy => Last().Stage.Busy();

    public (string Pose, bool Has) PoseOf() => HasPose ? (Pose, true) : (Act, HasAct);

    /// <summary>
    /// subagentsOut: the helpers the session has out now. Its live turn's subagents not yet ended, once
    /// its bot is at its desk and at work; at most as many as there are places to stand around the desk.
    /// </summary>
    public int SubagentsOut() => !Busy || !B.Seated || B.Walking ? 0 : Math.Min(Last().Agents, Mini.Spots.Length);
}

public enum TimeOfDay : byte { Night, Day }

/// <summary>A light's place, colour × intensity, distance and decay.</summary>
public readonly record struct PointLight(V3 P, Rgb C, double Dist, double Decay);

/// <summary>The lights as the renderer takes them.</summary>
public sealed class Lights
{
    public Rgb HemiSky, HemiGround;
    public V3 SunDir;
    public Rgb Sun;
    public M4 SunView;
    public V3 FillDir;
    public Rgb Fill;
    public List<PointLight> Points = [];
    public double Exposure;
}

/// <summary>One of the six props that can be clicked (PROPS).</summary>
public enum Prop : byte { TV, Board, Clock, Window, Door, Shelf }

public enum HoverKind : byte { None, Bot, Prop, Desk }

/// <summary>What the pointer is over.</summary>
public readonly record struct Hover(HoverKind Kind, int Id = 0, Prop Prop = default);

public enum ClickKind : byte { Nothing, Open, Desk, Panel, Toast, NewTask, Time, Fold }

/// <summary>What a click asks the host (the page's postMessage) or the page itself to do.</summary>
public readonly record struct Click(ClickKind Kind, int Id = 0, double X = 0, double Y = 0, string Panel = "", TimeOfDay Time = TimeOfDay.Night);

/// <summary>Where a tag sits (the bot's head projected), its words so far, its stage.</summary>
public sealed record Tag(int Id, double X, double Y, string Name, (byte R, byte G, byte B) Color, string Tool, string Text, Stage Stage, bool Hot,
    List<(byte R, byte G, byte B)> Helpers);

public sealed partial class OfficeModel
{
    sealed record Times(uint HemiSky, uint HemiGround, double HemiK, uint SunC, double SunK, uint FillC, double FillK, double Lamp, double Exposure,
        uint PatchC, double PatchK, uint BeamC, double BeamK, double Dust, uint Shade);

    static readonly Times NightT = new(0x8a78b8, 0x2a1812, 1.05, 0x8fa2ff, 0.6, 0xffc8a0, 0.5, 3.4, 1.3, 0x6f86ff, 0.1, 0x6f86ff, 0.05, 0.0, 0xffc27a);
    static readonly Times DayT = new(0xfff1de, 0x6a4a3a, 1.5, 0xffdcaa, 3.2, 0xfff0e0, 0.9, 0.0, 1.0, 0xffc070, 0.42, 0xffd79a, 0.13, 0.8, 0x8a7a66);

    /// <summary>The props' hit boxes: centre and size.</summary>
    public static readonly double[][] Props =
    [
        [5.4, 2.38, Layout.Z0 + 0.2, 2.6, 1.55, 0.35],
        [Layout.X0 + 0.2, 2.33, -0.7, 0.35, 1.75, 3.0],
        [Layout.X0 + 0.2, 2.72, 1.8, 0.35, 0.62, 1.2],
        [1.5, 2.3, Layout.Z0 + 0.2, 2.9, 2.1, 0.35],
        [Layout.Door.X, 1.2, Layout.Z0 + 0.3, 1.3, 2.45, 0.5],
        [Layout.X0 + 0.25, 1.21, -3.53, 0.5, 2.42, 1.6],
    ];

    readonly record struct Leaver(Bot B, int Desk);

    public readonly Graph G;
    public Room Room;
    readonly Rng r;
    readonly ITextRaster text;
    public readonly List<Session> Sessions = [];
    readonly List<Leaver> leaving = [];
    /// <summary>Bots that walked out, hidden, for the next session with their name.</summary>
    readonly List<Bot> spare = [];
    /// <summary>The helpers (subagents) at the desks, and the sheets they hand in.</summary>
    public readonly Crew Crew = new();
    bool firstState = true;
    public TimeOfDay Time = TimeOfDay.Night;
    public TimeOfDay? ManualTime;
    public double W, H;
    double aspect;
    public double[] Cam = [0, 1.7, 0, 1];
    double[] camTo = [0, 1.7, 0, 1];
    public double[] User = [0, 0, 1];
    /// <summary>The drawer's open session: the camera aims there.</summary>
    public int Sel;
    public bool HasSel, DrawerOpen;
    /// <summary>The session whose desk card or desk panel is open: its bot shows as hot.</summary>
    public int DeskOpen;
    public bool HasDeskOpen;
    public bool Viewing;
    /// <summary>A panel open over the office ("board", "tv", "history"): the camera aims there.</summary>
    public string Panel = "";
    public bool Dragging;
    public (double X, double Y) Pointer;
    public bool HasPointer;
    public Hover Hovered;
    /// <summary>Animations are off: the room only changes where something asked it to, and the bots move without easing.</summary>
    public bool Still;
    public double ClockT;
    long tvAt = -1, clockAt = -1;
    public bool Lively = true;
    double poked, nowMs, shadowAt = -1;
    public bool ShadowDirty = true;
    /// <summary>Which canvases changed since the renderer last took them.</summary>
    public readonly bool[] Dirty = [true, true, true, true, true, true];
    public readonly Canvas[] Canvases;
    public ulong Frames;
    double acc;
    /// <summary>The local clock (ms since the epoch, and the offset in minutes) for the LED clock.</summary>
    public Func<(double Ms, int OffsetMinutes)> WallClock = () =>
    {
        var now = DateTimeOffset.Now;
        return (now.ToUnixTimeMilliseconds(), (int)now.Offset.TotalMinutes);
    };

    static readonly V3 Right = new(Math.Sqrt(2) / 2, 0, -Math.Sqrt(2) / 2);
    static readonly V3 Fwd = new(-Math.Sqrt(2) / 2, 0, -Math.Sqrt(2) / 2);

    static V3 Iso() => new V3(1, 0.86, 1).Norm() * 40;

    public OfficeModel(double w, double h, bool still, ITextRaster? text = null)
    {
        G = new Graph();
        r = new Rng(11);
        Room = RoomBuilder.Build(G, r);
        for (var i = 0; i < Props.Length; i++)
        {
            var at = Props[i];
            var n = G.Add(Graph.Root, new V3(at[0], at[1], at[2]));
            G.Nodes[n].S = new V3(at[3], at[4], at[5]);
            G.Nodes[n].Hit = new Hit(HitKind.Prop, i);
        }
        // A desk's hit box (main.js: box(scene, 0.78, 1.36, 1.66, d.x + 0.08, 0.68, d.z, hitMat)).
        for (var i = 0; i < Layout.Desks.Length; i++)
        {
            var d = Layout.Desks[i];
            var n = G.Add(Graph.Root, new V3(d.X + 0.08, 0.68, d.Z));
            G.Nodes[n].S = new V3(0.78, 1.36, 1.66);
            G.Nodes[n].Hit = new Hit(HitKind.Desk, i);
        }
        this.text = text ?? NoText.Instance;
        W = w;
        H = h;
        aspect = w / h;
        Still = still;
        Canvases = [new(128, 96, this.text), new(208, 118, this.text), new(480, 280, this.text), new(96, 44, this.text), new(4, 64, this.text), new(64, 64, this.text)];
        DrawTextures();
    }

    public void Resize(double w, double h)
    {
        W = w;
        H = h;
        aspect = w / h;
    }

    double HalfWidth(double zoom) => Math.Max(6.2 * aspect, 9.2) / zoom;

    // ---- time of day --------------------------------------------------------------------

    public static TimeOfDay AutoTime(int hour) => hour is >= 7 and < 19 ? TimeOfDay.Day : TimeOfDay.Night;

    public void ApplyTime(TimeOfDay t)
    {
        Time = t;
        var tt = Times_();
        foreach (var s in Room.Shades.Append(Room.FloorShade))
        {
            ref var m = ref G.MatMut(s);
            if (m.Kind == MatKind.Basic) m.Color = Rgb.Hex(tt.Shade);
        }
        ref var patch = ref G.MatMut(Room.Patch);
        if (patch.Kind == MatKind.Basic)
        {
            patch.Color = Rgb.Hex(tt.PatchC);
            patch.Opacity = tt.PatchK;
        }
        ref var beam = ref G.MatMut(Room.Beam);
        if (beam.Kind == MatKind.Basic)
        {
            beam.Color = Rgb.Hex(tt.BeamC);
            beam.Opacity = tt.BeamK;
        }
        ref var dust = ref G.MatMut(Room.Dust);
        if (dust.Kind == MatKind.Points) dust.Opacity = tt.Dust;
        G.Nodes[Room.Dust].Visible = tt.Dust > 0;
        DrawSky();
        DrawTV(0);
        ShadowDirty = true;
    }

    Times Times_() => Time == TimeOfDay.Day ? DayT : NightT;

    public Lights Lights()
    {
        var t = Times_();
        var sunPos = new V3(-1.5, 10, -12);
        var target = new V3(1.5, 0, 1.5);
        var l = new Lights
        {
            HemiSky = Rgb.Hex(t.HemiSky).Mul(t.HemiK),
            HemiGround = Rgb.Hex(t.HemiGround).Mul(t.HemiK),
            SunDir = (sunPos - target).Norm(),
            Sun = Rgb.Hex(t.SunC).Mul(t.SunK),
            SunView = M4.LookAt(sunPos, target, new V3(0, 1, 0)).RigidInverse(),
            FillDir = new V3(8, 6, 10).Norm(),
            Fill = Rgb.Hex(t.FillC).Mul(t.FillK),
            Exposure = t.Exposure,
        };
        foreach (var lamp in Room.Lamps) l.Points.Add(new PointLight(lamp.P, Rgb.Hex(0xffa860).Mul(t.Lamp), 4.2, 1.6));
        l.Points.Add(new PointLight(new V3(-6.5, 1.6, 2.8), Rgb.Hex(0xffa860).Mul(t.Lamp * 0.9), 5.0, 1.5));
        return l;
    }

    // ---- sessions from Hover's state ----------------------------------------------------

    static List<(double X, double Z)> PathIn(int d) =>
        [(Layout.Door.X, 0.25), (Layout.Seat(d) + 0.05, 0.25), (Layout.Seat(d) + 0.05, Layout.Desks[d].Z)];

    static List<(double X, double Z)> PathOut(int d) =>
        [(Layout.Seat(d) + 0.05, 0.25), (Layout.Door.X, 0.25), (Layout.Door.X, Layout.Z0 + 0.1)];

    static int PosMod(int a, int n) => (a % n + n) % n;

    /// <summary>fromHost: sessions added (walking in when new after the first state), updated, retired.</summary>
    public void State(IReadOnlyList<SessionInput>? list)
    {
        if (list is null) return;
        var seen = new List<int>();
        foreach (var h in list)
        {
            seen.Add(h.Id);
            var (now, _) = WallClock();
            var turns = new List<Turn>();
            foreach (var t in h.Turns)
            {
                var tt = new Turn
                {
                    Prompt = t.Prompt,
                    Stage = StageExt.Parse(t.Stage),
                    Steps = t.Steps.Count,
                    Queued = t.Queued,
                    T0 = t.T0 is { } v && v != 0 ? v : now,
                };
                if (t.Took is { } took)
                {
                    tt.Took = took;
                    tt.HasTook = true;
                }
                // The chat's own rule: a subagent's row (k "agent") is out until it is completed or failed.
                foreach (var x in t.Steps)
                    if (x.K == "agent" && x.Status != "completed" && x.Status != "failed") tt.Agents++;
                turns.Add(tt);
            }
            if (turns.Count == 0) continue;
            var act = h.Act ?? "";
            var hasAct = h.Act is not null;
            var pose = h.Pose ?? "";
            var hasPose = h.Pose is not null;
            var ctx = h.Ctx ?? 0;
            var hasCtx = h.Ctx is not null;
            var ex = Sessions.FirstOrDefault(x => x.Id == h.Id);
            if (ex is not null)
            {
                var wasLen = ex.Turns.Count;
                var waking = ex.Last().Stage == Stage.Waking;
                if (waking && wasLen != turns.Count && ex.B.Seated) ex.B.SinceSeat = 0;
                ex.Turns = turns;
                ex.Title = h.Title;
                ex.Folder = h.Folder;
                ex.Ctx = ctx;
                ex.HasCtx = hasCtx;
                ex.Act = act;
                ex.HasAct = hasAct;
                ex.Pose = pose;
                ex.HasPose = hasPose;
                ex.File = h.File;
                continue;
            }
            // The protocol is untrusted: a seat or bot out of range still lands on a desk and a bot.
            var bot = PosMod(h.Bot, Bot.Defs.Length);
            var desk = PosMod(h.Seat, Layout.Desks.Length);
            var def = Bot.Defs[bot];
            var index = Sessions.Count;
            Bot b;
            var si = spare.FindIndex(x => x.Name == def.Name);
            if (si >= 0)
            {
                b = spare[si];
                spare[si] = spare[^1];
                spare.RemoveAt(spare.Count - 1);
                b = b.Renew(G, r, index);
            }
            else b = new Bot(G, r, def.Name, def.Color, index);
            var last = turns[0];
            for (var i = turns.Count - 1; i >= 0; i--)
                if (!turns[i].Queued) { last = turns[i]; break; }
            var walkIn = !firstState && last.Stage == Stage.Waking;
            if (walkIn)
            {
                b.Place(Layout.Door.X, Layout.Z0 + 0.1, false);
                b.Go(PathIn(desk), Arrive.Sit);
            }
            else b.Place(Layout.Seat(desk) + 0.05, Layout.Desks[desk].Z, true);
            var sess = new Session
            {
                Id = h.Id, Key = h.Key, Tool = h.Tool, Bot = bot, Desk = desk, Title = h.Title, Folder = h.Folder, Ctx = ctx, HasCtx = hasCtx,
                Turns = turns, Act = act, HasAct = hasAct, Pose = pose, HasPose = hasPose, File = h.File, B = b,
            };
            var (p, hasP) = sess.PoseOf();
            sess.B.Sync(sess.Last().Stage, p, hasP);
            Sessions.Add(sess);
        }
        var gone = Sessions.Where(s => !seen.Contains(s.Id)).Select(s => s.Id).ToList();
        foreach (var id in gone) Retire(id);
        firstState = false;
        DrawBoard();
        Poke();
    }

    /// <summary>The bot walks out and is gone; its desk is free once it has left.</summary>
    void Retire(int id)
    {
        var i = Sessions.FindIndex(s => s.Id == id);
        if (i < 0) return;
        var s = Sessions[i];
        Sessions.RemoveAt(i);
        var b = s.B;
        b.Sync(Stage.Done, "", false);
        b.Hot = false;
        b.Go(PathOut(s.Desk), Arrive.Gone);
        leaving.Add(new Leaver(b, s.Desk));
        Crew.Leave(id);
        if (HasSel && Sel == id)
        {
            DrawerOpen = false;
            HasSel = false;
        }
    }

    public void Poke() => poked = nowMs;

    // ---- the camera and the user's view -------------------------------------------------

    /// <summary>The view and projection matrices.</summary>
    public (M4 View, M4 Proj) Camera()
    {
        var t = new V3(Cam[0], Cam[1], Cam[2]);
        var world = M4.LookAt(t + Iso(), t, new V3(0, 1, 0));
        var w = HalfWidth(Cam[3]);
        var h = w / aspect;
        return (world.RigidInverse(), M4.Ortho(-w, w, h, -h, 1, 90));
    }

    /// <summary>Turns a move on screen (px, y up) into a move over the floor.</summary>
    (double, double) OverFloor(double dx, double dy, double zoom)
    {
        var w = 2 * HalfWidth(zoom) / W;
        var iso = Iso();
        var sinE = iso.Y / iso.Len;
        return ((Right.X * dx + Fwd.X * dy / sinE) * w, (Right.Z * dx + Fwd.Z * dy / sinE) * w);
    }

    public void ZoomBy(double k, double dx, double dy)
    {
        var old = User[2];
        var nz = Js.Clamp(old * k, 0.85, 2.8);
        if (nz == old) return;
        var (ax, ay) = OverFloor(dx, dy, old);
        var (bx, by) = OverFloor(dx, dy, nz);
        User[0] += ax - bx;
        User[1] += ay - by;
        User[2] = nz;
        ClampView();
    }

    public void ClampView()
    {
        User[0] = Js.Clamp(User[0], -6, 6);
        User[1] = Js.Clamp(User[1], -5, 5);
    }

    public void ResetView() => User = [0, 0, 1];

    /// <summary>Moves the view by (dx, dy) screen px: the floor follows the pointer.</summary>
    public void Drag(double dx, double dy)
    {
        var (gx, gy) = OverFloor(dx, -dy, User[2]);
        User[0] -= gx;
        User[1] -= gy;
        ClampView();
    }

    // ---- picking ------------------------------------------------------------------------

    /// <summary>Raycaster.setFromCamera for an orthographic camera, then the nearest hit box.</summary>
    public void Pick()
    {
        Hover hit = default;
        if (HasPointer && !Dragging) hit = Ray(Pointer.X, Pointer.Y);
        Hovered = hit;
        foreach (var s in Sessions)
            s.B.Hot = (hit.Kind == HoverKind.Bot && hit.Id == s.Id) || (hit.Kind == HoverKind.Desk && hit.Id == s.Id) ||
                (DrawerOpen && HasSel && s.Id == Sel) || (HasDeskOpen && DeskOpen == s.Id);
        int[] panes = [Room.TV, Room.Board, Room.Clock, Room.Sky];
        Prop[] props = [Prop.TV, Prop.Board, Prop.Clock, Prop.Window];
        for (var k = 0; k < 4; k++)
        {
            var on = hit.Kind == HoverKind.Prop && hit.Prop == props[k];
            ref var m = ref G.MatMut(panes[k]);
            if (m.Kind == MatKind.Basic)
            {
                var v = on ? 1.35 : 1.0;
                m.Color = new Rgb(v, v, v);
            }
        }
    }

    /// <summary>Where a ray (origin o, direction d) enters the box lo..hi, if it meets it.</summary>
    static (double T, bool Hit) Slab(V3 o, V3 d, V3 lo, V3 hi)
    {
        double t0 = double.NegativeInfinity, t1 = double.PositiveInfinity;
        double[][] axes = [[o.X, d.X, lo.X, hi.X], [o.Y, d.Y, lo.Y, hi.Y], [o.Z, d.Z, lo.Z, hi.Z]];
        foreach (var a in axes)
        {
            double oo = a[0], dd = a[1], l = a[2], h = a[3];
            if (Math.Abs(dd) < 1e-12)
            {
                if (oo < l || oo > h) t1 = -1;
                continue;
            }
            double u = (l - oo) / dd, v = (h - oo) / dd;
            t0 = Math.Max(t0, Math.Min(u, v));
            t1 = Math.Min(t1, Math.Max(u, v));
        }
        return (t0, t1 >= Math.Max(t0, 0));
    }

    Hover Ray(double px, double py)
    {
        var (view, proj) = Camera();
        var inv = (proj * view).Inverse();
        double nx = px / W * 2 - 1, ny = -(py / H) * 2 + 1;
        var a = inv.Point(new V3(nx, ny, 0));
        var b = inv.Point(new V3(nx, ny, 1));
        var dir = (b - a).Norm();
        var world = G.World();
        var shown = G.Shown();
        // Every hit box the ray goes through, with where it enters (Raycaster's `found`).
        var fs = new List<(double T, Hover H)>();
        var lo = new V3(-0.5, -0.5, -0.5);
        var hi = new V3(0.5, 0.5, 0.5);
        for (var i = 0; i < G.Nodes.Count; i++)
        {
            var n = G.Nodes[i];
            if (n.Hit is null || !shown[i]) continue;
            var m = world[i].Inverse();
            var (t0, ok) = Slab(m.Point(a), m.Dir(dir), lo, hi);
            if (!ok) continue;
            Hover hv;
            switch (n.Hit.Kind)
            {
                case HitKind.Prop:
                    hv = new Hover(HoverKind.Prop, 0, (Prop)n.Hit.I);
                    break;
                case HitKind.Desk:
                    // Only a desk with a session at it.
                    var ds = Sessions.FirstOrDefault(x => x.Desk == n.Hit.I);
                    if (ds is null) continue;
                    hv = new Hover(HoverKind.Desk, ds.Id);
                    break;
                default:
                    var bs = Sessions.FirstOrDefault(x => x.B.Hit == i);
                    if (bs is null) continue;
                    hv = new Hover(HoverKind.Bot, bs.Id);
                    break;
            }
            fs.Add((t0, hv));
        }
        fs = [.. fs.OrderBy(f => f.T)];
        int? bot = null, desk = null;
        foreach (var f in fs)
        {
            if (f.H.Kind == HoverKind.Bot && bot is null) bot = f.H.Id;
            if (f.H.Kind == HoverKind.Desk && desk is null) desk = f.H.Id;
        }
        var first = fs.Count > 0 ? fs[0].H : default;
        var firstProp = first.Kind == HoverKind.Prop;
        // Both: whichever surface is really under the pointer, the bot's body or the room's. Seen from
        // the camera a seated bot is behind its monitor, so its hit box and the desk's overlap, and the
        // nearer box isn't what the pointer is on.
        if (bot is { } bid && desk is { } did)
        {
            var s = Sessions.FirstOrDefault(x => x.Id == bid);
            if (s is null) return default;
            return BodyInFront(s, a, dir, world, shown) ? new Hover(HoverKind.Bot, bid) : new Hover(HoverKind.Desk, did);
        }
        if (bot is { } b2 && !firstProp) return new Hover(HoverKind.Bot, b2);
        if (firstProp) return first;
        if (desk is { } d2) return new Hover(HoverKind.Desk, d2);
        return first;
    }

    /// <summary>Whether the ray meets the bot's own boxes before the room's (its desk, chair and the rest of the room's merged boxes): main.js botParts against roomMesh.</summary>
    bool BodyInFront(Session s, V3 a, V3 dir, M4[] world, bool[] shown)
    {
        double body = double.PositiveInfinity, room = double.PositiveInfinity;
        var lo = new V3(-0.5, -0.5, -0.5);
        var hi = new V3(0.5, 0.5, 0.5);
        for (var i = 0; i < G.Nodes.Count; i++)
        {
            var n = G.Nodes[i];
            if (n.Draw is null || !shown[i]) continue;
            var m = world[i].Inverse();
            V3 ro = m.Point(a), rd = m.Dir(dir);
            switch (n.Draw.Geo.Kind)
            {
                case GeoKind.Unit:
                    if (Under(i, s.B.Root))
                    {
                        var (t, ok) = Slab(ro, rd, lo, hi);
                        if (ok) body = Math.Min(body, t);
                    }
                    break;
                case GeoKind.Merged:
                    foreach (var b in G.Merged[n.Draw.Geo.Index])
                    {
                        var (t, ok) = Slab(ro, rd, new V3(b.X, b.Y, b.Z), new V3(b.X + b.W, b.Y + b.H, b.Z + b.D));
                        if (ok) room = Math.Min(room, t);
                    }
                    break;
            }
        }
        return body < room;
    }

    /// <summary>Whether node i is root or inside it.</summary>
    bool Under(int i, int root)
    {
        while (true)
        {
            if (i == root) return true;
            var p = G.Nodes[i].Parent;
            if (p < 0) return false;
            i = p;
        }
    }

    /// <summary>pointerup without a drag: what the page does.</summary>
    public Click Click()
    {
        var h = Hovered;
        switch (h.Kind)
        {
            case HoverKind.Bot:
                return new Click(ClickKind.Open, h.Id);
            case HoverKind.Desk:
                return new Click(ClickKind.Desk, h.Id, Pointer.X, Pointer.Y);
            case HoverKind.Prop:
                switch (h.Prop)
                {
                    case Prop.TV: return new Click(ClickKind.Panel, Panel: "tv");
                    case Prop.Board: return new Click(ClickKind.Panel, Panel: "board");
                    case Prop.Shelf: return new Click(ClickKind.Panel, Panel: "history");
                    case Prop.Door: return new Click(ClickKind.NewTask);
                    case Prop.Clock: return new Click(ClickKind.Toast);
                    case Prop.Window: return new Click(ClickKind.Time, Time: Time == TimeOfDay.Day ? TimeOfDay.Night : TimeOfDay.Day);
                }
                break;
        }
        return new Click(ClickKind.Nothing);
    }

    /// <summary>The prop's tooltip ("Office overview", "Make it day"…); the clock's is the date.</summary>
    public string Hint(Prop p) => p switch
    {
        Prop.TV => "Office overview",
        Prop.Board => "Session board",
        Prop.Door => "New task",
        Prop.Shelf => "Session history",
        Prop.Window => Time == TimeOfDay.Day ? "Make it night" : "Make it day",
        _ => "",
    };

    // ---- the frame ----------------------------------------------------------------------

    /// <summary>bubbleFor: what the bot says over its head.</summary>
    public static string Bubble(Session s)
    {
        var t = s.Last();
        if (s.B.Walking) return "On my way…";
        switch (t.Stage)
        {
            case Stage.Waking:
                return "Waking up…";
            case Stage.Working:
                if (s.HasAct && s.Act == "Thinking") return "Thinking…";
                if (s.HasAct && s.Act == "Writing") return "Writing it up…";
                if (s.File != "") return s.Act + " " + Short(s.File);
                if (s.HasAct) return s.Act + "…";
                return "Working…";
            case Stage.Done:
                return s.B.Since < 6 ? "Done! ✓" : "";
            case Stage.Failed:
                return "Couldn’t finish";
            case Stage.Stopped:
                return "z z z";
        }
        // The question shows over the head in place of the bubble.
        return "";
    }

    static int RuneCount(string s) => s.EnumerateRunes().Count();

    static string TakeRunes(string s, int n) => string.Concat(s.EnumerateRunes().Take(n).Select(x => x.ToString()));

    /// <summary>
    /// One animation frame at nowMs. False when the page's pacing skips it (60 fps while lively, 10 idle, 1
    /// with reduced motion), so nothing needs drawing.
    /// </summary>
    public bool Frame(double nowMsArg, double dtMs)
    {
        nowMs = nowMsArg;
        var dt = Js.Clamp(dtMs / 1000, 0, 0.1);
        acc += dt;
        // Full speed while something happens (a bot walks or works, the camera moves) or the user did
        // something in the last 1.5 s; otherwise the room only idles, at 10 fps, or 1 with animations
        // off (main.js's pacing).
        var need = 1.0 / 60;
        if (!Lively && nowMs - poked > 1500)
        {
            need = 1.0 / 10;
            if (Still) need = 1;
        }
        if (acc < need) return false;
        var step = acc;
        acc = 0;
        ClockT += step;
        var still = Still;
        foreach (var s in Sessions)
        {
            var (p, hasP) = s.PoseOf();
            s.B.Sync(s.Last().Stage, p, hasP);
            s.B.Step(G, step, still);
        }
        var gone = new List<int>();
        for (var i = 0; i < leaving.Count; i++)
            if (leaving[i].B.Step(G, step, still) == Arrive.Gone)
            {
                leaving[i].B.Dispose(G);
                gone.Add(i);
            }
        // Gone: kept hidden, and used again for the next session with its name.
        for (var k = gone.Count - 1; k >= 0; k--)
        {
            var i = gone[k];
            var b = leaving[i].B;
            leaving.RemoveAt(i);
            spare.Add(b);
        }
        // The helpers: as many at each busy desk as its session has subagents out.
        foreach (var s in Sessions) Crew.Sync(G, r, s.Id, s.Desk, s.B.Color, s.SubagentsOut());
        Crew.Step(G, step, still, id =>
        {
            var s = Sessions.FirstOrDefault(x => x.Id == id);
            return s is null ? null : (s.B.X, s.B.Z);
        });
        // The door swings open while a bot is near it.
        var near = false;
        foreach (var s in Sessions) near |= Hypot(s.B.X - Layout.Door.X, s.B.Z - Layout.Z0) < 1.5;
        foreach (var l in leaving) near |= Hypot(l.B.X - Layout.Door.X, l.B.Z - Layout.Z0) < 1.5;
        var doorTo = near ? -1.3 : 0.0;
        G.Nodes[Room.Door].R = G.Nodes[Room.Door].R with { Y = Js.Ease(G.Nodes[Room.Door].R.Y, doorTo, 6, step) };
        // Screen light on each bot's face, by stage.
        for (var i = 0; i < Layout.Desks.Length; i++)
        {
            var s = Sessions.FirstOrDefault(x => x.Desk == i && x.B.Seated && !x.B.Walking);
            uint c = 0;
            var op = 0.0;
            if (s is not null) (c, op) = s.Last().Stage.ScreenLight();
            var running = s is not null && s.HasAct && s.Act == "Running" && Math.Sin(ClockT * 11) > 0;
            ref var m = ref G.MatMut(Room.DeskGlows[i]);
            if (m.Kind == MatKind.Glow)
            {
                m.Color = Rgb.Hex(c);
                var k = running ? 1.3 : 1.0;
                m.Opacity = Js.Ease(m.Opacity, op * k, 8, step);
            }
        }
        if (!still)
        {
            var a = ClockT * 0.21;
            G.Nodes[Room.Vac].P = new V3(0.6 + Math.Sin(a) * 3, 0, 4.3 + Math.Sin(a * 2.3) * 0.7);
            G.Nodes[Room.Vac].R = G.Nodes[Room.Vac].R with { Y = Math.Atan2(Math.Cos(a) * 3, Math.Cos(a * 2.3) * 0.7 * 2.3) };
            for (var i = 0; i < Room.Steam.Length; i++)
            {
                var n = Room.Steam[i];
                var f = (ClockT * 0.5 + i / 3.0) % 1;
                G.Nodes[n].P = new V3(-4.3 + Math.Sin(f * 6 + i) * 0.04, 1.5 + f * 0.6, Layout.Z0 + 0.3);
                var sc = 0.15 + f * 0.25;
                G.Nodes[n].S = new V3(sc, sc, sc);
                ref var m = ref G.MatMut(n);
                if (m.Kind == MatKind.Glow) m.Opacity = 0.3 * (1 - f);
            }
            G.Nodes[Room.Dust].R = G.Nodes[Room.Dust].R with { Y = Math.Sin(ClockT * 0.1) * 0.02 };
            G.Nodes[Room.Dust].P = G.Nodes[Room.Dust].P with { Y = Math.Sin(ClockT * 0.4) * 0.05 };
            ref var eg = ref G.MatMut(Room.ExitGlow);
            if (eg.Kind == MatKind.Glow) eg.Opacity = 0.5 + Math.Sin(ClockT * 2) * 0.05;
        }
        // Camera: the user's view; or close on the open session's bot, or on a panel's prop.
        var side = 0.0;
        if (W >= 700) side = Math.Min(424, W * 0.42) / 2;
        double[]? focus = null;
        if (DrawerOpen && !Viewing && HasSel)
            foreach (var s in Sessions)
                if (s.Id == Sel)
                    focus = [s.B.X, 0.9, s.B.Z, W < 700 ? 1.3 : 1.45];
        if (focus is null && Panel != "")
            focus = Panel switch
            {
                "board" => [Layout.X0 + 1.2, 2.1, -0.7, 2.6],
                "tv" => [5.2, 1.6, Layout.Z0 + 1.6, 1.35],
                _ => [Layout.X0 + 1.4, 1.4, -3.5, 1.35],
            };
        if (focus is not null)
        {
            var off = side * 2 * HalfWidth(focus[3]) / W;
            camTo = [focus[0] + Right.X * off, focus[1], focus[2] + Right.Z * off, focus[3]];
        }
        else camTo = [User[0], 1.7, User[1], User[2]];
        var ck = still || Dragging ? 60.0 : 5.0;
        for (var i = 0; i < 4; i++) Cam[i] = Js.Ease(Cam[i], camTo[i], ck, step);
        Pick();
        // Bubbles type out new text, 45 characters a second.
        foreach (var s in Sessions)
        {
            var want = Bubble(s);
            if (want != s.TagText)
            {
                s.TagText = want;
                s.TagShown = 0;
            }
            s.TagShown += step * 45;
        }
        var tv = Js.FloorI(ClockT * 4);
        if (tv != tvAt)
        {
            tvAt = tv;
            DrawTV(ClockT);
        }
        var ck2 = Js.FloorI(ClockT * 2);
        if (ck2 != clockAt)
        {
            clockAt = ck2;
            DrawClock();
        }
        var walking = leaving.Count > 0 || Crew.Moving;
        foreach (var s in Sessions) walking |= s.B.Walking;
        if (walking || ShadowDirty || ClockT - shadowAt >= 0.1)
        {
            shadowAt = ClockT;
            ShadowDirty = true;
        }
        var doorMoving = Math.Abs(G.Nodes[Room.Door].R.Y - doorTo) > 0.01;
        var lively = walking || Dragging || doorMoving || Crew.Any;
        foreach (var s in Sessions) lively |= s.Busy || s.B.Since < 2 || s.TagShown < RuneCount(s.TagText);
        for (var i = 0; i < 4; i++) lively |= Math.Abs(Cam[i] - camTo[i]) > 0.002;
        Lively = lively;
        Frames++;
        return true;
    }

    static double Hypot(double a, double b) => Math.Sqrt(a * a + b * b);

    public List<Tag> Tags()
    {
        var (view, proj) = Camera();
        var vp = proj * view;
        var outp = new List<Tag>();
        foreach (var s in Sessions)
        {
            var p = vp.Point(s.B.Head3(G));
            var words = Still ? s.TagText : TakeRunes(s.TagText, (int)s.TagShown);
            outp.Add(new Tag(s.Id, (p.X + 1) / 2 * W, (1 - p.Y) / 2 * H, s.B.Name, s.B.Css, s.Tool, words, s.Last().Stage, s.B.Hot, Crew.Colors(s.Id)));
        }
        return outp;
    }

    /// <summary>short(f): the last part of a path.</summary>
    public static string Short(string f)
    {
        var i = f.LastIndexOfAny(['\\', '/']);
        return i >= 0 ? f[(i + 1)..] : f;
    }

    /// <summary>The camera at the open session's bot, or a panel's prop, or the user's view, without waiting for the easing.</summary>
    public bool Settled() => Enumerable.Range(0, 4).All(i => Math.Abs(Cam[i] - camTo[i]) <= 0.002);
}
