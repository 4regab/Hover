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
}
