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

    // MARK: Wheels and Escape

    [Fact]
    public void The_wheel_over_a_reply_box_with_more_lines_than_fit_scrolls_the_box_and_not_the_thread_behind_it()
    {
        var h = new PanelHarness();
        var answer = string.Join("\n\n", Enumerable.Range(0, 80).Select(i => "Paragraph number " + i));
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Tidy", answer)));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        Assert.True(h.Click("compose"));
        for (var i = 0; i < 30; i++)
        {
            h.Type("a line " + i);
            h.Key(UiKey.Enter, UiMods.Shift);
        }
        var boxScroll = h.Ui.ReplyScrollY;
        Assert.True(boxScroll > 0, "the words are taller than the box, and the caret is at their end");
        var threadAt = h.Ui.ScrollY;
        var box = h.Ui.Find("composer")!.Value.R;
        Assert.True(h.Ui.Wheel(box.X + box.W / 2, box.Y + box.H / 2, 60));
        h.Now += 2000;
        h.Frame();
        Assert.True(h.Ui.ReplyScrollY < boxScroll, "the box's words moved up the page");
        Assert.Equal(threadAt, h.Ui.ScrollY);
        // The place stays where the wheel left it; typing brings the caret back into view.
        var left = h.Ui.ReplyScrollY;
        h.Now += 2000;
        h.Frame();
        Assert.Equal(left, h.Ui.ReplyScrollY);
        h.Type("x");
        Assert.True(h.Ui.ReplyScrollY > left);

        // Over the thread the wheel still scrolls the thread.
        var area = h.Ui.ThreadArea;
        Assert.True(h.Ui.Wheel(area.X + area.W / 2, area.Y + area.H / 2, 60));
        h.Now += 2000;
        h.Frame();
        Assert.True(h.Ui.ScrollY < threadAt);
    }

    static PanelHarness DeskPanel(string tab, float w = 1200, float height = 720)
    {
        var h = new PanelHarness { W = w, H = height };
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Fix it", "Fixed.")));
        h.Frame();
        h.Ui.OpenDeskPanel(1, tab);
        h.Frame();
        h.Accept(new DeskEvent
        {
            Id = 1, What = "probe",
            Data = System.Text.Json.JsonDocument.Parse("""{"folder":true,"git":true,"branch":"feat/x","changed":3,"add":29,"del":11,"gh":true,"ghAuth":true,"pr":null,"commands":4,"agents":2,"running":1,"pages":0,"linked":1}""").RootElement.Clone(),
        });
        return h;
    }

    [Fact]
    public void The_wheel_over_the_desk_tab_strip_moves_the_tabs_sideways_when_they_overflow_and_never_scrolls_the_panel()
    {
        var h = DeskPanel("terminal", 520, 346);
        var lines = Enumerable.Range(1, 3000).Select(i => new TermLine { Text = "tick " + i }).ToList();
        h.Accept(new TermEvent
        {
            Id = 1, Ok = true, Cwd = "/w", Running = true, Total = 1, From = 0,
            Entries = [new TermEntryInfo { Cwd = "/w", Cmd = "ping -t host", Lines = lines, Run = new TermRun { Kind = "running" } }],
        });
        Assert.Contains("tick 3000", h.Frame().Texts);
        var tabs = h.Ui.Targets.Where(t => t.Id == "deskTab").ToList();
        var panel = h.Ui.DeskPanelRect;
        Assert.True(tabs.Count > 3);
        var strip = (X: panel.X + 40, Y: tabs[0].R.Y + 5);
        var firstX = tabs[0].R.X;
        var tabsFit = tabs[^1].R.Right <= panel.Right - 10 - 28 - 6;
        Assert.False(tabsFit, $"the tabs do not all fit in a panel this narrow (the panel takes the whole of a window this small), so the strip scrolls: {panel} {string.Join(" ", tabs.Select(t => t.R))}");

        // Wheeled down (negative) the tabs go left; the output below stays at its end.
        Assert.True(h.Ui.Wheel(strip.X, strip.Y, -90));
        h.Frame();
        Assert.True(h.Ui.Targets.First(t => t.Id == "deskTab").R.X < firstX, "the tabs moved sideways");
        Assert.Contains("tick 3000", h.Frame().Texts);
        // And back.
        Assert.True(h.Ui.Wheel(strip.X, strip.Y, 900));
        h.Frame();
        Assert.Equal(firstX, h.Ui.Targets.First(t => t.Id == "deskTab").R.X, 0.5);
        Assert.Contains("tick 3000", h.Frame().Texts);

        // Wide enough for every tab: the wheel over the strip is taken and moves nothing, and the output stays.
        var wide = DeskPanel("terminal", 1920, 1080);
        wide.Accept(new TermEvent
        {
            Id = 1, Ok = true, Cwd = "/w", Running = true, Total = 1, From = 0,
            Entries = [new TermEntryInfo { Cwd = "/w", Cmd = "ping -t host", Lines = lines, Run = new TermRun { Kind = "running" } }],
        });
        var wt = wide.Ui.Targets.Where(t => t.Id == "deskTab").ToList();
        var x0 = wt[0].R.X;
        Assert.True(wide.Ui.Wheel(wt[0].R.X + 5, wt[0].R.Y + 5, 90));
        wide.Frame();
        Assert.Equal(x0, wide.Ui.Targets.First(t => t.Id == "deskTab").R.X, 0.5);
        Assert.Contains("tick 3000", wide.Frame().Texts);
    }

    [Fact]
    public void Escape_in_a_box_of_the_desk_panel_leaves_the_box_and_a_second_Escape_closes_the_panel()
    {
        var h = DeskPanel("terminal");
        h.Accept(new TermEvent { Id = 1, Ok = true, Cwd = "/w", Total = 0, From = 0 });
        h.Type("ls");
        Assert.True(h.Ui.TakesKeys, "the prompt has the keyboard");
        Assert.True(h.Key(UiKey.Escape));
        Assert.False(h.Ui.TakesKeys, "Escape left the box");
        Assert.Equal(1, h.Ui.DeskPanelId);
        Assert.Equal("ls", h.Ui.Field("term"));
        Assert.True(h.Key(UiKey.Escape));
        Assert.Null(h.Ui.DeskPanelId);
    }

    // MARK: The desk card

    static SessionInfo Answered(string answer) => PanelHarness.Session(1, "done", null, PanelHarness.Turn("Tidy", answer, "done"));

    [Fact]
    public void The_desk_cards_status_box_scrolls_with_the_wheel_when_its_content_is_cut_and_a_button_scrolled_out_of_it_is_gone()
    {
        // A window this short leaves the status box 56 px; a four-line answer and its line of counts are taller.
        var h = new PanelHarness { W = 828, H = 250 };
        h.State(Answered(string.Join(" ", Enumerable.Range(0, 120).Select(i => "word" + i))));
        h.Ui.OpenDeskCard(1);
        h.Frame();
        var box = h.Ui.DeskCardStatusRect;
        Assert.True(h.Ui.DeskCardStatusScrollMax > 0, "the content is taller than the box");
        Assert.Equal(0, h.Ui.DeskCardStatusScroll);
        Assert.True(h.Ui.Wheel(box.X + box.W / 2, box.Y + box.H / 2, -30));
        h.Frame();
        Assert.Equal(30, h.Ui.DeskCardStatusScroll, 0.5);
        h.Ui.Wheel(box.X + 5, box.Y + 5, -900);
        h.Frame();
        Assert.Equal(h.Ui.DeskCardStatusScrollMax, h.Ui.DeskCardStatusScroll, 0.5);
        h.Ui.Wheel(box.X + 5, box.Y + 5, 900);
        h.Frame();
        Assert.Equal(0, h.Ui.DeskCardStatusScroll);

        // A question's buttons are in the box: one scrolled out of it answers no click.
        var ask = PanelHarness.Permission("p1") with { Command = new string('x', 300) };
        var asking = PanelHarness.Session(1, "waiting", ask, PanelHarness.Turn("Tidy", "", "waiting"));
        var q = new PanelHarness { W = 828, H = 250 };
        q.State(asking);
        q.Ui.OpenDeskCard(1);
        q.Frame();
        var qbox = q.Ui.DeskCardStatusRect;
        var deny = q.Ui.Find("cardDeny");
        Assert.NotNull(deny);
        Assert.True(qbox.Contains(deny!.Value.R.X + 1, deny.Value.R.Y + 1) || q.Ui.DeskCardStatusScrollMax > 0);
        Assert.All(q.Ui.Targets.Where(t => t.Id.StartsWith("cardD", StringComparison.Ordinal) || t.Id is "cardReview" or "cardAllow" or "cardTrust"),
            t => Assert.True(t.R.Y >= qbox.Y - 0.5f && t.R.Bottom <= qbox.Bottom + 0.5f, $"{t.Id} {t.R} is outside the status box {qbox}"));
    }

    [Fact]
    public void The_desk_cards_title_is_cut_with_an_ellipsis_and_its_Editor_and_Expand_chips_stay_whole()
    {
        var h = new PanelHarness { W = 828, H = 480 };
        var s = Answered("Done.") with { Title = string.Concat(Enumerable.Repeat("A very long title that goes on and on ", 8)), Folder = @"C:\work\a-folder-with-a-rather-long-name" };
        h.State(s);
        h.Ui.OpenDeskCard(1);
        var c = h.Frame();
        var card = h.Ui.DeskCardRect;
        foreach (var id in new[] { "cardEditor", "cardExpand" })
        {
            var r = h.Ui.Find(id)!.Value.R;
            Assert.True(r.X >= card.X && r.Right <= card.Right - 6 + 0.5f, $"{id} {r} is cut by the card {card}");
        }
        var title = c.Drawn.Single(t => t.Text.StartsWith("A very long title", StringComparison.Ordinal));
        Assert.EndsWith("…", title.Text);
        Assert.True(title.R.Right <= card.Right - 6 + 0.5f, $"the title {title.R} runs past the card {card}");

        // A narrow card drops the chips at the end of the row until the buttons fit; the status chip stays.
        var narrow = new PanelHarness { W = 360, H = 480 };
        narrow.State(s);
        narrow.Ui.OpenDeskCard(1);
        narrow.Frame();
        var ncard = narrow.Ui.DeskCardRect;
        foreach (var id in new[] { "cardEditor", "cardExpand" })
        {
            var r = narrow.Ui.Find(id)!.Value.R;
            Assert.True(r.Right <= ncard.Right - 6 + 0.5f, $"{id} {r} is cut by the narrow card {ncard}");
        }
    }

    // MARK: Layers over a button

    static bool Lit(RecordingCanvas c, Rect4 button) => c.Boxes.Any(b => b.R.X == button.X && b.R.Y == button.Y && b.R.W == button.W && b.R.H == button.H);

    [Fact]
    public void A_button_under_a_panel_drawn_over_it_gets_no_hover_look_and_no_click()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = false;
        h.Frame();
        var button = h.Ui.Find("topMenu")!.Value.R;
        var at = (X: button.X + button.W / 2, Y: button.Y + button.H / 2);

        // Nothing over it: the pointer lights it, shows its name and is the hand.
        h.Ui.PointerMove(at.X, at.Y);
        var c = h.Frame();
        Assert.True(Lit(c, button), "the button is lit under the pointer");
        Assert.Contains("Menu", c.Texts);
        Assert.Equal(UiCursor.Hand, h.Ui.CursorAt(at.X, at.Y));

        // The history panel comes over the place: the button is under it.
        h.Ui.OpenOfficePanel("history");
        h.Frame();
        h.Ui.PointerMove(0, 0);
        h.Frame();
        h.Ui.PointerMove(at.X, at.Y);
        c = h.Frame();
        Assert.True(h.Ui.OfficePanel.Length > 0 && h.Ui.Find("officePanel")!.Value.R.Contains(at.X, at.Y), "the panel is over the button");
        Assert.False(Lit(c, button), "a button under the panel is not lit");
        Assert.DoesNotContain("Menu", c.Texts);
        Assert.NotEqual("topMenu", h.Ui.TargetAt(at.X, at.Y)?.Id);
        h.Ui.PointerDown(at.X, at.Y, 1, UiMods.None);
        h.Ui.PointerUp(at.X, at.Y);
        h.Frame();
        Assert.Null(h.Ui.OpenMenuKind);
    }

    [Fact]
    public void A_button_under_a_toast_gets_no_hover_look_and_no_click_either()
    {
        var h = new PanelHarness { W = 400, H = 346 };
        h.State();
        h.Ui.ChatView = false;
        h.Frame();
        var button = h.Ui.Find("topMenu")!.Value.R;
        h.Ui.Say("Codex is not signed in. Open Settings to sign in. Open it again.");
        h.Frame();
        h.Frame();
        // The point is on the button's edge that the toast reaches.
        var at = (X: button.X + 3, Y: button.Y + button.H / 2);
        var toast = h.Canvas.Drawn.Single(t => t.Text.StartsWith("Codex is not", StringComparison.Ordinal)).R;
        Assert.True(toast.Contains(at.X, at.Y), $"the toast {toast} is over the point {at} of the button {button}");
        h.Ui.PointerMove(at.X, at.Y);
        var c = h.Frame();
        Assert.False(Lit(c, button));
        Assert.True(h.Ui.TargetAt(at.X, at.Y) is null, "nothing answers under the toast");
        h.Ui.PointerDown(at.X, at.Y, 1, UiMods.None);
        h.Ui.PointerUp(at.X, at.Y);
        h.Frame();
        Assert.Null(h.Ui.OpenMenuKind);
    }

    // MARK: Tips

    static OfficeUi TipUi(Func<(int Kind, int Id, string Hint)> tip, SessionInfo session)
    {
        var ui = new OfficeUi(new FakeShaper(), new UiEnv { Send = _ => { }, Clipboard = new FakeClipboard(), Tip = tip });
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [session] });
        return ui;
    }

    [Fact]
    public void The_tip_over_a_bot_is_not_shown_while_its_chat_is_open_or_while_a_desk_card_is_open()
    {
        var session = PanelHarness.Session(1, "working", null, PanelHarness.Turn("Go", "", "working"));
        var tip = (Kind: 1, Id: 1, Hint: "");
        var ui = TipUi(() => tip, session);
        string[] Drawn() { var c = new RecordingCanvas(); ui.Paint(c, 1200, 720, 1000); return [.. c.Texts]; }
        ui.PointerMove(900, 400);
        Assert.Contains("Chat with", Drawn());

        // The bot whose chat is open needs no "Chat with".
        ui.OpenChat(1);
        ui.ChatView = false;
        ui.PointerMove(100, 400);
        Assert.DoesNotContain("Chat with", Drawn());
        // Another bot's tip still shows.
        tip = (1, 2, "");
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [session, session with { Id = 2, Key = "k2" }] });
        Assert.Contains("Chat with", Drawn());

        // With a desk card open neither a bot nor a desk has its tip.
        ui.CloseChat();
        ui.OpenDeskCard(1);
        tip = (1, 1, "");
        Assert.DoesNotContain("Chat with", Drawn());
        tip = (2, 1, "");
        Assert.DoesNotContain(Drawn(), t => t.EndsWith("’s desk", StringComparison.Ordinal) && t != "Pip’s desk".ToUpperInvariant());
    }

    [Fact]
    public void The_office_and_chat_switch_names_the_half_the_pointer_is_on()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = false;
        h.Frame();
        var office = h.Ui.Targets.First(t => t.Id == "view" && t.N == 0).R;
        var chat = h.Ui.Targets.First(t => t.Id == "view" && t.N == 1).R;
        Assert.DoesNotContain("Office", h.Frame().Texts);
        h.Ui.PointerMove(office.X + office.W / 2, office.Y + office.H / 2);
        var c = h.Frame();
        Assert.Contains("Office", c.Texts);
        Assert.DoesNotContain("Chat", c.Texts);
        h.Ui.PointerMove(chat.X + chat.W / 2, chat.Y + chat.H / 2);
        c = h.Frame();
        Assert.Contains("Chat", c.Texts);
        Assert.DoesNotContain("Office", c.Texts);
        // The name sits under the switch.
        var name = c.Drawn.Single(t => t.Text == "Chat").R;
        Assert.True(name.Y > chat.Bottom, $"the name {name} is under the switch {chat}");
    }

    // MARK: The folder list

    [Fact]
    public void A_folder_row_shows_its_path_on_one_cut_line_and_says_the_whole_of_it_in_a_tip()
    {
        var path = @"C:\Users\someone\Documents\clients\acme\projects\a-very-long-folder-name\app";
        var h = new PanelHarness { W = 828, H = 346 };
        h.Accept(new StateEvent
        {
            CanStart = true, Tool = "kiro", Folder = path,
            Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Access = "full" }],
            History = [new HistoryRow { Key = "a", Tool = "kiro", Title = "Chat a", Folder = path.Replace("app", "api"), At = 1_793_000_300_000 }],
        });
        h.Ui.ChatView = true;
        h.Frame();
        Assert.True(h.Click("pillFolder"));
        Assert.Equal("project", h.Ui.OpenMenuKind);
        var rows = h.Ui.Targets.Where(t => t.Id == "menu" && t.S is "app" or "api").ToList();
        Assert.Equal(2, rows.Count);
        // Two lines high (the name and one cut line); the wrapped path was a third line taller or more.
        Assert.All(rows, r => Assert.InRange(r.R.H, 30, 52));
        var c = h.Frame();
        Assert.DoesNotContain(path, c.Texts);
        var cut = c.Texts.Where(t => t.StartsWith(@"C:\Users", StringComparison.Ordinal)).ToList();
        Assert.Equal(2, cut.Count);
        Assert.All(cut, t => Assert.EndsWith("…", t));

        // The pointer on a row says the path in full.
        var row = rows[0].R;
        h.Ui.PointerMove(row.X + row.W / 2, row.Y + row.H / 2);
        Assert.Contains(h.Frame().Texts, t => t.StartsWith(@"C:\Users", StringComparison.Ordinal) && !t.EndsWith("…", StringComparison.Ordinal));
    }
}
