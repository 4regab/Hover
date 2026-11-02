using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// What the owner saw in v5.0.18: the lists of the new-task bar ran off the window edge and the wheel did nothing. Every menu the panels
/// open is opened here in the small windows the app is used in, with far more rows than fit. Each must stay inside the window, show its
/// picked row at once, take the wheel (and nothing behind it), and let every row be reached.
/// </summary>
public class MenuFitTests
{
    const int Models = 40, Repos = 45, Projects = 30;

    internal static PanelHarness Rig(float w, float h, string pickedFolder = @"C:\work\project0")
    {
        var rig = new PanelHarness { W = w, H = h };
        var models = Enumerable.Range(1, Models).Select(i => new ModelInfo { Id = "m" + i, Name = "Model number " + i }).ToList();
        ToolInfo Tool(string id, string name) => new() { Id = id, Name = name, Ready = true, Access = "risky", Models = [new ModelInfo { Id = id + "-a", Name = name + " A" }] };
        rig.Accept(new StateEvent
        {
            CanStart = true, Tool = "kiro", Folder = pickedFolder,
            Tools =
            [
                new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Access = "full", Model = "m" + Models, Effort = "high", Efforts = ["low", "medium", "high", "x-high"], EffortLabel = "Effort", Models = models },
                Tool("codex", "Codex"), Tool("cursor", "Cursor"), Tool("opencode", "OpenCode"), Tool("claude", "Claude Code"), Tool("agy", "Antigravity"),
            ],
            Sessions = [PanelHarness.Session(1, "done", null, PanelHarness.Turn("Tidy it", "Done."))],
            History = [.. Enumerable.Range(0, 14).Select(i => new HistoryRow { Key = "h" + i, Tool = "kiro", Title = "Old " + i, Folder = @"C:\work\hist" + i, At = 1_793_000_000_000 - i })],
        });
        rig.Accept(new ProjectsEvent { Ok = true, Projects = [.. Enumerable.Range(0, Projects).Select(i => new ProjectInfo { Id = "p" + i, Name = "Project " + i, Folder = @"C:\work\project" + i }) ] });
        rig.Accept(new CloudReposEvent { Ok = true, Done = true, Repos = [.. Enumerable.Range(1, Repos).Select(i => "me/repo-" + i)] });
        return rig;
    }

    internal static void StartScreen(PanelHarness h)
    {
        h.Ui.ChatView = true;
        h.Frame();
    }

    internal static void OfficeBox(PanelHarness h)
    {
        h.Ui.ChatView = false;
        h.Frame();
        Assert.True(h.Click("fab"));
    }

    static void ChatPane(PanelHarness h, bool chatView)
    {
        h.Ui.ChatView = chatView;
        h.Ui.OpenChat(1);
        h.Frame();
    }

    /// <summary>Every menu, by where it opens from: the steps that open it and the kind it must be.</summary>
    static readonly (string Name, string Kind, Action<PanelHarness> Open)[] All =
    [
        ("top menu", "top", h => { StartOffice(h); Assert.True(h.Click("topMenu")); }),
        ("start: agent", "agent", h => { StartScreen(h); Assert.True(h.Click("pillAgent")); }),
        ("start: where", "where", h => { StartScreen(h); Assert.True(h.Click("pillWhere")); }),
        ("start: access", "access", h => { StartScreen(h); Assert.True(h.Click("pillAccess")); }),
        ("start: folder", "project", h => { StartScreen(h); Assert.True(h.Click("pillFolder")); }),
        ("start: model", "model", h => { StartScreen(h); Assert.True(h.Click("pillModel")); }),
        ("start: repo", "repo", h => { StartScreen(h); Cloud(h); Assert.True(h.Click("pillRepo")); }),
        ("box: agent", "agent", h => { OfficeBox(h); Assert.True(h.Click("pillAgent")); }),
        ("box: folder", "project", h => { OfficeBox(h); Assert.True(h.Click("pillFolder")); }),
        ("box: access", "access", h => { OfficeBox(h); Assert.True(h.Click("pillAccess")); }),
        ("box: model", "model", h => { OfficeBox(h); Assert.True(h.Click("pillModel")); }),
        ("box: repo", "repo", h => { OfficeBox(h); Assert.True(h.Click("cloud")); Assert.True(h.Click("pillRepo")); }),
        ("chat view: more", "more", h => { ChatPane(h, true); Assert.True(h.Click("more")); }),
        ("drawer: more", "more", h => { ChatPane(h, false); Assert.True(h.Click("more")); }),
        ("chat view: reply model", "rmodel", h => { ChatPane(h, true); Assert.True(h.Click("compose")); Assert.True(h.Click("modelPill")); }),
        ("drawer: reply model", "rmodel", h => { ChatPane(h, false); Assert.True(h.Click("compose")); Assert.True(h.Click("modelPill")); }),
    ];

    static void StartOffice(PanelHarness h)
    {
        h.Ui.ChatView = false;
        h.Frame();
    }

    /// <summary>Kiro Web: the where pill is Kiro's, and the repositories were listed.</summary>
    internal static void Cloud(PanelHarness h)
    {
        Assert.True(h.Click("pillWhere"));
        Assert.True(h.Click("menu", 1));
    }

    /// <summary>The windows: the notch's open size on the owner's PC, the Small notch, the app window's smallest (880 x 480), and a large notch.</summary>
    public static IEnumerable<object[]> Cases()
    {
        foreach (var (w, h) in new[] { (828f, 346f), (840f, 340f), (880f, 480f), (1560f, 600f) })
            for (var i = 0; i < All.Length; i++)
                yield return [w, h, i];
    }

    internal static bool Inside(Rect4 r, Rect4 outer) => r.X >= outer.X - 0.01 && r.Y >= outer.Y - 0.01 && r.Right <= outer.Right + 0.01 && r.Bottom <= outer.Bottom + 0.01;

    [Theory]
    [MemberData(nameof(Cases))]
    public void Every_menu_fits_the_window_and_the_wheel_reaches_its_last_row(float w, float h, int index)
    {
        var (name, kind, open) = All[index];
        var rig = Rig(w, h);
        open(rig);
        var where = $"{name} in {w} x {h}";
        Assert.True(kind == rig.Ui.OpenMenuKind, $"{where}: the menu that is open is {rig.Ui.OpenMenuKind}, not {kind}");

        // Inside the window, with 8 px to spare, and no taller than the window less 16.
        var box = rig.Ui.Find("menuBox")!.Value.R;
        Assert.True(Inside(box, new Rect4(8, 8, w - 16, h - 16)), $"{where}: the box {box} is not inside the window");
        Assert.True(box.H <= h - 16 + 0.01, $"{where}: the box is {box.H} tall in a window of {h}");

        // The wheel moves it from the first row to the last, and every row that can be picked shows on the way, whole and inside the box.
        var want = rig.Ui.OpenMenuPickable.ToList();
        Assert.NotEmpty(want);
        var seen = new SortedSet<int>();
        var scrolledBefore = rig.Ui.ScrollY;
        void Note()
        {
            foreach (var t in rig.Ui.Targets.Where(t => t.Id == "menu"))
            {
                Assert.True(Inside(t.R, box), $"{where}: row {t.N} ({t.S}) at {t.R} is outside the box {box}");
                seen.Add(t.N);
            }
        }
        // Back to the top first (a menu opens on its picked row), then down.
        var mid = (X: box.X + box.W / 2, Y: box.Y + box.H / 2);
        rig.Ui.PointerMove(mid.X, mid.Y);
        for (var i = 0; i < 200; i++)
        {
            Assert.True(rig.Ui.Wheel(mid.X, mid.Y, 90), "the wheel over an open menu is the menu's");
            var before = rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.N).ToArray();
            rig.Frame();
            if (before.SequenceEqual(rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.N))) break;
        }
        Note();
        for (var i = 0; i < 200; i++)
        {
            Assert.True(rig.Ui.Wheel(mid.X, mid.Y, -90), "the wheel over an open menu is the menu's");
            var before = rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.N).ToArray();
            rig.Frame();
            Note();
            if (before.SequenceEqual(rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.N))) break;
        }
        Assert.True(want.All(seen.Contains), $"{where}: rows {string.Join(", ", want.Except(seen))} of {want.Count} could not be reached with the wheel");
        Assert.Equal(want[^1], seen.Max);
        // The box did not move or grow while it scrolled, and the chat behind it did not scroll.
        Assert.Equal(box, rig.Ui.Find("menuBox")!.Value.R);
        Assert.Equal(scrolledBefore, rig.Ui.ScrollY);
        Assert.Equal(kind, rig.Ui.OpenMenuKind);
    }

    [Theory]
    [InlineData(828f, 346f)]
    [InlineData(880f, 480f)]
    public void A_menu_opens_on_its_picked_row_even_when_that_is_far_down_the_list(float w, float h)
    {
        // Kiro's picked model is the 40th, the picked folder the 30th of the projects.
        var rig = Rig(w, h, @"C:\work\project29");
        StartScreen(rig);
        Assert.True(rig.Click("pillModel"));
        var model = rig.Ui.Targets.Where(t => t.Id == "menu" && t.S == "Model number " + Models).ToList();
        Assert.Single(model);
        Assert.True(Inside(model[0].R, rig.Ui.Find("menuBox")!.Value.R));
        // The pinned part stays in view while the models scroll: the effort levels and the heading.
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "high");
        Assert.Contains("Model", rig.Canvas.Texts.Select(t => t.ToUpperInvariant()).Select(t => t == "MODEL" ? "Model" : t));
        rig.Key(UiKey.Escape);
        Assert.Null(rig.Ui.OpenMenuKind);

        Assert.True(rig.Click("pillFolder"));
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "Project 29");
    }

    [Theory]
    [InlineData(828f, 346f)]
    [InlineData(1560f, 600f)]
    public void A_repository_picked_from_far_down_the_list_sticks_and_is_what_the_task_clones(float w, float h)
    {
        var rig = Rig(w, h);
        StartScreen(rig);
        Cloud(rig);
        Assert.True(rig.Click("pillRepo"));
        var box = rig.Ui.Find("menuBox")!.Value.R;
        var mid = (X: box.X + box.W / 2, Y: box.Y + box.H / 2);
        rig.Ui.PointerMove(mid.X, mid.Y);
        for (var i = 0; i < 100 && !rig.Ui.Targets.Any(t => t.Id == "menu" && t.S == "me/repo-" + Repos); i++)
        {
            Assert.True(rig.Ui.Wheel(mid.X, mid.Y, -90));
            rig.Frame();
        }
        Assert.True(rig.Click("menu", s: "me/repo-" + Repos), "the last repository can be clicked");
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.Contains("repo-" + Repos, rig.Frame().Texts);
        Assert.NotNull(rig.Ui.Find("pillRepo"));

        // Opening the list again shows the pick, ticked, where it is.
        Assert.True(rig.Click("pillRepo"));
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "me/repo-" + Repos);
        rig.Key(UiKey.Escape);

        rig.Click("taskText");
        rig.Type("clone it");
        rig.Click("startTask");
        Assert.Equal(["me/repo-" + Repos], rig.Last<NewTaskCommand>().Cloud!);
    }

    [Fact]
    public void The_bar_of_a_long_list_is_dragged_and_a_click_in_its_track_pages()
    {
        var rig = Rig(828, 346);
        StartScreen(rig);
        Assert.True(rig.Click("pillModel"));
        var strip = rig.Ui.Find("menuBar")!.Value.R;
        int First() => rig.Ui.Targets.Where(t => t.Id == "menu" && t.S.StartsWith("Model number", StringComparison.Ordinal)).Select(t => t.N).Min();
        // The list opens on the 40th model, so the thumb is at the bottom; a press in the track above it pages up.
        var opened = First();
        rig.Ui.PointerMove(strip.X + 6, strip.Y + 8);
        rig.Ui.PointerDown(strip.X + 6, strip.Y + 8, 1, UiMods.None);
        rig.Ui.PointerUp(strip.X + 6, strip.Y + 8);
        rig.Frame();
        Assert.True(First() < opened, "a press in the track above the thumb scrolls up");
        // Back to the top with the wheel, where the thumb is at the top of the track; hold it, and drag it past the bottom.
        var box = rig.Ui.Find("menuBox")!.Value.R;
        rig.Ui.PointerMove(box.X + 40, box.Y + 40);
        for (var i = 0; i < 100; i++) rig.Ui.Wheel(box.X + 40, box.Y + 40, 90);
        rig.Frame();
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "Model number 1");
        Assert.DoesNotContain(rig.Ui.Targets, t => t.Id == "menu" && t.S == "Model number " + Models);
        var grabY = strip.Y + 6;
        rig.Ui.PointerMove(strip.X + 6, grabY);
        Assert.True(rig.Ui.PointerDown(strip.X + 6, grabY, 1, UiMods.None));
        rig.Ui.PointerMove(strip.X + 6, strip.Bottom + 40);
        rig.Ui.PointerUp(strip.X + 6, strip.Bottom + 40);
        rig.Frame();
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "Model number " + Models);
        Assert.Equal("model", rig.Ui.OpenMenuKind);
    }

    [Fact]
    public void The_keys_move_through_a_menu_and_pick_and_Escape_closes_it()
    {
        var rig = Rig(828, 346);
        StartScreen(rig);
        Assert.True(rig.Click("pillAgent"));
        // Down, Down: the second tool (Codex); Enter picks it.
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Enter);
        Assert.Null(rig.Ui.OpenMenuKind);
        rig.Click("taskText");
        rig.Type("x");
        rig.Click("startTask");
        Assert.Equal("codex", rig.Last<NewTaskCommand>().Tool);

        // Home goes to the first row that can be picked (the list scrolls up to it), End to the last (the pinned note), Up one back.
        rig = Rig(828, 346);
        StartScreen(rig);
        Assert.True(rig.Click("pillModel"));
        rig.Key(UiKey.Home);
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.S == "Default");
        rig.Key(UiKey.End);
        var last = rig.Ui.OpenMenuPickable[^1];
        Assert.Contains(rig.Ui.Targets, t => t.Id == "menu" && t.N == last);
        rig.Key(UiKey.Up);
        rig.Key(UiKey.Escape);
        Assert.Null(rig.Ui.OpenMenuKind);
        // Enter with no row lit picks nothing and leaves the menu open.
        Assert.True(rig.Click("pillAgent"));
        rig.Key(UiKey.Enter);
        Assert.Equal("agent", rig.Ui.OpenMenuKind);
        // A press away from the menu closes it and does nothing else.
        rig.Ui.PointerDown(2, 2, 1, UiMods.None);
        rig.Ui.PointerUp(2, 2);
        rig.Frame();
        Assert.Null(rig.Ui.OpenMenuKind);
    }

    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    [Theory]
    [InlineData(828f, 346f)]
    [InlineData(880f, 480f)]
    public void The_at_list_over_the_reply_box_stays_in_the_window_and_the_wheel_reaches_its_last_file(float w, float h)
    {
        var rig = Rig(w, h);
        var tree = string.Join(",", Enumerable.Range(1, 12).Select(i => $"\"src/file{i:00}.ts\""));
        rig.Accept(new DeskEvent { Id = 1, What = "files", Data = J("{\"git\":true,\"changed\":[],\"touched\":[],\"more\":false,\"tree\":[" + tree + "]}") });
        ChatPane(rig, true);
        Assert.True(rig.Click("compose"));
        rig.Click("replyText");
        rig.Type("@file");
        var window = new Rect4(0, 8, w, h - 16);
        var seen = new SortedSet<int>();
        void Note()
        {
            foreach (var t in rig.Ui.Targets.Where(t => t.Id == "popRow"))
            {
                Assert.True(Inside(t.R, window), $"a row at {t.R} is outside the window {w} x {h}");
                seen.Add(t.N);
            }
        }
        Note();
        Assert.NotEmpty(seen);
        var row = rig.Ui.Targets.First(t => t.Id == "popRow").R;
        var (x, y) = (row.X + row.W / 2, row.Y + row.H / 2);
        rig.Ui.PointerMove(x, y);
        for (var i = 0; i < 40; i++)
        {
            Assert.True(rig.Ui.Wheel(x, y, -90));
            rig.Frame();
            Note();
        }
        // The list holds at most eight files; every one of them can be reached.
        Assert.Equal(Enumerable.Range(0, 8), seen);
    }

    [Fact]
    public void The_Open_file_in_menu_stays_inside_a_short_panel_and_the_wheel_reaches_its_last_editor()
    {
        var rig = Rig(828, 346);
        rig.Ui.ChatView = false;
        rig.Ui.OpenDeskPanel(1, "files");
        rig.Frame();
        rig.Accept(new EditorsEvent { Ok = true, Choices = [.. Enumerable.Range(1, 12).Select(i => new EditorChoice { Id = "ed" + i, Name = "Editor number " + i, Last = i == 1 }), new EditorChoice { Id = "fm", Name = "File Explorer" }] });
        rig.Accept(new DeskEvent { Id = 1, What = "files", Data = J("""{"git":true,"changed":[],"touched":[],"more":false,"tree":["src/refresh.ts"]}""") });
        Assert.True(rig.Ui.Click("drow", s: "dir:src"));
        rig.Frame();
        Assert.True(rig.Ui.Click("drow", s: "file:src/refresh.ts"));
        rig.Accept(new DeskEvent { Id = 1, What = "file", Arg = "src/refresh.ts", Data = J("""{"path":"src/refresh.ts","text":"hello\n","truncated":false,"size":6}""") });
        Assert.True(rig.Click("openPill"));
        var panel = rig.Ui.DeskPanelRect;
        var seen = new SortedSet<int>();
        void Note()
        {
            foreach (var t in rig.Ui.Targets.Where(t => t.Id == "openIn"))
            {
                Assert.True(Inside(t.R, panel) && Inside(t.R, new Rect4(0, 0, rig.W, rig.H)), $"an editor at {t.R} is outside the panel {panel}");
                seen.Add(t.N);
            }
        }
        Note();
        Assert.NotEmpty(seen);
        var box = rig.Ui.Find("openMenuBox")!.Value.R;
        Assert.True(box.Bottom <= panel.Bottom + 0.01 && box.Bottom <= rig.H, $"the menu {box} runs past the panel {panel}");
        var (x, y) = (box.X + box.W / 2, box.Y + box.H / 2);
        rig.Ui.PointerMove(x, y);
        var scroll = rig.Ui.DeskListRect;
        for (var i = 0; i < 40; i++)
        {
            Assert.True(rig.Ui.Wheel(x, y, -90));
            rig.Frame();
            Note();
        }
        Assert.Equal(Enumerable.Range(0, 13), seen);
        Assert.Equal(scroll, rig.Ui.DeskListRect);
        Assert.True(rig.Ui.Click("openIn", 12));
        Assert.Equal("fm", rig.Last<OpenInCommand>().Editor);
    }

    [Fact]
    public void A_wheel_away_from_an_open_menu_is_still_not_the_chats()
    {
        var rig = Rig(880, 480);
        ChatPane(rig, true);
        Assert.True(rig.Click("more"));
        var scroll = rig.Ui.ScrollY;
        Assert.True(rig.Ui.Wheel(rig.W / 2, rig.H / 2, -90));
        rig.Frame();
        Assert.Equal(scroll, rig.Ui.ScrollY);
        Assert.Equal("more", rig.Ui.OpenMenuKind);
    }
}
