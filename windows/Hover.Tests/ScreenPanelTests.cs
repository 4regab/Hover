using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Desk;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>The desk's Screen tab: the tile, the pictures asked for and shown, the watch button and the lines (internal/shell/desk.go, internal/ui/deskpanel.go).</summary>
public class ScreenPanelTests
{
    sealed class FakeFeed : IScreenFeed
    {
        public bool Supported { get; set; } = true;
        public readonly List<(IReadOnlyList<uint> Pids, bool Live, Action<string?, string?> Done)> Asked = [];
        public readonly List<string> Released = [];

        public void Take(IReadOnlyList<uint> pids, bool live, Action<string?, string?> done) => Asked.Add((pids, live, done));

        public void Release(string src) => Released.Add(src);
    }

    /// <summary>A canvas that writes down the texts and the pictures a frame drew.</summary>
    sealed class Frame : ICanvas
    {
        public readonly List<string> Texts = [];
        public readonly List<(string Src, Rect4 R)> Images = [];

        public void PushClip(Rect4 r) { }
        public void PopClip() { }
        public void PushMove(float dx, float dy) { }
        public void PopMove() { }
        public void PushOpacity(float opacity) { }
        public void PopOpacity() { }
        public void Fill(Rect4 r, float[] radius, Rgba c) { }
        public void Stroke(Rect4 r, float[] radius, float width, Rgba c) { }
        public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) { }
        public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) { }
        public void Glow(float cx, float cy, float radius, Rgba c) { }
        public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y) => Texts.Add(((FakeLayout)layout).Text);
        public void Svg(string svg, Rect4 r) { }
        public void Image(string src, Rect4 r, float radius, bool cover) => Images.Add((src, r));
        public void BrokenImage(float x, float y) { }
        public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash) { }
        public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family) => Texts.Add(text);
    }

    sealed class Rig
    {
        public readonly FakeFeed Feed = new();
        public readonly OfficeUi Ui;
        public readonly Dictionary<string, (float W, float H)> Pictures = [];
        public double Now = 1000;

        public Rig(SessionInfo session, bool feed = true)
        {
            Ui = new OfficeUi(new FakeShaper(), new UiEnv
            {
                Send = _ => { },
                Clipboard = new FakeClipboard(),
                Screen = feed ? Feed : null,
                Images = src => Pictures.TryGetValue(src, out var size) ? new ImageState(ImageKind.Ready, size.W, size.H) : new ImageState(ImageKind.Pending),
            });
            Ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [session] });
        }

        /// <summary>A frame, a quarter second after the one before.</summary>
        public Frame Draw(double after = 300)
        {
            Now += after;
            var f = new Frame();
            Ui.Paint(f, 1200, 720, Now);
            return f;
        }

        public void OpenScreen()
        {
            Draw();
            Ui.OpenDeskPanel(1, "screen");
        }
    }

    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    static SessionInfo Session(string? extra = null) => PanelHarness.Session(1, "working", null, PanelHarness.Turn("Test it", "", "working")) with
    {
        Extra = extra is null ? null : JsonDocument.Parse(extra).RootElement.EnumerateObject().ToDictionary(p => p.Name, p => p.Value.Clone()),
    };

    const string WithApps = """{"testing":false,"apps":{"pids":[4242,4243],"bundles":[],"names":["Calculator"]}}""";

    [Fact]
    public void The_tile_is_open_with_a_feed_and_says_what_the_agent_has_and_is_grey_without_one()
    {
        // Desktop, Desktop + its apps, Live: the card's line under the Screen tile.
        Assert.Equal("Desktop", ScreenInfo.Of(Session()).Detail);
        Assert.Equal("Desktop + its apps", ScreenInfo.Of(Session(WithApps)).Detail);
        Assert.Equal("Live", ScreenInfo.Of(Session("""{"testing":true,"apps":{"pids":[1]}}""")).Detail);

        var rig = new Rig(Session(WithApps));
        rig.Draw();
        rig.Ui.OpenDeskCard(1, 300, 200);
        // The line is cut to the tile's width, as the Go card cuts it.
        Assert.Contains(rig.Draw().Texts, t => t.StartsWith("Desktop +", StringComparison.Ordinal));
        Assert.True(rig.Ui.Click("cardTile", DeskTiles.IndexOf("screen")));
        rig.Draw();
        Assert.Equal(["screen"], rig.Ui.DeskTabs.Where(t => t == "screen"));

        // With nothing to take pictures, the tile says why and does not open.
        var none = new Rig(Session(), feed: false);
        none.Draw();
        none.Ui.OpenDeskCard(1, 300, 200);
        none.Draw();
        Assert.True(none.Ui.Click("cardTile", DeskTiles.IndexOf("screen")));
        none.Draw();
        Assert.Equal(DeskTiles.ScreenNote, none.Ui.ToastText);
        Assert.DoesNotContain("screen", none.Ui.DeskTabs);
    }

    [Fact]
    public void The_apps_of_a_session_are_read_from_what_the_backend_sent_and_odd_values_are_left_out()
    {
        var a = ScreenInfo.Of(Session("""{"testing":true,"apps":{"pids":[7,7,"x",-1,4294967296,9],"bundles":[]}}"""));
        Assert.True(a.Testing);
        Assert.True(a.HasApps);
        Assert.Equal<uint>([7, 9], a.Pids);
        foreach (var none in new[] { ScreenInfo.Of(Session("""{"testing":"yes","apps":[1,2]}""")), ScreenInfo.Of(Session()), ScreenInfo.Of(null) })
        {
            Assert.False(none.Testing);
            Assert.False(none.HasApps);
            Assert.Empty(none.Pids);
        }
    }

    [Fact]
    public void The_desktop_is_taken_once_at_rest_and_shown_whole_in_its_box_with_the_line_under_it()
    {
        var rig = new Rig(Session());
        rig.OpenScreen();
        var first = rig.Draw();
        Assert.Contains("Desktop", first.Texts);
        Assert.Contains("Watch live", first.Texts);
        Assert.Contains("Reading…", first.Texts);
        Assert.Contains("Your desktop with no apps yet: Moss hasn’t opened any, never your own windows.", first.Texts);
        // One picture is asked for, the desktop alone, and no more while it is on its way.
        Assert.Single(rig.Feed.Asked);
        Assert.False(rig.Feed.Asked[0].Live);
        rig.Draw();
        rig.Draw();
        Assert.Single(rig.Feed.Asked);

        // It arrives: shown inside the box with its shape kept, and not asked for again.
        rig.Pictures["pic:1"] = (1280, 720);
        rig.Feed.Asked[0].Done("pic:1", null);
        var shown = rig.Draw();
        var (src, box) = Assert.Single(shown.Images);
        Assert.Equal("pic:1", src);
        Assert.Equal(16.0 / 9, box.W / box.H, 3);
        Assert.DoesNotContain("Reading…", shown.Texts);
        rig.Draw();
        rig.Draw();
        Assert.Single(rig.Feed.Asked);
    }

    [Fact]
    public void Watching_live_takes_a_picture_four_times_a_second_of_the_agents_windows_and_stopping_takes_the_desktop_again()
    {
        var rig = new Rig(Session(WithApps));
        rig.OpenScreen();
        rig.Draw();
        rig.Pictures["pic:1"] = (1280, 720);
        rig.Feed.Asked[0].Done("pic:1", null);
        Assert.Contains("Your desktop with the apps Moss opened, never your own windows.", rig.Draw().Texts);

        Assert.True(rig.Ui.Click("sWatch"));
        var live = rig.Draw();
        Assert.Contains("Live", live.Texts);
        Assert.Contains("Stop watching", live.Texts);
        Assert.Contains("Live: your desktop with the apps Moss opened. Your own windows aren’t shown.", live.Texts);
        // Live, with the processes of the agent's apps.
        Assert.Equal(2, rig.Feed.Asked.Count);
        Assert.True(rig.Feed.Asked[1].Live);
        Assert.Equal<uint>([4242, 4243], rig.Feed.Asked[1].Pids);

        // The next picture is due a quarter second after the last arrived; the old one is let go.
        rig.Pictures["pic:2"] = (1280, 720);
        rig.Feed.Asked[1].Done("pic:2", null);
        Assert.Equal(["pic:1"], rig.Feed.Released);
        rig.Draw(100);
        Assert.Equal(2, rig.Feed.Asked.Count);
        rig.Draw(200);
        Assert.Equal(3, rig.Feed.Asked.Count);

        // Stop: the desktop alone once more.
        rig.Feed.Asked[2].Done(null, "Windows didn’t give a picture of the screen.");
        Assert.True(rig.Ui.Click("sWatch"));
        var stopped = rig.Draw();
        Assert.Contains("Watch live", stopped.Texts);
        Assert.Equal(4, rig.Feed.Asked.Count);
        Assert.False(rig.Feed.Asked[3].Live);
    }

    [Fact]
    public void An_agent_testing_with_computer_use_is_live_without_the_button_and_a_failure_is_said_under_the_old_picture()
    {
        var rig = new Rig(Session("""{"testing":true,"apps":{"pids":[5]}}"""));
        rig.OpenScreen();
        var f = rig.Draw();
        Assert.Contains("Live", f.Texts);
        Assert.Contains("Moss is testing with computer use, live: your desktop with the apps Moss opened. Your own windows aren’t shown.", f.Texts);
        Assert.True(rig.Feed.Asked[0].Live);
        rig.Pictures["pic:1"] = (800, 600);
        rig.Feed.Asked[0].Done("pic:1", null);
        rig.Draw();
        // Taking the next fails: the line says why and the last picture stays.
        rig.Draw();
        rig.Feed.Asked[1].Done(null, "Windows didn’t give a picture of the screen.");
        var failed = rig.Draw(10);
        Assert.Contains("Windows didn’t give a picture of the screen.", failed.Texts);
        Assert.Equal("pic:1", Assert.Single(failed.Images).Src);
    }

    [Fact]
    public void The_pictures_go_when_the_panel_shuts_and_the_desktop_is_taken_again_when_the_tab_comes_back()
    {
        var rig = new Rig(Session());
        rig.OpenScreen();
        rig.Draw();
        rig.Pictures["pic:1"] = (1280, 720);
        rig.Feed.Asked[0].Done("pic:1", null);
        rig.Draw();
        // Another tab: nothing is asked; back: the desktop is taken again.
        rig.Ui.OpenDeskPanel(1, "terminal");
        rig.Draw();
        rig.Draw();
        Assert.Single(rig.Feed.Asked);
        rig.Ui.OpenDeskPanel(1, "screen");
        rig.Draw();
        Assert.Equal(2, rig.Feed.Asked.Count);
        // The panel shut: the picture is let go.
        rig.Ui.CloseDeskPanel();
        rig.Draw();
        Assert.Equal(["pic:1"], rig.Feed.Released);
    }
}
