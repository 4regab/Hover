using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// The small faults sweeps 1 and 2 found and left (reports/sweep1.md section 5, reports/sweep2.md section 5). Each test here
/// failed on the code before its fix; the sweep's own rules are in SweepTests and SweepFixTests.
/// </summary>
public class Sweep3Tests
{
    static AskInfo CustomQuestion(string id) => new()
    {
        Id = id, Kind = "question", Title = "A question",
        Questions = [new AskQuestion { Header = "Pick", Question = "Which one?", Options = [new AskOption { Label = "Red" }], Custom = true }],
    };

    [Fact]
    public void Enter_in_the_custom_answer_box_answers_the_chat_that_is_open_not_the_first_one_that_asks()
    {
        var h = new PanelHarness();
        h.State(
            PanelHarness.Session(1, "waiting", CustomQuestion("q1"), PanelHarness.Turn("First", "", "waiting")),
            PanelHarness.Session(2, "waiting", CustomQuestion("q2"), PanelHarness.Turn("Second", "", "waiting")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(2);
        h.Frame();
        Assert.True(h.Click("askCustom", 0, s: "q2"));
        h.Type("my own answer");
        h.Key(UiKey.Enter);
        var sent = h.Last<AnswerCommand>();
        Assert.Equal((2, "q2"), (sent.Id, sent.Ask));
        Assert.Equal([["my own answer"]], sent.Answers);
        Assert.Single(h.Sent.OfType<AnswerCommand>());
    }

    [Fact]
    public void Chips_that_do_not_fit_the_reply_row_wrap_and_the_rest_is_counted_so_that_what_is_sent_is_what_is_shown()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Tidy", "Done.")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        Assert.True(h.Click("compose"));
        for (var i = 0; i < 14; i++) h.Ui.AttachChip(1, Hover.Chat.Desk.Chip.FileLive($"src/file-number-{i:00}.txt"));
        var c = h.Frame();
        var composer = h.Ui.Find("composer")!.Value.R;
        var shown = h.Ui.Targets.Where(t => t.Id == "unchip").ToList();
        var plus = c.Texts.SingleOrDefault(t => t.StartsWith('+') && t.Length > 1 && char.IsDigit(t[1]));
        Assert.NotNull(plus);
        var hidden = int.Parse(plus![1..]);
        Assert.True(hidden > 0 && shown.Count > 0);
        Assert.Equal(14, shown.Count + hidden);
        Assert.All(shown, t => Assert.True(composer.Contains(t.R.X, t.R.Y) && composer.Contains(t.R.Right, t.R.Bottom), $"the remove button {t.R} is outside the box {composer}"));
        Assert.True(shown.Select(t => t.R.Y).Distinct().Count() > 1, "the chips wrap onto more than one row");

        // Removing one brings the next into view; the count follows. Sending takes every chip, the ones counted too.
        h.Click("unchip", 0);
        var after = h.Frame().Texts.Single(t => t.StartsWith('+') && t.Length > 1 && char.IsDigit(t[1]));
        Assert.Equal(13, h.Ui.Targets.Count(t => t.Id == "unchip") + int.Parse(after[1..]));
        h.Type("go");
        h.Key(UiKey.Enter);
        Assert.Equal(13, h.Last<ReplyCommand>().Chips!.Count());
    }
}
