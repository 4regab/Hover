using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Desk;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>The reply box's attachments, pictures and lists, the model pill, and the panels over the office (any system).</summary>
public class DeskReplyTests
{
    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    const string OnePixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    static SessionInfo Session(string stage = "done", int id = 1) => (PanelHarness.Session(id, stage, null, PanelHarness.Turn("Fix it", "All fixed.", stage == "working" ? "working" : "done"))) with
    {
        Commands = [new SlashCommand { Name = "compact", Description = "Shrink the context" }, new SlashCommand { Name = "review", Description = "Review the change" }],
    };

    static PanelHarness Chat(SessionInfo? s = null)
    {
        var h = new PanelHarness();
        h.State(s ?? Session());
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        return h;
    }

    static void Files(PanelHarness h, params string[] tree)
        => h.Accept(new DeskEvent { Id = 1, What = "files", Data = J("{\"git\":true,\"changed\":[],\"touched\":[],\"more\":false,\"tree\":" + JsonSerializer.Serialize(tree) + "}") });

    static void Compose(PanelHarness h)
    {
        Assert.True(h.Click("compose"));
    }

    // MARK: The stop square

    [Fact]
    public void An_empty_reply_box_stops_a_run_with_the_square_and_a_typed_reply_is_queued()
    {
        var h = Chat(Session("working"));
        Compose(h);
        Assert.NotNull(h.Ui.Find("pause"));
        Assert.Null(h.Ui.Find("send"));
        Assert.Contains("Queue a reply…", h.Frame().Texts);
        Assert.True(h.Click("pause"));
        Assert.Equal(1, h.Last<PauseCommand>().Id);
        h.Click("replyText");
        h.Type("then this");
        Assert.NotNull(h.Ui.Find("send"));
        Assert.Null(h.Ui.Find("pause"));
        h.Key(UiKey.Enter);
        Assert.Equal("then this", h.Last<ReplyCommand>().Text);
    }

    [Fact]
    public void A_stopped_runs_held_queue_is_resumed_from_the_note()
    {
        var s = Session("done") with { Held = true };
        var h = Chat(s);
        Assert.Contains("The queue is held after a stop.", h.Frame().Texts);
        Assert.True(h.Click("resume"));
        Assert.Equal(1, h.Last<QueueResumeCommand>().Id);
    }

    // MARK: Chips and the @ list

    [Fact]
    public void The_at_list_offers_files_and_a_pick_becomes_a_chip_that_goes_with_the_reply()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Type("Look at @app");
        // The files are asked for, and the list says it is reading until they arrive.
        Assert.Contains(h.Sent, c => c is DeskCommand { What: "files" });
        Assert.Contains("Reading the folder…", h.Frame().Texts);
        Files(h, "app.js", "src/app.ts", "README.md");
        var c = h.Frame();
        Assert.Contains("Files in project", c.Texts);
        Assert.Equal(["app.js", "app.ts"], h.Ui.PopRows.Where(r => r.Kind == 1).Select(r => r.Name).ToArray());
        // Down moves the lit row; Enter picks it; the @ and its word leave the words.
        h.Key(UiKey.Down);
        h.Key(UiKey.Enter);
        Assert.Equal("Look at ", h.Ui.ReplyText);
        var chip = Assert.Single(h.Ui.ChipsOf(1));
        Assert.Equal(("file", "src/app.ts", true), (chip.Kind, chip.Source, chip.Live));
        Assert.Contains("src/app.ts (reference)", h.Frame().Texts);
        Assert.Empty(h.Ui.PopRows);
        h.Type("please");
        h.Key(UiKey.Enter);
        var reply = h.Last<ReplyCommand>();
        Assert.Equal("Look at please", reply.Text);
        var json = Assert.Single(reply.Chips!);
        Assert.Equal("file", json.GetProperty("Kind").GetString());
        Assert.Equal("src/app.ts", json.GetProperty("Source").GetString());
        Assert.True(json.GetProperty("Live").GetBoolean());
        Assert.Empty(h.Ui.ChipsOf(1));
    }

    [Fact]
    public void A_chip_is_taken_off_with_its_cross_and_an_at_path_outside_the_folder_is_not_offered()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Type("@");
        Files(h, "a.txt", "dir/b.txt");
        Assert.Equal(["a.txt", "b.txt"], h.Ui.PopRows.Where(r => r.Kind == 1).Select(r => r.Name).ToArray());
        h.Key(UiKey.Enter);
        Assert.Single(h.Ui.ChipsOf(1));
        Assert.True(h.Click("unchip", 0));
        Assert.Empty(h.Ui.ChipsOf(1));
        // Only what the folder has can be picked: a path that climbs out finds nothing.
        h.Type("@../../etc/hosts");
        Assert.Empty(h.Ui.PopRows.Where(r => r.Kind == 1));
        Assert.Contains("No file matches that.", h.Frame().Texts);
        h.Key(UiKey.Enter);
        Assert.Empty(h.Ui.ChipsOf(1));
        h.Key(UiKey.Escape);
        Assert.Empty(h.Ui.PopRows);
    }

    [Fact]
    public void A_chip_the_backend_refuses_comes_back_into_the_box_with_the_words()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Type("Read @a.t");
        Files(h, "a.txt");
        h.Key(UiKey.Tab);
        h.Type("now");
        h.Key(UiKey.Enter);
        Assert.Single(h.Last<ReplyCommand>().Chips!);
        Assert.Equal("", h.Ui.ReplyText);
        Assert.Empty(h.Ui.ChipsOf(1));
        h.Accept(new ToastEvent { Text = "a.txt isn’t there any more." });
        Assert.Equal("Read now", h.Ui.ReplyText);
        Assert.Single(h.Ui.ChipsOf(1));
    }

    [Fact]
    public void The_slash_list_offers_the_agents_commands_and_Hovers_and_runs_Hovers_at_once()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Type("/");
        var names = h.Ui.PopRows.Where(r => r.Kind == 2).Select(r => r.Name).ToArray();
        Assert.Equal(["/compact", "/review", "/model", "/terminal", "/files", "/fork"], names);
        Assert.Contains("From Kiro", h.Frame().Texts);
        Assert.Contains("Hover", h.Frame().Texts);
        h.Type("co");
        Assert.Equal(["/compact"], h.Ui.PopRows.Where(r => r.Kind == 2).Select(r => r.Name).ToArray());
        h.Key(UiKey.Enter);
        Assert.Equal("/compact ", h.Ui.ReplyText);
        // A command that matches nothing says so.
        h.Ui.Key(UiKey.A, UiMods.Control);
        h.Type("/zzz");
        Assert.Contains("No command starts with that.", h.Frame().Texts);
        h.Ui.Key(UiKey.A, UiMods.Control);
        h.Type("/term");
        h.Key(UiKey.Enter);
        Assert.Equal("", h.Ui.ReplyText);
        Assert.Equal("terminal", h.Ui.DeskTabId);
        Assert.Equal(1, h.Ui.DeskPanelId);
    }

    [Fact]
    public void The_model_command_opens_the_model_menu_and_a_pick_is_sent_to_the_agent()
    {
        var h = new PanelHarness();
        h.Accept(new StateEvent
        {
            CanStart = true, Tool = "kiro", Folder = @"C:\work\project",
            Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Model = "auto", Models = [new ModelInfo { Id = "auto", Name = "Auto" }, new ModelInfo { Id = "opus", Name = "Opus 5.5", Levels = ["Low", "High"] }] }],
            Sessions = [Session()],
        });
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        Compose(h);
        Assert.NotNull(h.Ui.Find("modelPill"));
        Assert.Contains("Auto", h.Frame().Texts);
        Assert.True(h.Click("modelPill"));
        Assert.Equal("rmodel", h.Ui.OpenMenuKind);
        // Header, Default, Auto, Opus 5.5.
        Assert.True(h.Click("menu", 3));
        var set = h.Last<SetModelCommand>();
        Assert.Equal(("kiro", "opus"), (set.Tool, set.Model));
        Assert.Null(set.Effort);
        // The /model command opens the same menu.
        h.Click("replyText");
        h.Type("/model");
        h.Key(UiKey.Enter);
        Assert.Equal("rmodel", h.Ui.OpenMenuKind);
    }

    // MARK: Pictures

    [Fact]
    public void A_picture_is_pasted_into_the_reply_and_goes_with_it_and_five_are_too_many()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Clip.Image = OnePixel;
        h.Key(UiKey.V, UiMods.Control);
        Assert.Single(h.Ui.ShotsOf(1));
        h.Key(UiKey.V, UiMods.Control);
        h.Key(UiKey.V, UiMods.Control);
        h.Key(UiKey.V, UiMods.Control);
        Assert.Equal(4, h.Ui.ShotsOf(1).Count);
        h.Key(UiKey.V, UiMods.Control);
        Assert.Equal(4, h.Ui.ShotsOf(1).Count);
        Assert.Equal("Four images at most.", h.Ui.ToastText);
        // One is taken off with its cross; the box with only pictures can be sent.
        Assert.True(h.Click("unshot", 0));
        Assert.Equal(3, h.Ui.ShotsOf(1).Count);
        Assert.NotNull(h.Ui.Find("send"));
        h.Key(UiKey.Enter);
        var reply = h.Last<ReplyCommand>();
        Assert.Equal("", reply.Text);
        Assert.Equal(3, reply.Images!.Length);
        Assert.StartsWith("data:image/png;base64,", reply.Images[0]);
        Assert.Empty(h.Ui.ShotsOf(1));
    }

    [Fact]
    public void A_picture_over_eight_megabytes_and_a_file_that_is_not_a_picture_are_refused()
    {
        var h = Chat();
        Compose(h);
        h.Click("replyText");
        h.Clip.Image = "data:image/png;base64," + new string('A', 12 * 1024 * 1024);
        h.Key(UiKey.V, UiMods.Control);
        Assert.Empty(h.Ui.ShotsOf(1));
        Assert.Equal("That image is over 8 MB.", h.Ui.ToastText);
        h.Clip.Image = "data:text/plain;base64,QUJD";
        h.Key(UiKey.V, UiMods.Control);
        Assert.Equal("Only PNG, JPEG, GIF and WebP images.", h.Ui.ToastText);
        // The attach button asks the system's dialog; a cancelled dialog changes nothing, a refused file says why.
        h.Pick = () => null;
        Assert.True(h.Click("attachReply"));
        Assert.Empty(h.Ui.ShotsOf(1));
        h.Pick = () => new PickedImage(null, "That image is over 8 MB.");
        h.Click("attachReply");
        Assert.Equal("That image is over 8 MB.", h.Ui.ToastText);
        h.Pick = () => new PickedImage(OnePixel, null);
        h.Click("attachReply");
        Assert.Single(h.Ui.ShotsOf(1));
        // Without a picture on the clipboard, Ctrl+V pastes words.
        h.Clip.Image = null;
        h.Clip.Value = "pasted words";
        h.Key(UiKey.V, UiMods.Control);
        Assert.Equal("pasted words", h.Ui.ReplyText);
    }

    [Fact]
    public void A_picture_pasted_into_the_new_task_goes_with_the_task()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = true;
        h.Frame();
        h.Click("taskText");
        h.Clip.Image = OnePixel;
        h.Key(UiKey.V, UiMods.Control);
        Assert.Single(h.Ui.TaskShots);
        h.Type("describe it");
        h.Click("startTask");
        var task = h.Last<NewTaskCommand>();
        Assert.Equal([OnePixel], task.Images);
        Assert.Empty(h.Ui.TaskShots);
    }

    // MARK: The panels over the office

    static SessionInfo Working(int id, string title, int bot, int ctx) => PanelHarness.Session(id, "working", null, PanelHarness.Turn("go", "", "working", new StepInfo { K = "read", Verb = "Read", Name = "a.ts" })) with { Title = title, Bot = bot, Ctx = ctx };

    [Fact]
    public void The_board_lists_the_sessions_by_how_they_are_going_and_a_card_opens_its_chat()
    {
        var h = new PanelHarness();
        h.State(Working(1, "Make it fast", 0, 12), PanelHarness.Session(2, "done", null, PanelHarness.Turn("x", "ok")) with { Title = "Tidy imports", Bot = 1 });
        string? shown = null;
        h.Ui.OfficePanelChanged += n => shown = n;
        h.Ui.OpenOfficePanel("board");
        var c = h.Frame();
        Assert.Equal("board", shown);
        Assert.Contains("Session board", c.Texts);
        Assert.Contains("2 of 6 desks in use", c.Texts);
        Assert.Contains("DOING", c.Texts);
        Assert.Contains("FINISHED", c.Texts);
        Assert.Contains("WAKING", c.Texts);
        Assert.Contains("Nobody here", c.Texts);
        Assert.Contains("Make it fast", c.Texts);
        Assert.Contains("Pip · Kiro · Working", c.Texts);
        Assert.True(h.Click("panelRow", 3));
        Assert.Equal(1, h.Ui.OpenId);
        Assert.Equal("", h.Ui.OfficePanel);
        Assert.Equal("", shown);
        // The X closes it.
        h.Ui.OpenOfficePanel("tv");
        h.Frame();
        Assert.True(h.Click("panelClose"));
        Assert.Equal("", h.Ui.OfficePanel);
    }

    [Fact]
    public void The_overview_counts_the_sessions_and_shows_each_ones_context()
    {
        var h = new PanelHarness();
        h.State(Working(1, "Make it fast", 0, 12), PanelHarness.Session(2, "failed", null, PanelHarness.Turn("x", "no")) with { Title = "Broken", Bot = 1 });
        h.Ui.OpenOfficePanel("tv");
        var c = h.Frame();
        Assert.Contains("Office overview", c.Texts);
        Assert.Contains("CONTEXT USED", c.Texts);
        Assert.Contains("12%", c.Texts);
        Assert.Contains("—", c.Texts);
        Assert.Equal(["Working", "Done", "Failed", "Stopped"], new[] { "Working", "Done", "Failed", "Stopped" }.Where(c.Texts.Contains));
    }

    [Fact]
    public void The_history_lists_saved_chats_by_day_finds_them_and_deletes_one_after_asking()
    {
        var h = new PanelHarness();
        var now = new DateTimeOffset(2026, 11, 1, 7, 20, 0, TimeSpan.Zero).ToUnixTimeMilliseconds();
        h.Accept(new StateEvent
        {
            CanStart = true, Tool = "kiro", Folder = @"C:\work\project", Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }],
            Sessions = [],
            History =
            [
                new HistoryRow { Key = "k1", Tool = "kiro", Title = "Fix the build", Folder = @"C:\work\project", At = now - 60_000, Stage = "done" },
                new HistoryRow { Key = "k2", Tool = "codex", Title = "Write the docs", Folder = @"C:\work\docs", At = now - 3 * 86_400_000L, Stage = "failed" },
            ],
        });
        h.Ui.OpenOfficePanel("history");
        Assert.Contains(h.Sent, c => c is WebSessionsCommand);
        Assert.Contains(h.Sent, c => c is CliSessionsCommand);
        h.Accept(new WebSessionsEvent { Ok = true, Sessions = [] });
        var c = h.Frame();
        Assert.Contains("Session history", c.Texts);
        Assert.Contains("2 sessions, kept until you delete them", c.Texts);
        Assert.Contains("Fix the build", c.Texts);
        Assert.Contains("Write the docs", c.Texts);
        Assert.Contains("TODAY", c.Texts);
        // Typing in the find box narrows the list.
        Assert.True(h.Click("field", s: "histFind"));
        h.Type("docs");
        var narrowed = h.Frame().Texts;
        Assert.DoesNotContain("Fix the build", narrowed);
        Assert.Contains("Write the docs", narrowed);
        h.Type("zzz");
        Assert.Contains("Nothing matches.", h.Frame().Texts);
        h.Ui.Key(UiKey.A, UiMods.Control);
        h.Ui.Key(UiKey.Backspace, UiMods.None);
        h.Frame();
        // A bin on a row deletes it, after a question.
        Assert.True(h.Click("panelDelete", 3));
        Assert.Contains("Delete this chat?", h.Frame().Texts);
        Assert.True(h.Click("confirmYes"));
        var del = h.Last<DeleteCommand>();
        Assert.Equal("k2", del.Key);
        // A click on a saved row asks for its transcript.
        Assert.True(h.Click("panelRow", 1));
        Assert.Contains(h.Sent, c => c is HistoryCommand { Key: "k1" });
    }

    [Fact]
    public void The_history_lists_Kiro_Web_and_Kiro_CLI_sessions_and_adopts_one_on_a_click()
    {
        var h = new PanelHarness();
        var now = new DateTimeOffset(2026, 11, 1, 7, 20, 0, TimeSpan.Zero).ToUnixTimeMilliseconds();
        h.State();
        h.Ui.OpenOfficePanel("history");
        h.Accept(new WebSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "w1", Title = "Cloud task", Updated = now - 120_000 }] });
        h.Accept(new CliSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "c1", Title = "", Folder = @"C:\work\project", Updated = now - 240_000 }] });
        var c = h.Frame();
        Assert.Contains("Cloud task", c.Texts);
        Assert.Contains("Kiro CLI session", c.Texts);
        Assert.Contains("Kiro Web", c.Texts);
        Assert.Contains("CLI", c.Texts);
        Assert.Contains("· click to read it", c.Texts);
        // A click on the web row asks the backend to read it from the cloud.
        Assert.True(h.Click("panelRow", 1));
        Assert.Equal("w1", h.Last<AdoptWebCommand>().Id);
        Assert.Contains("Opening…", h.Frame().Texts);
        // The answer opens the chat of the desk it came to.
        h.State(PanelHarness.Session(4, "done", null, PanelHarness.Turn("cloud", "ok")));
        h.Accept(new AdoptWebEvent { Id = "w1", Ok = true, SessionId = 4 });
        Assert.Equal(4, h.Ui.OpenId);
        Assert.Equal("", h.Ui.OfficePanel);
        Assert.DoesNotContain(h.Ui.WebSessions, s => s.Id == "w1");
        // And the CLI one, with a failure first.
        h.Ui.OpenOfficePanel("history");
        h.Accept(new CliSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "c1", Title = "Local one", Folder = @"C:\work\project", Updated = now - 240_000 }] });
        h.Frame();
        Assert.True(h.Click("panelRow", 1));
        Assert.Equal("c1", h.Last<AdoptCliCommand>().Id);
        h.Accept(new AdoptCliEvent { Id = "c1", Ok = false, Error = "All six desks are busy. Stop or remove a session first." });
        Assert.Equal("All six desks are busy. Stop or remove a session first.", h.Ui.ToastText);
        Assert.Null(h.Ui.OpenId);
        Assert.Equal("history", h.Ui.OfficePanel);
    }

    [Fact]
    public void A_failure_to_list_Kiro_Web_is_said_at_the_top_of_the_history()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.OpenOfficePanel("history");
        h.Accept(new WebSessionsEvent { Ok = false, Error = "Kiro isn’t set up on this computer." });
        Assert.Contains("Kiro Web sessions couldn’t be listed. Kiro isn’t set up on this computer.", h.Frame().Texts);
        h.Accept(new WebSessionsEvent { Ok = true, Sessions = [], Note = "All 3 are in Hover’s history already." });
        Assert.Contains("No Kiro Web sessions to add. All 3 are in Hover’s history already.", h.Frame().Texts);
    }

    [Fact]
    public void The_chat_views_session_list_shows_this_computers_Kiro_sessions_with_a_badge()
    {
        var h = new PanelHarness();
        h.State(Session());
        h.Ui.ChatView = true;
        h.Frame();
        // The list asks Kiro once, not on every frame.
        Assert.Equal(1, h.Sent.Count(c => c is CliSessionsCommand));
        h.Accept(new CliSessionsEvent { Ok = true, Sessions = [new ListedSession { Id = "c9", Title = "Old Kiro chat", Folder = @"C:\work\project", Updated = 1_793_000_000_000 }] });
        var c = h.Frame();
        Assert.Contains("Old Kiro chat", c.Texts);
        Assert.Contains("CLI", c.Texts);
        var row = h.Ui.Targets.Where(t => t.Id == "row" && t.S == "cli:c9").ToList();
        Assert.Single(row);
        h.Click("row", row[0].N);
        Assert.Equal("c9", h.Last<AdoptCliCommand>().Id);
    }
}
