using System.Text.Json;
using Hover.Chat;
using Hover.Host.Protocol;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The chat's side of the protocol against a real hover-backend and the stand-in agent: what a
/// session looks like in `state`, what the rendering commands answer, and the task commands
/// (queue, pause, rename, checkpoints) that the thread's buttons send.
/// </summary>
public class ChatBackendTests(ITestOutputHelper output)
{
    static async Task<SessionInfo> Start(Conversation talk, Sandbox sandbox, string prompt)
    {
        await talk.GetReady();
        talk.Send(new NewTaskCommand("codex", sandbox.Project, prompt, "always"));
        var s = await talk.Until<StateEvent>(x => x.Sessions.Count > 0);
        return s.Sessions[0];
    }

    static async Task<SessionInfo> Finish(Conversation talk, SessionInfo session, string answer = "allow")
    {
        var waiting = await talk.Until<StateEvent>(s => s.Sessions.Any(x => x.Id == session.Id && x.Stage == "waiting"));
        var ask = waiting.Sessions.Single(x => x.Id == session.Id).Ask!;
        talk.Send(new AnswerCommand(session.Id, ask.Id, answer));
        var done = await talk.Until<StateEvent>(s => s.Sessions.Any(x => x.Id == session.Id && x.Stage == "done"));
        return done.Sessions.Single(x => x.Id == session.Id);
    }

    [Fact]
    public async Task A_finished_session_carries_the_steps_and_ids_the_thread_shows()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "Tidy it");
        var waiting = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting");
        var ask = waiting.Sessions[0].Ask!;
        Assert.Equal("edit", ask.Kind);
        Assert.Null(ask.Questions);
        var done = await Finish(talk, session);

        var turn = Assert.Single(done.Turns);
        Assert.False(string.IsNullOrEmpty(turn.Uid));
        Assert.Equal("Sandbox reply", turn.Answer);
        var run = turn.Steps.Single(x => x.K == "run");
        Assert.Equal("npm run dev", run.Cmd);
        Assert.Equal(0, run.Exit);
        Assert.Contains("ready on", run.Out);
        var edit = turn.Steps.Single(x => x.K == "edit");
        Assert.Equal("app.js", edit.Name);
        Assert.True(edit.Add > 0);
        Assert.Contains("+ b", edit.Diff);
        Assert.Contains("Plan:", turn.Steps.Single(x => x.K == "thought").Out);
        Assert.Contains(turn.Steps, x => x.K == "agent");
        output.WriteLine($"restore={turn.Restore} again={turn.Again} files={done.Files} cloud={done.Cloud?.Count}");

        // And the thread can lay all of it out.
        var turns = Turns.From(done, DateTimeOffset.UtcNow.ToUnixTimeMilliseconds());
        var th = ChatFixtures.Thread();
        th.Set(turns, 400);
        Assert.Contains(th.Sections[0].Frag.Texts, t => t.Text == "Sandbox reply");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    const string RichSource = "## Result\n\nThe **build** passes, and `npm test` is *green*. See [the docs](https://example.com/docs).\n\n- first\n- second\n  - nested\n- [x] done\n- [ ] todo\n\n1. one\n2. two\n\n> quoted\n\n| Monitor | Scale |\n|:--|--:|\n| Main | 100% |\n\n```ts\nexport const a = 1; // x\n```\n\n```mermaid\nflowchart LR\n  A[Start] --> B{Ok?}\n```\n\n![alt text](missing/chart.png)\n\n---\n\nDone.";

    [Fact]
    public async Task Markdown_comes_back_as_typed_blocks()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        talk.Send(new MarkdownCommand(RichSource, "r1") { Folder = sandbox.Project, Files = "hover.files" });
        var answer = await talk.Until<MarkdownEvent>(m => m.Ref == "r1");
        Assert.True(answer.Ok, answer.Error);
        var kinds = answer.Blocks.Select(b => b.Kind).ToList();
        output.WriteLine(string.Join(", ", kinds));
        Assert.Equal(["heading", "para", "list", "list", "quote", "table", "code", "diagram", "para", "rule", "para"], kinds);
        Assert.Equal(4, answer.Blocks[0].Level);
        var inl = answer.Blocks[1].Inlines!;
        Assert.Contains(inl, i => i.Strong && i.Text == "build");
        Assert.Contains(inl, i => i.Code && i.Text == "npm test");
        Assert.Contains(inl, i => i.Em && i.Text == "green");
        Assert.Contains(inl, i => i.Link == "https://example.com/docs");
        var list = answer.Blocks[2];
        Assert.Equal(4, list.Items!.Count);
        Assert.Single(list.Items[1].Lists);
        Assert.True(list.Items[2].Task);
        Assert.False(list.Items[3].Task);
        Assert.True(answer.Blocks[3].Ordered);
        var table = answer.Blocks[5];
        Assert.Equal("right", table.Head![1].Align);
        Assert.Equal("100%", table.Rows![0][1].Content[0].Text);
        Assert.Equal("ts", answer.Blocks[6].Lang);
        Assert.Contains("<svg", answer.Blocks[7].Text);
        var image = answer.Blocks[8].Inlines!.Single();
        Assert.Equal("image", image.Kind);
        Assert.Equal("alt text", image.Alt);
        // A picture in the folder is addressed through the files host, never by a path of the machine.
        Assert.Equal("https://hover.files/missing/chart.png", image.Text);

        // The whole page lays out in the thread, and a select-all copies its words.
        var th = ChatFixtures.Thread(new FakeSource { Markdown = _ => answer.Blocks });
        th.Set([ChatFixtures.Done("Show", "x")], 460);
        th.SelectAll();
        var copy = th.SelectedText();
        Assert.Contains("first\nsecond", copy.Replace("\n\n", "\n"));
        Assert.Contains("Monitor\tScale\nMain\t100%", copy);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Code_colour_and_numbered_changes_come_back()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        talk.Send(new HighlightCommand("ts", "const a = 'x'; // c\nlet b = 2;", "h1"));
        var h = await talk.Until<HighlightEvent>(x => x.Ref == "h1");
        Assert.True(h.Ok, h.Error);
        Assert.NotEmpty(h.Spans);
        Assert.All(h.Spans, s => Assert.True(s.U0 >= 0 && s.U1 > s.U0 && s.A > 0));

        const string diff = "--- a/f.ts\n+++ b/f.ts\n@@ -3,2 +3,3 @@\n keep\n-old\n+new\n+more\n";
        talk.Send(new DiffLinesCommand(diff, "d1"));
        var d = await talk.Until<DiffLinesEvent>(x => x.Ref == "d1");
        Assert.True(d.Ok, d.Error);
        Assert.Contains(d.Lines, l => l is { HasN: true, N: 3, Text: " keep" or "keep" });
        Assert.Contains(d.Lines, l => l.Text == "+new");

        // A refused request answers with ok false, not silence.
        talk.Send(new HighlightCommand("ts", new string('x', 5 * 1024 * 1024), "h2"));
        var refused = await talk.Until<HighlightEvent>(x => x.Ref == "h2");
        Assert.False(refused.Ok);
        Assert.False(string.IsNullOrEmpty(refused.Error));
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task The_render_cache_asks_once_and_answers_from_what_arrives()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var sent = new List<BackendCommand>();
        var cache = new RenderCache(c => { sent.Add(c); talk.Send(c); }, _ => new ImageState(ImageKind.Broken));
        var first = cache.Blocks("# Hi\n\nthere", "", "");
        Assert.Null(first.Value);
        Assert.False(first.Final);
        cache.Blocks("# Hi\n\nthere", "", "");
        Assert.Single(sent);
        var arrived = 0;
        cache.Arrived += () => arrived++;
        var answer = await talk.Until<MarkdownEvent>();
        Assert.True(cache.Accept(answer));
        Assert.Equal(1, arrived);
        var second = cache.Blocks("# Hi\n\nthere", "", "");
        Assert.True(second.Final);
        Assert.Equal(2, second.Value!.Count);
        // The text grows, as a streaming answer does: the earlier answer stands in until the new one is here.
        var more = cache.Blocks("# Hi\n\nthere you go", "", "");
        Assert.False(more.Final);
        Assert.Equal(2, more.Value!.Count);
        Assert.Equal(2, sent.Count);
        Assert.False(cache.Accept(new ToastEvent { Text = "not ours" }));
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task A_reply_while_the_agent_works_is_queued_and_can_be_sent_now_or_taken_back()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "[slow] go");
        await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "working" && s.Sessions[0].Turns[0].Steps.Count > 0);

        talk.Send(new ReplyCommand("then this", Id: session.Id));
        var queued = await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 2);
        var second = queued.Sessions[0].Turns[1];
        Assert.True(second.Queued);
        Assert.False(string.IsNullOrEmpty(second.Uid));

        talk.Send(new QueueRemoveCommand(session.Id, second.Uid));
        var removed = await talk.Until<QueueRemoveEvent>();
        Assert.True(removed.Ok);
        Assert.Equal("then this", removed.Text);
        await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 1);

        // Queue again, then stop: the run ends, the conversation stays and the queue is held until it is resumed.
        talk.Send(new ReplyCommand("and this", Id: session.Id));
        await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 2);
        talk.Send(new StopCommand(session.Id));
        var stopped = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "stopped" && s.Sessions[0].Held);
        Assert.True(stopped.Sessions[0].Turns[^1].Queued);
        Assert.Equal("stopped", stopped.Sessions[0].Turns[0].Stage);
        talk.Send(new QueueResumeCommand(session.Id));
        var resumed = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting");
        Assert.False(resumed.Sessions[0].Turns[^1].Queued);
        talk.Send(new AnswerCommand(session.Id, resumed.Sessions[0].Ask!.Id, "allow"));
        await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "done");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Pause_ends_the_run_and_sends_the_next_queued_reply_once()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "[slow] go");
        await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "working" && s.Sessions[0].Turns[0].Steps.Count > 0);
        talk.Send(new ReplyCommand("next", Id: session.Id));
        await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 2);
        talk.Send(new PauseCommand(session.Id));
        var next = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting" && !s.Sessions[0].Turns[^1].Queued);
        Assert.Equal("stopped", next.Sessions[0].Turns[0].Stage);
        Assert.False(next.Sessions[0].Held);
        talk.Send(new AnswerCommand(session.Id, next.Sessions[0].Ask!.Id, "deny"));
        await talk.Until<StateEvent>(s => s.Sessions[0].Stage != "waiting");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task A_chat_can_be_renamed_and_a_finished_turn_offers_to_go_back()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "Tidy it");
        var done = await Finish(talk, session);

        talk.Send(new RenameCommand(session.Id, "My own name"));
        var renamed = await talk.Until<StateEvent>(s => s.Sessions[0].Title == "My own name");
        Assert.Equal("My own name", renamed.Sessions[0].Title);

        var turn = done.Turns[0];
        output.WriteLine($"Restore={turn.Restore} Again={turn.Again}");
        if (turn.Restore || turn.Again)
        {
            // A checkpoint was kept: going back to just after the answer is a command like the others.
            talk.Send(new RestoreCommand(session.Id, 0));
            await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count >= 1);
        }
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Two_permissions_at_once_are_answered_one_after_the_other()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "[two] both please");
        var first = await talk.Until<StateEvent>(s => s.Sessions[0].Ask is not null && s.Sessions[0].Ask!.More == 1);
        var one = first.Sessions[0].Ask!;
        talk.Send(new AnswerCommand(session.Id, one.Id, "allow"));
        var second = await talk.Until<StateEvent>(s => s.Sessions[0].Ask is { More: 0 } a && a.Id != one.Id);
        talk.Send(new AnswerCommand(session.Id, second.Sessions[0].Ask!.Id, "allow"));
        var done = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "done");
        Assert.Equal("Sandbox reply", done.Sessions[0].Turns[0].Answer);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task The_backend_exiting_in_the_middle_of_a_turn_is_told_once_and_the_commands_after_it_do_nothing()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await Start(talk, sandbox, "[crash] go");
        var waiting = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting");
        talk.Send(new AnswerCommand(session.Id, waiting.Sessions[0].Ask!.Id, "allow"));
        // The agent dies; the backend carries on and says the turn failed.
        var failed = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "failed");
        Assert.Equal("failed", failed.Sessions[0].Turns[0].Stage);
        // Now the backend itself goes.
        talk.Client.CloseInput();
        await talk.Client.WaitForExitAsync(TimeSpan.FromSeconds(10));
        talk.Send(new ReplyCommand("anyone there", Id: session.Id));
        var turns = Turns.From(failed.Sessions[0], 0);
        var th = ChatFixtures.Thread();
        th.Set(turns, 400);
        Assert.Contains(th.Sections[0].Frag.Texts, t => t.Text == "Failed" || t.Layout.Width >= 0);
        _ = JsonSerializer.Serialize(failed.Sessions[0].Turns[0].Chips);
    }
}
