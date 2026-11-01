using System.Text.Json;
using Hover.Host.Protocol;
using Hover.Notch;
using Xunit;

namespace Hover.Tests;

/// <summary>A desktop that is not there: one display, a pointer that tests move, a keyboard that tests hand around.</summary>
sealed class FakeDesktop : INotchPlatform
{
    public Rect Work = new(0, 0, 1920, 1040);
    public double Scale = 1;
    public string Displays = "one";
    public int X = -100, Y = -100;
    public bool Down;
    public Rect? Placed;
    public bool Hit, AcceptsKeys, Focused, ForegroundMine;
    public int Raised, Remembered, Restored;
    public (Rect, double) Primary() => (Work, Scale);
    public string Signature() => Displays;
    public (int, int) Cursor() => (X, Y);
    public bool Buttons() => Down;
    public void Place(Rect window) => Placed = window;
    public void Raise() => Raised++;
    public void SetAcceptsKeys(bool on) => AcceptsKeys = on;
    public void RememberForeground() => Remembered++;
    public void RestoreForeground() => Restored++;
    public void Focus() { Focused = true; ForegroundMine = true; }
    public void SetHit(bool over) => Hit = over;
    public bool ForegroundIsOurs() => ForegroundMine;
}

public sealed class NotchControllerTests
{
    sealed class Rig
    {
        public readonly FakeDesktop Desk = new();
        public double Time;
        public readonly NotchController Notch;

        public Rig(bool island = true)
        {
            Notch = new NotchController(Desk, () => Time);
            Notch.Layout();
            if (island) Notch.SetRest(new Size(128, 32));
        }

        public void Advance(double ms, double step = 50)
        {
            for (var t = 0.0; t < ms; t += step) { Time += step; Notch.Tick(out _); }
        }
    }

    [Fact]
    public void The_window_is_placed_at_the_top_centre_of_the_main_display()
    {
        var rig = new Rig();
        Assert.Equal(new Rect(360, 0, 1560, 480), rig.Desk.Placed);
        Assert.Equal(new Size(1120, 440), rig.Notch.OpenSize);
    }

    [Fact]
    public void A_screen_of_150_percent_scales_the_window()
    {
        var rig = new Rig();
        rig.Desk.Work = new Rect(-2560, 0, 0, 1400);
        rig.Desk.Scale = 1.5;
        rig.Notch.Layout();
        Assert.Equal(1800, rig.Desk.Placed!.Value.Width);
        Assert.Equal(720, rig.Desk.Placed!.Value.Height);
    }

    [Fact]
    public void Staying_in_the_strip_for_120_ms_peeks_and_leaving_for_350_ms_folds()
    {
        var rig = new Rig();
        // The strip is 220 wide at the top centre.
        rig.Desk.X = 960; rig.Desk.Y = 2;
        rig.Advance(100);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
        rig.Advance(100);
        Assert.Equal(NotchState.Peek, rig.Notch.State);
        Assert.True(rig.Desk.AcceptsKeys);
        Assert.Equal(1, rig.Desk.Remembered);

        rig.Advance(1000);
        // The pointer is still over the panel: it stays.
        Assert.Equal(NotchState.Peek, rig.Notch.State);
        rig.Desk.X = 100; rig.Desk.Y = 900;
        rig.Advance(300);
        Assert.Equal(NotchState.Peek, rig.Notch.State);
        rig.Advance(100);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
        Assert.False(rig.Desk.AcceptsKeys);
        Assert.Equal(1, rig.Desk.Restored);
    }

    [Fact]
    public void After_a_fold_the_pointer_must_leave_the_strip_before_hovering_opens_again()
    {
        var rig = new Rig();
        rig.Desk.X = 960; rig.Desk.Y = 2;
        rig.Advance(200);
        Assert.Equal(NotchState.Peek, rig.Notch.State);
        rig.Notch.Collapse();
        rig.Advance(1000);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
        rig.Desk.X = 100; rig.Desk.Y = 900;
        rig.Advance(100);
        rig.Desk.X = 960; rig.Desk.Y = 2;
        rig.Advance(200);
        Assert.Equal(NotchState.Peek, rig.Notch.State);
    }

    [Fact]
    public void A_question_waiting_or_the_setting_off_stops_hovering_from_opening_the_office()
    {
        var rig = new Rig();
        rig.Notch.Asking = true;
        rig.Desk.X = 960; rig.Desk.Y = 2;
        rig.Advance(1000);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
        rig.Notch.Asking = false;
        rig.Notch.HoverOpens = false;
        rig.Advance(1000);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
    }

    [Fact]
    public void A_held_mouse_button_stops_a_peek()
    {
        var rig = new Rig();
        rig.Desk.X = 960; rig.Desk.Y = 2; rig.Desk.Down = true;
        rig.Advance(1000);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
    }

    [Fact]
    public void The_pointer_goes_through_the_window_except_over_the_shape()
    {
        var rig = new Rig();
        rig.Time = 10;
        rig.Notch.Poll();
        rig.Notch.Shape();
        Assert.False(rig.Desk.Hit, "the pointer is far away");

        // Inside the window (360..1560 wide) but away from the 128 x 32 island.
        rig.Desk.X = 400; rig.Desk.Y = 300;
        rig.Notch.Poll(); rig.Notch.Shape();
        Assert.False(rig.Desk.Hit, "empty window space is click-through");

        rig.Desk.X = 960; rig.Desk.Y = 10;
        rig.Notch.Poll(); rig.Notch.Shape();
        Assert.True(rig.Desk.Hit, "the island takes the pointer");

        // Just beyond the shadow's reach.
        rig.Desk.X = 960 + 64 + 40; rig.Desk.Y = 10;
        rig.Notch.Poll(); rig.Notch.Shape();
        Assert.False(rig.Desk.Hit);
    }

    [Fact]
    public void With_nothing_to_show_the_whole_window_is_click_through()
    {
        var rig = new Rig(island: false);
        rig.Desk.X = 960; rig.Desk.Y = 2;
        rig.Notch.Poll(); rig.Notch.Shape();
        Assert.False(rig.Desk.Hit);
    }

    [Fact]
    public void Toggling_opens_with_the_keyboard_and_folds_again()
    {
        var rig = new Rig();
        rig.Notch.Toggle();
        Assert.Equal(NotchState.Open, rig.Notch.State);
        Assert.True(rig.Desk.AcceptsKeys);
        Assert.True(rig.Desk.Focused);
        rig.Advance(700);
        Assert.False(rig.Notch.Animating);
        Assert.InRange(rig.Notch.OpennessNow, 0.999, 1.001);

        // An open notch does not fold on its own when the pointer leaves.
        rig.Desk.X = 100; rig.Desk.Y = 900;
        rig.Advance(2000);
        Assert.Equal(NotchState.Open, rig.Notch.State);

        rig.Notch.Toggle();
        Assert.Equal(NotchState.Rest, rig.Notch.State);
        Assert.False(rig.Desk.AcceptsKeys);
        rig.Advance(400);
        Assert.Equal(0, rig.Notch.OpennessNow, 6);
    }

    [Fact]
    public void An_open_notch_folds_when_the_keyboard_goes_to_another_app()
    {
        var rig = new Rig();
        rig.Notch.Toggle();
        rig.Advance(100);
        Assert.Equal(NotchState.Open, rig.Notch.State);
        rig.Desk.ForegroundMine = false;
        rig.Advance(100);
        Assert.Equal(NotchState.Rest, rig.Notch.State);
    }

    [Fact]
    public void An_open_notch_that_never_had_the_keyboard_does_not_fold_on_click_away()
    {
        var rig = new Rig();
        rig.Notch.Expand(peek: false, focus: false);
        rig.Desk.ForegroundMine = false;
        rig.Advance(500);
        Assert.Equal(NotchState.Open, rig.Notch.State);
    }

    [Fact]
    public void A_pressed_peek_stays_open()
    {
        var rig = new Rig();
        rig.Notch.Expand(peek: true, focus: false);
        Assert.Equal(NotchState.Peek, rig.Notch.State);
        rig.Notch.Stay();
        Assert.Equal(NotchState.Open, rig.Notch.State);
    }

    [Fact]
    public void A_change_of_displays_is_noticed_within_two_seconds_and_the_window_is_placed_again()
    {
        var rig = new Rig();
        rig.Desk.Raised = 0;
        rig.Desk.Displays = "two";
        rig.Desk.Work = new Rect(0, 0, 2560, 1400);
        var relaid = false;
        for (var i = 0; i < 60 && !relaid; i++)
        {
            rig.Time += 50;
            rig.Notch.Tick(out var relayout);
            relaid = relayout;
        }
        Assert.True(relaid);
        Assert.True(rig.Desk.Raised >= 1, "the resting notch is raised above other topmost windows");
        rig.Notch.Layout();
        Assert.Equal(680, rig.Desk.Placed!.Value.Left);
    }

    [Fact]
    public void The_office_size_setting_changes_the_window()
    {
        var rig = new Rig();
        rig.Notch.Size = OfficeSize.Small;
        rig.Notch.Layout();
        Assert.Equal(new Size(840, 340), rig.Notch.OpenSize);
        Assert.Equal(920, rig.Desk.Placed!.Value.Width);
    }

    [Fact]
    public void A_small_display_holds_the_open_size_inside_the_work_area()
    {
        var rig = new Rig();
        rig.Desk.Work = new Rect(0, 0, 800, 500);
        rig.Notch.Layout();
        Assert.Equal(new Size(776, 440), rig.Notch.OpenSize);
        Assert.True(rig.Desk.Placed!.Value.Width <= 800 + 80);
    }
}

public sealed class IslandTests
{
    static SessionInfo Session(int id, string tool, string stage, string verb = "", string obj = "", AskInfo? ask = null, long t0 = 0, double? took = null) => new()
    {
        Id = id, Tool = tool, Stage = stage, Ask = ask, Activity = new ActivityInfo { Verb = verb, Obj = obj },
        Turns = [new TurnInfo { T0 = t0, Took = took }],
    };

    static Island Make(IReadOnlyList<SessionInfo> sessions, Unseen? unseen = null, string[]? on = null, int speaker = 0, long now = 0, bool card = false,
        Dictionary<string, QuotaValue>? readings = null)
    {
        on ??= ["claude", "kiro", "codex", "cursor"];
        readings ??= [];
        return Rest.Make(id => on.Contains(id), id => readings.GetValueOrDefault(id), sessions, unseen, speaker, now, card);
    }

    [Fact]
    public void No_sessions_and_no_quotas_show_nothing()
    {
        var island = Make([], on: []);
        Assert.Equal(IslandKind.None, island.Kind);
        Assert.Equal("", island.Key);
    }

    [Fact]
    public void No_sessions_shows_the_quotas_in_their_order_with_a_dash_until_a_reading_arrives()
    {
        var island = Make([], readings: new() { ["kiro"] = new QuotaValue { Ok = true, Used = 82.2 } });
        Assert.Equal(IslandKind.Pill, island.Kind);
        Assert.False(island.Divider);
        Assert.Equal("claude,kiro,codex,cursor", island.Key);
        Assert.Equal(new[] { "—", "82", "—", "—" }, island.Quotas.Select(q => q.Value));
        Assert.Equal(new[] { false, true, false, false }, island.Quotas.Select(q => q.Pct));
    }

    [Fact]
    public void A_quota_with_no_reading_and_a_failed_one_are_told_apart()
    {
        var island = Make([], on: ["claude", "codex"], readings: new() { ["codex"] = new QuotaValue { Ok = false, Detail = "not signed in" } });
        Assert.False(island.Quotas[0].Dim);
        Assert.True(island.Quotas[1].Dim);
        Assert.Null(island.Quotas[1].Ring);
    }

    [Fact]
    public void Agents_at_work_come_first_then_a_divider_then_the_quotas()
    {
        var sessions = new[] { Session(1, "kiro", "waking", "Waking up", t0: 66_000), Session(2, "codex", "working", "Editing", "refresh.ts", t0: 1000) };
        var island = Make(sessions, on: ["claude"], now: 66_000);
        Assert.Equal(SegKind.Work, island.Seg.Kind);
        Assert.True(island.Divider);
        Assert.Equal("work,|,claude", island.Key);
        Assert.Equal(new[] { "kiro", "codex" }, island.Seg.Tools);
        Assert.Equal(1, island.Seg.More);
        Assert.Equal("Waking up", island.Seg.Verb);
        Assert.Equal(0.0, island.Seg.Secs);
        // The next speaker is the second agent, and its timer counts from its own start.
        var next = Make(sessions, on: ["claude"], speaker: 1, now: 66_000);
        Assert.Equal(1, next.Seg.Active);
        Assert.Equal("Editing", next.Seg.Verb);
        Assert.Equal("refresh.ts", next.Seg.Obj);
        Assert.Equal(65.0, next.Seg.Secs);
        Assert.Equal("1:05", Rest.Clock(next.Seg.Secs));
    }

    [Fact]
    public void The_speaker_goes_round_and_wraps()
    {
        var sessions = new[] { Session(1, "kiro", "working"), Session(2, "codex", "working"), Session(3, "cursor", "working") };
        Assert.Equal(new[] { 0, 1, 2, 0, 1 }, Enumerable.Range(0, 5).Select(i => Make(sessions, speaker: i).Seg.Active));
    }

    [Fact]
    public void A_task_that_has_ended_shows_how_long_it_took_not_a_running_timer()
    {
        var sessions = new[] { Session(1, "kiro", "working", t0: 0, took: 12.5) };
        Assert.Equal(12.5, Make(sessions, now: 9_999_000).Seg.Secs);
    }

    [Fact]
    public void A_question_wins_over_work_and_counts_all_that_wait()
    {
        var ask = new AskInfo { Id = "n1", Kind = "execute", Line = "Wants to run npm install", More = 1 };
        var ask2 = new AskInfo { Id = "n2", Kind = "edit", Line = "Wants to edit a.go", More = 0 };
        var sessions = new[] { Session(1, "kiro", "working"), Session(2, "claude", "waiting", ask: ask), Session(3, "codex", "waiting", ask: ask2) };
        var island = Make(sessions, on: []);
        Assert.Equal(SegKind.Ask, island.Seg.Kind);
        Assert.Equal(2, island.Seg.Session);
        Assert.Equal(3, island.Seg.Total);
        Assert.Equal(IslandKind.Pill, island.Kind);
        Assert.Equal(IslandKind.Card, Make(sessions, on: [], card: true).Kind);
        Assert.Equal(("Wants to run", "npm install"), Rest.AskWords(ask));
        Assert.Equal(("Wants to edit", "a.go"), Rest.AskWords(ask2));
    }

    [Theory]
    [InlineData("question", "Asks you", "Pick one")]
    [InlineData("delete", "Wants to delete", "files")]
    [InlineData("move", "Wants to move", "a.txt")]
    [InlineData("fetch", "Wants to go online", "")]
    [InlineData("other", "Wants to use", "Some tool")]
    public void Every_kind_of_question_has_its_words(string kind, string verb, string obj)
    {
        var line = (verb + " " + obj).Trim();
        Assert.Equal((verb, obj), Rest.AskWords(new AskInfo { Kind = kind, Line = line }));
    }

    [Fact]
    public void The_card_is_only_for_a_question()
    {
        Assert.Equal(IslandKind.Pill, Make([Session(1, "kiro", "working")], card: true).Kind);
    }

    [Fact]
    public void An_end_nobody_has_seen_shows_when_nothing_else_does_and_counts_the_rest()
    {
        var island = Make([], unseen: new Unseen(2, "codex", EndState.Completed, "Look for dead code", 187), on: ["kiro"]);
        Assert.Equal(SegKind.Done, island.Seg.Kind);
        Assert.Equal("done,|,kiro", island.Key);
        Assert.Equal(2, island.Seg.Count);
        Assert.Equal("3m 07s", Rest.Took(island.Seg.Took));
    }

    [Fact]
    public void A_very_long_working_text_stays_one_line_of_data()
    {
        var obj = new string('x', 5000);
        var island = Make([Session(1, "kiro", "working", "Editing", obj)], on: []);
        Assert.Equal(5000, island.Seg.Obj.Length);
    }

    [Fact]
    public void The_notch_settings_come_from_the_model_with_defaults_for_what_is_missing()
    {
        using var empty = JsonDocument.Parse("{}");
        Assert.Equal(NotchSettings.Default, NotchSettings.From(empty.RootElement));
        using var model = JsonDocument.Parse("""
            {"HoverOpensWorkspace":false,"NotchItems":["claude","kiro",7,null],"WorkspaceSize":"ExtraLarge","Appearance":"Dark",
             "ScWorkspace":{"Key":"F5","Modifiers":"Control, Shift"},"Other":1}
            """);
        var s = NotchSettings.From(model.RootElement);
        Assert.False(s.HoverOpens);
        Assert.Equal(new[] { "claude", "kiro" }, s.Items);
        Assert.Equal(OfficeSize.ExtraLarge, s.Size);
        Assert.Equal("Dark", s.Appearance);
        Assert.Equal("Ctrl+Shift+F5", s.Shortcut.Label);
        Assert.Equal((ushort)0x74, s.Shortcut.VirtualKey);
    }

    [Fact]
    public void A_model_of_the_wrong_kind_changes_nothing()
    {
        using var bad = JsonDocument.Parse("""{"HoverOpensWorkspace":"yes","NotchItems":5,"WorkspaceSize":"Huge","ScWorkspace":{"Key":"Nope"}}""");
        Assert.Equal(NotchSettings.Default, NotchSettings.From(bad.RootElement));
        using var list = JsonDocument.Parse("[]");
        Assert.Equal(NotchSettings.Default, NotchSettings.From(list.RootElement));
    }
}
