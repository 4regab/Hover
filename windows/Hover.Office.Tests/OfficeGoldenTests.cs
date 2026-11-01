using System.Text.Json;
using Xunit;

namespace Hover.Office.Tests;

/// <summary>
/// The C# office against numbers the Go office printed (internal/office/golden_test.go writes
/// golden/office.json). Each test feeds the file's inputs to the C# code and compares its answers with
/// the file's, so a change in either language shows up here or in the Go test.
/// </summary>
public sealed class OfficeGoldenTests
{
    static readonly JsonElement Golden = Load();

    static JsonElement Load()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "golden", "office.json");
        using var doc = JsonDocument.Parse(File.ReadAllText(file));
        return doc.RootElement.Clone();
    }

    // Go and .NET may differ in the last digits of sin, cos, exp and pow.
    static void Near(double want, double got, string what) =>
        Assert.True(Math.Abs(want - got) <= 1e-6 * Math.Max(1, Math.Abs(want)), $"{what}: Go printed {want}, C# gave {got}");

    static void NearAll(JsonElement want, ReadOnlySpan<double> got, string what)
    {
        Assert.Equal(want.GetArrayLength(), got.Length);
        var i = 0;
        foreach (var w in want.EnumerateArray()) Near(w.GetDouble(), got[i++], $"{what}[{i - 1}]");
    }

    static double[] Of(M4 m)
    {
        var o = new double[16];
        for (var i = 0; i < 16; i++) o[i] = m[i];
        return o;
    }

    [Fact]
    public void The_seeded_generator_gives_the_same_numbers()
    {
        foreach (var row in Golden.GetProperty("rng").EnumerateArray())
        {
            var r = new Rng(row.GetProperty("seed").GetInt32());
            var i = 0;
            foreach (var v in row.GetProperty("values").EnumerateArray()) Near(v.GetDouble(), r.Next(), $"seed {row.GetProperty("seed")} #{i++}");
        }
    }

    [Fact]
    public void The_room_is_built_box_for_box_in_the_pages_order()
    {
        var o = new OfficeModel(1104, 424, false);
        var room = Golden.GetProperty("room");
        Assert.Equal(room.GetProperty("nodes").GetInt32(), o.G.Nodes.Count);
        var merged = room.GetProperty("merged").EnumerateArray().ToList();
        Assert.Equal(merged.Count, o.G.Merged.Count);
        for (var i = 0; i < merged.Count; i++)
        {
            double sum = 0, col = 0;
            foreach (var b in o.G.Merged[i])
            {
                sum += b.X + b.Y + b.Z + b.W + b.H + b.D;
                col += b.C.R + 2 * b.C.G + 3 * b.C.B;
            }
            Assert.Equal(merged[i].GetProperty("boxes").GetInt32(), o.G.Merged[i].Length);
            Near(merged[i].GetProperty("sum").GetDouble(), sum, $"mesh {i} boxes");
            Near(merged[i].GetProperty("color").GetDouble(), col, $"mesh {i} colours");
        }
        var world = o.G.World();
        foreach (var w in room.GetProperty("world").EnumerateArray())
            NearAll(w.GetProperty("m"), Of(world[w.GetProperty("node").GetInt32()]), $"world of node {w.GetProperty("node")}");
        Lights(room.GetProperty("lightsNight"), o.Lights(), "night");
        o.ApplyTime(TimeOfDay.Day);
        Lights(Golden.GetProperty("lightsDay"), o.Lights(), "day");
    }

    static void Lights(JsonElement want, Lights got, string what)
    {
        static double[] C(Rgb c) => [c.R, c.G, c.B];
        static double[] V(V3 v) => [v.X, v.Y, v.Z];
        NearAll(want.GetProperty("hemiSky"), C(got.HemiSky), what + " hemisphere sky");
        NearAll(want.GetProperty("hemiGround"), C(got.HemiGround), what + " hemisphere ground");
        NearAll(want.GetProperty("sunDir"), V(got.SunDir), what + " sun direction");
        NearAll(want.GetProperty("sun"), C(got.Sun), what + " sun");
        NearAll(want.GetProperty("sunView"), Of(got.SunView), what + " sun view");
        NearAll(want.GetProperty("fillDir"), V(got.FillDir), what + " fill direction");
        NearAll(want.GetProperty("fill"), C(got.Fill), what + " fill");
        Near(want.GetProperty("exposure").GetDouble(), got.Exposure, what + " exposure");
        var pts = want.GetProperty("points").EnumerateArray().ToList();
        Assert.Equal(pts.Count, got.Points.Count);
        for (var i = 0; i < pts.Count; i++)
        {
            NearAll(pts[i].GetProperty("p"), V(got.Points[i].P), $"{what} point {i} place");
            NearAll(pts[i].GetProperty("c"), C(got.Points[i].C), $"{what} point {i} colour");
            Near(pts[i].GetProperty("dist").GetDouble(), got.Points[i].Dist, $"{what} point {i} distance");
            Near(pts[i].GetProperty("decay").GetDouble(), got.Points[i].Decay, $"{what} point {i} decay");
        }
    }

    static void Picture(string name, byte[] got, string what)
    {
        var want = Convert.FromBase64String(Golden.GetProperty("canvases").GetProperty(name).GetString()!);
        Assert.Equal(want.Length, got.Length);
        var worst = 0;
        for (var i = 0; i < want.Length; i++) worst = Math.Max(worst, Math.Abs(want[i] - got[i]));
        Assert.True(worst <= 1, $"{what}: the pictures differ by {worst} levels");
    }

    [Fact]
    public void The_wall_pictures_without_text_are_the_same_bytes()
    {
        var o = new OfficeModel(1104, 424, false);
        Picture("skyNight", o.Canvases[Layout.TexSky].Rgba(), "the night sky");
        Picture("beam", o.Canvases[Layout.TexBeam].Rgba(), "the light beam");
        Picture("patch", o.Canvases[Layout.TexPatch].Rgba(), "the light patch");
        o.ApplyTime(TimeOfDay.Day);
        Picture("skyDay", o.Canvases[Layout.TexSky].Rgba(), "the day sky");
    }

    static void Sessions(JsonElement want, OfficeModel o, string what)
    {
        var list = want.EnumerateArray().ToList();
        Assert.Equal(list.Count, o.Sessions.Count);
        for (var i = 0; i < list.Count; i++)
        {
            var w = list[i];
            var s = o.Sessions[i];
            var at = $"{what} session {i} ({s.B.Name})";
            Assert.Equal(w.GetProperty("id").GetInt32(), s.Id);
            Assert.Equal(w.GetProperty("desk").GetInt32(), s.Desk);
            Assert.Equal(w.GetProperty("bot").GetInt32(), s.Bot);
            Assert.Equal(w.GetProperty("name").GetString(), s.B.Name);
            Near(w.GetProperty("x").GetDouble(), s.B.X, at + " x");
            Near(w.GetProperty("z").GetDouble(), s.B.Z, at + " z");
            Near(w.GetProperty("yaw").GetDouble(), s.B.Yaw, at + " yaw");
            Near(w.GetProperty("rootY").GetDouble(), o.G.Nodes[s.B.Root].P.Y, at + " height");
            Assert.Equal(w.GetProperty("seated").GetBoolean(), s.B.Seated);
            Assert.Equal(w.GetProperty("walking").GetBoolean(), s.B.Walking);
            Assert.Equal(w.GetProperty("stage").GetInt32(), (int)s.Last().Stage);
            Assert.Equal(w.GetProperty("bubble").GetString(), OfficeModel.Bubble(s));
            Assert.Equal(w.GetProperty("hot").GetBoolean(), s.B.Hot);
            Assert.Equal(w.GetProperty("agents").GetInt32(), s.Last().Agents);
            Assert.Equal(w.GetProperty("out").GetInt32(), s.SubagentsOut());
        }
    }

    static void Tags(JsonElement want, OfficeModel o, string what)
    {
        var list = want.EnumerateArray().ToList();
        var got = o.Tags();
        Assert.Equal(list.Count, got.Count);
        for (var i = 0; i < list.Count; i++)
        {
            Assert.Equal(list[i].GetProperty("id").GetInt32(), got[i].Id);
            Near(list[i].GetProperty("x").GetDouble(), got[i].X, $"{what} tag {i} x");
            Near(list[i].GetProperty("y").GetDouble(), got[i].Y, $"{what} tag {i} y");
            Assert.Equal(list[i].GetProperty("text").GetString(), got[i].Text);
            Assert.Equal(list[i].GetProperty("name").GetString(), got[i].Name);
            Assert.Equal(list[i].GetProperty("stage").GetInt32(), (int)got[i].Stage);
            Assert.Equal(list[i].GetProperty("hot").GetBoolean(), got[i].Hot);
            Assert.Equal(list[i].GetProperty("helpers").GetInt32(), got[i].Helpers.Count);
        }
    }

    static void Camera(JsonElement want, OfficeModel o, string what)
    {
        NearAll(want.GetProperty("cam"), o.Cam, what + " camera");
        NearAll(want.GetProperty("user"), o.User, what + " user view");
        var (v, p) = o.Camera();
        NearAll(want.GetProperty("view"), Of(v), what + " view matrix");
        NearAll(want.GetProperty("proj"), Of(p), what + " projection matrix");
    }

    [Fact]
    public void The_fixture_sessions_settle_at_their_desks_with_the_same_camera_tags_and_picks()
    {
        var (o, _, _) = Fixture.Settled();
        var want = Golden.GetProperty("settled");
        Sessions(want.GetProperty("sessions"), o, "settled");
        Tags(want.GetProperty("tags"), o, "settled");
        Camera(want.GetProperty("camera"), o, "settled");
        Assert.Equal(want.GetProperty("frames").GetUInt64(), o.Frames);
        var world = o.G.World();
        foreach (var w in want.GetProperty("world").EnumerateArray())
            NearAll(w.GetProperty("m"), Of(world[w.GetProperty("node").GetInt32()]), $"world of node {w.GetProperty("node")}");
        foreach (var row in want.GetProperty("pick").EnumerateArray())
        {
            o.Pointer = (row.GetProperty("x").GetDouble(), row.GetProperty("y").GetDouble());
            o.HasPointer = true;
            o.Pick();
            var h = row.GetProperty("hover");
            Assert.True(
                h.GetProperty("kind").GetInt32() == (int)o.Hovered.Kind && h.GetProperty("id").GetInt32() == o.Hovered.Id && h.GetProperty("prop").GetInt32() == (int)o.Hovered.Prop,
                $"at {o.Pointer}: Go has {h}, C# has {o.Hovered}");
        }
    }

    [Fact]
    public void Zoom_drag_and_the_limits_move_the_view_as_they_do_in_Go()
    {
        var (o, _, _) = Fixture.Settled();
        var want = Golden.GetProperty("settled").GetProperty("view");
        o.ZoomBy(1.25, 100, -40);
        NearAll(want.GetProperty("zoomed"), o.User, "zoomed about a point");
        o.Drag(60, -30);
        NearAll(want.GetProperty("dragged"), o.User, "dragged");
        o.ZoomBy(100, 0, 0);
        NearAll(want.GetProperty("zoomMax"), o.User, "zoom limit in");
        o.ZoomBy(0.001, 0, 0);
        NearAll(want.GetProperty("zoomMin"), o.User, "zoom limit out");
        for (var i = 0; i < 5; i++) o.Drag(4000, 4000);
        NearAll(want.GetProperty("clamped"), o.User, "view limits");
        o.ResetView();
        NearAll(want.GetProperty("reset"), o.User, "reset");
    }

    [Fact]
    public void A_new_session_walks_in_through_the_door_and_a_gone_one_walks_out()
    {
        var (o, clock, state) = Fixture.Settled();
        o.State(Fixture.WithNewSession(state, 9));
        var rows = Golden.GetProperty("walk").EnumerateArray().ToList();
        for (var i = 0; i < rows.Count - 1; i++)
        {
            clock.Run(o, rows[i].GetProperty("after").GetInt32());
            Sessions(rows[i].GetProperty("sessions"), o, $"walk after {rows[i].GetProperty("after")}");
        }
        o.State(Fixture.Inputs(state));
        clock.Run(o, 600);
        var last = rows[^1];
        Sessions(last.GetProperty("sessions"), o, "retired");
        Tags(last.GetProperty("tags"), o, "retired");
    }

    [Fact]
    public void Subagents_become_helpers_at_the_desk_that_hop_out_write_and_hand_in_sheets()
    {
        var (o, clock) = Fixture.Fresh();
        o.State(Fixture.WithAgents(Fixture.State(), 0, [("agent", "in_progress"), ("agent", "running"), ("agent", "completed"), ("agent", "in_progress"), ("read", "completed")]));
        foreach (var row in Golden.GetProperty("helpers").EnumerateArray())
        {
            clock.Run(o, row.GetProperty("after").GetInt32());
            var at = $"helpers after {row.GetProperty("after")}";
            var minis = row.GetProperty("minis").EnumerateArray().ToList();
            Assert.Equal(minis.Count, o.Crew.Minis.Count);
            for (var i = 0; i < minis.Count; i++)
            {
                var m = o.Crew.Minis[i];
                Assert.Equal(minis[i].GetProperty("sid").GetInt32(), m.Sid);
                Assert.Equal(minis[i].GetProperty("slot").GetInt32(), m.Slot);
                Assert.Equal(minis[i].GetProperty("leaving").GetBoolean(), m.Leaving);
                Assert.Equal(minis[i].GetProperty("duty").GetInt32(), (int)m.DutyNow);
                Near(minis[i].GetProperty("out").GetDouble(), m.Out, $"{at} helper {i} out");
                var p = o.G.Nodes[m.Root].P;
                NearAll(minis[i].GetProperty("p"), [p.X, p.Y, p.Z], $"{at} helper {i} place");
            }
            Assert.Equal(row.GetProperty("sheets").GetInt32(), o.Crew.SheetsInAir);
            Sessions(row.GetProperty("sessions"), o, at);
            Tags(row.GetProperty("tags"), o, at);
        }
    }

    [Fact]
    public void The_camera_goes_to_an_open_session_a_panel_and_the_day_as_in_Go()
    {
        var (o, clock, _) = Fixture.Settled();
        var rows = Golden.GetProperty("focus").EnumerateArray().ToList();
        o.DrawerOpen = true;
        o.Sel = 2;
        o.HasSel = true;
        clock.Run(o, 300);
        Camera(rows[0].GetProperty("camera"), o, "drawer");
        Tags(rows[0].GetProperty("tags"), o, "drawer");
        o.DrawerOpen = false;
        o.HasSel = false;
        var i = 1;
        foreach (var panel in new[] { "board", "tv", "history" })
        {
            o.Panel = panel;
            clock.Run(o, 300);
            Camera(rows[i++].GetProperty("camera"), o, panel);
        }
        o.Panel = "";
        o.ApplyTime(TimeOfDay.Day);
        clock.Run(o, 300);
        Camera(rows[i].GetProperty("camera"), o, "day");
    }

    [Fact]
    public void A_quiet_room_draws_ten_frames_a_second_and_a_busy_one_sixty()
    {
        var o = new OfficeModel(1104, 424, false);
        var clock = new Fixture.Clock();
        var drew = new List<int>();
        for (var i = 0; i < 600; i++)
        {
            if (o.Frame(clock.Now, 16)) drew.Add(i);
            clock.Now += 16;
        }
        var want = Golden.GetProperty("pacing");
        Assert.Equal(want.GetProperty("quietDrew").GetInt32(), drew.Count);
        Assert.Equal(want.GetProperty("firstFrames").EnumerateArray().Select(x => x.GetInt32()), drew.Take(30));
        var (busy, bc, _) = Fixture.Settled();
        var n = 0;
        for (var i = 0; i < 120; i++)
        {
            if (busy.Frame(bc.Now, 16)) n++;
            bc.Now += 16;
        }
        Assert.Equal(Golden.GetProperty("pacingBusy").GetProperty("drew").GetInt32(), n);
    }
}
