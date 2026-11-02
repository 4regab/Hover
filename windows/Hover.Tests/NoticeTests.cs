using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// "Review agent access in Settings before starting your first task." (owner, v5.0.18): the backend refuses a new task until Settings'
/// KiroNoticeSeen is true, and the C# office had no card to say so. Until the note is seen it stands over the office and the chat view,
/// nothing under it takes the pointer, and Got it saves the setting.
/// </summary>
public class NoticeTests
{
    static BackendEvent Settings(string json) => new SettingsEvent { Model = JsonDocument.Parse(json).RootElement.Clone() };

    static PanelHarness Unseen(float w = 1200, float h = 720, bool inNotch = false)
    {
        var rig = new PanelHarness(inNotch) { W = w, H = h };
        rig.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Tidy it", "Done.")));
        rig.Accept(Settings("""{"KiroNoticeSeen":false,"HoverOpensWorkspace":true}"""));
        return rig;
    }

    [Fact]
    public void The_panels_ask_for_the_settings_once_and_show_nothing_until_the_backend_says_the_note_is_not_seen()
    {
        var rig = new PanelHarness();
        rig.State();
        Assert.Single(rig.Sent.OfType<SettingsCommand>());
        rig.State();
        Assert.Single(rig.Sent.OfType<SettingsCommand>());
        Assert.False(rig.Ui.NoticeShown);
        Assert.DoesNotContain("Before an agent starts", rig.Frame().Texts);
        // A model without the key says nothing; a preference that says "not seen" does not bring the card (only setSetting does).
        rig.Accept(Settings("{}"));
        rig.Accept(new PreferencesEvent { NoticeSeen = false });
        Assert.False(rig.Ui.NoticeShown);
        rig.Accept(Settings("""{"KiroNoticeSeen":false}"""));
        Assert.True(rig.Ui.NoticeShown);
    }

    [Theory]
    [InlineData(828f, 346f, true)]
    [InlineData(840f, 340f, true)]
    [InlineData(880f, 480f, false)]
    [InlineData(1560f, 600f, true)]
    public void The_note_shows_its_words_and_Got_it_inside_the_window(float w, float h, bool inNotch)
    {
        var rig = Unseen(w, h, inNotch);
        var texts = rig.Frame().Texts;
        Assert.Contains("Before an agent starts", texts);
        Assert.Contains(texts, t => t.StartsWith("Kiro, Codex, Cursor, OpenCode and Claude Code work on their own", StringComparison.Ordinal));
        Assert.Contains(texts, t => t.StartsWith("So pick the folder with care", StringComparison.Ordinal));
        Assert.Contains("Got it", texts);
        var ok = rig.Ui.Find("noticeOk")!.Value.R;
        Assert.True(ok.X >= 0 && ok.Y >= 0 && ok.Right <= w && ok.Bottom <= h, $"Got it at {ok} is outside the window {w} x {h}");
        Assert.True(rig.Ui.NoticeShown);
    }

    [Fact]
    public void Nothing_under_the_note_takes_the_pointer_the_wheel_or_typing()
    {
        var rig = Unseen();
        // The office under the card still has its places (the + circle, the menu button); a press on them does nothing.
        var fab = rig.Ui.Find("fab")!.Value.R;
        var (x, y) = (fab.X + fab.W / 2, fab.Y + fab.H / 2);
        Assert.True(rig.Ui.Covers(x, y));
        Assert.True(rig.Ui.Covers(3, 3));
        rig.Ui.PointerMove(x, y);
        Assert.True(rig.Ui.PointerDown(x, y, 1, UiMods.None));
        rig.Ui.PointerUp(x, y);
        rig.Frame();
        Assert.Null(rig.Ui.Find("taskText"));
        Assert.True(rig.Click("topMenu"));
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.True(rig.Ui.Wheel(x, y, -90));
        Assert.False(rig.Ui.Char("x"));
        Assert.Equal("", rig.Ui.TaskText);
        // Escape is not the panels': the notch still folds on it.
        Assert.False(rig.Ui.Key(UiKey.Escape, UiMods.None));
        Assert.True(rig.Ui.NoticeShown);
        Assert.DoesNotContain(rig.Sent, c => c is SaveSettingsCommand);
        // The chat view is covered too.
        rig.Ui.ChatView = true;
        rig.Frame();
        Assert.Contains("Before an agent starts", rig.Canvas.Texts);
        Assert.True(rig.Click("taskText"));
        Assert.Equal("", rig.Ui.TaskText);
    }

    [Fact]
    public void Got_it_saves_the_setting_and_the_card_goes_by_a_click_and_by_Enter()
    {
        var rig = Unseen();
        Assert.True(rig.Click("noticeOk"));
        var save = rig.Last<SaveSettingsCommand>();
        Assert.True(save.NoticeSeen);
        Assert.False(rig.Ui.NoticeShown);
        Assert.DoesNotContain("Before an agent starts", rig.Frame().Texts);
        // The office is the user's again.
        Assert.True(rig.Click("fab"));
        Assert.NotNull(rig.Ui.Find("taskText"));

        // The backend's answer agrees.
        rig.Accept(new PreferencesEvent { NoticeSeen = true });
        Assert.False(rig.Ui.NoticeShown);

        // Settings > Kiro > "Show the note again" sets it to false with setSetting: the card is back, and Enter answers it.
        rig.Accept(Settings("""{"KiroNoticeSeen":false}"""));
        Assert.True(rig.Ui.NoticeShown);
        Assert.True(rig.Key(UiKey.Enter));
        Assert.False(rig.Ui.NoticeShown);
        Assert.Equal(2, rig.Sent.OfType<SaveSettingsCommand>().Count(c => c.NoticeSeen == true));
    }
}
