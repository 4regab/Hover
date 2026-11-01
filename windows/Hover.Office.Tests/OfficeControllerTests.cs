using Xunit;

namespace Hover.Office.Tests;

/// <summary>The input rules of internal/office/live.go and the office.json the camera and the time of day are kept in.</summary>
public sealed class OfficeControllerTests
{
    static (OfficeController Ctl, OfficeModel Office, List<Click> Clicks, Fixture.Clock Clock) Room()
    {
        var (o, clock, _) = Fixture.Settled();
        var ctl = new OfficeController(o, () => 12);
        var clicks = new List<Click>();
        ctl.Clicked += clicks.Add;
        return (ctl, o, clicks, clock);
    }

    [Fact]
    public void A_press_and_release_on_a_bot_clicks_it_and_a_drag_does_not()
    {
        var (ctl, o, clicks, _) = Room();
        var t = o.Tags()[0];
        // The head is a few pixels under the tag point: the first of these that the pointer reads as the bot.
        double x = t.X, y = t.Y;
        foreach (var dy in new[] { 14.0, 20.0, 28.0, 36.0 })
        {
            o.Pointer = (t.X, t.Y + dy);
            o.HasPointer = true;
            o.Pick();
            if (o.Hovered.Kind == HoverKind.Bot && o.Hovered.Id == t.Id)
            {
                y = t.Y + dy;
                break;
            }
        }
        ctl.Pointer(x, y, true);
        ctl.Down(x, y);
        ctl.Up();
        var c = Assert.Single(clicks);
        Assert.Equal(ClickKind.Open, c.Kind);
        Assert.Equal(t.Id, c.Id);

        // Past 6 px the press is a drag: the view moves, and the release is no click.
        clicks.Clear();
        o.ZoomBy(2, 0, 0);
        var before = (double[])o.User.Clone();
        ctl.Down(300, 200);
        ctl.Pointer(304, 200, true);
        Assert.False(o.Dragging);
        ctl.Pointer(330, 220, true);
        Assert.True(o.Dragging);
        ctl.Up();
        Assert.Empty(clicks);
        Assert.False(before.SequenceEqual(o.User), "the drag did not move the view");
    }

    [Fact]
    public void The_wheel_and_the_keys_zoom_and_a_drawer_or_panel_holds_the_view()
    {
        var (ctl, o, _, _) = Room();
        var changed = 0;
        ctl.Changed += () => changed++;
        ctl.Wheel(-100, o.W / 2, o.H / 2);
        Assert.True(o.User[2] > 1, "a notch of the wheel away from the user should zoom in");
        ctl.Key('0');
        Assert.Equal(1, o.User[2]);
        ctl.Key('+');
        Assert.Equal(1.25, o.User[2], 6);
        ctl.Key('-');
        Assert.Equal(1.0, o.User[2], 6);
        Assert.Equal(4, changed);

        ctl.Drawer(2, true);
        ctl.Wheel(-100, 10, 10);
        Assert.Equal(1.0, o.User[2], 6);
        ctl.Drawer(0, false);
        ctl.Panel("tv");
        ctl.Wheel(-100, 10, 10);
        Assert.Equal(1.0, o.User[2], 6);
    }

    [Fact]
    public void Clicking_the_window_prop_switches_day_and_night_and_asks_to_keep_it()
    {
        var (ctl, o, clicks, _) = Room();
        var changed = 0;
        ctl.Changed += () => changed++;
        // The window prop's centre on screen.
        var (view, proj) = o.Camera();
        var at = OfficeModel.Props[(int)Prop.Window];
        var p = (proj * view).Point(new V3(at[0], at[1], at[2]));
        double x = (p.X + 1) / 2 * o.W, y = (1 - p.Y) / 2 * o.H;
        ctl.Pointer(x, y, true);
        ctl.Down(x, y);
        ctl.Up();
        var c = Assert.Single(clicks);
        Assert.Equal(ClickKind.Time, c.Kind);
        Assert.Equal(TimeOfDay.Day, o.ManualTime);
        Assert.Equal(TimeOfDay.Day, o.Time);
        Assert.True(changed >= 1);
    }

    [Fact]
    public void The_time_of_day_follows_the_clock_unless_picked()
    {
        var hour = 3;
        var (o, _, _) = Fixture.Settled();
        var ctl = new OfficeController(o, () => hour);
        ctl.SetTime(null);
        Assert.Equal(TimeOfDay.Night, o.Time);
        hour = 10;
        ctl.Tick(61_000);
        Assert.Equal(TimeOfDay.Day, o.Time);
        ctl.SetTime(TimeOfDay.Night);
        hour = 10;
        ctl.Tick(130_000);
        Assert.Equal(TimeOfDay.Night, o.Time);
    }

    [Fact]
    public void A_hidden_office_makes_no_frames()
    {
        var (ctl, o, _, clock) = Room();
        // Times continue the room's own clock; 20 ms is more than the 1/60 s a busy room waits between frames.
        var t = clock.Now;
        ctl.Visible = false;
        Assert.False(ctl.Tick(t + 20));
        ctl.Visible = true;
        o.Poke();
        Assert.True(ctl.Tick(t + 40));
    }

    [Fact]
    public void Office_json_keeps_the_view_the_time_and_the_beats_and_survives_a_bad_file()
    {
        var dir = Path.Combine(Path.GetTempPath(), "hover-office-store-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            var file = Path.Combine(dir, "office.json");
            var store = new OfficeStore(file);
            Assert.Null(store.View());
            Assert.Null(store.Time());
            store.SetView(1.5, -2, 1.25);
            store.SetTime(TimeOfDay.Day);
            store.Set("beats", "on");
            var again = new OfficeStore(file);
            Assert.Equal((1.5, -2.0, 1.25), again.View());
            Assert.Equal(TimeOfDay.Day, again.Time());
            Assert.Equal("on", again.Get("beats"));
            again.SetTime(null);
            Assert.Null(again.Time());
            Assert.Equal("on", again.Get("beats"));

            // A file the Go or Mac build wrote is a flat object of strings, whatever its spacing.
            File.WriteAllText(file, "{ \"view\" : \"0,0,1\", \"time\":\"night\" }");
            Assert.Equal((0.0, 0.0, 1.0), new OfficeStore(file).View());
            Assert.Equal(TimeOfDay.Night, new OfficeStore(file).Time());

            // Damaged: reads as nothing, and the next write replaces it.
            File.WriteAllText(file, "{ not json");
            var bad = new OfficeStore(file);
            Assert.Null(bad.View());
            bad.Set("beats", "off");
            Assert.Equal("off", new OfficeStore(file).Get("beats"));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }
}
