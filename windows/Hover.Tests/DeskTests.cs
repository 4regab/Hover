using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Chat;
using Hover.Chat.Desk;
using Hover.Chat.Ui;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>The desk's rows, its card and its panel, with a made-up backend (any system).</summary>
public class DeskTests
{
    static JsonElement J(string json) => JsonDocument.Parse(json).RootElement.Clone();

    const string Probe = """{"folder":true,"git":true,"branch":"feat/refresh","changed":3,"add":29,"del":11,"gh":true,"ghAuth":false,"pr":null,"commands":4,"agents":2,"running":1,"pages":0,"linked":1}""";

    const string Files = """{"git":true,"branch":"feat/refresh","changed":[{"path":"src/refresh.ts","status":"M","old":null,"add":5,"del":2}],"touched":[],"tree":["README.md","docs/guide.md","src/refresh.ts","src/view.test.ts"],"more":false}""";

    const string Diff = """
        {"git":true,"branch":"feat/refresh","truncated":false,"files":[
          {"path":"src/refresh.ts","old":null,"status":"M","add":2,"del":1,"binary":false,
           "patch":"@@ -8,4 +8,5 @@ class View {\n   private rows: Row[] = [];\n-  old();\n+  first();\n+  second();\n   done();\n"},
          {"path":"logo.png","old":null,"status":"A","add":0,"del":0,"binary":true,"patch":""}]}
        """;

    static SessionInfo Busy(int id = 1) => PanelHarness.Session(id, "working", null, PanelHarness.Turn("Make it faster", "", "working",
        new StepInfo { K = "run", Verb = "Ran", Cmd = "cargo build --release", Status = "completed" },
        new StepInfo { K = "edit", Verb = "Edited", Name = "refresh.ts", Dir = "src", Status = "completed" },
        new StepInfo { K = "agent", Verb = "Subagent", Status = "in_progress" }));

    static SessionInfo Done(int id = 1) => PanelHarness.Session(id, "done", null, PanelHarness.Turn("Fix it", "All fixed. See [the log](https://example.com/log).", "done",
        new StepInfo { K = "edit", Verb = "Edited", Name = "refresh.ts", Dir = "src", Status = "completed" }));

    static PanelHarness Open(SessionInfo? s = null)
    {
        var h = new PanelHarness();
        h.State(s ?? Done());
        h.Frame();
        return h;
    }

    static void Desk(PanelHarness h, string what, string json, string? arg = null, int id = 1)
        => h.Accept(new DeskEvent { Id = id, What = what, Arg = arg, Data = J(json) });

    static void ClickTile(PanelHarness h, string tile) => Assert.True(h.Ui.Click("cardTile", DeskTiles.IndexOf(tile)));

    // MARK: Rows

    [Fact]
    public void Numbers_and_durations_are_written_as_the_desk_writes_them()
    {
        Assert.Equal("1,234,567", DeskRows.Num(1234567));
        Assert.Equal("-1,000", DeskRows.Num(-1000));
        Assert.Equal("640 ms", DeskRows.Dur(640));
        Assert.Equal("3.2 s", DeskRows.Dur(3200));
        Assert.Equal("41 s", DeskRows.Dur(41_000));
        Assert.Equal("2m 05s", DeskRows.Dur(125_000));
    }

    [Fact]
    public void The_diff_numbers_both_sides_and_folds_a_binary_file()
    {
        var laid = DeskRows.DiffRows(new Jv(J(Diff)), new Dictionary<string, bool>(), true);
        var lines = laid.Rows.Where(r => r.Kind == 10).ToList();
        Assert.Equal(["  private rows: Row[] = [];", "  old();", "  first();", "  second();", "  done();"], lines.Select(r => r.Text).Select(t => t.Length > 0 ? t : " ").ToArray());
        Assert.Equal("8", lines[0].Num);
        Assert.Equal("8", lines[0].Num2);
        // A removed line has only the old number, an added line only the new one.
        Assert.Equal("9", lines[1].Num);
        Assert.Equal("", lines[1].Num2);
        Assert.Equal("", lines[2].Num);
        Assert.Equal("9", lines[2].Num2);
        Assert.Equal("10", lines[3].Num2);
        Assert.Contains(laid.Rows, r => r.Kind == 11 && r.Text == "Binary file");
        Assert.Equal("gopr", laid.Rows[0].Act);
        Assert.Equal("2 files changed", laid.Rows[0].Text);
        Assert.Equal("+2", laid.Rows[0].Add);
        // A line that is in the file now opens in the editor at its place.
        Assert.Equal("open:src/refresh.ts:9", lines[2].Act);
        Assert.Equal("", lines[1].Act);
    }

    [Fact]
    public void A_diff_of_fifty_thousand_lines_is_cut_at_its_budget()
    {
        var patch = "@@ -1,50000 +1,50000 @@\\n" + string.Join("\\n", Enumerable.Range(1, 50_000).Select(i => (i % 2 == 0 ? "+" : "-") + "line " + i));
        var json = "{\"git\":true,\"files\":[{\"path\":\"big.txt\",\"status\":\"M\",\"add\":25000,\"del\":25000,\"binary\":false,\"patch\":\"" + patch + "\"}]}";
        var clock = System.Diagnostics.Stopwatch.StartNew();
        var laid = DeskRows.DiffRows(new Jv(J(json)), new Dictionary<string, bool>(), false);
        Assert.True(clock.Elapsed < TimeSpan.FromSeconds(5), "laid out in " + clock.Elapsed);
        // 4,000 lines are shown and the rest is said to be cut; the window draws a few dozen of them.
        Assert.Equal(3999, laid.Rows.Count(r => r.Kind == 10));
        Assert.Contains(laid.Rows, r => r.Kind == 11 && r.Text == "More lines aren’t shown.");
        var (from, to) = laid.Window(2000, 400);
        Assert.InRange(to - from, 1, 80);
    }

    [Fact]
    public void The_file_tree_lists_folders_first_and_marks_what_changed()
    {
        var files = new Jv(J(Files));
        var closed = DeskRows.FilesRows(files, new HashSet<string>(), new HashSet<string>());
        Assert.Equal(["docs", "src", "README.md"], closed.Rows.Select(r => r.Text).ToArray());
        Assert.Equal(2, closed.Rows[1].Flag & 2);
        var open = DeskRows.FilesRows(files, new HashSet<string> { "src" }, new HashSet<string>());
        Assert.Equal(["docs", "src", "refresh.ts", "view.test.ts", "README.md"], open.Rows.Select(r => r.Text).ToArray());
        Assert.Equal("M", open.Rows[2].Badge);
        Assert.Equal("file:src/refresh.ts", open.Rows[2].Act);
        // A file saved here is marked changed even when git does not list it.
        var saved = DeskRows.FilesRows(files, new HashSet<string> { "docs" }, new HashSet<string> { "docs/guide.md" });
        Assert.Equal("M", saved.Rows.First(r => r.Text == "guide.md").Badge);
        Assert.Equal(["file:docs/guide.md"], DeskRows.FindRows(files["tree"].Strings(), "GUIDE").Rows.Select(r => r.Act).ToArray());
        Assert.Equal("Nothing matches.", DeskRows.FindRows(files["tree"].Strings(), "zzz").Rows[0].Text);
    }

    [Fact]
    public void A_file_of_text_shows_its_lines_with_numbers_and_a_binary_one_says_so()
    {
        var text = DeskRows.FileRows(new Jv(J("""{"path":"a.txt","text":"one\r\n\ttwo\r\n","truncated":false}""")), null, 0);
        var lines = text.Rows.Where(r => r.Kind == 7).ToList();
        Assert.Equal(["one", "    two"], lines.Select(r => r.Text).ToArray());
        Assert.Equal("open:a.txt:2", lines[1].Act);
        Assert.Equal("Not a text file. Binary files aren’t shown here.", DeskRows.FileRows(new Jv(J("""{"path":"a.bin","binary":true,"size":9}""")), null, 0).Rows[0].Text);
        Assert.StartsWith("Couldn’t open it.", DeskRows.FileRows(new Jv(J("""{"path":"gone","error":"That file isn’t there any more."}""")), null, 0).Rows[0].Text);
    }

    [Fact]
    public void The_terminals_wrap_long_lines_and_say_how_a_command_ended()
    {
        var term = new Jv(J("""{"commands":[{"id":"1","turn":0,"cmd":"npm test","status":"failed","exit":1,"ms":3200,"out":"FAIL a\n  expected 3, got 2\n"},{"id":"2","turn":0,"cmd":"sleep 9","status":"in_progress","exit":null,"ms":null,"out":""}]}"""));
        var rows = DeskRows.TerminalRows(term, 40, "Pip").Rows;
        var first = rows.First(r => r.Kind == 27);
        Assert.Equal("npm test", first.Text);
        Assert.Equal("exit 1 · 3.2 s", first.Right);
        Assert.Equal(2, first.Tone);
        Assert.Contains(rows, r => r.Kind == 28 && r.Text == "FAIL a" && r.Tone == 2);
        Assert.Equal("running", rows.Last(r => r.Kind == 27).Right);
        var long8 = DeskRows.WrapChars(new string('x', 100), 40);
        Assert.Equal([40, 40, 20], long8.Select(l => l.Length).ToArray());
    }

    [Fact]
    public void Addresses_are_taken_as_the_browser_box_takes_them()
    {
        Assert.True(DeskRows.NormalizeUrl("localhost:3000", out var a));
        Assert.Equal("http://localhost:3000", a);
        Assert.True(DeskRows.NormalizeUrl("example.com/a?b=1", out var b));
        Assert.Equal("https://example.com/a?b=1", b);
        Assert.False(DeskRows.NormalizeUrl("file:///etc/passwd", out _));
        Assert.False(DeskRows.NormalizeUrl("javascript:alert(1)", out _));
        Assert.False(DeskRows.NormalizeUrl("exa mple.com", out _));
        Assert.False(DeskRows.NormalizeUrl("example.com:99999999", out _));
    }

    // MARK: The card

    [Fact]
    public void A_click_on_a_desk_opens_its_card_with_the_tiles_the_probe_allows()
    {
        var h = Open();
        string? hot = "none";
        h.Ui.DeskChanged += id => hot = id?.ToString();
        h.Ui.OpenDeskCard(1);
        var c = h.Frame();
        Assert.Equal("1", hot);
        Assert.Equal(1, h.Ui.DeskCardId);
        Assert.Contains(h.Sent, x => x is DeskCommand { What: "probe" });
        // The name gives way to the chips when the row is full, down to its "…" (internal/ui/deskcard.go; the Go picture shows that too).
        Assert.Contains("…", c.Texts);
        Assert.Contains("Open the chat with Moss", c.Texts);
        // Until the probe is back, the tiles say what the session's own steps do.
        Assert.Contains("Terminal", c.Texts);
        Desk(h, "probe", Probe);
        var texts = h.Frame().Texts;
        Assert.Contains("4 commands", texts);
        Assert.Contains("3 changed", texts);
        Assert.Contains("+29 −11", texts);
        Assert.Contains("1 working", texts);
        Assert.Contains("1 mentioned", texts);
        Assert.Contains("Linked PRs", texts);
        // The browser belongs to the Mac; the screen has its own panel.
        var tiles = DeskTiles.Of(new Jv(J(Probe)), Done(), null, 0);
        Assert.False(tiles.First(t => t.Id == "browser").Enabled);
        Assert.Equal(DeskTiles.AgentBrowserNote, tiles.First(t => t.Id == "browser").Reason);
        // A click elsewhere puts the card away.
        Assert.True(h.Click("cardAway"));
        Assert.Null(h.Ui.DeskCardId);
        Assert.Null(hot);
    }

    [Fact]
    public void The_card_shows_the_last_steps_while_it_works_and_the_answer_when_it_is_done()
    {
        var h = Open(Busy());
        h.Ui.OpenDeskCard(1);
        var live = h.Frame().Texts;
        Assert.Contains("Ran", live);
        Assert.Contains("cargo build --release", live);
        Assert.Contains("src/refresh.ts", live);
        h.State(Done());
        var done = h.Frame().Texts;
        Assert.Contains(done, t => t.StartsWith("All fixed. See the log", StringComparison.Ordinal));
        Assert.Contains("1 step · 1 file edited", done);
    }

    [Fact]
    public void The_cards_reply_is_sent_and_an_empty_box_stops_a_run()
    {
        var h = Open(Busy());
        h.Ui.OpenDeskCard(1);
        h.Frame();
        // Empty and busy: the square pauses the run.
        Assert.True(h.Click("cardStop"));
        Assert.Equal(1, h.Last<PauseCommand>().Id);
        Assert.True(h.Ui.Click("field", s: "card"));
        h.Ui.Char("Then do this");
        h.Frame();
        Assert.True(h.Click("cardSend"));
        Assert.Equal("Then do this", h.Last<ReplyCommand>().Text);
        Assert.Equal(1, h.Last<ReplyCommand>().Id);
    }

    [Fact]
    public void The_card_answers_a_permission_and_its_keys_work()
    {
        var s = PanelHarness.Session(1, "waiting", PanelHarness.Permission(), PanelHarness.Turn("Install", "", "waiting"));
        var h = Open(s);
        h.Ui.OpenDeskCard(1);
        var c = h.Frame();
        Assert.Contains("Wants to run a command", c.Texts);
        Assert.True(h.Click("cardTrust"));
        Assert.Equal("trust", h.Last<AnswerCommand>().Answer);
        Assert.True(h.Click("cardAllow"));
        Assert.Equal("allow", h.Last<AnswerCommand>().Answer);
        Assert.True(h.Click("cardDeny"));
        Assert.Equal("deny", h.Last<AnswerCommand>().Answer);
        // Enter allows, Shift+Enter trusts, Esc denies, as on the notch.
        h.Key(UiKey.Enter);
        Assert.Equal("allow", h.Last<AnswerCommand>().Answer);
        h.Key(UiKey.Enter, UiMods.Shift);
        Assert.Equal("trust", h.Last<AnswerCommand>().Answer);
        h.Key(UiKey.Escape);
        Assert.Equal("deny", h.Last<AnswerCommand>().Answer);
        Assert.Equal(1, h.Ui.DeskCardId);
    }

    [Fact]
    public void A_letter_picks_a_tile_and_Escape_closes_the_card()
    {
        var h = Open();
        h.Ui.OpenDeskCard(1);
        h.Frame();
        Desk(h, "probe", Probe);
        h.Ui.Char("t");
        h.Frame();
        Assert.Equal("terminal", h.Ui.DeskTabId);
        Assert.Equal(1, h.Ui.DeskPanelId);
        Assert.Null(h.Ui.DeskCardId);
        // A grey tile says why instead.
        h.Ui.CloseDeskPanel();
        h.Ui.OpenDeskCard(1);
        h.Frame();
        h.Ui.Char("b");
        h.Frame();
        Assert.Equal(DeskTiles.AgentBrowserNote, h.Ui.ToastText);
        h.Key(UiKey.Escape);
        Assert.Null(h.Ui.DeskCardId);
    }

    // MARK: The panel

    [Fact]
    public void The_diff_tab_reads_the_diff_and_attaches_a_file_to_the_reply()
    {
        var h = Open();
        h.Ui.OpenDeskCard(1);
        h.Frame();
        Desk(h, "probe", Probe);
        ClickTile(h, "diff");
        h.Frame();
        Assert.Equal("diff", h.Ui.DeskTabId);
        Assert.Contains(h.Sent, x => x is DeskCommand { What: "diff" });
        Assert.Contains("Reading…", h.Frame().Texts);
        Desk(h, "diff", Diff);
        var c = h.Frame();
        Assert.Contains("2 files changed", c.Texts);
        Assert.Contains("first();", c.Texts.Select(t => t.Trim()));
        Assert.Contains("Create PR", c.Texts);
        // Attach: the file's change becomes a chip of the chat's reply.
        Assert.True(h.Ui.Click("attachDiff"));
        h.Frame();
        var chip = Assert.Single(h.Ui.ChipsOf(1));
        Assert.Equal("diff", chip.Kind);
        Assert.Equal("change in src/refresh.ts", chip.Label);
        Assert.Contains("+  first();", chip.Text);
        Assert.Equal("Attached to Moss’s reply.", h.Ui.ToastText);
        // The same twice adds nothing.
        h.Ui.Click("attachDiff");
        Assert.Single(h.Ui.ChipsOf(1));
        Assert.Equal("Already attached.", h.Ui.ToastText);
        // A folded file hides its lines.
        Assert.True(h.Ui.Click("drow", s: "df:src/refresh.ts"));
        Assert.DoesNotContain("first();", h.Frame().Texts.Select(t => t.Trim()));
        // Create PR opens the pull request tab.
        Assert.True(h.Ui.Click("drowBtn", s: "gopr"));
        h.Frame();
        Assert.Equal("pr", h.Ui.DeskTabId);
    }

    [Fact]
    public void A_file_is_opened_edited_and_saved_through_the_backend()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "files");
        h.Frame();
        Desk(h, "files", Files);
        Assert.Contains("README.md", h.Frame().Texts);
        Assert.True(h.Ui.Click("drow", s: "dir:src"));
        h.Frame();
        Assert.True(h.Ui.Click("drow", s: "file:src/refresh.ts"));
        h.Frame();
        Assert.Contains(h.Sent, x => x is DeskCommand { What: "file", Arg: "src/refresh.ts" });
        Desk(h, "file", """{"path":"src/refresh.ts","text":"let a = 1;\nlet b = 2;\n","truncated":false,"size":22}""", "src/refresh.ts");
        var c = h.Frame();
        Assert.Contains("let a = 1;", c.Texts);
        Assert.Contains("src/refresh.ts", c.Texts);
        Assert.True(h.Ui.Click("fEdit"));
        h.Frame();
        // The box holds the text; typing at its end and Ctrl+S saves it.
        h.Key(UiKey.End, UiMods.Control);
        h.Type("let c = 3;");
        h.Key(UiKey.S, UiMods.Control);
        var write = h.Last<WriteFileCommand>();
        Assert.Equal(1, write.Id);
        Assert.Equal("src/refresh.ts", write.Path);
        Assert.Equal("let a = 1;\nlet b = 2;\nlet c = 3;", write.Text);
        Assert.Contains("Saving…", h.Frame().Texts);
        // The backend refuses a second save while one is going, and answers the first.
        h.Accept(new WriteFileEvent { Id = 1, Path = "src/refresh.ts", Ok = true });
        Assert.Equal("Saved.", h.Ui.ToastText);
        Assert.Contains("let c = 3;", h.Frame().Texts);
        Assert.Contains(h.Sent, x => x is DeskCommand { What: "diff" });
    }

    [Fact]
    public void A_save_the_backend_refuses_stays_in_the_editor_and_says_why()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "files");
        h.Frame();
        Desk(h, "files", Files);
        h.Ui.Click("drow", s: "file:README.md");
        Desk(h, "file", """{"path":"README.md","text":"hello\n","truncated":false,"size":6}""", "README.md");
        h.Ui.Click("fEdit");
        h.Frame();
        h.Type("!");
        h.Key(UiKey.S, UiMods.Control);
        h.Accept(new WriteFileEvent { Id = 1, Path = "README.md", Ok = false, Error = "That file isn’t in the session’s folder." });
        Assert.Equal("That file isn’t in the session’s folder.", h.Ui.ToastText);
        // Still editing: Cancel puts the text away without a write.
        var writes = h.Sent.Count(x => x is WriteFileCommand);
        Assert.True(h.Ui.Click("fCancel"));
        h.Frame();
        Assert.Equal(writes, h.Sent.Count(x => x is WriteFileCommand));
        Assert.DoesNotContain("Editing", h.Frame().Texts);
    }

    [Fact]
    public void A_binary_file_and_a_file_that_went_are_told_apart_and_cannot_be_edited()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "files");
        h.Frame();
        Desk(h, "files", Files);
        h.Ui.Click("drow", s: "file:README.md");
        Desk(h, "file", """{"path":"README.md","binary":true,"size":12}""", "README.md");
        Assert.Contains("Not a text file. Binary files aren’t shown here.", h.Frame().Texts);
        Assert.Null(h.Ui.Find("fEdit"));
        // The file is deleted while open: the next read says so.
        Desk(h, "file", """{"path":"README.md","error":"That file isn’t there any more."}""", "README.md");
        Assert.Contains("Couldn’t open it. That file isn’t there any more.", h.Frame().Texts);
        Assert.True(h.Ui.Click("fileBack"));
        Assert.Contains("docs", h.Frame().Texts);
    }

    [Fact]
    public void The_terminal_runs_a_command_shows_its_output_interrupts_and_clears()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "terminal");
        h.Frame();
        Assert.Contains(h.Sent, x => x is TermCommand { Op: "view" });
        h.Accept(new TermEvent { Id = 1, Ok = true, Cwd = @"C:\work\project", Total = 0, From = 0 });
        var c = h.Frame();
        Assert.Contains(c.Texts, t => t.Contains("runs as you"));
        // The prompt has the keyboard; a command is typed and run with Enter.
        h.Type("echo hi");
        h.Key(UiKey.Enter);
        var run = h.Last<TermCommand>();
        Assert.Equal(("run", "echo hi"), (run.Op, run.Cmd));
        h.Accept(new TermEvent
        {
            Id = 1, Ok = true, Cwd = @"C:\work\project", Running = true, Total = 1, From = 0,
            Entries = [new TermEntryInfo { Cwd = @"C:\work\project", Cmd = "echo hi", Lines = [new TermLine { Text = "hi" }], Run = new TermRun { Kind = "running" } }],
        });
        Assert.Contains("hi", h.Frame().Texts);
        // While it runs there is no prompt, and Ctrl+C interrupts it.
        Assert.Null(h.Ui.Find("termBox"));
        h.Key(UiKey.C, UiMods.Control);
        Assert.Equal("interrupt", h.Last<TermCommand>().Op);
        h.Accept(new TermEvent
        {
            Id = 1, Ok = true, Cwd = @"C:\work\project", Running = false, Total = 1, From = 0,
            Entries = [new TermEntryInfo { Cwd = @"C:\work\project", Cmd = "echo hi", Lines = [new TermLine { Text = "hi" }, new TermLine { Text = "^C" }], Run = new TermRun { Kind = "stopped" } }],
        });
        Assert.NotNull(h.Ui.Find("termBox"));
        // Up brings the last command back; Ctrl+L clears.
        h.Key(UiKey.Up);
        Assert.Equal("echo hi", h.Ui.Field("term"));
        h.Key(UiKey.L, UiMods.Control);
        Assert.Equal("clear", h.Last<TermCommand>().Op);
    }

    [Fact]
    public void A_terminal_that_never_ends_keeps_its_output_in_view_and_can_be_interrupted()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "terminal");
        h.Frame();
        var lines = Enumerable.Range(1, 3000).Select(i => new TermLine { Text = "tick " + i }).ToList();
        h.Accept(new TermEvent
        {
            Id = 1, Ok = true, Cwd = "/w", Running = true, Total = 1, From = 0,
            Entries = [new TermEntryInfo { Cwd = "/w", Cmd = "ping -t host", Lines = lines, Run = new TermRun { Kind = "running" } }],
        });
        var c = h.Frame();
        // The end follows the output.
        Assert.Contains("tick 3000", c.Texts);
        Assert.DoesNotContain("tick 1", c.Texts);
        h.Key(UiKey.C, UiMods.Control);
        Assert.Equal("interrupt", h.Last<TermCommand>().Op);
    }

    [Fact]
    public void The_agents_terminal_and_subagents_are_read_only_lists()
    {
        var h = Open(Busy());
        h.Ui.OpenDeskPanel(1, "terminal");
        h.Frame();
        Assert.True(h.Ui.Click("termTab", 1));
        h.Frame();
        Assert.Contains(h.Sent, x => x is DeskCommand { What: "terminal" });
        Desk(h, "terminal", """{"commands":[{"id":"1","turn":0,"cmd":"cargo build --release","status":"completed","exit":0,"ms":41000,"out":"Finished"}]}""");
        var c = h.Frame();
        Assert.Contains("cargo build --release", c.Texts);
        Assert.Contains("exit 0 · 41 s", c.Texts);
        h.Ui.OpenDeskPanel(1, "agents");
        Desk(h, "agents", """{"agents":[{"id":"a1","turn":0,"name":"Subagent","task":"Find every caller","prompt":"Look","status":"in_progress","ms":null,"out":null}],"running":1}""");
        var a = h.Frame();
        Assert.Contains("1 subagent", a.Texts);
        Assert.Contains("Find every caller", a.Texts);
        h.Ui.Click("drow", s: "sa:a1");
        Assert.Contains("Still working…", h.Frame().Texts);
    }

    [Fact]
    public void The_pull_request_tab_sets_up_the_GitHub_CLI_and_the_setup_can_be_cancelled()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "pr");
        h.Frame();
        Assert.Contains(h.Sent, x => x is GhCommand { Step: null });
        Desk(h, "pr", """{"setup":"signin","error":"gh isn’t signed in."}""");
        h.Accept(new GhEvent { Checked = true, Installed = true, SignedIn = false, Url = "https://github.com/login/device" });
        var c = h.Frame();
        Assert.Contains("Sign in to GitHub", c.Texts);
        Assert.Contains("Sign in with GitHub", c.Texts);
        Assert.True(h.Ui.Click("ghStart"));
        Assert.Equal("setup", h.Last<GhCommand>().Step);
        // GitHub's one-time code is shown with Copy and the address; Cancel ends the setup.
        h.Accept(new GhEvent { Checked = true, Installed = true, Busy = true, Step = "signing in", Line = "Waiting for you to sign in…", Code = "ABCD-1234", Url = "https://github.com/login/device" });
        var busy = h.Frame().Texts;
        Assert.Contains("ABCD-1234", busy);
        Assert.Contains("Copy code", busy);
        Assert.Contains("Cancel", busy);
        Assert.DoesNotContain("Sign in with GitHub", busy);
        Assert.True(h.Ui.Click("ghCopy"));
        Assert.Equal("ABCD-1234", h.Clip.Value);
        Assert.True(h.Ui.Click("ghCancel"));
        Assert.Equal("cancel", h.Last<GhCommand>().Step);
        h.Accept(new GhEvent { Checked = true, Installed = true, Busy = false, Error = "Cancelled." });
        var after = h.Frame().Texts;
        Assert.Contains("Cancelled.", after);
        Assert.Contains("Sign in with GitHub", after);
    }

    [Fact]
    public void Without_gh_the_card_offers_to_install_it_and_a_failed_install_says_why()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "pr");
        h.Frame();
        Desk(h, "pr", """{"setup":"install","error":"gh isn’t installed."}""");
        h.Accept(new GhEvent { Checked = true, Installed = false });
        var c = h.Frame().Texts;
        Assert.Contains("Set up the GitHub CLI", c);
        Assert.Contains("Install and sign in", c);
        h.Ui.Click("ghStart");
        h.Accept(new GhEvent { Checked = true, Installed = false, Busy = false, Error = "Install the GitHub CLI from cli.github.com. (winget isn’t available.)" });
        var failed = h.Frame().Texts;
        Assert.Contains("Install the GitHub CLI from cli.github.com. (winget isn’t available.)", failed);
        Assert.Contains("Install and sign in", failed);
    }

    [Fact]
    public void A_pull_request_is_opened_from_the_form_and_a_failure_names_what_was_done_first()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "pr");
        h.Frame();
        Desk(h, "pr", """{"none":true,"error":"No pull request for this branch.","create":{"branch":"main","base":"main","onDefault":true,"suggest":"hover/refresh","ahead":0,"changed":2,"title":"Fix refresh","body":"Skips clean views.","busy":false}}""");
        var c = h.Frame().Texts;
        Assert.Contains("No pull request yet", c);
        Assert.Contains("A new branch from main · 2 files not committed yet", c);
        Assert.Equal("Fix refresh", h.Ui.Field("prTitle"));
        Assert.Equal("hover/refresh", h.Ui.Field("prBranch"));
        Assert.True(h.Ui.Click("prCheck", s: "draft"));
        Assert.True(h.Ui.Click("prCreate"));
        var action = h.Last<DeskActionCommand>();
        Assert.Equal("prCreate", action.What);
        Assert.Equal("Fix refresh", (string?)action.Args["title"]);
        Assert.Equal("hover/refresh", (string?)action.Args["branch"]);
        Assert.Equal("main", (string?)action.Args["base"]);
        Assert.True((bool)action.Args["draft"]!);
        Assert.True((bool)action.Args["commit"]!);
        Assert.Contains("Opening…", h.Frame().Texts);
        h.Accept(new DeskActionEvent { Id = 1, What = "prCreate", Data = J("""{"error":"gh isn’t signed in.","steps":["Committed 2 files","Pushed hover/refresh"]}""") });
        var failed = h.Frame().Texts;
        Assert.Contains("gh isn’t signed in.", failed);
        Assert.Contains("Done before it: Committed 2 files, Pushed hover/refresh.", failed);
        Assert.Contains("Create pull request", failed);
        // Success: the tab reads the pull request again.
        h.Ui.Click("prCreate");
        h.Accept(new DeskActionEvent { Id = 1, What = "prCreate", Data = J("""{"ok":true,"url":"https://github.com/me/app/pull/7","steps":[]}""") });
        Assert.Contains(h.Sent.TakeLast(4), x => x is DeskCommand { What: "pr" });
        Assert.Contains("Reading…", h.Frame().Texts);
    }

    [Fact]
    public void A_pull_request_form_with_a_run_going_cannot_be_sent()
    {
        var h = Open(Busy());
        h.Ui.OpenDeskPanel(1, "pr");
        h.Frame();
        Desk(h, "pr", """{"none":true,"create":{"branch":"feat","base":"main","onDefault":false,"ahead":2,"changed":0,"title":"t","body":"","busy":false}}""");
        var c = h.Frame().Texts;
        Assert.Contains("The agent is still working in this folder; open it when the run ends.", c);
        Assert.Contains("Branch feat, 2 commits ahead of main", c);
        var before = h.Sent.Count(x => x is DeskActionCommand);
        h.Ui.Click("prCreate");
        Assert.Equal(before, h.Sent.Count(x => x is DeskActionCommand));
    }

    [Fact]
    public void A_pull_request_the_branch_has_is_shown_with_its_checks()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "pr");
        Desk(h, "pr", """{"number":7,"title":"Fix refresh","state":"open","isDraft":false,"url":"https://github.com/me/app/pull/7","head":"feat","base":"main","additions":12,"deletions":3,"changedFiles":2,"body":"Skips clean views.","author":"me","review":"APPROVED","comments":1,"checks":[{"name":"build","state":"pass","url":"https://x/1"},{"name":"lint","state":"fail","url":null}],"pass":1,"fail":1,"pending":0,"skip":0}""");
        var c = h.Frame().Texts;
        Assert.Contains("Open", c);
        Assert.Contains("#7", c);
        Assert.Contains("Fix refresh", c);
        Assert.Contains("feat → main · by me · +12 −3 · 2 files · 1 comment · Approved", c);
        Assert.Contains("✗ 1 failing", c);
        Assert.Contains("build", c);
        Assert.Contains("Open on GitHub", c);
        string? opened = null;
        // The address opens through the host.
        Assert.Equal("ext:https://github.com/me/app/pull/7", h.Ui.DeskRowActs().First(a => a.StartsWith("ext:", StringComparison.Ordinal)));
        _ = opened;
    }

    [Fact]
    public void Linked_pull_requests_list_what_the_session_mentions()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "linked");
        Desk(h, "linked", """{"gh":false,"prs":[{"url":"https://github.com/me/app/pull/9","repo":"me/app","number":9}]}""");
        var c = h.Frame().Texts;
        Assert.Contains("Set up the GitHub CLI in the Pull request tab to see their state.", c);
        Assert.Contains("me/app#9", c);
    }

    [Fact]
    public void The_chat_menu_opens_the_terminal_and_the_changes_beside_the_chat()
    {
        var h = Open();
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        h.Accept(new EditorsEvent { Ok = true, Choices = [new EditorChoice { Id = "vscode", Name = "VS Code", Last = true }, new EditorChoice { Id = "fm", Name = "File Explorer" }] });
        h.Click("more");
        Assert.Contains("Terminal", h.Frame().Texts);
        Assert.Contains("Files & changes", h.Frame().Texts);
        Assert.Contains("VS Code", h.Frame().Texts);
        var rows = new List<string>();
        for (var i = 0; i < 12; i++)
        {
            var t = h.Ui.Targets.FirstOrDefault(x => x.Id == "menu" && x.S == "Terminal");
            if (t.Id is not null) { h.Click("menu", t.N); break; }
        }
        Assert.Equal("terminal", h.Ui.DeskTabId);
        Assert.Equal(1, h.Ui.DeskPanelId);
        // The chat stays, and the panel sits beside it.
        Assert.True(h.Ui.ChatView);
        Assert.Equal(1, h.Ui.OpenId);
        _ = rows;
        // The panel and the chat do not overlap.
        Assert.True(h.Ui.ThreadArea.Right <= h.Ui.DeskPanelRect.X + 1, $"{h.Ui.ThreadArea} against {h.Ui.DeskPanelRect}");
    }

    [Fact]
    public void Open_in_asks_the_backend_to_open_the_file_in_an_editor()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "files");
        h.Frame();
        h.Accept(new EditorsEvent { Ok = true, Choices = [new EditorChoice { Id = "vscode", Name = "VS Code", Last = true }, new EditorChoice { Id = "fm", Name = "File Explorer" }] });
        Desk(h, "files", Files);
        h.Ui.Click("drow", s: "dir:src");
        h.Frame();
        Assert.True(h.Ui.Click("drow", s: "file:src/refresh.ts"));
        Desk(h, "file", """{"path":"src/refresh.ts","text":"hello\n","truncated":false,"size":6}""", "src/refresh.ts");
        Assert.True(h.Ui.Click("openPill"));
        Assert.Contains("Open file in", h.Frame().Texts);
        Assert.True(h.Ui.Click("openIn", 0));
        var open = h.Last<OpenInCommand>();
        Assert.Equal(("vscode", "src/refresh.ts"), (open.Editor, open.File));
        // A line of a file opens at its place, with a button that shows while the pointer is on the line.
        h.Ui.CloseDeskPanel();
        h.Ui.OpenDeskPanel(1, "files");
        h.Frame();
        var list = h.Ui.DeskListRect;
        h.Ui.PointerMove(list.X + 100, list.Y + 8 + 9);
        h.Frame();
        Assert.True(h.Ui.Click("drowBtn", s: "open:src/refresh.ts:1"));
        Assert.Equal(1, h.Last<OpenInCommand>().Line);
    }

    [Fact]
    public void The_cloud_chip_opens_the_session_on_Kiro_Web_as_the_Go_chip_does()
    {
        var opened = new List<string>();
        var ui = new OfficeUi(new FakeShaper(), new UiEnv { Send = _ => { }, Clipboard = new FakeClipboard(), OpenUrl = opened.Add });
        ui.Accept(new StateEvent
        {
            Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }],
            Sessions = [Done() with { Cloud = [], Extra = new() { ["kiroId"] = J("\"abc 1\"") } }],
        });
        ui.OpenChat(1);
        ui.Paint(new RecordingCanvas(), 1200, 720, 1000);
        Assert.True(ui.Click("openCloud"));
        // internal/shell/chatact.go dOpenCloud: agents.KiroWebSession + the session's id.
        Assert.Equal(["https://app.kiro.dev/session/abc%201"], opened);
        // Without the id (the backend sends it once the first turn has told it) the chip is not there, as in Go (d.Cloud needs KiroID).
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Done() with { Cloud = [] }] });
        ui.Paint(new RecordingCanvas(), 1200, 720, 1000);
        Assert.False(ui.Click("openCloud"));
    }

    [Fact]
    public void The_card_sits_where_the_desk_was_clicked_and_says_how_many_helpers_are_out()
    {
        var ui = new OfficeUi(new FakeShaper(), new UiEnv
        {
            Send = _ => { }, Clipboard = new FakeClipboard(),
            Helpers = id => id == 1 ? [(0x9b, 0x6b, 0xff), (0xff, 0x9f, 0x0a)] : [],
        });
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Busy()] });
        // internal/ui/officeview.go: the card's corner is 6 right of the click and 12 above it, held inside the office.
        ui.OpenDeskCard(1, 300, 200);
        var canvas = new RecordingCanvas();
        ui.Paint(canvas, 1200, 720, 1000);
        Assert.Equal(306, ui.DeskCardRect.X, 0.5);
        Assert.Equal(188, ui.DeskCardRect.Y, 0.5);
        // internal/shell/desk.go: the line under the status box, with a coloured square for each helper.
        Assert.Contains(canvas.Texts, t => t.Contains("2 helpers are on subagent tasks at the desk", StringComparison.Ordinal));
        ui.OpenDeskCard(1, 5000, 5000);
        ui.Paint(new RecordingCanvas(), 1200, 720, 1000);
        Assert.True(ui.DeskCardRect.X + ui.DeskCardRect.W <= 1200 - 8 + 0.5 && ui.DeskCardRect.Y + ui.DeskCardRect.H <= 720 - 8 + 0.5, "a click far past the office's corner holds the card inside it");
    }

    [Fact]
    public void The_tip_over_a_bot_a_desk_and_a_prop_says_what_a_click_does()
    {
        var tip = (Kind: 0, Id: 0, Hint: "");
        var ui = new OfficeUi(new FakeShaper(), new UiEnv { Send = _ => { }, Clipboard = new FakeClipboard(), Tip = () => tip });
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Busy()] });
        ui.PointerMove(900, 400);
        string[] Drawn() { var c = new RecordingCanvas(); ui.Paint(c, 1200, 720, 1000); return [.. c.Texts]; }
        Assert.DoesNotContain(Drawn(), t => t.Contains("Chat with", StringComparison.Ordinal));
        tip = (1, 1, "");
        Assert.Contains(Drawn(), t => t == "Chat with");
        tip = (2, 1, "");
        var desk = Drawn();
        Assert.Contains(desk, t => t.EndsWith("’s desk", StringComparison.Ordinal));
        Assert.Contains(desk, t => t == "Browser, terminal, files, screen…");
        tip = (0, 0, "Session board");
        Assert.Contains(Drawn(), t => t == "Session board");
    }

    [Fact]
    public void A_bot_has_a_name_tag_with_its_bubble_a_question_hides_the_bubble_and_the_tag_opens_the_chat()
    {
        var sent = new List<BackendCommand>();
        var tags = new List<BotTag> { new(1, 400, 200, "Pip", Rgba.Of(0x9b6bff), "kiro", "Reading the code", 1, false) };
        var ui = new OfficeUi(new FakeShaper(), new UiEnv { Send = sent.Add, Clipboard = new FakeClipboard(), Tags = () => tags });
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Busy()] });
        var c = new RecordingCanvas();
        ui.Paint(c, 1200, 720, 1000);
        Assert.Contains("Pip", c.Texts);
        Assert.Contains("Reading the code", c.Texts);
        Assert.True(ui.Click("botTag", 1));
        Assert.Contains(sent, x => x is OpenCommand { Id: 1 });
        ui.CloseChat();
        // A question stands on the tag in place of the bubble (internal/ui/officeview.go tag).
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Busy() with { Stage = "waiting", Ask = new AskInfo { Id = "a1", Kind = "execute", Title = "Wants to run a command", Command = "npm test", Allow = "Run" } }] });
        c = new RecordingCanvas();
        ui.Paint(c, 1200, 720, 1000);
        Assert.Contains("Pip", c.Texts);
        Assert.DoesNotContain("Reading the code", c.Texts);
        Assert.Contains("Wants to run a command", c.Texts);
    }

    [Fact]
    public void A_pull_requests_text_is_shown_formatted_once_the_backend_has_read_it()
    {
        var h = Open();
        h.Ui.OpenDeskPanel(1, "pr");
        Desk(h, "pr", """{"number":7,"title":"Skip clean views","state":"open","isDraft":false,"url":"https://github.com/o/r/pull/7","head":"feat","base":"main","additions":1,"deletions":0,"changedFiles":1,"body":"Some **words** here","checks":[],"pass":0,"fail":0,"pending":0,"skip":0}""");
        // The first layout shows plain words and asks; the answer comes while the frames are drawn, and the rows are made again with it.
        h.Frame();
        Assert.False(h.Ui.Waiting, "the description is still waiting for words the backend has already sent");
        Assert.Contains(h.Sent, c => c is MarkdownCommand);
    }

    [Fact]
    public void The_card_follows_the_bot_when_the_office_says_where_it_is()
    {
        var sent = new List<BackendCommand>();
        var ui = new OfficeUi(new FakeShaper(), new UiEnv
        {
            Send = sent.Add, Clipboard = new FakeClipboard(), BotRect = id => id == 1 ? new Rect4(300, 200, 40, 60) : null,
        });
        ui.Accept(new StateEvent { Tools = [new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true }], Sessions = [Done()] });
        ui.OpenDeskCard(1);
        ui.Paint(new RecordingCanvas(), 1200, 720, 1000);
        // The card sits to the right of the bot, a little above its head.
        Assert.InRange(ui.DeskCardRect.X, 340, 360);
        Assert.InRange(ui.DeskCardRect.Y, 150, 200);
    }
}
