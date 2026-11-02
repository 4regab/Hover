using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// Faults the sweep of the notch, office, new-task bar, chat and desk found against the Go app. Each test here failed before its fix
/// (see reports/sweep1.md); the sweep's own invariants are in SweepTests.
/// </summary>
public class SweepFixTests
{
    static bool Dot(PanelHarness rig)
    {
        var fab = rig.Ui.Find("fab")!.Value.R;
        return rig.Canvas.Boxes.Any(b => b.C == Pen.Lilac && b.R.W == 6 && b.R.H == 6 && fab.Contains(b.R.X + 3, b.R.Y + 3));
    }

    [Fact]
    public void The_new_task_button_shows_its_lilac_dot_only_while_words_wait_in_the_box()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        rig.Ui.ChatView = false;
        rig.Frame();
        Assert.False(Dot(rig), "nothing waits in the box, so the button has no dot");
        Assert.True(rig.Click("fab"));
        rig.Type("fix it");
        rig.Key(UiKey.Escape);
        Assert.NotNull(rig.Ui.Find("fab"));
        Assert.True(Dot(rig), "words wait in the box: the dot shows");
        rig.Ui.ClearTask();
        rig.Frame();
        Assert.False(Dot(rig), "the box is empty again: the dot is gone");
    }

    [Fact]
    public void The_repository_menu_has_its_heading_and_a_search_box_that_filters_and_Enter_picks_the_first_match()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        MenuFitTests.OfficeBox(rig);
        Assert.True(rig.Click("cloud"));
        Assert.True(rig.Click("pillRepo"));
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        Assert.Contains("KIRO WEB CLONES", rig.Canvas.Texts);
        Assert.Contains("Search repositories", rig.Canvas.Texts);
        Assert.NotNull(rig.Ui.Find("menuSearch"));
        Assert.Equal("", rig.Ui.MenuSearchText);
        Assert.Equal(46, rig.Ui.OpenMenuPickable.Count);

        // Typing keeps the repositories that match (in any case); the heading, the box and "Empty workspace" stay.
        rig.Type("REPO-4");
        Assert.Equal("REPO-4", rig.Ui.MenuSearchText);
        var shown = rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.S).ToList();
        Assert.Equal(["Empty workspace", "me/repo-4", "me/repo-40", "me/repo-41", "me/repo-42", "me/repo-43", "me/repo-44", "me/repo-45"], shown);
        rig.Key(UiKey.Backspace);
        Assert.Equal("REPO-", rig.Ui.MenuSearchText);

        // Nothing matches: the Go words, and Enter picks nothing.
        rig.Type("zzz");
        Assert.Equal(["Empty workspace"], rig.Ui.Targets.Where(t => t.Id == "menu").Select(t => t.S));
        Assert.Contains("No repository matches “REPO-zzz”.", rig.Canvas.Texts);
        rig.Key(UiKey.Enter);
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        rig.Key(UiKey.A, UiMods.Control);
        rig.Type("repo-4");

        // Enter picks the first match and closes the menu; the task is for that repository.
        rig.Key(UiKey.Enter);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.Contains("repo-4", rig.Canvas.Texts);
        Assert.True(rig.Click("taskText"));
        rig.Type("web task");
        Assert.True(rig.Click("startTask"));
        Assert.Equal(["me/repo-4"], rig.Last<NewTaskCommand>().Cloud);
    }

    [Fact]
    public void Enter_in_the_repository_search_does_nothing_while_it_is_empty_and_Escape_closes_the_menu()
    {
        var rig = MenuFitTests.Rig(1200, 720);
        MenuFitTests.OfficeBox(rig);
        Assert.True(rig.Click("cloud"));
        Assert.True(rig.Click("pillRepo"));
        rig.Key(UiKey.Enter);
        Assert.Equal("repo", rig.Ui.OpenMenuKind);
        // The Up and Down keys light a row; Enter then picks that row, as in every other menu.
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Down);
        rig.Key(UiKey.Enter);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.True(rig.Click("pillRepo"));
        rig.Key(UiKey.Escape);
        Assert.Null(rig.Ui.OpenMenuKind);
    }

    /// <summary>The box's folder chip says "Choose a folder", or the start of it with an ellipsis where the bar is short.</summary>
    static bool Chip(PanelHarness rig) => rig.Canvas.Texts.Any(t => t.StartsWith("Choose", StringComparison.Ordinal) && "Choose a folder".StartsWith(t.TrimEnd('…'), StringComparison.Ordinal));

    [Fact]
    public void The_boxs_folder_chip_says_Choose_a_folder_and_opens_the_folder_dialog_at_once_while_it_has_no_folder()
    {
        var rig = MenuFitTests.Rig(1200, 720, pickedFolder: "");
        MenuFitTests.OfficeBox(rig);
        Assert.True(Chip(rig), "the chip says Choose a folder (cut with an ellipsis in a box this narrow)");
        Assert.DoesNotContain("Folder", rig.Canvas.Texts);
        rig.FolderAnswer = null;
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal(1, rig.FolderAsked);
        Assert.Null(rig.Ui.OpenMenuKind);
        Assert.True(Chip(rig));

        // A folder picked is named by its last part, and the chip then opens the list of recent folders, which ends in "Open folder…".
        rig.FolderAnswer = @"C:\work\picked";
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal(2, rig.FolderAsked);
        Assert.Contains("picked", rig.Canvas.Texts);
        Assert.False(Chip(rig));
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal("project", rig.Ui.OpenMenuKind);
        Assert.Equal(2, rig.FolderAsked);
        Assert.Contains("Open folder…", rig.Canvas.Texts);
        Assert.True(rig.Click("menu", s: "Open folder…"));
        Assert.Equal(3, rig.FolderAsked);
    }

    [Fact]
    public void The_start_screens_folder_chip_always_opens_the_list()
    {
        var rig = MenuFitTests.Rig(1200, 720, pickedFolder: "");
        MenuFitTests.StartScreen(rig);
        Assert.Contains("Choose a folder", rig.Canvas.Texts);
        Assert.True(rig.Click("pillFolder"));
        Assert.Equal("project", rig.Ui.OpenMenuKind);
        Assert.Equal(0, rig.FolderAsked);
    }
    // MARK: Keys, the reply box and the chat

    static PanelHarness OpenChat(int id = 1, bool chatView = false, TurnInfo? turn = null, string stage = "done")
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(id, stage, null, turn ?? PanelHarness.Turn("Tidy it", "Done. " + new string('x', 40))));
        h.Ui.ChatView = chatView;
        h.Ui.OpenChat(id);
        h.Frame();
        return h;
    }

    [Fact]
    public void Escape_closes_the_panel_over_the_office_before_the_notch_folds()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.OpenOfficePanel("history");
        h.Frame();
        Assert.Equal("history", h.Ui.OfficePanel);
        Assert.True(h.Key(UiKey.Escape), "Escape was left to the notch, which folds");
        Assert.Equal("", h.Ui.OfficePanel);
        Assert.False(h.Key(UiKey.Escape), "with nothing left to close, Escape is the notch's");
    }

    [Fact]
    public void A_box_with_the_keyboard_takes_every_plain_key_so_the_office_behind_does_not_zoom()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = false;
        h.Frame();
        Assert.True(h.Click("fab"));
        Assert.True(h.Ui.TakesKeys);
        // The digits and the minus and plus keys are the office camera's; typed in a name they are the box's alone.
        Assert.True(h.Key(UiKey.Other), "a plain key the box does not use is left to the office");
        Assert.False(h.Key(UiKey.Other, UiMods.Control), "Ctrl+key chords stay the host's");
    }

    [Fact]
    public void A_press_on_the_thread_takes_the_keyboard_from_the_reply_box_and_an_empty_box_closes()
    {
        var h = OpenChat();
        Assert.True(h.Click("compose"));
        Assert.True(h.Ui.Composing);
        var area = h.Ui.ThreadArea;
        h.ClickAt((area.X + 4, area.Y + 4));
        Assert.False(h.Ui.Composing, "an empty reply box closes at a press on the thread");

        // With words in it, it stays; typing then gives it the keyboard back, and Escape closes it without leaving the chat.
        Assert.True(h.Click("compose"));
        h.Type("keep me");
        h.ClickAt((area.X + 4, area.Y + 4));
        Assert.True(h.Ui.Composing);
        h.Type("!");
        Assert.Equal("keep me!", h.Ui.ReplyText);
        h.ClickAt((area.X + 4, area.Y + 4));
        Assert.True(h.Key(UiKey.Escape));
        Assert.False(h.Ui.Composing);
        Assert.Equal(1, h.Ui.OpenId);
        Assert.Equal("keep me!", h.Ui.ReplyText);
    }

    [Fact]
    public void A_new_name_is_saved_when_the_title_box_is_left()
    {
        var h = OpenChat(chatView: true);
        Assert.True(h.Click("rename"));
        h.Type("A better name");
        var area = h.Ui.ThreadArea;
        h.ClickAt((area.X + 4, area.Y + 4));
        Assert.Equal("A better name", h.Last<RenameCommand>().Name);
        Assert.Null(h.Ui.Find("titleBox"));
    }

    [Fact]
    public void Taking_a_queued_message_back_or_editing_a_sent_one_keeps_what_was_already_written()
    {
        var h = OpenChat(chatView: true);
        Assert.True(h.Click("compose"));
        h.Type("keep me");
        h.Accept(new QueueRemoveEvent { Ok = true, Text = "queued words" });
        Assert.Equal("keep me\nqueued words", h.Ui.ReplyText);
        var edit = h.Ui.ThreadActs().First(a => a.Kind == "EditPrompt");
        h.ClickAt((edit.R.X + edit.R.W / 2, edit.R.Y + edit.R.H / 2));
        Assert.Equal("keep me\nqueued words\nTidy it", h.Ui.ReplyText);
    }

    [Fact]
    public void Retry_goes_only_for_the_newest_turn_of_a_chat_that_is_not_busy_and_takes_the_prompts_pictures()
    {
        var turn = PanelHarness.Turn("Tidy it", "Done.") with { Images = ["data:image/png;base64,AAAA"] };
        var h = OpenChat(chatView: true, turn: turn);
        var retry = h.Ui.ThreadActs().FirstOrDefault(a => a.Kind == "Retry");
        Assert.NotEqual(default, retry);
        h.ClickAt((retry.R.X + retry.R.W / 2, retry.R.Y + retry.R.H / 2));
        var sent = h.Last<ReplyCommand>();
        Assert.Equal("Tidy it", sent.Text);
        Assert.Equal(["data:image/png;base64,AAAA"], sent.Images);

        // While a run is in flight the button does nothing.
        var busy = OpenChat(chatView: true, turn: PanelHarness.Turn("Tidy it", "Done.") with { Stage = "done" }, stage: "working");
        var again = busy.Ui.ThreadActs().FirstOrDefault(a => a.Kind == "Retry");
        if (again != default) busy.ClickAt((again.R.X + again.R.W / 2, again.R.Y + again.R.H / 2));
        Assert.DoesNotContain(busy.Sent, c => c is ReplyCommand);
    }

    [Fact]
    public void A_stray_Enter_does_not_delete_a_chat_but_the_Delete_button_does()
    {
        var h = OpenChat(chatView: true);
        Assert.True(h.Click("more"));
        Assert.True(h.Click("menu", s: "Delete chat"));
        Assert.Contains("Delete this chat?", h.Frame().Texts);
        h.Key(UiKey.Enter);
        Assert.DoesNotContain(h.Sent, c => c is DeleteCommand);
        Assert.Contains("Delete this chat?", h.Frame().Texts);
        Assert.True(h.Click("confirmYes"));
        Assert.Contains(h.Sent, c => c is DeleteCommand);
    }

    [Fact]
    public void A_question_with_two_parts_is_not_sent_with_one_of_them_unanswered()
    {
        var ask = new AskInfo
        {
            Id = "q1", Kind = "question", Title = "Two questions",
            Questions =
            [
                new AskQuestion { Header = "A", Question = "Which database?", Options = [new AskOption { Label = "Postgres" }, new AskOption { Label = "SQLite" }] },
                new AskQuestion { Header = "B", Question = "Which cache?", Options = [new AskOption { Label = "Redis" }, new AskOption { Label = "None" }] },
            ],
        };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "waiting", ask, PanelHarness.Turn("Pick", "", "waiting")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        Assert.True(h.Click("askOpt", 0, s: "Postgres"));
        Assert.True(h.Click("askAnswer"));
        Assert.Equal("Answer each question first.", h.Ui.ToastText);
        Assert.DoesNotContain(h.Sent, c => c is AnswerCommand);
        Assert.True(h.Click("askOpt", 1, s: "None"));
        Assert.True(h.Click("askAnswer"));
        Assert.Equal([["Postgres"], ["None"]], h.Last<AnswerCommand>().Answers);
    }

    [Fact]
    public void The_thread_keeps_room_under_its_last_turn_for_the_reply_circle()
    {
        var h = OpenChat(chatView: true);
        Assert.Equal(56, h.Ui.Thread.ExtraBottom);
    }

    [Fact]
    public void The_pointer_is_the_hand_over_a_button_the_text_cursor_over_a_box_of_words_and_the_arrow_elsewhere()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = false;
        h.Frame();
        UiCursor At(string id) { var r = h.Ui.Find(id)!.Value.R; return h.Ui.CursorAt(r.X + r.W / 2, r.Y + r.H / 2); }
        Assert.Equal(UiCursor.Hand, At("fab"));
        Assert.Equal(UiCursor.Hand, At("topMenu"));
        Assert.Equal(UiCursor.Arrow, h.Ui.CursorAt(300, 300));
        Assert.True(h.Click("fab"));
        Assert.Equal(UiCursor.Text, At("taskText"));
        Assert.Equal(UiCursor.Hand, At("pillAccess"));
        // A button with nothing to send yet is not a button for the pointer.
        Assert.Equal(UiCursor.Arrow, At("startOff"));
        // A menu takes the pointer from what is under it.
        Assert.True(h.Click("pillAccess"));
        Assert.Equal(UiCursor.Arrow, h.Ui.CursorAt(5, 5));
        var row = h.Ui.Targets.Last(t => t.Id == "menu").R;
        Assert.Equal(UiCursor.Hand, h.Ui.CursorAt(row.X + row.W / 2, row.Y + row.H / 2));
    }
}
