using System.Diagnostics;
using Hover.Chat;
using Hover.Host.Protocol;
using Xunit;
using static Hover.Tests.ChatFixtures;

namespace Hover.Tests;

/// <summary>The chat thread laid out with a fixed-width stand-in for the text shaper: placement, copy, selection, folding.</summary>
public class ChatThreadTests
{
    static Step Run(string cmd, int? exit = 0, string output = "ok", double ms = 300) => new()
    {
        Kind = StepIcon.Run, Verb = "Ran", Cmd = cmd, Out = output, HasExit = exit is not null, Exit = exit ?? 0, Ms = ms, HasMs = true,
    };

    static Step Edit(string name, string dir, int add, int del, string diff = "") => new()
    {
        Kind = StepIcon.Edit, Verb = "Edited", Name = name, Dir = dir, Add = add, Del = del, Diff = diff,
    };

    static string All(ChatThread t)
    {
        t.SelectAll();
        return t.SelectedText();
    }

    [Fact]
    public void A_turn_is_a_bubble_a_name_the_answer_and_its_buttons()
    {
        var th = Thread();
        th.Set([Done("Make it faster", "Done. It is faster now.")], 400);
        Assert.Single(th.Sections);
        var s = th.Sections[0];
        Assert.True(s.H > 80);
        Assert.Equal(s.H + Palette.ThreadPad[0] + Palette.ThreadPad[2], th.Height, 0.01);
        // Edit and Copy on the prompt; Copy under the answer.
        var acts = s.Frag.Hits.Select(h => h.Act.Kind).ToList();
        Assert.Contains(ActKind.EditPrompt, acts);
        Assert.Equal(2, acts.Count(a => a == ActKind.Copy));
        // The newest finished turn offers Retry, unless a checkpoint offers Try again.
        Assert.Contains(ActKind.Retry, acts);
        Assert.Contains(s.Frag.Texts, t => t.Text == "Moss");
    }

    [Fact]
    public void A_select_all_copies_the_prompt_and_the_answer_in_order_with_a_blank_line_between_paragraphs()
    {
        var th = Thread();
        th.Set([Done("Make it faster", "First part.\n\nSecond part.")], 400);
        var copy = All(th);
        var iPrompt = copy.IndexOf("Make it faster", StringComparison.Ordinal);
        var iFirst = copy.IndexOf("First part.", StringComparison.Ordinal);
        var iSecond = copy.IndexOf("Second part.", StringComparison.Ordinal);
        Assert.True(iPrompt >= 0 && iPrompt < iFirst && iFirst < iSecond, copy);
        Assert.Contains("First part.\n\nSecond part.", copy);
        // What is drawn but not selectable is not in the copy: the buttons' words.
        Assert.DoesNotContain("Retry", copy);
        Assert.DoesNotContain("Copy", copy);
        Assert.DoesNotContain(Runs.GapChar.ToString(), copy);
    }

    [Fact]
    public void The_answer_alone_is_what_a_select_all_inside_it_copies()
    {
        var th = Thread();
        th.Set([Done("Hi", "One.\n\nTwo.")], 400);
        Assert.Equal("One.\n\nTwo.", th.AnswerText(0));
    }

    [Fact]
    public void A_drag_selects_part_of_a_line_and_copies_just_that()
    {
        var th = Thread();
        th.Set([Done("Make it faster", "Hello there world")], 600);
        var s = th.Sections[0];
        var ti = s.Frag.Texts.FindIndex(t => t.Text == "Hello there world");
        var box = s.Frag.Texts[ti];
        // From before "there" to the end of it, by pixels of the box.
        float x0 = Palette.ThreadPad[3] + box.X + box.Layout.SnapX(6 * 12.5f * 0.5f), y = s.Y + box.Y + 3;
        var a = th.Hit(x0 + 1, y);
        var b = th.Hit(Palette.ThreadPad[3] + box.X + 11 * 12.5f * 0.5f, y);
        Assert.Equal(HitKind.Text, a.Kind);
        th.Select(a.Pos, b.Pos);
        Assert.True(th.HasSel);
        Assert.Equal("there", th.SelectedText());
    }

    [Fact]
    public void A_double_click_selects_a_word_and_a_triple_click_the_line()
    {
        var th = Thread();
        th.Set([Done("Make it faster", "alpha beta.gamma 3.14 x\nsecond line")], 900);
        var s = th.Sections[0];
        var ti = s.Frag.Texts.FindIndex(t => t.Text.StartsWith("alpha", StringComparison.Ordinal));
        var text = s.Frag.Texts[ti].Text;
        var p = new Pos(0, ti, text.IndexOf("gamma", StringComparison.Ordinal) + 2);
        var (a, z, tail) = th.WordAt(p);
        Assert.Equal("gamma", text[a.Index..z.Index]);
        // 3.14 is one word.
        var pi = new Pos(0, ti, text.IndexOf("3.14", StringComparison.Ordinal) + 1);
        (a, z, _) = th.WordAt(pi);
        Assert.Equal("3.14", text[a.Index..z.Index]);
        var (la, lz, lt) = th.ParagraphAt(p);
        Assert.Equal("alpha beta.gamma 3.14 x\n", text[la.Index..lz.Index]);
        Assert.Equal(Tail.None, lt);
        Assert.Equal(Tail.None, tail);
    }

    [Fact]
    public void A_command_row_says_its_exit_code_and_a_failed_one_is_red_and_opens_its_output()
    {
        var th = Thread();
        var failed = Run("npm test", exit: 1, output: "1 failing");
        th.Set([Done("Test", "It failed.", failed)], 400);
        var s = th.Sections[0];
        Assert.Contains(s.Frag.Texts, t => t.Text.StartsWith("exit 1", StringComparison.Ordinal));
        Assert.Contains(s.Frag.Hits, h => h.Act.Kind == ActKind.Step);
        var folded = s.H;
        // Opening it shows the output and its exit code in the box.
        th.ToggleStep([Done("Test", "It failed.", failed)], 0, 0, false);
        var open = th.Sections[0];
        Assert.True(open.H > folded);
        Assert.Contains(open.Frag.Texts, t => t.Text == "1 failing");
    }

    [Fact]
    public void A_group_of_steps_has_one_sentence_and_opens_on_a_click()
    {
        var turn = Done("Go", "Done", Run("a"), Run("b"), new Step { Kind = StepIcon.Read, Verb = "Read", Name = "x.ts", Dir = "src" }, new Step { Kind = StepIcon.Read, Verb = "Read", Name = "y.ts", Dir = "src" });
        Assert.Equal("Read 2 files, ran 2 commands", ChatThread.StepSentence([turn.Steps[2], turn.Steps[3], turn.Steps[0], turn.Steps[1]], false));
        var th = Thread();
        th.Set([turn], 400);
        var folded = th.Sections[0].H;
        Assert.Contains(th.Sections[0].Frag.Hits, h => h.Act.Kind == ActKind.Group);
        th.ToggleGroup([turn], 0, 0);
        Assert.True(th.Sections[0].H > folded + 40);
    }

    [Fact]
    public void A_live_turn_shows_its_steps_open_and_a_finished_one_folded()
    {
        var steps = new[] { Run("make", output: "line"), Edit("a.ts", "src", 2, 1, "@@ -1 +1 @@\n-a\n+b") };
        var live = Done("Go", "", steps);
        live.Live = true;
        live.Stage = Stage.Working;
        var done = Done("Go", "Fine", steps);
        var th = Thread();
        th.Set([live], 400);
        var liveH = th.Sections[0].H;
        var th2 = Thread();
        th2.Set([done], 400);
        Assert.True(liveH > th2.Sections[0].H - 20);
        Assert.Contains(th.Sections[0].Frag.Texts, t => t.Text == "line");
        Assert.DoesNotContain(th2.Sections[0].Frag.Texts, t => t.Text == "line");
    }

    [Fact]
    public void A_change_lists_its_files_with_counts_and_a_click_opens_the_diff()
    {
        var turn = Done("Go", "Fine", Edit("a.ts", "src/app", 3, 1, "@@ -1 +1,3 @@\n-a\n+b\n+c\n+d"), Edit("b.ts", "src/app", 1, 0));
        var th = Thread();
        th.Set([turn], 400);
        var texts = th.Sections[0].Frag.Texts.Select(t => t.Text).ToList();
        Assert.Contains("2 files changed", texts);
        Assert.Contains("+4 −1", texts);
        Assert.Contains("app/a.ts", texts);
        var open = Assert.Single(th.Sections[0].Frag.Hits, h => h.Act.Kind == ActKind.OpenDiff);
        var y = th.OpenDiff([turn], 0, open.Act.I);
        Assert.NotNull(y);
        Assert.Contains(th.Sections[0].Frag.Texts, t => t.Text == "+d");
    }

    [Fact]
    public void A_long_output_shows_its_last_lines_and_all_when_asked()
    {
        var output = string.Join("\n", Enumerable.Range(1, 40).Select(i => $"line {i}"));
        var turn = Done("Go", "Fine", Run("ls", output: output));
        var th = Thread();
        th.Set([turn], 500);
        // A finished turn's steps are folded; a click opens the command's output.
        th.ToggleStep([turn], 0, 0, false);
        var texts = th.Sections[0].Frag.Texts.Select(t => t.Text).ToList();
        Assert.Contains("line 40", texts);
        Assert.DoesNotContain("line 3", texts);
        var fold = th.Sections[0].Frag.Hits.Single(h => h.Act.Kind == ActKind.Flag);
        th.ToggleFlag([turn], 0, fold.Act.I, 0);
        Assert.Contains("line 3", th.Sections[0].Frag.Texts.Select(t => t.Text));
    }

    [Fact]
    public void A_10000_line_answer_lays_out_without_trouble_and_selects_whole()
    {
        var code = string.Join("\n", Enumerable.Range(1, 10_000).Select(i => i == 5000 ? "const wide = '" + new string('x', 400) + "';" : $"const v{i} = {i};"));
        var source = new FakeSource { Markdown = _ => [Code(code)] };
        var th = Thread(source);
        var sw = Stopwatch.StartNew();
        th.Set([Done("Big", "x")], 500);
        Assert.True(sw.Elapsed < TimeSpan.FromSeconds(5), $"took {sw.Elapsed}");
        var copy = All(th);
        Assert.Contains("const v10000 = 10000;", copy);
        Assert.Contains("const v1 = 1;", copy);
        // A code block this wide scrolls sideways.
        Assert.Single(th.Sections[0].Frag.Scrollers);
    }

    [Fact]
    public void A_code_block_has_a_copy_button_that_copies_the_code_and_says_so()
    {
        var source = new FakeSource { Markdown = _ => [Code("let a = 1;")] };
        var th = Thread(source);
        var turn = Done("Show", "x");
        th.Set([turn], 500);
        var copy = th.Sections[0].Frag.Hits.First(h => h.Act is { Kind: ActKind.Copy, Text: "let a = 1;" });
        Assert.NotNull(copy.Act.Text);
        th.SetCopied([turn], copy.Act.Text);
        Assert.Contains(th.Sections[0].Frag.Texts, t => t.Text == "Copied");
    }

    [Fact]
    public void Markdown_that_has_not_come_back_shows_plain_text_and_is_laid_out_again_when_it_does()
    {
        var pending = new PendingSource(new FakeSource());
        var th2 = new ChatThread(new FakeShaper(), pending, "Moss");
        th2.Set([Done("Hi", "# Title")], 400);
        Assert.True(th2.Sections[0].Waiting);
        Assert.Contains(th2.Sections[0].Frag.Texts, t => t.Text == "# Title");
        pending.Done = true;
        Assert.True(th2.Refresh());
        th2.Set([Done("Hi", "# Title")], 400);
        Assert.False(th2.Sections[0].Waiting);
        Assert.Contains(th2.Sections[0].Frag.Texts, t => t.Text == "Title");
    }

    sealed class PendingSource(FakeSource inner) : IRenderSource
    {
        public bool Done;
        public Rendered<IReadOnlyList<MdBlock>> Blocks(string src, string folder, string files)
            => Done ? new([new MdBlock { Kind = "heading", Level = 3, Inlines = [new MdInline { Text = "Title" }] }], true) : new(null, false);
        public Rendered<IReadOnlyList<HighlightSpan>> Highlight(string info, string text) => inner.Highlight(info, text);
        public Rendered<IReadOnlyList<DiffLine>> Diff(string diff) => inner.Diff(diff);
        public ImageState Image(string src) => inner.Image(src);
    }

    [Fact]
    public void A_picture_that_will_not_load_is_a_box_with_its_alt_text_that_copies_nothing()
    {
        var source = new FakeSource { Markdown = _ => [new MdBlock { Kind = "para", Inlines = [new MdInline { Kind = "image", Text = "https://x/y.png", Alt = "a chart" }] }] };
        var th = Thread(source);
        th.Set([Done("Hi", "x")], 400);
        var s = th.Sections[0];
        Assert.Contains(s.Frag.Shapes, sh => sh.Kind == ShapeKind.Broken);
        Assert.Contains("https://x/y.png", s.Images);
        Assert.DoesNotContain("a chart", All(th));
    }

    [Fact]
    public void A_picture_that_loads_is_drawn_no_wider_than_the_answer_and_no_taller_than_320()
    {
        var source = new FakeSource
        {
            Markdown = _ => [new MdBlock { Kind = "para", Inlines = [new MdInline { Kind = "image", Text = "p", Alt = "" }] }],
            Pictures = _ => new ImageState(ImageKind.Ready, 2000, 1000),
        };
        var th = Thread(source);
        th.Set([Done("Hi", "x")], 400);
        var img = th.Sections[0].Frag.Shapes.Single(s => s is { Kind: ShapeKind.Image, Cover: false });
        Assert.True(img.W <= 400 - 24 + 0.01 && img.H <= 320.01, $"{img.W} x {img.H}");
        Assert.Equal(2.0, img.W / img.H, 2);
    }

    [Fact]
    public void A_table_has_a_header_cells_and_copies_with_tabs()
    {
        var cell = (string a, string text) => new MdCell { Align = a, Content = [new MdInline { Text = text }] };
        var table = new MdBlock { Kind = "table", Head = [cell("left", "Monitor"), cell("right", "Scale")], Rows = [[cell("left", "Main"), cell("right", "100%")], [cell("left", "Second"), cell("right", "150%")]] };
        var th = Thread(new FakeSource { Markdown = _ => [table] });
        th.Set([Done("Hi", "x")], 400);
        Assert.Contains("Monitor\tScale\nMain\t100%\nSecond\t150%", All(th));
    }

    [Fact]
    public void Lists_have_markers_and_a_task_item_has_a_box()
    {
        var item = (string text, bool? task) => new MdItem { Task = task, Content = [new MdInline { Text = text }] };
        var list = new MdBlock { Kind = "list", Ordered = true, Start = 3, Items = [item("one", null), item("two", true), item("three", false)] };
        var th = Thread(new FakeSource { Markdown = _ => [list] });
        th.Set([Done("Hi", "x")], 400);
        var s = th.Sections[0];
        Assert.Contains(s.Frag.Texts, t => t.Text == "" && t.Layout.Width > 0);
        Assert.Equal(2, s.Frag.Shapes.Count(sh => sh is { Kind: ShapeKind.Rect, Radius: [3, 3, 3, 3] }));
        Assert.Contains("one", All(th));
    }

    [Fact]
    public void A_queued_message_has_send_now_and_edit_and_a_dashed_outline()
    {
        var q = Done("Later", "");
        q.Queued = true;
        q.Stage = Stage.Waking;
        var th = Thread();
        th.Set([q], 400);
        var s = th.Sections[0];
        var acts = s.Frag.Hits.Select(h => h.Act.Kind).ToList();
        Assert.Contains(ActKind.SendNow, acts);
        Assert.Contains(ActKind.Edit, acts);
        Assert.Contains(s.Frag.Shapes, sh => sh.Kind == ShapeKind.Line);
        Assert.Contains(s.Frag.Texts, t => t.Text == "Queued · sends when this run ends");
    }

    [Fact]
    public void A_turn_that_can_go_back_offers_restore_and_try_again_instead_of_retry()
    {
        var t = Done("Go", "Fine");
        t.Restore = true;
        t.Again = true;
        var th = Thread();
        th.Set([t], 400);
        var acts = th.Sections[0].Frag.Hits.Select(h => h.Act.Kind).ToList();
        Assert.Contains(ActKind.Restore, acts);
        Assert.Contains(ActKind.TryAgain, acts);
        Assert.DoesNotContain(ActKind.Retry, acts);
    }

    [Fact]
    public void A_section_is_laid_out_again_only_when_its_turn_changes()
    {
        var th = Thread();
        var a = Done("One", "A");
        var b = Done("Two", "B");
        th.Set([a, b], 400);
        var n = th.Relayouts;
        th.Set([a, b], 400);
        Assert.Equal(n, th.Relayouts);
        b.Answer = "B and more";
        th.Set([a, b], 400);
        Assert.Equal(n + 1, th.Relayouts);
        th.Set([a, b], 300);
        Assert.Equal(n + 3, th.Relayouts);
    }

    [Fact]
    public void A_failed_turn_has_the_red_rule()
    {
        var failed = Done("Go", "It broke");
        failed.Stage = Stage.Failed;
        var th = Thread();
        th.Set([failed], 400);
        Assert.Contains(th.Sections[0].Frag.Shapes, s => s is { Kind: ShapeKind.Rect, W: 2 } && s.Fill == Palette.Bad);
    }

    [Fact]
    public void Times_and_costs_are_worded_as_the_page_words_them()
    {
        Assert.Equal("1 s", Turns.TookText(200));
        Assert.Equal("3m 07s", Turns.TookText(187_000));
        Assert.Equal("1h 05m", Turns.TookText(3_900_000));
        Assert.Equal("0:12", Turns.ClockText(12_400));
        Assert.Equal("<0.01 credits", Turns.CreditsText(0.001));
        Assert.Equal("1.00 credit", Turns.CreditsText(1.0));
        Assert.Equal("0.09 credits", Turns.CreditsText(0.09));
        Assert.Equal("18s", Turns.Secs(18_000));
        Assert.Equal("1m 05s", Turns.Secs(65_000));
        Assert.Equal("1:47 PM", Turns.Hm(13 * 3600_000.0 + 47 * 60_000, 0));
        Assert.Equal("12:05 AM", Turns.Hm(5 * 60_000.0, 0));
    }

    [Fact]
    public void A_session_from_the_backend_becomes_turns_the_thread_can_show()
    {
        var session = new SessionInfo
        {
            Id = 3, Stage = "working",
            Turns =
            [
                new TurnInfo { Prompt = "Old", Stage = "done", Answer = "Done", T0 = 1_000, Took = 2000, Credits = 0.1 },
                new TurnInfo { Prompt = "New", Stage = "working", Steps = [new StepInfo { K = "run", Verb = "Ran", Cmd = "ls", Status = "completed", Exit = 0, Ms = 20 }], T0 = 70_000 },
                new TurnInfo { Prompt = "Later", Stage = "queued", Queued = true, Uid = "u1" },
            ],
        };
        var turns = Turns.From(session, 75_000, ms => "t");
        Assert.Equal(3, turns.Count);
        Assert.False(turns[0].Live);
        Assert.True(turns[1].Live);
        Assert.Equal("0:05", turns[1].Clock);
        Assert.True(turns[2].Queued);
        Assert.Equal("u1", turns[2].Uid);
        Assert.Equal("2 s", turns[0].Took);
        Assert.True(turns[1].Steps[0].HasExit);
        var th = Thread();
        th.Set(turns, 400);
        Assert.Equal(3, th.Sections.Count);
    }
}
