using Xunit;
using System.Diagnostics;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host.Protocol;

namespace Hover.Tests;

/// <summary>Writes down what a frame asked to draw, so the panels' drawing rules run on any system.</summary>
sealed class RecordingCanvas : ICanvas
{
    public readonly List<string> Texts = [];
    public readonly List<string> Svgs = [];
    public int Broken, Fills;
    int clips;

    /// <summary>A line of text as drawn: its words, the box it takes, and the clip it was drawn under (null: none), all in the window's pixels.</summary>
    public readonly record struct DrawnText(string Text, Rect4 R, Rect4? Clip);

    public readonly List<DrawnText> Drawn = [];

    /// <summary>Every call of the frame as one line each, in order: two frames that differ in what they draw differ here.</summary>
    public readonly List<string> Ops = [];

    float moveX, moveY;
    readonly Stack<(float X, float Y)> moves = new();
    readonly List<Rect4> clipBoxes = [];

    Rect4 Moved(Rect4 r) => new(r.X + moveX, r.Y + moveY, r.W, r.H);

    /// <summary>The part of the window every clip of the stack leaves (null when nothing is clipped).</summary>
    Rect4? Clip()
    {
        if (clipBoxes.Count == 0) return null;
        var c = clipBoxes[0];
        foreach (var b in clipBoxes.Skip(1))
        {
            float x = Math.Max(c.X, b.X), y = Math.Max(c.Y, b.Y);
            c = new Rect4(x, y, Math.Max(Math.Min(c.Right, b.Right) - x, 0), Math.Max(Math.Min(c.Bottom, b.Bottom) - y, 0));
        }
        return c;
    }

    public void PushClip(Rect4 r) { clips++; clipBoxes.Add(Moved(r)); Ops.Add($"clip {r}"); }
    public void PopClip() { clips--; clipBoxes.RemoveAt(clipBoxes.Count - 1); Ops.Add("unclip"); }
    public void PushMove(float dx, float dy) { moves.Push((dx, dy)); moveX += dx; moveY += dy; Ops.Add($"move {dx},{dy}"); }
    public void PopMove() { var (dx, dy) = moves.Pop(); moveX -= dx; moveY -= dy; Ops.Add("unmove"); }
    public void PushOpacity(float opacity) => Ops.Add($"opacity {opacity}");
    public void PopOpacity() => Ops.Add("unopacity");
    /// <summary>Every filled box of the frame, in the order drawn, for the checks of what is on the screen.</summary>
    public readonly List<(Rect4 R, Rgba C)> Boxes = [];

    public void Fill(Rect4 r, float[] radius, Rgba c) { Fills++; Boxes.Add((Moved(r), c)); Ops.Add($"fill {r} {c}"); }
    public void Stroke(Rect4 r, float[] radius, float width, Rgba c) => Ops.Add($"stroke {r} {width} {c}");
    public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) => Ops.Add($"line {points.Count} {points.FirstOrDefault()} {width} {c}");
    public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) => Ops.Add($"polygon {points.Count} {points.FirstOrDefault()} {c}");
    public void Glow(float cx, float cy, float radius, Rgba c) => Ops.Add($"glow {cx},{cy} {radius} {c}");
    public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y)
    {
        var text = ((FakeLayout)layout).Text;
        Texts.Add(text);
        Drawn.Add(new DrawnText(text, Moved(new Rect4(x, y, layout.Width, layout.Height)), Clip()));
        Ops.Add($"text {text} {x},{y} {string.Join('/', runs.Select(r => r.Color))}");
    }
    public void Svg(string svg, Rect4 r) { Svgs.Add(svg); Ops.Add($"svg {svg.GetHashCode()} {r}"); }
    public void Image(string src, Rect4 r, float radius, bool cover) => Ops.Add($"image {src.Length} {r}");
    public void BrokenImage(float x, float y) { Broken++; Ops.Add("broken"); }
    public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash) => Ops.Add($"path {path.Count} {fill} {stroke}");
    public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family)
    {
        Texts.Add(text);
        Ops.Add($"label {text} {x},{y} {c}");
    }

    public bool Balanced => clips == 0;
}

sealed class FakeClipboard : IClipboard
{
    public string? Value;
    public string? Image;
    public string? Text() => Value;
    public string? ImageUrl() => Image;
    public void Set(string text) => Value = text;
}

/// <summary>One set of panels with a made-up backend: commands are written down, rendering answers are queued and delivered between frames.</summary>
sealed class PanelHarness
{
    public readonly List<BackendCommand> Sent = [];
    public readonly FakeClipboard Clip = new();
    public readonly OfficeUi Ui;
    readonly Queue<BackendEvent> inbox = new();
    public RecordingCanvas Canvas = new();
    public double Now = 1000;
    public float W = 1200, H = 720;
    public Func<string, IReadOnlyList<MdBlock>> Markdown = src => ChatFixtures.Paragraphs(src);

    /// <summary>The made-up backend's answers to the desk's commands, in the order they were asked.</summary>
    public Action<BackendCommand>? Respond;

    /// <summary>The page of Settings the panels ask for by name.</summary>
    public Action<string>? OpenPage;

    /// <summary>What the system's picture dialog answers.</summary>
    public Func<PickedImage?>? Pick;

    public PanelHarness(bool inNotch = false)
    {
        Ui = new OfficeUi(new FakeShaper(), new UiEnv
        {
            InNotch = inNotch,
            Send = Send,
            Clipboard = Clip,
            PickImage = () => Pick?.Invoke(),
            ShowSettingsPage = page => OpenPage?.Invoke(page),
            TimeMode = () => TimeMode,
            SetTime = mode => TimeMode = mode,
            Beats = () => BeatsOn,
            SetBeats = on => BeatsOn = on,
            OpenSettings = () => SettingsOpened++,
            Utc = () => new DateTime(2026, 11, 1, 7, 20, 0, DateTimeKind.Utc),
            PickFolder = () => { FolderAsked++; return FolderAnswer; },
        });
    }

    /// <summary>What the made-up folder dialog answers (null: cancelled), and how often it was shown.</summary>
    public string? FolderAnswer;
    public int FolderAsked;

    /// <summary>The office menu's state in the made-up app: the time of day (0 clock, 1 night, 2 day), the beats switch, and how often Settings was asked for.</summary>
    public int TimeMode, SettingsOpened;
    public bool BeatsOn;

    void Send(BackendCommand c)
    {
        Sent.Add(c);
        Respond?.Invoke(c);
        switch (c)
        {
            case MarkdownCommand m: inbox.Enqueue(new MarkdownEvent { Ref = m.Ref, Ok = true, Blocks = [.. Markdown(m.Src)] }); break;
            case HighlightCommand h: inbox.Enqueue(new HighlightEvent { Ref = h.Ref, Ok = true }); break;
            case DiffLinesCommand d: inbox.Enqueue(new DiffLinesEvent { Ref = d.Ref, Ok = true, Lines = [.. d.Diff.Split('\n').Select(l => new DiffLine { Text = l })] }); break;
        }
    }

    /// <summary>Draws a frame, delivers what the backend answered meanwhile, and draws again until nothing is waiting.</summary>
    public RecordingCanvas Frame()
    {
        for (var round = 0; round < 20; round++)
        {
            Canvas = new RecordingCanvas();
            Ui.Paint(Canvas, W, H, Now);
            Assert.True(Canvas.Balanced, "every clip pushed was popped");
            if (inbox.Count == 0) break;
            while (inbox.Count > 0) Ui.Accept(inbox.Dequeue());
        }
        return Canvas;
    }

    public void Accept(BackendEvent e)
    {
        Ui.Accept(e);
        Frame();
    }

    /// <summary>Hands an answer to the panels at the next frame (a reply to a command the panels just sent).</summary>
    public void Later(BackendEvent e) => inbox.Enqueue(e);

    public void State(params SessionInfo[] sessions) => Accept(new StateEvent
    {
        CanStart = true, Tool = "kiro", Folder = @"C:\work\project",
        Tools =
        [
            new ToolInfo { Id = "kiro", Name = "Kiro", Ready = true, Access = "full", Models = [new ModelInfo { Id = "auto", Name = "Auto" }, new ModelInfo { Id = "opus", Name = "Opus" }] },
            new ToolInfo { Id = "codex", Name = "Codex", Ready = true, Access = "risky" },
        ],
        Sessions = [.. sessions],
    });

    public bool Click(string id, int n = -1, string? s = null)
    {
        var ok = Ui.Click(id, n, 1, s);
        Frame();
        return ok;
    }

    public void Type(string text)
    {
        foreach (var ch in text) Ui.Char(ch.ToString());
        Frame();
    }

    public bool Key(UiKey k, UiMods m = UiMods.None)
    {
        var used = Ui.Key(k, m);
        Frame();
        return used;
    }

    public T Last<T>() where T : BackendCommand => Sent.OfType<T>().Last();

    public static SessionInfo Session(int id, string stage = "done", AskInfo? ask = null, params TurnInfo[] turns) => new()
    {
        Id = id, Key = "k" + id, Tool = "kiro", Bot = 2, Title = "Tidy the imports again", Folder = @"C:\work\project", Stage = stage, Ask = ask,
        Turns = [.. turns],
    };

    public static TurnInfo Turn(string prompt, string answer = "", string stage = "done", params StepInfo[] steps) => new()
    {
        Prompt = prompt, Answer = answer, Stage = stage, T0 = 1_793_000_000_000, Steps = [.. steps], Took = 3200,
    };

    public static AskInfo Permission(string id = "a1") => new()
    {
        Id = id, Kind = "execute", Title = "Wants to run a command", Command = "npm install three@0.171.0", Allow = "Run",
        Reason = "Installs packages or uses the network",
    };

    /// <summary>The middle of a place in the thread, in the panels' coordinates.</summary>
    public (float X, float Y) InThread(int section, Rect4 r)
    {
        var s = Ui.Thread.Sections[section];
        return (Ui.ThreadArea.X + Palette.ThreadPad[3] + r.X + r.W / 2, Ui.ThreadArea.Y + s.Y + r.Y + r.H / 2 - Ui.ScrollY);
    }

    public void ClickAt((float X, float Y) p)
    {
        Ui.PointerMove(p.X, p.Y);
        Ui.PointerDown(p.X, p.Y, 1, UiMods.None);
        Ui.PointerUp(p.X, p.Y);
        Frame();
    }
}

public class PanelTests
{
    [Fact]
    public void The_start_screen_starts_a_task_with_the_picks_it_shows()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = true;
        var c = h.Frame();
        Assert.Contains("What should we work on?", c.Texts);
        h.Click("taskText");
        h.Type("fix the build");
        Assert.Equal("fix the build", h.Ui.TaskText);
        h.Click("startTask");
        var cmd = h.Last<NewTaskCommand>();
        Assert.Equal("kiro", cmd.Tool);
        Assert.Equal(@"C:\work\project", cmd.Folder);
        Assert.Equal("fix the build", cmd.Prompt);
        Assert.Equal("full", cmd.Access);
        Assert.Null(cmd.Cloud);
        Assert.Equal("", h.Ui.TaskText);
    }

    [Fact]
    public void The_office_menu_picks_the_time_of_day_turns_the_beats_and_opens_the_history_and_Settings()
    {
        var h = new PanelHarness { TimeMode = 1 };
        h.State();
        h.Click("topMenu");
        Assert.Equal("top", h.Ui.OpenMenuKind);
        Assert.Equal(["Chill beats", "Session history", "Settings"], h.Canvas.Texts.Where(t => t is "Chill beats" or "Session history" or "Settings").ToArray());
        Assert.DoesNotContain("On", h.Canvas.Texts);

        // The time of day: three icons; a pick is kept and the menu stays open.
        Assert.True(h.Click("menuTime", 2));
        Assert.Equal(2, h.TimeMode);
        Assert.True(h.Click("menuTime", 0));
        Assert.Equal(0, h.TimeMode);
        Assert.Equal("top", h.Ui.OpenMenuKind);

        // Chill beats: the switch turns, the menu stays open, and the row says On.
        Assert.True(h.Click("menu", 0));
        Assert.True(h.BeatsOn);
        Assert.Equal("top", h.Ui.OpenMenuKind);
        Assert.Contains("On", h.Frame().Texts);

        // Session history closes the menu and opens the history panel; Settings closes it and asks for Settings.
        Assert.True(h.Click("menu", 1));
        Assert.Null(h.Ui.OpenMenuKind);
        Assert.Equal("history", h.Ui.OfficePanel);
        h.Ui.OpenOfficePanel("");
        h.Frame();
        Assert.True(h.Click("topMenu"));
        Assert.True(h.Click("menu", 2));
        Assert.Null(h.Ui.OpenMenuKind);
        Assert.Equal(1, h.SettingsOpened);
    }

    [Fact]
    public void The_access_menu_changes_what_the_task_starts_with()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = true;
        h.Frame();
        h.Click("pillAccess");
        Assert.Equal("access", h.Ui.OpenMenuKind);
        // Row 0 is the heading; the second choice is "Ask first".
        Assert.True(h.Click("menu", 2));
        Assert.Null(h.Ui.OpenMenuKind);
        h.Click("taskText");
        h.Type("x");
        h.Click("startTask");
        Assert.Equal("risky", h.Last<NewTaskCommand>().Access);
    }

    [Fact]
    public void The_agent_menu_picks_the_tool_and_Kiro_Web_asks_for_repositories()
    {
        var h = new PanelHarness();
        h.State();
        h.Ui.ChatView = true;
        h.Frame();
        h.Click("pillAgent");
        // Kiro is first, Codex second.
        h.Click("menu", 1);
        h.Click("taskText");
        h.Type("hello");
        h.Click("startTask");
        Assert.Equal("codex", h.Last<NewTaskCommand>().Tool);
        // The cloud choice belongs to Kiro.
        h.Click("pillAgent");
        h.Click("menu", 0);
        h.Click("pillWhere");
        h.Click("menu", 1);
        Assert.Contains(h.Sent, c => c is CloudReposCommand);
        h.Accept(new CloudReposEvent { Ok = true, Done = true, Repos = ["me/app", "me/site"] });
        h.Click("pillRepo");
        h.Click("menu", s: "me/app");
        h.Click("taskText");
        h.Type("web task");
        h.Click("startTask");
        var cmd = h.Last<NewTaskCommand>();
        Assert.Equal(["me/app"], cmd.Cloud);
    }

    [Theory]
    [InlineData("askAllow", "allow")]
    [InlineData("askTrust", "trust")]
    [InlineData("askDeny", "deny")]
    public void A_permission_is_answered_from_the_chat(string button, string answer)
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "waiting", PanelHarness.Permission(), PanelHarness.Turn("Install three", "", "waiting")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        var c = h.Frame();
        Assert.Contains("Wants to run a command", c.Texts);
        Assert.True(h.Click(button));
        var cmd = h.Last<AnswerCommand>();
        Assert.Equal(1, cmd.Id);
        Assert.Equal("a1", cmd.Ask);
        Assert.Equal(answer, cmd.Answer);
    }

    [Fact]
    public void A_permission_is_answered_over_the_bot_in_the_office()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "waiting", PanelHarness.Permission(), PanelHarness.Turn("Install three", "", "waiting")));
        h.Frame();
        Assert.NotNull(h.Ui.Find("askAllow"));
        h.Click("askAllow");
        Assert.Equal("allow", h.Last<AnswerCommand>().Answer);
    }

    [Fact]
    public void Two_questions_at_once_are_answered_one_by_one()
    {
        var h = new PanelHarness();
        h.State(
            PanelHarness.Session(1, "waiting", PanelHarness.Permission("a1"), PanelHarness.Turn("One", "", "waiting")),
            PanelHarness.Session(2, "waiting", PanelHarness.Permission("a2") with { Command = "rm -rf build" }, PanelHarness.Turn("Two", "", "waiting")));
        h.Frame();
        Assert.Equal(2, h.Ui.Targets.Count(t => t.Id == "askDeny"));
        var second = h.Ui.Targets.Last(t => t.Id == "askDeny" && t.S == "a2");
        h.ClickAt((second.R.X + second.R.W / 2, second.R.Y + second.R.H / 2));
        var cmd = h.Last<AnswerCommand>();
        Assert.Equal((2, "a2", "deny"), (cmd.Id, cmd.Ask, cmd.Answer));
        // The other still waits.
        Assert.Single(h.Sent.OfType<AnswerCommand>());
    }

    [Fact]
    public void A_question_with_choices_sends_the_picked_answers()
    {
        var ask = new AskInfo
        {
            Id = "q1", Kind = "question", Title = "A question",
            Questions = [new AskQuestion { Header = "Pick", Question = "Which one?", Options = [new AskOption { Label = "Red" }, new AskOption { Label = "Blue" }] }],
        };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "waiting", ask, PanelHarness.Turn("Colour", "", "waiting")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        var red = h.Ui.Targets.First(t => t.Id == "askOpt" && t.S == "Red");
        h.ClickAt((red.R.X + red.R.W / 2, red.R.Y + red.R.H / 2));
        h.Click("askAnswer");
        var cmd = h.Last<AnswerCommand>();
        Assert.Null(cmd.Answer);
        Assert.Equal([["Red"]], cmd.Answers);
    }

    [Fact]
    public void A_reply_is_sent_and_an_empty_box_pauses_the_run()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "working", null, PanelHarness.Turn("Go", "", "working")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        h.Click("compose");
        Assert.True(h.Ui.Composing);
        h.Type("and this too");
        h.Key(UiKey.Enter);
        var reply = h.Last<ReplyCommand>();
        Assert.Equal(("and this too", 1), (reply.Text, reply.Id));
        Assert.False(h.Ui.Composing);
        // Open it again with nothing typed: the round button pauses.
        h.Click("compose");
        Assert.True(h.Click("pause"));
        Assert.Equal(1, h.Last<PauseCommand>().Id);
    }

    [Fact]
    public void A_queued_message_can_be_sent_now_or_taken_back_into_the_box()
    {
        var queued = PanelHarness.Turn("Then this", "", "done") with { Queued = true, Uid = "u9" };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "working", null, PanelHarness.Turn("First", "Done first.", "working"), queued));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        var hits = h.Ui.Thread.Sections[1].Frag.Hits;
        var now = hits.First(x => x.Act.Kind == ActKind.SendNow);
        h.ClickAt(h.InThread(1, now.R));
        Assert.Equal("u9", h.Last<QueueSendNowCommand>().Uid);
        var edit = h.Ui.Thread.Sections[1].Frag.Hits.First(x => x.Act.Kind == ActKind.Edit);
        h.ClickAt(h.InThread(1, edit.R));
        Assert.Equal("u9", h.Last<QueueRemoveCommand>().Uid);
        h.Accept(new QueueRemoveEvent { Id = 1, Uid = "u9", Ok = true, Text = "Then this" });
        Assert.True(h.Ui.Composing);
        Assert.Equal("Then this", h.Ui.ReplyText);
    }

    [Fact]
    public void A_checkpoint_is_restored_only_after_the_user_confirms()
    {
        var turn = PanelHarness.Turn("Change it", "Changed.") with { Restore = true, Again = true };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, turn));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        var restore = h.Ui.Thread.Sections[0].Frag.Hits.First(x => x.Act.Kind == ActKind.Restore);
        h.ClickAt(h.InThread(0, restore.R));
        Assert.DoesNotContain(h.Sent, c => c is RestoreCommand);
        Assert.Contains("Restore to after this answer?", h.Frame().Texts);
        h.Click("confirmNo");
        Assert.DoesNotContain(h.Sent, c => c is RestoreCommand);
        restore = h.Ui.Thread.Sections[0].Frag.Hits.First(x => x.Act.Kind == ActKind.Restore);
        h.ClickAt(h.InThread(0, restore.R));
        h.Click("confirmYes");
        Assert.Equal((1, 0), (h.Last<RestoreCommand>().Id, h.Last<RestoreCommand>().Turn));
        var again = h.Ui.Thread.Sections[0].Frag.Hits.First(x => x.Act.Kind == ActKind.TryAgain);
        h.ClickAt(h.InThread(0, again.R));
        h.Key(UiKey.Enter);
        Assert.Equal(0, h.Last<AgainCommand>().Turn);
    }

    [Fact]
    public void Selected_text_is_copied()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Say hello", "Hello there, friend.")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        var answer = h.Ui.Thread.Sections[0].Frag.Texts.First(t => t.Text.StartsWith("Hello there", StringComparison.Ordinal));
        var s = h.Ui.Thread.Sections[0];
        float x0 = h.Ui.ThreadArea.X + Palette.ThreadPad[3] + answer.X, y = h.Ui.ThreadArea.Y + s.Y + answer.Y + 4 - h.Ui.ScrollY;
        h.Ui.PointerMove(x0 + 1, y);
        h.Ui.PointerDown(x0 + 1, y, 1, UiMods.None);
        h.Ui.PointerMove(x0 + 400, y);
        h.Ui.PointerUp(x0 + 400, y);
        h.Frame();
        Assert.True(h.Ui.Thread.HasSel);
        Assert.True(h.Key(UiKey.C, UiMods.Control));
        Assert.Equal("Hello there, friend.", h.Clip.Value);
    }

    [Fact]
    public void A_chat_is_renamed_in_its_title()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Hi", "Hi.")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        h.Click("rename");
        // The old name is selected, so typing replaces it.
        h.Type("A better name");
        h.Key(UiKey.Enter);
        Assert.Equal("A better name", h.Last<RenameCommand>().Name);
    }

    [Fact]
    public void A_ten_thousand_line_answer_lays_out_and_scrolls()
    {
        var text = string.Join("\n\n", Enumerable.Range(1, 10_000).Select(i => $"Line {i} of the answer"));
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Print it all", text)));
        h.Ui.ChatView = true;
        var clock = Stopwatch.StartNew();
        h.Ui.OpenChat(1);
        h.Frame();
        Assert.True(clock.Elapsed < TimeSpan.FromSeconds(20), $"took {clock.Elapsed}");
        Assert.True(h.Ui.Thread.Height > 100_000);
        // It opens at the end; the wheel moves it up.
        var end = h.Ui.ScrollY;
        Assert.True(end > 0);
        Assert.True(h.Ui.Wheel(600, 400, 300));
        h.Now += 500;
        h.Frame();
        Assert.True(h.Ui.ScrollY < end);
    }

    [Fact]
    public void Bad_diagrams_and_pictures_that_fail_draw_without_breaking_the_frame()
    {
        var h = new PanelHarness();
        h.Markdown = _ =>
        [
            new MdBlock { Kind = "diagram", Text = "<svg width='10' height='10'><path d='M 0 0 L'" },
            new MdBlock { Kind = "diagram", Text = "not svg at all" },
            new MdBlock { Kind = "para", Inlines = [new MdInline { Kind = "image", Text = "https://example.invalid/x.png", Alt = "broken" }] },
        ];
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Draw", "see below")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        var c = h.Frame();
        Assert.True(c.Fills > 0);
    }

    [Fact]
    public void A_huge_change_is_laid_out_and_the_backend_going_away_is_said()
    {
        var diff = "--- a/f.ts\n+++ b/f.ts\n" + string.Join("\n", Enumerable.Range(1, 5000).Select(i => (i % 2 == 0 ? "+" : "-") + $"line {i}"));
        var step = new StepInfo { K = "edit", Verb = "Edited", Name = "f.ts", Dir = "src", Status = "completed", Add = 2500, Del = 2500, Diff = diff };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Change it", "Changed.", "done", step)));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        Assert.True(h.Ui.Thread.Height > 100);
        h.Accept(new ToastEvent { Text = "The agent backend stopped." });
        Assert.Equal("The agent backend stopped.", h.Ui.ToastText);
        Assert.Contains("The agent backend stopped.", h.Canvas.Texts);
    }

    [Fact]
    public void Escape_leaves_the_reply_box_and_keeps_the_words()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Hi", "Hi.")));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        h.Click("compose");
        h.Type("draft words");
        h.Key(UiKey.Escape);
        Assert.False(h.Ui.Composing);
        // Back in the box, the words are there.
        h.Click("compose");
        Assert.Equal("draft words", h.Ui.ReplyText);
    }

    [Fact]
    public void The_session_list_opens_a_chat()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Hi", "Hi.")), PanelHarness.Session(2, "done", null, PanelHarness.Turn("Other", "Other.")) with { Title = "Another chat", Key = "k2" });
        h.Ui.ChatView = true;
        var c = h.Frame();
        Assert.Contains("Another chat", c.Texts);
        h.Click("row", 0);
        Assert.NotNull(h.Ui.OpenId);
        Assert.Contains(h.Sent, x => x is OpenCommand);
    }

    [Fact]
    public void Opening_and_closing_a_chat_tells_the_office_and_the_door_opens_the_new_task_box()
    {
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("Hi", "Hi.")));
        int? told = -1;
        h.Ui.DrawerChanged += id => told = id;
        h.Ui.OpenChat(1);
        Assert.Equal(1, told);
        h.Frame();
        h.Ui.CloseChat();
        Assert.Null(told);
        h.Ui.OpenNewTask();
        h.Frame();
        Assert.NotNull(h.Ui.Find("taskText"));
        // The session going away closes its chat too.
        h.Ui.OpenChat(1);
        h.State();
        Assert.Null(told);
        Assert.Null(h.Ui.OpenId);
    }

    [Fact]
    public void The_chat_menu_forks_switches_agent_and_deletes_after_asking()
    {
        var turn = PanelHarness.Turn("Hi", "Hi.") with { SwitchTo = "codex" };
        var h = new PanelHarness();
        h.State(PanelHarness.Session(1, "done", null, PanelHarness.Turn("One", "One."), turn));
        h.Ui.ChatView = true;
        h.Ui.OpenChat(1);
        h.Frame();
        void Pick(string label)
        {
            h.Click("more");
            var row = h.Ui.Targets.Last(t => t.Id == "menu" && t.S == label);
            h.ClickAt((row.R.X + row.R.W / 2, row.R.Y + row.R.H / 2));
        }
        Pick("Fork this chat");
        Assert.Equal((1, 1), (h.Last<ForkCommand>().Id, h.Last<ForkCommand>().Turn));
        Pick("Codex");
        Assert.Equal(("codex", 1), (h.Last<SwitchProviderCommand>().To, h.Last<SwitchProviderCommand>().Id));
        Pick("Bring back messages");
        Assert.Equal(1, h.Last<BringBackCommand>().Id);
        // A fork that was made opens.
        h.Accept(new ForkEvent { Id = 1, Ok = true, ForkId = 2 });
        // Delete asks first.
        h.State(PanelHarness.Session(1, "done", null, turn), PanelHarness.Session(2, "done", null, turn) with { Key = "k2" });
        h.Ui.OpenChat(1);
        h.Frame();
        Pick("Delete chat");
        Assert.DoesNotContain(h.Sent, c => c is DeleteCommand);
        h.Click("confirmYes");
        Assert.Equal(1, h.Last<DeleteCommand>().Id);
        Assert.Null(h.Ui.OpenId);
        // A refused fork says why.
        h.Accept(new ForkEvent { Id = 1, Ok = false, Error = "Nothing to fork." });
        Assert.Equal("Nothing to fork.", h.Ui.ToastText);
    }
}
