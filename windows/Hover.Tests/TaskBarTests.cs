using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// The bar at the foot of the new-task box (v5.0.18: "Cla…" for the model pill, the send button squeezed). Whatever the window, the
/// pills stay inside the box, none runs into the next, the send button keeps its size, and a name that does not fit is cut with an
/// ellipsis, never in the middle of a word without one.
/// </summary>
public class TaskBarTests
{
    const string LongModel = "Claude Sonnet 5.5 with extended thinking";
    const string LongFolder = @"C:\work\an-unusually-long-project-folder-name";
    const string LongRepo = "me/an-unusually-long-repository-name";

    /// <summary>The owner's windows: the notch on their PC, the Small notch, the app window's smallest, a large notch, and one narrower than any.</summary>
    public static IEnumerable<object[]> Sizes => [[828f, 346f], [840f, 340f], [880f, 480f], [1560f, 600f], [520f, 420f]];

    static PanelHarness Rig(float w, float h)
    {
        var rig = MenuFitTests.Rig(w, h, LongFolder);
        rig.Accept(new StateEvent
        {
            CanStart = true, Tool = "kiro", Folder = LongFolder,
            Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Access = "full", Model = "long", Effort = "high", Models = [new ModelInfo { Id = "long", Name = LongModel }] },
                new ToolInfo { Id = "codex", Name = "Codex", Ready = true, Access = "risky" }],
        });
        return rig;
    }

    static void Check(PanelHarness rig, string where, bool start)
    {
        var box = rig.Ui.Find("taskBox")!.Value.R;
        var send = (rig.Ui.Find("startTask") ?? rig.Ui.Find("startOff"))!.Value.R;
        Assert.True(send.W == (start ? 32 : 30) && send.H == send.W, $"{where}: the send button is {send.W} x {send.H}");
        Assert.True(MenuFitTests.Inside(send, box), $"{where}: the send button {send} is outside the box {box}");
        var pills = new[] { "pillAgent", "pillWhere", "pillRepo", "pillFolder", "pillAccess", "pillModel" }
            .Select(id => (Id: id, T: rig.Ui.Find(id)))
            .Where(p => p.T is not null && p.T.Value.R.Y > box.Y + box.H / 2 && p.T.Value.R.H <= 30)
            .Select(p => (p.Id, R: p.T!.Value.R))
            .OrderBy(p => p.R.X).ToList();
        Assert.NotEmpty(pills);
        var attach = rig.Ui.Find("attach")!.Value.R;
        var edge = attach.Right;
        foreach (var (id, r) in pills)
        {
            Assert.True(MenuFitTests.Inside(r, box), $"{where}: {id} {r} is outside the box {box}");
            Assert.True(r.X >= edge - 0.01, $"{where}: {id} at {r.X} runs into what is before it, which ends at {edge}");
            edge = r.Right;
        }
        Assert.True(edge <= send.X + 0.01, $"{where}: the pills end at {edge}, over the send button at {send.X}");
    }

    /// <summary>A label drawn in a pill is whole, or cut short with an ellipsis; never a bare prefix.</summary>
    static void WholeOrEllipsis(RecordingCanvas canvas, string label, string where)
    {
        var drawn = canvas.Texts.Where(t => t.Length > 0 && (label.StartsWith(t.TrimEnd('…'), StringComparison.Ordinal))).ToList();
        Assert.True(drawn.Count > 0 || label.Length == 0, $"{where}: {label} is not drawn at all");
        Assert.All(drawn.Where(t => t.Length < label.Length), t => Assert.EndsWith("…", t));
    }

    [Theory]
    [MemberData(nameof(Sizes))]
    public void The_boxs_bar_keeps_its_pills_inside_and_the_send_button_whole_in_the_office(float w, float h)
    {
        var rig = Rig(w, h);
        MenuFitTests.OfficeBox(rig);
        var where = $"office box {w} x {h}";
        Check(rig, where, false);
        // Kiro Web puts the repository where the folder was, and has no access chip.
        Assert.NotNull(rig.Ui.Find("pillAccess"));
        WholeOrEllipsis(rig.Canvas, LongModel + " · high", where);
        Assert.True(rig.Click("cloud"));
        rig.Accept(new CloudReposEvent { Ok = true, Done = true, Repos = [LongRepo] });
        Assert.True(rig.Click("pillRepo"));
        Assert.True(rig.Click("menu", s: LongRepo));
        Check(rig, where + " (Kiro Web)", false);
        Assert.Null(rig.Ui.Find("pillAccess"));
        WholeOrEllipsis(rig.Canvas, "an-unusually-long-repository-name", where);
    }

    [Theory]
    [MemberData(nameof(Sizes))]
    public void The_boxs_bar_keeps_its_pills_inside_and_the_send_button_whole_on_the_start_screen(float w, float h)
    {
        var rig = Rig(w, h);
        MenuFitTests.StartScreen(rig);
        var where = $"start screen {w} x {h}";
        Check(rig, where, true);
        WholeOrEllipsis(rig.Canvas, LongModel + " · high", where);
        MenuFitTests.Cloud(rig);
        Assert.True(rig.Click("pillRepo"));
        rig.Accept(new CloudReposEvent { Ok = true, Done = true, Repos = [LongRepo] });
        Assert.True(rig.Click("menu", s: LongRepo));
        Check(rig, where + " (Kiro Web)", true);
    }

    [Fact]
    public void A_tool_without_Kiro_Web_has_no_cloud_switch_to_click()
    {
        var rig = Rig(828, 346);
        MenuFitTests.OfficeBox(rig);
        Assert.NotNull(rig.Ui.Find("cloud"));
        Assert.True(rig.Click("pillAgent"));
        Assert.True(rig.Click("menu", s: "Codex"));
        Assert.Null(rig.Ui.Find("cloud"));
        Assert.NotNull(rig.Ui.Find("pillFolder"));
    }
}
