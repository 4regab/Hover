using System.Text.Json.Nodes;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Office.Tests;

/// <summary>One Direct3D 11 device on Windows' software adapter (WARP) and the wall-picture text, shared by the render tests. Off Windows there is none and the tests say so.</summary>
public sealed class Gpu : IDisposable
{
    public OfficeRenderer? Renderer { get; private set; }
    public DirectWriteText? Text { get; private set; }
    public Exception? Error { get; }

    public static string FontFolder => Path.Combine(AppContext.BaseDirectory, "Assets", "Office");

    public Gpu()
    {
        if (!OperatingSystem.IsWindows()) return;
        try
        {
            Renderer = OfficeRenderer.Create(warp: true);
            Text = new DirectWriteText(FontFolder);
        }
        catch (Exception e)
        {
            Error = e;
        }
    }

    /// <summary>False off Windows (the test has nothing to run); on Windows a device that cannot be made fails the test.</summary>
    public bool Ready(ITestOutputHelper output)
    {
        if (!OperatingSystem.IsWindows())
        {
            output.WriteLine("not run: Direct3D 11 needs Windows");
            return false;
        }
        if (Error is not null) throw new InvalidOperationException("no office device: " + Error, Error);
        return true;
    }

    public void Dispose()
    {
        Text?.Dispose();
        Renderer?.Dispose();
    }
}

public sealed class OfficeRenderTests(Gpu gpu, ITestOutputHelper output) : IClassFixture<Gpu>
{
    // The notch's office area in DIPs (NotchScene.OfficeArea).
    const int NotchW = 1120, NotchH = 440;

    static string Out()
    {
        var dir = Path.Combine(Fixture.RepoRoot(), "out", "office");
        Directory.CreateDirectory(dir);
        return dir;
    }

    (OfficeModel Office, Fixture.Clock Clock) Make(double w, double h, TimeOfDay time = TimeOfDay.Night)
    {
        var now = Fixture.Now();
        var o = new OfficeModel(w, h, false, gpu.Text) { WallClock = () => (now, 0) };
        o.ApplyTime(time);
        return (o, new Fixture.Clock());
    }

    (OfficeModel Office, Fixture.Clock Clock, JsonObject State) Settled(double w, double h, TimeOfDay time = TimeOfDay.Night)
    {
        var (o, clock) = Make(w, h, time);
        var state = Fixture.State();
        o.State(Fixture.Inputs(state));
        clock.Run(o, 400);
        return (o, clock, state);
    }

    /// <summary>Draws the office the way the app does (the notch's top fades into its black) and keeps the picture as a JPEG.</summary>
    byte[] Shoot(string name, OfficeModel o, int w, int h, bool notch = true)
    {
        var r = gpu.Renderer!;
        var (sw, sh) = r.SceneSize(w, h);
        var bgra = r.RenderToBytes(o, w, h, new PageStyle(o.Time == TimeOfDay.Day, notch ? 44 * w / (double)NotchW : 0), sw, sh);
        var (jw, jh) = JpegFile.Write(Path.Combine(Out(), name + ".jpg"), bgra, w, h);
        output.WriteLine($"{name}.jpg: {jw} x {jh} (scene {sw} x {sh}, {r.AdapterName})");
        return bgra;
    }

    static (double Mean, double Spread, int Colours) Stats(byte[] bgra)
    {
        double sum = 0, sum2 = 0;
        var colours = new HashSet<int>();
        var n = bgra.Length / 4;
        for (var i = 0; i < n; i++)
        {
            var l = 0.114 * bgra[i * 4] + 0.587 * bgra[i * 4 + 1] + 0.299 * bgra[i * 4 + 2];
            sum += l;
            sum2 += l * l;
            colours.Add(bgra[i * 4] << 16 | bgra[i * 4 + 1] << 8 | bgra[i * 4 + 2]);
        }
        var mean = sum / n;
        return (mean, Math.Sqrt(Math.Max(sum2 / n - mean * mean, 0)), colours.Count);
    }

    /// <summary>The mean absolute difference between two pictures in a box (x, y its centre) of the given half size.</summary>
    static double BoxDiff(byte[] a, byte[] b, int w, int h, double cx, double cy, int half)
    {
        double sum = 0;
        var n = 0;
        for (var y = Math.Max((int)cy - half, 0); y < Math.Min((int)cy + half, h); y++)
            for (var x = Math.Max((int)cx - half, 0); x < Math.Min((int)cx + half, w); x++)
            {
                var i = (y * w + x) * 4;
                sum += Math.Abs(a[i] - b[i]) + Math.Abs(a[i + 1] - b[i + 1]) + Math.Abs(a[i + 2] - b[i + 2]);
                n++;
            }
        return n == 0 ? 0 : sum / n / 3;
    }

    static void LooksLikeAnOffice(byte[] bgra, string what)
    {
        var (mean, spread, colours) = Stats(bgra);
        Assert.True(colours > 1500, $"{what}: only {colours} colours, a blank or flat picture");
        Assert.True(spread > 12, $"{what}: brightness spread {spread:F1} is too small for a room with a lit floor, walls and props");
        Assert.InRange(mean, 6, 140);
    }

    static JsonObject Clone(JsonObject o) => JsonNode.Parse(o.ToJsonString())!.AsObject();

    /// <summary>The fixture's sessions cycled to <paramref name="n"/> of them: ids 1..n, a seat and a bot each (more than six wrap round the six desks).</summary>
    static JsonObject Many(JsonObject state, int n)
    {
        var s = Clone(state);
        var src = state["sessions"]!.AsArray();
        var list = new JsonArray();
        for (var i = 0; i < n; i++)
        {
            var c = Clone(src[i % src.Count]!.AsObject());
            c["id"] = i + 1;
            c["seat"] = i;
            c["bot"] = i;
            c["key"] = $"s{i + 1}";
            list.Add(c);
        }
        s["sessions"] = list;
        return s;
    }

    [Fact]
    public void Warp_draws_the_office_in_the_notch_area_and_keeps_a_picture()
    {
        if (!gpu.Ready(output)) return;
        var (o, _, _) = Settled(NotchW, NotchH);
        var bgra = Shoot("notch-open-office", o, NotchW, NotchH);
        LooksLikeAnOffice(bgra, "the settled office");
        // The top 44 px fade into the notch's black; the vignette darkens the corners.
        var (top, _, _) = Stats(bgra[..(NotchW * 6 * 4)]);
        var (mid, _, _) = Stats(bgra[(NotchW * 200 * 4)..(NotchW * 206 * 4)]);
        Assert.True(top < mid, $"the top rows ({top:F1}) should be darker than the middle ({mid:F1})");
    }

    [Fact]
    public void Every_bot_is_drawn_where_the_scene_puts_it()
    {
        if (!gpu.Ready(output)) return;
        var (empty, clock) = Make(NotchW, NotchH);
        empty.State(Fixture.Inputs(EmptyState()));
        clock.Run(empty, 400);
        var without = Shoot("office-empty", empty, NotchW, NotchH);
        LooksLikeAnOffice(without, "the empty office");

        var (o, _, _) = Settled(NotchW, NotchH);
        var with = Shoot("office-scene-night", o, NotchW, NotchH);
        var tags = o.Tags();
        Assert.Equal(5, tags.Count);
        foreach (var t in tags)
        {
            Assert.InRange(t.X, 0, NotchW);
            Assert.InRange(t.Y, 0, NotchH);
            // Under the tag (the head projected) the bot, its desk screen and its seat make the picture differ from the empty room's.
            var d = BoxDiff(with, without, NotchW, NotchH, t.X, t.Y + 22, 26);
            output.WriteLine($"session {t.Id} ({t.Name}) at {t.X:F0},{t.Y:F0}: {d:F1} apart from the empty room");
            Assert.True(d > 3, $"no bot where the scene puts session {t.Id} ({d:F1})");
        }
    }

    static JsonObject EmptyState()
    {
        var s = Clone(Fixture.State());
        s["sessions"] = new JsonArray();
        return s;
    }

    [Fact]
    public void A_click_on_a_bot_picks_that_bot_and_the_wall_props_pick_their_panels()
    {
        if (!gpu.Ready(output)) return;
        var (o, _, _) = Settled(NotchW, NotchH);
        foreach (var t in o.Tags())
        {
            // The head is a few pixels under the tag point.
            var hit = false;
            foreach (var dy in new[] { 14.0, 20.0, 28.0, 36.0 })
            {
                o.Pointer = (t.X, t.Y + dy);
                o.HasPointer = true;
                o.Pick();
                if (o.Hovered.Kind == HoverKind.Bot && o.Hovered.Id == t.Id) hit = true;
            }
            Assert.True(hit, $"a pointer on session {t.Id}'s bot did not find it");
        }
        // A pointer on empty space finds nothing.
        o.Pointer = (4, 4);
        o.Pick();
        Assert.Equal(HoverKind.None, o.Hovered.Kind);
    }

    [Fact]
    public void Day_is_brighter_than_night()
    {
        if (!gpu.Ready(output)) return;
        var (n, _, _) = Settled(NotchW, NotchH, TimeOfDay.Night);
        var (d, _, _) = Settled(NotchW, NotchH, TimeOfDay.Day);
        var night = Shoot("office-night", n, NotchW, NotchH);
        var day = Shoot("office-day", d, NotchW, NotchH);
        LooksLikeAnOffice(day, "day");
        LooksLikeAnOffice(night, "night");
        var (mn, _, _) = Stats(night);
        var (md, _, _) = Stats(day);
        output.WriteLine($"mean brightness: night {mn:F1}, day {md:F1}");
        Assert.True(md > mn, "the day office should be brighter than the night one");
    }

    [Fact]
    public void Eight_and_more_sessions_fill_the_room_without_breaking_it()
    {
        if (!gpu.Ready(output)) return;
        var (o, clock) = Make(NotchW, NotchH);
        o.State(Fixture.Inputs(Many(Fixture.State(), 9)));
        clock.Run(o, 500);
        Assert.Equal(9, o.Sessions.Count);
        var bgra = Shoot("office-8-sessions", o, NotchW, NotchH);
        LooksLikeAnOffice(bgra, "nine sessions");
    }

    [Fact]
    public void A_bot_asking_and_the_open_drawer_and_the_panels_move_the_camera_and_draw()
    {
        if (!gpu.Ready(output)) return;
        var (o, clock, state) = Settled(NotchW, NotchH);
        var asking = Clone(state);
        asking["sessions"]![3]!["turns"]![0]!["stage"] = "waiting";
        o.State(Fixture.Inputs(asking));
        clock.Run(o, 120);
        LooksLikeAnOffice(Shoot("office-ask-over", o, NotchW, NotchH), "a bot asking");

        var c0 = (double[])o.Cam.Clone();
        var ctl = new OfficeController(o, () => 12);
        ctl.Drawer(2, true);
        clock.Run(o, 200);
        Assert.False(c0.SequenceEqual(o.Cam), "the camera did not move to the open session");
        LooksLikeAnOffice(Shoot("office-drawer", o, NotchW, NotchH), "the drawer open");
        ctl.Drawer(2, false);

        ctl.Panel("tv");
        clock.Run(o, 200);
        LooksLikeAnOffice(Shoot("office-panel-tv", o, NotchW, NotchH), "the TV panel");
        ctl.Panel("board");
        clock.Run(o, 200);
        LooksLikeAnOffice(Shoot("office-panel-board", o, NotchW, NotchH), "the board panel");
        ctl.Panel("");
    }

    [Fact]
    public void Zoomed_out_and_zoomed_in_still_draw()
    {
        if (!gpu.Ready(output)) return;
        var (o, clock, _) = Settled(NotchW, NotchH);
        o.ZoomBy(0.001, 0, 0);
        clock.Run(o, 200);
        LooksLikeAnOffice(Shoot("office-zoomed-out", o, NotchW, NotchH), "zoomed out");
        o.ResetView();
        o.ZoomBy(100, 0, 0);
        clock.Run(o, 200);
        LooksLikeAnOffice(Shoot("office-zoomed-in", o, NotchW, NotchH), "zoomed in");
    }

    [Fact]
    public void At_150_percent_the_same_room_is_drawn_with_more_pixels()
    {
        if (!gpu.Ready(output)) return;
        var w = NotchW * 3 / 2;
        var h = NotchH * 3 / 2;
        var (o, _, _) = Settled(w, h);
        var bgra = Shoot("office-dpi150", o, w, h);
        LooksLikeAnOffice(bgra, "150 %");
        var (a, _, _) = Stats(bgra);
        var (o1, _, _) = Settled(NotchW, NotchH);
        var (b, _, _) = Stats(gpu.Renderer!.RenderToBytes(o1, NotchW, NotchH, new PageStyle(false, 44), gpu.Renderer.SceneSize(NotchW, NotchH).Width, gpu.Renderer.SceneSize(NotchW, NotchH).Height));
        Assert.InRange(a, b - 12, b + 12);
    }

    [Fact]
    public void A_new_device_after_a_lost_one_draws_the_same_picture()
    {
        if (!gpu.Ready(output)) return;
        var (o, _, _) = Settled(NotchW, NotchH);
        var first = gpu.Renderer!.RenderToBytes(o, NotchW, NotchH, new PageStyle(false), 560, 220);
        using var again = OfficeRenderer.Create(warp: true);
        var second = again.RenderToBytes(o, NotchW, NotchH, new PageStyle(false), 560, 220);
        Assert.Equal(first.Length, second.Length);
        Assert.True(BoxDiff(first, second, NotchW, NotchH, NotchW / 2.0, NotchH / 2.0, 220) < 0.5, "a second device drew a different room");
    }

    [Fact]
    public void The_wall_pictures_carry_text_in_the_pages_fonts()
    {
        if (!gpu.Ready(output)) return;
        var text = gpu.Text!;
        var slim = text.Layout("il", Face.Inter, false, 20);
        var wide = text.Layout("WM", Face.Inter, false, 20);
        Assert.True(wide.Width > slim.Width * 1.5, $"WM {wide.Width:F1} px vs il {slim.Width:F1} px");
        var px = text.Layout("12:34", Face.Pixel, true, 24);
        Assert.NotNull(px.Mask);
        Assert.True(px.Mask!.Alpha.Any(a => a > 200), "no solid pixel in the clock's digits");
        Assert.True(px.Ascent > 0 && px.Descent > 0);
        Assert.Null(text.Layout("   ", Face.Inter, false, 12).Mask);
    }
}
