using System.Collections.Concurrent;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Owl;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// OpenCode through Hover's host, against a stand-in "opencode serve": the same
/// routes, Basic auth and event stream the real one has (checked against 1.18.31).
[NonParallelizable]
public sealed class OpenCodeHostTests
{
    private string _dir = "";
    private Fake _fake = null!;

    [SetUp]
    public void Up()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover oc ü " + Guid.NewGuid().ToString("N")[..6]);
        Directory.CreateDirectory(_dir);
        _fake = new Fake();
        OpenCodeHost.SendTimeout = TimeSpan.FromSeconds(1);
        OpenCodeHost.StopGrace = TimeSpan.FromSeconds(2);
        OpenCodeHost.Quiet = TimeSpan.FromSeconds(2);
    }

    [TearDown]
    public void Down()
    {
        _fake.Dispose();
        OpenCodeHost.SendTimeout = TimeSpan.FromSeconds(10);
        OpenCodeHost.StopGrace = TimeSpan.FromSeconds(8);
        OpenCodeHost.Quiet = TimeSpan.FromSeconds(20);
        Directory.Delete(_dir, true);
    }

    private OpenCodeHost Host(AgentOptions? o = null) =>
        new(() => o ?? AgentOptions.Default, _ => Task.FromResult<OpenCodeLink?>(new OpenCodeLink(_fake.Url, Fake.Password, () => { })));

    /// A run that hangs fails the test instead of the whole suite.
    private static Task<KiroResult> Within(Task<KiroResult> run) => run.WaitAsync(TimeSpan.FromSeconds(20));

    private sealed class Events : IProgress<KiroEvent>
    {
        public readonly List<KiroEvent> Seen = new();
        public void Report(KiroEvent value) { lock (Seen) Seen.Add(value); }
    }

    // A turn as the real server sends it: the user message, busy, the assistant
    // message, a text part made of a delta and then the whole part, a write tool,
    // and idle.
    private static void Reply(Fake f, string sid, string mid, string text = "Hello world")
    {
        var am = "msg_zz" + mid[6..];
        f.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
        f.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "busy" } } });
        f.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = am, parentID = mid, role = "assistant", sessionID = sid, providerID = "p", modelID = "a/b", tokens = new { input = 400, output = 100, cache = new { read = 0, write = 0 } } } } });
        f.Push(new { type = "message.part.updated", properties = new { sessionID = sid, part = new { id = "prt_t1", messageID = am, sessionID = sid, type = "tool", tool = "write", callID = "call_1", state = new { status = "completed", input = new { filePath = "hello.txt", content = "hi\n" }, title = "hello.txt" } } } });
        f.Push(new { type = "message.part.updated", properties = new { sessionID = sid, part = new { id = "prt_x1", messageID = am, sessionID = sid, type = "text", text = "" } } });
        f.Push(new { type = "message.part.delta", properties = new { sessionID = sid, messageID = am, partID = "prt_x1", field = "text", delta = text[..5] } });
        f.Push(new { type = "message.part.updated", properties = new { sessionID = sid, part = new { id = "prt_x1", messageID = am, sessionID = sid, type = "text", text } } });
        f.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "idle" } } });
    }

    [Test]
    public async Task A_turn_streams_text_once_steps_and_the_session_id_and_sends_the_model_as_named()
    {
        _fake.OnPrompt = (sid, mid, _) => Reply(_fake, sid, mid);
        var events = new Events();
        var host = Host(new AgentOptions(Model: "p/a/b", Effort: "high"));
        var r = await Within(host.Run(_dir, "Say hello", null, CancellationToken.None, events: events));
        var prompt = _fake.Prompts.Single();
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Completed));
            Assert.That(r.Text, Is.EqualTo("Hello world"), "the delta and the whole part say it once");
            Assert.That(events.Seen.Select(e => e.SessionId).OfType<string>().First(), Is.EqualTo("ses_1"));
            Assert.That(events.Seen.Select(e => e.Step).OfType<KiroStep>().Last(), Has.Property("Kind").EqualTo("edit").And.Property("Added").EqualTo(1));
            Assert.That(events.Seen.Select(e => e.Context).OfType<double>().Last(), Is.EqualTo(50).Within(0.1), "500 of a 1000-token window");
            Assert.That(prompt["model"]!["providerID"]!.GetValue<string>(), Is.EqualTo("p"));
            Assert.That(prompt["model"]!["modelID"]!.GetValue<string>(), Is.EqualTo("a/b"), "a model id with a slash is kept whole");
            Assert.That(prompt["variant"]!.GetValue<string>(), Is.EqualTo("high"));
            Assert.That(prompt["messageID"]!.GetValue<string>(), Does.StartWith("msg_"));
            Assert.That(_fake.Directories.All(d => d == _dir), Is.True, "every call names the folder");
            Assert.That(_fake.Unauthorized, Is.Zero);
        });
    }

    [Test]
    public async Task A_variant_the_model_hasnt_got_and_a_model_it_doesnt_offer_are_never_sent()
    {
        _fake.OnPrompt = (sid, mid, _) => Reply(_fake, sid, mid);
        var r = await Within(Host(new AgentOptions(Model: "p/m", Effort: "high")).Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(r.State, Is.EqualTo(KiroState.Completed));
        Assert.That(_fake.Prompts.Single().ContainsKey("variant"), Is.False);

        var gone = await Within(Host(new AgentOptions(Model: "p/gone")).Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(gone.State, Is.EqualTo(KiroState.Failed));
        Assert.That(gone.Text, Does.Contain("p/gone"));
        Assert.That(_fake.Prompts, Has.Count.EqualTo(1), "nothing sent with a model that isn't there");
    }

    [Test]
    public async Task An_idle_from_before_the_prompt_doesnt_end_the_turn()
    {
        _fake.OnPrompt = (sid, mid, _) =>
        {
            // Left over from an earlier turn: no user message or busy for this one yet.
            _fake.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "idle" } } });
            _fake.Push(new { type = "session.status", properties = new { sessionID = "ses_other", status = new { type = "idle" } } });
            Task.Delay(300).ContinueWith(_ => Reply(_fake, sid, mid, "Real answer"));
        };
        var r = await Within(Host().Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(r.State, Is.EqualTo(KiroState.Completed));
        Assert.That(r.Text, Is.EqualTo("Real answer"));
    }

    [Test]
    public async Task A_denied_command_is_rejected_and_trust_answers_the_same_again_itself()
    {
        var asked = 0;
        _fake.OnPrompt = (sid, mid, _) =>
        {
            _fake.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
            _fake.Push(new { type = "permission.asked", properties = new { id = "per_1", sessionID = sid, permission = "bash", patterns = new[] { "git status" }, metadata = new { command = "git status" }, always = new[] { "git status *" } } });
        };
        _fake.OnReply = (kind, id, _) =>
        {
            if (id == "per_1") _fake.Push(new { type = "permission.asked", properties = new { id = "per_2", sessionID = "ses_1", permission = "bash", patterns = new[] { "git status" }, metadata = new { command = "git status" }, always = new[] { "git status *" } } });
            if (id == "per_2") Reply(_fake, _fake.LastSession!, _fake.LastMessageId!);
        };
        var host = Host(new AgentOptions(Approval: AgentApproval.Always));
        host.Asking = (_, ask, _) => { asked++; Assert.That(ask.Command, Is.EqualTo("git status")); return Task.FromResult(AskAnswer.Trust); };
        var r = await Within(host.Run(_dir, "Go", null, CancellationToken.None));
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Completed));
            Assert.That(asked, Is.EqualTo(1), "the second is Hover's own yes");
            Assert.That(_fake.Replies.Where(x => x.Kind == "permission").Select(x => x.Body!["reply"]!.GetValue<string>()), Is.EqualTo(new[] { "once", "once" }),
                "trust is Hover's; OpenCode's lasting always is never sent");
        });

        _fake.Replies.Clear();
        _fake.OnReply = (_, _, _) => Reply(_fake, _fake.LastSession!, _fake.LastMessageId!);
        _fake.Sessions.Add("ses_2");
        var host2 = Host(new AgentOptions(Approval: AgentApproval.Always));
        host2.Asking = (_, _, _) => Task.FromResult(AskAnswer.Deny);
        await Within(host2.Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(_fake.Replies.Single().Body!["reply"]!.GetValue<string>(), Is.EqualTo("reject"));
    }

    [Test]
    public async Task Read_only_is_the_servers_rule_and_the_agents_own_denies_come_last()
    {
        _fake.OnPrompt = (sid, mid, _) =>
        {
            _fake.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
            _fake.Push(new { type = "permission.asked", properties = new { id = "per_9", sessionID = sid, permission = "edit", patterns = new[] { "x.txt" }, metadata = new { }, always = Array.Empty<string>() } });
        };
        _fake.OnReply = (_, _, _) => Reply(_fake, _fake.LastSession!, _fake.LastMessageId!, "");
        var host = Host(new AgentOptions(ReadOnly: true));
        host.Asking = (_, _, _) => throw new AssertionException("read only never asks");
        var r = await Within(host.Run(_dir, "Go", null, CancellationToken.None));
        var rules = _fake.Created.Single()["permission"]!.AsArray().Select(x => (x!["permission"]!.GetValue<string>(), x["pattern"]!.GetValue<string>(), x["action"]!.GetValue<string>())).ToList();
        Assert.Multiple(() =>
        {
            Assert.That(rules[0], Is.EqualTo(("*", "*", "ask")), "unknown tools ask, and read only turns every ask down");
            Assert.That(rules, Does.Contain(("external_directory", "*", "ask")));
            Assert.That(rules.Where(x => x.Item1 is "bash" or "edit" or "task" or "*").All(x => x.Item3 != "allow"), Is.True, "nothing that changes things is allowed; Hover refuses the asks");
            Assert.That(rules[^1], Is.EqualTo(("bash", "rm *", "deny")), "the agent's deny is the last word");
            Assert.That(_fake.Replies.Single().Body!["reply"]!.GetValue<string>(), Is.EqualTo("reject"));
            Assert.That(r.State, Is.EqualTo(KiroState.Failed));
            Assert.That(r.Text, Does.Contain("read only"));
        });
    }

    [Test]
    public void Full_access_keeps_the_agents_denies_and_opencodes_own_loop_stop()
    {
        var agents = JsonNode.Parse(Fake.Agents)!.AsArray();
        var full = OpenCodeHost.Rules(AgentOptions.Default, agents, "plan").Select(x => (x!["permission"]!.GetValue<string>(), x["action"]!.GetValue<string>())).ToList();
        var ask = OpenCodeHost.Rules(new AgentOptions(Approval: AgentApproval.Risky), agents, "build").Select(x => (x!["permission"]!.GetValue<string>(), x["action"]!.GetValue<string>())).ToList();
        Assert.Multiple(() =>
        {
            Assert.That(full[0], Is.EqualTo(("*", "allow")));
            Assert.That(full, Does.Contain(("doom_loop", "ask")));
            Assert.That(full[^1], Is.EqualTo(("edit", "deny")), "Plan still can't edit under Full");
            Assert.That(ask, Does.Contain(("bash", "ask")));
            Assert.That(ask.Last(x => x.Item1 == "edit"), Is.EqualTo(("edit", "allow")), "Ask first lets edits in the folder go ahead");
            Assert.That(ask.Last(x => x.Item1 == "question"), Is.EqualTo(("question", "allow")), "a deny the agent itself overrules later isn't carried over");
            Assert.That(ask[^1], Is.EqualTo(("bash", "deny")));
        });
    }

    [Test]
    public async Task A_question_gets_the_users_labels_and_a_skipped_one_is_rejected()
    {
        _fake.OnPrompt = (sid, mid, _) =>
        {
            _fake.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
            _fake.Push(new
            {
                type = "question.asked",
                properties = new { id = "que_1", sessionID = sid, questions = new[] { new { question = "Tabs or spaces?", header = "Indent", options = new[] { new { label = "Tabs", description = "" }, new { label = "Spaces", description = "" } } } } },
            });
        };
        _fake.OnReply = (_, _, _) => Reply(_fake, _fake.LastSession!, _fake.LastMessageId!);
        var host = Host();
        AgentAsk? seen = null;
        host.Questioning = (_, ask, _) => { seen = ask; return Task.FromResult<IReadOnlyList<IReadOnlyList<string>>?>(new[] { new[] { "Tabs" } }); };
        host.Asking = (_, _, _) => throw new AssertionException("a question isn't an approval");
        var r = await Within(host.Run(_dir, "Go", null, CancellationToken.None));
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Completed));
            Assert.That(seen!.Questions![0].Options.Select(o => o.Label), Is.EqualTo(new[] { "Tabs", "Spaces" }));
            Assert.That(_fake.Replies.Single().Kind, Is.EqualTo("question"));
            Assert.That(_fake.Replies.Single().Body!["answers"]!.ToJsonString(), Is.EqualTo("[[\"Tabs\"]]"));
        });

        _fake.Replies.Clear();
        _fake.Sessions.Add("ses_2");
        var host2 = Host();
        host2.Questioning = (_, _, _) => Task.FromResult<IReadOnlyList<IReadOnlyList<string>>?>(null);
        await Within(host2.Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(_fake.Replies.Single().Kind, Is.EqualTo("reject"));
    }

    [Test]
    public async Task Stop_while_a_question_waits_withdraws_it_and_aborts_only_that_session()
    {
        _fake.OnPrompt = (sid, mid, _) =>
        {
            _fake.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
            _fake.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "busy" } } });
            _fake.Push(new { type = "question.asked", properties = new { id = "que_2", sessionID = sid, questions = new[] { new { question = "?", header = "H", options = new[] { new { label = "A", description = "" } } } } } });
        };
        _fake.OnAbort = sid =>
        {
            _fake.Push(new { type = "session.error", properties = new { sessionID = sid, error = new { name = "MessageAbortedError", data = new { message = "aborted" } } } });
            _fake.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "idle" } } });
        };
        using var cts = new CancellationTokenSource();
        var host = Host();
        host.Questioning = (_, _, ct) => { cts.CancelAfter(200); return Task.Delay(Timeout.Infinite, ct).ContinueWith(_ => (IReadOnlyList<IReadOnlyList<string>>?)null); };
        var r = await Within(host.Run(_dir, "Go", null, cts.Token));
        await Task.Delay(300);
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Cancelled));
            Assert.That(_fake.Aborts, Is.EqualTo(new[] { "ses_1" }));
            Assert.That(_fake.Replies.Single().Kind, Is.EqualTo("reject"), "the withdrawn question is told so");
        });
    }

    [Test]
    public async Task A_lost_prompt_answer_is_looked_up_not_sent_twice()
    {
        // The server takes the prompt and runs it, but its answer never comes back.
        _fake.PromptHangs = true;
        _fake.OnPrompt = (sid, mid, _) => { _fake.Messages.Add(mid); Reply(_fake, sid, mid, "Did it"); };
        var r = await Within(Host().Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(r.State, Is.EqualTo(KiroState.Completed));
        Assert.That(r.Text, Is.EqualTo("Did it"));
        Assert.That(_fake.Prompts, Has.Count.EqualTo(1));

        // Not taken at all: a clear failure, still sent once.
        _fake.Prompts.Clear();
        _fake.OnPrompt = (_, _, _) => { };
        var lost = await Within(Host().Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(lost.State, Is.EqualTo(KiroState.Failed));
        Assert.That(lost.Text, Does.Contain("wasn’t sent again"));
        Assert.That(_fake.Prompts, Has.Count.EqualTo(1));
    }

    [Test]
    public async Task A_dropped_event_stream_reconnects_and_reads_the_finished_turn_back()
    {
        _fake.OnPrompt = (sid, mid, _) =>
        {
            _fake.Push(new { type = "message.updated", properties = new { sessionID = sid, info = new { id = mid, role = "user", sessionID = sid } } });
            _fake.Push(new { type = "session.status", properties = new { sessionID = sid, status = new { type = "busy" } } });
            // The stream drops; the turn finishes while nobody listens.
            _fake.Messages.Add(mid);
            _fake.History = new JsonArray(
                JsonNode.Parse($$"""{"info":{"id":"{{mid}}","role":"user","sessionID":"{{sid}}"},"parts":[]}"""),
                JsonNode.Parse($$"""{"info":{"id":"msg_zzz","parentID":"{{mid}}","role":"assistant","sessionID":"{{sid}}"},"parts":[{"id":"prt_1","messageID":"msg_zzz","sessionID":"{{sid}}","type":"text","text":"Finished while away"}]}"""));
            _fake.Status[sid] = "idle";
            _fake.DropStreams();
        };
        var r = await Within(Host().Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(r.State, Is.EqualTo(KiroState.Completed));
        Assert.That(r.Text, Is.EqualTo("Finished while away"));
        Assert.That(_fake.Connects, Is.GreaterThanOrEqualTo(2));
        Assert.That(_fake.Prompts, Has.Count.EqualTo(1));
    }

    [Test]
    public async Task A_conversation_opencode_lost_fails_and_isnt_quietly_replaced()
    {
        var r = await Within(Host().Run(_dir, "Go on", null, CancellationToken.None, resume: "ses_gone"));
        Assert.That(r.State, Is.EqualTo(KiroState.Failed));
        Assert.That(r.Text, Does.Contain("no longer has this conversation"));
        Assert.That(_fake.Created, Is.Empty, "no new conversation in its place");
        Assert.That(_fake.Prompts, Is.Empty);
    }

    [Test]
    public async Task A_reply_resumes_the_same_session_and_sets_its_rules_again()
    {
        _fake.Sessions.Add("ses_old");
        _fake.OnPrompt = (sid, mid, _) => Reply(_fake, sid, mid);
        var r = await Within(Host().Run(_dir, "Go on", null, CancellationToken.None, resume: "ses_old"));
        Assert.That(r.State, Is.EqualTo(KiroState.Completed));
        Assert.That(_fake.Created, Is.Empty);
        Assert.That(_fake.Patched, Is.EqualTo(new[] { "ses_old" }));
        Assert.That(_fake.PromptSessions, Is.EqualTo(new[] { "ses_old" }));
    }

    [Test]
    public async Task A_server_too_old_or_one_that_doesnt_start_is_a_readable_failure()
    {
        _fake.Version = "1.2.0";
        var old = await Within(Host().Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(old.State, Is.EqualTo(KiroState.Failed));
        Assert.That(old.Text, Does.Contain("too old"));

        var none = await Within(new OpenCodeHost(() => AgentOptions.Default, _ => Task.FromResult<OpenCodeLink?>(null)).Run(_dir, "Go", null, CancellationToken.None));
        Assert.That(none.State, Is.EqualTo(KiroState.Failed));
        Assert.That(none.Text, Does.Contain("isn’t installed"));
    }

    [Test]
    public void Offers_keep_each_models_own_variants_and_leave_hidden_agents_out()
    {
        var offers = OpenCodeHost.Offers(JsonNode.Parse(Fake.Providers)!.AsObject(), JsonNode.Parse(Fake.Agents)!.AsArray());
        var models = offers.Single(o => o.Category == "model").Choices;
        Assert.Multiple(() =>
        {
            Assert.That(models.Select(m => m.Value), Is.EqualTo(new[] { "p/a/b", "p/m" }));
            Assert.That(models[0].Levels, Is.EqualTo(new[] { "low", "high" }));
            Assert.That(models[1].Levels, Is.Empty);
            Assert.That(offers.Single(o => o.Category == "mode").Choices.Select(c => c.Value), Is.EqualTo(new[] { "build", "plan" }));
        });
    }

    [Test]
    public void OpenCode_never_falls_through_to_cursors_program_or_arguments()
    {
        Assert.That(Agents.Arguments(AgentTool.OpenCode), Is.EqualTo(new[] { "serve", "--hostname=127.0.0.1", "--port=0", "--mdns=false" }));
        Assert.That(Agents.InstallHint(AgentTool.OpenCode), Does.Contain("OpenCode"));
        Assert.That(Agents.SignInHint(AgentTool.OpenCode), Does.Not.Contain("cursor"));
        Assert.That(Agents.Exe(AgentTool.OpenCode) ?? "", Does.Not.Contain("cursor-agent"));
        Assert.That(Agents.Parse("opencode"), Is.EqualTo(AgentTool.OpenCode));
        Assert.That(Agents.Parse("cursor"), Is.EqualTo(AgentTool.Cursor), "old ids read as before");
    }

    [Test]
    public void Message_ids_are_opencodes_shape_and_keep_rising()
    {
        var ids = Enumerable.Range(0, 50).Select(_ => OpenCodeHost.NewMessageId()).ToList();
        Assert.That(ids.All(i => System.Text.RegularExpressions.Regex.IsMatch(i, "^msg_[0-9a-f]{12}[0-9A-Za-z]{14}$")), Is.True);
        Assert.That(ids.Select(i => i[..16]), Is.Ordered.Using((IComparer<string>)StringComparer.Ordinal));
    }

    [Test]
    public void A_permission_request_reads_as_the_notch_shows_it()
    {
        var bash = OpenCodeHost.Describe(JsonNode.Parse("""{"id":"per_1","permission":"bash","patterns":["rm -rf build"],"metadata":{"command":"rm -rf build"},"always":[]}""")!.AsObject(), _dir);
        var outside = OpenCodeHost.Describe(JsonNode.Parse("""{"id":"per_2","permission":"external_directory","patterns":["C:\\Windows\\*"],"metadata":{},"always":[]}""")!.AsObject(), _dir);
        Assert.Multiple(() =>
        {
            Assert.That(bash.Kind, Is.EqualTo("execute"));
            Assert.That(bash.Command, Is.EqualTo("rm -rf build"));
            Assert.That(bash.Danger, Is.True);
            Assert.That(outside.Reason, Is.EqualTo("Reaches outside the folder"));
        });
    }

    // MARK: Sessions and history

    private static AgentAsk Q(string id) => new(id, "question", "Indent", null, null, null, 0, 0, "Tabs or spaces?", false,
        new[] { new AgentQuestion("Indent", "Tabs or spaces?", new[] { ("Tabs", ""), ("Spaces", "") }, false, true) });

    [Test]
    public async Task A_session_holds_a_question_until_its_answered_and_a_stop_skips_it()
    {
        var asked = new TaskCompletionSource<Task<IReadOnlyList<IReadOnlyList<string>>?>>();
        KiroSession s = null!;
        s = new KiroSession(async (_, _, _, ct, _, _) =>
        {
            var a = s.AskQuestion(Q("que_1"), ct);
            asked.TrySetResult(a);
            var got = await a;
            return new KiroResult(KiroState.Completed, got is null ? "skipped" : got[0][0]);
        });
        s.Start(_dir, "go");
        var pending = await asked.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Multiple(() =>
        {
            Assert.That(s.Asking!.IsQuestion, Is.True);
            Assert.That(s.AnswerQuestion("que_1", new[] { Array.Empty<string>() }), Is.False, "an empty answer isn't one");
            Assert.That(s.AnswerQuestion("que_other", new[] { new[] { "Tabs" } }), Is.False, "a stale id is turned down");
            Assert.That(s.AnswerQuestion("que_1", new[] { new[] { "Tabs" } }), Is.True);
            Assert.That(s.AnswerQuestion("que_1", new[] { new[] { "Tabs" } }), Is.False, "answered once only");
        });
        Assert.That((await pending)![0][0], Is.EqualTo("Tabs"));

        asked = new TaskCompletionSource<Task<IReadOnlyList<IReadOnlyList<string>>?>>();
        await Task.Delay(100);
        s.Reply("again");
        pending = await asked.Task.WaitAsync(TimeSpan.FromSeconds(5));
        s.Stop();
        Assert.That(await pending.WaitAsync(TimeSpan.FromSeconds(5)), Is.Null, "a stop skips the question, it doesn't answer it");
        Assert.That(s.Waiting, Is.False);
    }

    [Test]
    public void History_keeps_opencodes_session_and_old_tools_read_as_before()
    {
        var dir = Path.Combine(_dir, "history");
        var h = new AgentHistory(dir);
        h.Save(new SavedSession("abc123", AgentTool.OpenCode, _dir, "Task", "ses_keep", 12, Array.Empty<SavedTurn>(), DateTime.Now));
        h.Save(new SavedSession("def456", AgentTool.Cursor, _dir, "Old", "acp-1", null, Array.Empty<SavedTurn>(), DateTime.Now));
        h.Flush();
        var again = new AgentHistory(dir);
        Assert.Multiple(() =>
        {
            Assert.That(again.Load("abc123")!.Tool, Is.EqualTo(AgentTool.OpenCode));
            Assert.That(again.Load("abc123")!.AcpId, Is.EqualTo("ses_keep"), "the conversation it resumes is OpenCode's own id");
            Assert.That(again.Load("def456")!.Tool, Is.EqualTo(AgentTool.Cursor));
            Assert.That(again.Entries.Select(e => e.Tool), Is.EquivalentTo(new[] { AgentTool.OpenCode, AgentTool.Cursor }));
        });
    }

    // MARK: The stand-in server

    private sealed class Fake : IDisposable
    {
        public const string Password = "pw-for-tests";
        public const string Providers = """
            {"providers":[{"id":"p","name":"Prov","models":{"a/b":{"id":"a/b","name":"A B","variants":{"low":{},"high":{}},"limit":{"context":1000}},"m":{"id":"m","name":"M","limit":{"context":1000}}}}],"default":{"p":"m"}}
            """;
        public const string Agents = """
            [{"name":"build","mode":"primary","permission":[{"permission":"*","pattern":"*","action":"allow"},{"permission":"question","pattern":"*","action":"deny"},{"permission":"question","pattern":"*","action":"allow"},{"permission":"bash","pattern":"rm *","action":"deny"}]},
             {"name":"plan","mode":"primary","permission":[{"permission":"edit","pattern":"*","action":"deny"}]},
             {"name":"title","mode":"primary","hidden":true,"permission":[]},
             {"name":"explore","mode":"subagent","permission":[]}]
            """;

        private readonly HttpListener _listener = new();
        private readonly List<StreamWriter> _streams = new();
        private readonly CancellationTokenSource _stop = new();
        public Uri Url { get; }
        public string Version = "1.18.31";
        public int Unauthorized, Connects;
        public bool PromptHangs;
        /// Each session's status, as the last session.status pushed said.
        public readonly ConcurrentDictionary<string, string> Status = new();
        public string? LastMessageId, LastSession;
        public readonly ConcurrentBag<string> Directories = new();
        public readonly List<JsonObject> Prompts = new(), Created = new();
        public readonly List<string> PromptSessions = new(), Patched = new(), Aborts = new(), Messages = new();
        public readonly HashSet<string> Sessions = new();
        public readonly List<(string Kind, string Id, JsonObject? Body)> Replies = new();
        public JsonArray History = new();
        public Action<string, string, JsonObject>? OnPrompt;
        public Action<string, string, JsonObject?>? OnReply;
        public Action<string>? OnAbort;

        public Fake()
        {
            var l = new TcpListener(IPAddress.Loopback, 0);
            l.Start();
            var port = ((IPEndPoint)l.LocalEndpoint).Port;
            l.Stop();
            Url = new Uri($"http://localhost:{port}/");
            _listener.Prefixes.Add(Url.ToString());
            _listener.Start();
            _ = Task.Run(Serve);
        }

        public void Push(object e)
        {
            var line = "data: " + JsonSerializer.Serialize(e) + "\n\n";
            lock (_streams)
                foreach (var w in _streams.ToList())
                    try { w.Write(line); w.Flush(); } catch { _streams.Remove(w); }
        }

        public void DropStreams()
        {
            lock (_streams) { foreach (var w in _streams) try { w.BaseStream.Close(); } catch { } _streams.Clear(); }
        }

        private async Task Serve()
        {
            while (!_stop.IsCancellationRequested)
            {
                HttpListenerContext ctx;
                try { ctx = await _listener.GetContextAsync(); } catch { return; }
                _ = Task.Run(() => Answer(ctx));
            }
        }

        private void Answer(HttpListenerContext ctx)
        {
            var req = ctx.Request;
            var res = ctx.Response;
            try
            {
                if (req.Headers["Authorization"] != "Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes("opencode:" + Password)))
                {
                    Unauthorized++;
                    Send(res, 401, "{}");
                    return;
                }
                if (req.QueryString["directory"] is { } dir) Directories.Add(dir);
                var path = req.Url!.AbsolutePath;
                var body = req.HasEntityBody ? JsonNode.Parse(new StreamReader(req.InputStream).ReadToEnd()) as JsonObject : null;
                var parts = path.Trim('/').Split('/').Select(Uri.UnescapeDataString).ToArray();
                switch (req.HttpMethod, parts)
                {
                    case ("GET", ["global", "health"]): Send(res, 200, $$"""{"healthy":true,"version":"{{Version}}"}"""); return;
                    case ("GET", ["config", "providers"]): Send(res, 200, Providers); return;
                    case ("GET", ["config"]): Send(res, 200, "{}"); return;
                    case ("GET", ["agent"]): Send(res, 200, Agents); return;
                    case ("GET", ["permission"]): Send(res, 200, "[]"); return;
                    case ("GET", ["question"]): Send(res, 200, "[]"); return;
                    case ("GET", ["event"]):
                        Connects++;
                        res.StatusCode = 200;
                        res.ContentType = "text/event-stream";
                        res.SendChunked = true;
                        var w = new StreamWriter(res.OutputStream, new UTF8Encoding(false));
                        w.Write("data: {\"type\":\"server.connected\",\"properties\":{}}\n\n");
                        w.Flush();
                        lock (_streams) _streams.Add(w);
                        return;
                    case ("POST", ["session"]):
                        lock (Created) Created.Add(body!);
                        var sid = Sessions.Contains("ses_2") && Created.Count > 1 ? "ses_2" : "ses_1";
                        Sessions.Add(sid);
                        Send(res, 200, $$"""{"id":"{{sid}}","directory":"x"}""");
                        return;
                    case ("GET", ["session", "status"]):
                        Send(res, 200, new JsonObject(Status.Where(s => s.Value != "idle").Select(s => KeyValuePair.Create(s.Key, (JsonNode?)new JsonObject { ["type"] = s.Value }))).ToJsonString());
                        return;
                    case ("GET", ["session", var id]):
                        if (Sessions.Contains(id)) Send(res, 200, $$"""{"id":"{{id}}"}""");
                        else Send(res, 404, """{"name":"NotFoundError","data":{"message":"Session not found"}}""");
                        return;
                    case ("PATCH", ["session", var id]):
                        lock (Patched) Patched.Add(id);
                        Send(res, 200, $$"""{"id":"{{id}}"}""");
                        return;
                    case ("GET", ["session", _, "message", var mid]):
                        if (Messages.Contains(mid)) Send(res, 200, $$"""{"info":{"id":"{{mid}}","role":"user"},"parts":[]}""");
                        else Send(res, 404, """{"name":"NotFoundError","data":{"message":"no"}}""");
                        return;
                    case ("GET", ["session", _, "message"]): Send(res, 200, History.ToJsonString()); return;
                    case ("POST", ["session", var id, "prompt_async"]):
                        lock (Prompts) { Prompts.Add(body!); PromptSessions.Add(id); }
                        LastMessageId = body!["messageID"]!.GetValue<string>();
                        LastSession = id;
                        OnPrompt?.Invoke(id, LastMessageId, body);
                        if (PromptHangs) { Thread.Sleep(2500); try { res.Abort(); } catch { } return; }
                        Send(res, 204, "");
                        return;
                    case ("POST", ["session", var id, "abort"]):
                        lock (Aborts) Aborts.Add(id);
                        Send(res, 200, "true");
                        OnAbort?.Invoke(id);
                        return;
                    case ("POST", ["permission", var id, "reply"]):
                        lock (Replies) Replies.Add(("permission", id, body));
                        Send(res, 200, "true");
                        OnReply?.Invoke("permission", id, body);
                        return;
                    case ("POST", ["question", var id, "reply"]):
                        lock (Replies) Replies.Add(("question", id, body));
                        Send(res, 200, "true");
                        OnReply?.Invoke("question", id, body);
                        return;
                    case ("POST", ["question", var id, "reject"]):
                        lock (Replies) Replies.Add(("reject", id, body));
                        Send(res, 200, "true");
                        OnReply?.Invoke("reject", id, body);
                        return;
                }
                Send(res, 404, """{"name":"NotFoundError","data":{"message":"no route"}}""");
            }
            catch (Exception) { try { res.Abort(); } catch { } }
        }

        private static void Send(HttpListenerResponse res, int status, string body)
        {
            res.StatusCode = status;
            if (status != 204)
            {
                var bytes = Encoding.UTF8.GetBytes(body);
                res.ContentType = "application/json";
                res.OutputStream.Write(bytes);
            }
            res.Close();
        }

        public void Dispose()
        {
            _stop.Cancel();
            DropStreams();
            try { _listener.Stop(); _listener.Close(); } catch { }
        }
    }
}
