using System.Text.Json;
using Hover.Host.Protocol;
using Hover.Notch;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// The C# notch rules against numbers the Go code printed (internal/app/notch_golden_test.go
/// writes golden/notch.json). Each test feeds the file's inputs to the C# code and compares its
/// answers with the file's, so a change in either language shows up here or in the Go test.
/// </summary>
public sealed class NotchGoldenTests
{
    static readonly JsonElement Golden = Load();

    static JsonElement Load()
    {
        var file = Path.Combine(AppContext.BaseDirectory, "golden", "notch.json");
        using var doc = JsonDocument.Parse(File.ReadAllText(file));
        return doc.RootElement.Clone();
    }

    static IEnumerable<JsonElement> Rows(string name) => Golden.GetProperty(name).EnumerateArray();

    // Go and .NET may differ in the last digits of sin, cos and pow.
    static void Near(double want, double got, string what)
    {
        Assert.True(Math.Abs(want - got) <= 1e-9 * Math.Max(1, Math.Abs(want)), $"{what}: Go printed {want}, C# gave {got}");
    }

    static double D(JsonElement row, string name) => row.GetProperty(name).GetDouble();
    static int I(JsonElement row, string name) => row.GetProperty(name).GetInt32();
    static bool B(JsonElement row, string name) => row.GetProperty(name).GetBoolean();

    static Rect R(JsonElement a) => new(a[0].GetInt32(), a[1].GetInt32(), a[2].GetInt32(), a[3].GetInt32());

    static void SameRect(JsonElement want, Rect got, string what) => Assert.True(R(want) == got, $"{what}: Go printed {R(want)}, C# gave {got}");

    [Fact]
    public void The_open_size_is_the_same()
    {
        var n = 0;
        foreach (var r in Rows("openSize"))
        {
            var s = Geometry.OpenSize((OfficeSize)I(r, "size"), new Size(D(r, "workW"), D(r, "workH")));
            Near(D(r, "w"), s.W, "open width");
            Near(D(r, "h"), s.H, "open height");
            n++;
        }
        Assert.Equal(16, n);
    }

    [Fact]
    public void The_window_is_placed_in_the_same_pixels()
    {
        foreach (var r in Rows("placement"))
            SameRect(r.GetProperty("rect"), Geometry.Placement(R(r.GetProperty("work")), D(r, "scale"), new Size(D(r, "openW"), D(r, "openH"))), "placement");
    }

    [Fact]
    public void The_resting_shape_and_its_corners_are_the_same()
    {
        foreach (var r in Rows("restSize"))
        {
            var s = Geometry.RestSize((RestKind)I(r, "kind"), D(r, "w"), D(r, "h"));
            Near(D(r, "outW"), s.W, "rest width");
            Near(D(r, "outH"), s.H, "rest height");
        }
        foreach (var r in Rows("restCorners"))
        {
            var (radius, ear) = Geometry.RestCorners(D(r, "h"));
            Near(D(r, "r"), radius, "corner");
            Near(D(r, "ear"), ear, "ear");
        }
    }

    [Fact]
    public void The_easing_is_the_same()
    {
        foreach (var r in Rows("ease"))
        {
            Near(D(r, "back"), Geometry.BackEaseOut(D(r, "t"), D(r, "amp")), "back ease");
            Near(D(r, "sine"), Geometry.SineInOut(D(r, "t")), "sine ease");
        }
    }

    [Fact]
    public void Every_frame_of_the_shape_is_the_same()
    {
        var n = 0;
        foreach (var r in Rows("frame"))
        {
            var f = Geometry.FrameAt(D(r, "t"), new Size(D(r, "restW"), D(r, "restH")), new Size(D(r, "openW"), D(r, "openH")), B(r, "closing"));
            Near(D(r, "w"), f.W, "w"); Near(D(r, "h"), f.H, "h"); Near(D(r, "r"), f.R, "r"); Near(D(r, "ear"), f.Ear, "ear");
            Near(D(r, "view"), f.ViewOpacity, "view opacity"); Near(D(r, "mini"), f.MiniOpacity, "mini opacity"); Near(D(r, "fill"), f.FillMix, "fill");
            Assert.Equal(B(r, "hit"), f.ViewHit);
            n++;
        }
        Assert.Equal(72, n);
    }

    [Fact]
    public void The_outline_text_is_the_same()
    {
        foreach (var r in Rows("outline"))
            Assert.Equal(r.GetProperty("path").GetString(), Geometry.Outline(D(r, "w"), D(r, "h"), D(r, "r"), D(r, "ear"), D(r, "x0")));
    }

    [Fact]
    public void The_hit_region_is_the_same_at_every_point()
    {
        var n = 0;
        foreach (var r in Rows("hittable"))
        {
            var f = Geometry.FrameAt(D(r, "t"), new Size(D(r, "restW"), D(r, "restH")), new Size(1120, 440));
            Assert.True(B(r, "hit") == Geometry.Hittable(D(r, "x"), D(r, "y"), 1200, f, NotchController.ShadowBlur, NotchController.ShadowDepth),
                $"rest {D(r, "restW")}x{D(r, "restH")} t {D(r, "t")} at {D(r, "x")},{D(r, "y")}");
            n++;
        }
        Assert.Equal(108, n);
    }

    [Fact]
    public void The_zones_are_the_same()
    {
        foreach (var r in Rows("zones"))
        {
            var work = R(r.GetProperty("work"));
            SameRect(r.GetProperty("zone"), Geometry.Zone(work, D(r, "scale"), new Size(D(r, "restW"), D(r, "restH"))), "zone");
            SameRect(r.GetProperty("panel"), Geometry.PanelZone(work, D(r, "scale"), new Size(1120, 440)), "panel zone");
        }
    }

    [Fact]
    public void The_resting_shape_springs_the_same_way()
    {
        var a = new RestAnim();
        foreach (var r in Rows("restAnim"))
        {
            if (r.GetProperty("op").GetString() == "go")
            {
                a.Go(new Size(D(r, "w"), D(r, "h")), D(r, "now"), B(r, "still"));
                Near(D(r, "targetW"), a.Target.W, "target width");
                Near(D(r, "targetH"), a.Target.H, "target height");
            }
            else
            {
                var v = a.Value(D(r, "now"));
                Near(D(r, "w"), v.W, $"width at {D(r, "now")}");
                Near(D(r, "h"), v.H, $"height at {D(r, "now")}");
                Assert.Equal(B(r, "animating"), a.Animating(D(r, "now")));
            }
        }
    }

    [Fact]
    public void Opening_and_closing_move_the_same_way()
    {
        var o = new Openness();
        foreach (var r in Rows("openness"))
        {
            if (r.GetProperty("op").GetString() == "go")
            {
                o.Go(D(r, "to"), D(r, "now"));
                Assert.Equal(B(r, "closing"), o.Closing);
            }
            else
            {
                Near(D(r, "v"), o.Value(D(r, "now")), $"openness at {D(r, "now")}");
                Assert.Equal(B(r, "animating"), o.Animating(D(r, "now")));
            }
        }
    }

    [Fact]
    public void The_pointer_rules_answer_the_same_at_every_poll()
    {
        var h = new HoverRules();
        var polls = 0;
        foreach (var r in Rows("hover"))
        {
            switch (r.GetProperty("op").GetString())
            {
                case "poll":
                    var p = new Pointer(B(r, "zone"), B(r, "panel"), B(r, "buttons"), B(r, "popover"), B(r, "hoverOpens"));
                    Assert.Equal(I(r, "action"), (int)h.Poll((long)D(r, "now"), p));
                    // The Go script follows a peek or a collapse with the matching call, as the shell does.
                    Assert.Equal(I(r, "state"), (int)h.State);
                    polls++;
                    break;
                case "new":
                    h = new HoverRules();
                    break;
                case "opened":
                    h.Opened(B(r, "peek"));
                    Assert.Equal(I(r, "state"), (int)h.State);
                    break;
                case "collapsed":
                    h.Collapsed();
                    Assert.Equal(I(r, "state"), (int)h.State);
                    break;
            }
        }
        Assert.True(polls >= 20);
    }

    [Fact]
    public void The_island_clock_and_the_took_text_are_the_same()
    {
        foreach (var r in Rows("clock")) Assert.Equal(r.GetProperty("clock").GetString(), Rest.Clock(D(r, "secs")));
        foreach (var r in Rows("took")) Assert.Equal(r.GetProperty("took").GetString(), Rest.Took(D(r, "secs")));
    }

    [Fact]
    public void The_quota_numbers_are_the_same()
    {
        foreach (var r in Rows("quotas"))
        {
            var has = B(r, "has");
            var reading = has
                ? new QuotaValue { Ok = true, Used = D(r, "used") }
                : B(r, "failed") ? new QuotaValue { Ok = false, Detail = "not signed in" } : null;
            var id = has ? "kiro" : r.GetProperty("id").GetString()!;
            var seg = Rest.QuotaSegment(id, reading);
            Assert.Equal(r.GetProperty("value").GetString(), seg.Value);
            Assert.Equal(B(r, "pct"), seg.Pct);
            Assert.Equal(B(r, "dim"), seg.Dim);
            Assert.Equal(r.GetProperty("name").GetString(), seg.Name);
        }
    }

    [Fact]
    public void The_tray_menu_is_the_same()
    {
        foreach (var r in Rows("tray"))
        {
            var want = r.GetProperty("items").EnumerateArray().ToList();
            var got = TrayMenu.Build(r.GetProperty("shortcut").GetString()!, B(r, "login"));
            Assert.Equal(want.Count, got.Count);
            for (var i = 0; i < want.Count; i++)
            {
                if (want[i].TryGetProperty("separator", out _)) { Assert.Null(got[i]); continue; }
                Assert.Equal(want[i].GetProperty("label").GetString(), got[i]!.Label);
                Assert.Equal(want[i].TryGetProperty("check", out var c) ? c.GetBoolean() : (bool?)null, got[i]!.Check);
            }
        }
    }

    [Fact]
    public void Every_key_has_the_same_virtual_key_and_label()
    {
        var n = 0;
        foreach (var r in Rows("keys"))
        {
            var key = I(r, "key");
            var vk = new Shortcut(key, Shortcut.Alt).VirtualKey;
            Assert.Equal(B(r, "ok"), vk is not null);
            if (vk is { } v) Assert.Equal(I(r, "vk"), v);
            // Key 0 is "no key": the label is an em dash whatever the modifiers.
            Assert.Equal(r.GetProperty("alt").GetString(), new Shortcut(key, Shortcut.Alt).Label);
            Assert.Equal(r.GetProperty("all").GetString(),
                new Shortcut(key, Shortcut.Control | Shortcut.Alt | Shortcut.Shift | Shortcut.Windows).Label);
            n++;
        }
        Assert.Equal(173, n);
        Assert.Equal(Golden.GetProperty("noKeyLabel").GetString(), new Shortcut(0, 0).Label);
    }

    [Fact]
    public void The_shortcut_in_a_settings_model_is_read_by_name_and_by_number()
    {
        static Shortcut? Read(string json) { using var d = JsonDocument.Parse(json); return Shortcut.FromJson(d.RootElement); }
        Assert.Equal(new Shortcut(57, Shortcut.Alt), Read("""{"Key":"N","Modifiers":"Alt"}"""));
        Assert.Equal(new Shortcut(18, Shortcut.Control | Shortcut.Alt), Read("""{"Key":"Space","Modifiers":"Alt, Control"}"""));
        Assert.Equal(new Shortcut(6, 0), Read("""{"Key":"Enter","Modifiers":"None"}"""));
        Assert.Equal(new Shortcut(57, 9), Read("""{"Key":57,"Modifiers":9}"""));
        Assert.Null(Read("""{"Key":"NotAKey","Modifiers":"Alt"}"""));
        Assert.Null(Read("""{"Key":"N","Modifiers":"Hyper"}"""));
        Assert.Null(Read("[]"));
    }
}
