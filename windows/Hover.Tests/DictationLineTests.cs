using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>The line at the right of the reply box's bar that says what dictation is doing (internal/ui/officechat.go, d.Voice).</summary>
public class DictationLineTests
{
    static PanelHarness Chat()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Fix it", "All fixed.", "done")));
        h.Ui.OpenChat(1);
        h.Frame();
        // Dictation goes into the reply box: it is open.
        Assert.True(h.Click("compose"));
        Assert.True(h.Ui.Composing);
        return h;
    }

    [Fact]
    public void The_line_shows_while_dictation_works_and_goes_when_it_is_cleared()
    {
        var h = Chat();
        Assert.DoesNotContain("Listening…", h.Canvas.Texts);
        h.Ui.DictationLine = "Listening…";
        h.Frame();
        Assert.Contains("Listening…", h.Canvas.Texts);
        h.Ui.DictationLine = "Writing it down…";
        h.Frame();
        Assert.Contains("Writing it down…", h.Canvas.Texts);
        Assert.DoesNotContain("Listening…", h.Canvas.Texts);
        h.Ui.DictationLine = "";
        h.Frame();
        Assert.DoesNotContain("Writing it down…", h.Canvas.Texts);
        // Setting nothing for nothing is not a change to draw.
        h.Ui.DictationLine = null!;
        Assert.Equal("", h.Ui.DictationLine);
    }

    [Fact]
    public void A_long_reason_is_cut_to_the_room_it_has_and_does_not_crowd_the_stopping_word()
    {
        var h = Chat();
        h.Ui.DictationLine = "No microphone is connected, and the one you picked in Settings has gone as well.";
        h.Frame();
        var line = Assert.Single(h.Canvas.Texts, t => t.StartsWith("No microphone", StringComparison.Ordinal));
        Assert.EndsWith("…", line);
        Assert.True(line.Length < 60, line);
    }
}
