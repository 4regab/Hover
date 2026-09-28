using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using Hover.Owl;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// The stream-json reader, on lines shaped as kiro-cli's ACP events are.
public sealed class KiroStreamTests
{
    private static KiroStream Fed(params string[] lines)
    {
        var s = new KiroStream();
        foreach (var l in lines) s.Feed(l);
        return s;
    }

    private const string Started = "{\"type\":\"runStarted\",\"data\":{\"acpProtocolVersion\":1}}";
    private static string Update(string body) => "{\"type\":\"sessionUpdate\",\"data\":{\"update\":{" + body + "}}}";
    private static string Chunk(string text) => Update("\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"" + text + "\"}");
    private static string Tool(string kind, string title = "") => Update("\"sessionUpdate\":\"tool_call\",\"kind\":\"" + kind + "\",\"title\":\"" + title + "\"");

    [Test]
    public void Tool_calls_become_broad_phases_and_repeats_are_not_reported_twice()
    {
        var s = new KiroStream();
        Assert.Multiple(() =>
        {
            Assert.That(s.Feed(Started), Is.Null);
            Assert.That(s.Feed(Update("\"sessionUpdate\":\"agent_thought_chunk\"")), Is.EqualTo(KiroPhase.Thinking));
            Assert.That(s.Feed(Tool("read", "Reading Program.cs")), Is.EqualTo(KiroPhase.Reading));
            Assert.That(s.Feed(Tool("read", "Reading App.cs")), Is.Null, "same phase, nothing new to show");
            Assert.That(s.Feed(Tool("edit")), Is.EqualTo(KiroPhase.Editing));
            Assert.That(s.Feed(Tool("execute")), Is.EqualTo(KiroPhase.Running));
            Assert.That(s.Feed(Tool("search")), Is.EqualTo(KiroPhase.Searching));
            Assert.That(s.Feed(Update("\"sessionUpdate\":\"plan\",\"entries\":[]")), Is.EqualTo(KiroPhase.Planning));
            Assert.That(s.Feed(Chunk("Hi")), Is.EqualTo(KiroPhase.Writing));
        });
    }

    [Test]
    public void Steps_context_and_the_session_id_come_out_of_real_shaped_events()
    {
        var s = new KiroStream();
        s.Feed("{\"type\":\"sessionUpdate\",\"data\":{\"sessionId\":\"sess_1\",\"update\":{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"t1\",\"title\":\"Read File\",\"kind\":\"read\",\"locations\":[{\"path\":\"C:\\\\p\\\\note.txt\"}]}}}");
        s.Feed("{\"type\":\"sessionUpdate\",\"data\":{\"sessionId\":\"sess_1\",\"update\":{\"sessionUpdate\":\"session_info_update\",\"_meta\":{\"kiro\":{\"contextUsage\":{\"usagePercentage\":3.37}}}}}}");
        s.Feed("{\"type\":\"sessionUpdate\",\"data\":{\"sessionId\":\"sess_1\",\"update\":{\"sessionUpdate\":\"tool_call_update\",\"toolCallId\":\"t1\",\"status\":\"completed\"}}}");
        var e = s.Drain();
        Assert.Multiple(() =>
        {
            Assert.That(s.SessionId, Is.EqualTo("sess_1"));
            Assert.That(s.Context, Is.EqualTo(3.37).Within(0.001));
            Assert.That(e.Where(x => x.Step is not null).Select(x => x.Step!.Status), Is.EqualTo(new[] { "in_progress", "completed" }));
            Assert.That(e.First(x => x.Step is not null).Step, Is.EqualTo(new KiroStep("t1", "read", "Read File", "C:\\p\\note.txt", "in_progress")));
            Assert.That(s.Drain(), Is.Empty, "drained once");
        });
    }

    [TestCase(null, "fs_read", KiroPhase.Reading)]
    [TestCase("other", "fs_write", KiroPhase.Editing)]
    [TestCase("other", "execute_bash", KiroPhase.Running)]
    [TestCase("other", "grep", KiroPhase.Searching)]
    [TestCase("other", "something new", KiroPhase.Working)]
    [TestCase(null, null, null)]
    public void A_tool_without_a_known_kind_is_placed_by_its_title(string? kind, string? title, KiroPhase? expected) =>
        Assert.That(KiroStream.ToolPhase(kind, title), Is.EqualTo(expected));

    [Test]
    public void The_final_text_wins_over_the_chunks_and_noise_is_skipped()
    {
        var s = Fed(Started, "\u001b[?25l▰▱▱ Loading", "not json {", Chunk("Fixed "), Chunk("the tests."),
            "{\"type\":\"runFinished\",\"data\":{\"stopReason\":\"end_turn\",\"finalText\":\"Fixed the failing tests.\",\"finalTextTruncated\":false}}");
        Assert.Multiple(() =>
        {
            Assert.That(s.Said, Is.EqualTo("Fixed the tests."));
            Assert.That(s.Finished, Is.True);
            Assert.That(s.StopReason, Is.EqualTo("end_turn"));
            Assert.That(s.Outcome(0, false, ""), Is.EqualTo(new KiroResult(KiroState.Completed, "Fixed the failing tests.", 0)));
        });
    }

    [Test]
    public void Chunks_stand_in_when_there_is_no_final_text() =>
        Assert.That(Fed(Chunk("All "), Chunk("done")).Outcome(0, false, "").Text, Is.EqualTo("All done"));

    [Test]
    public void An_event_keyed_by_its_name_is_read_too()
    {
        var s = Fed("{\"runFinished\":{\"finalText\":\"ok\"}}");
        Assert.That(s.Finished && s.FinalText == "ok", Is.True);
    }

    [Test]
    public void A_run_error_fails_even_with_a_zero_exit()
    {
        var r = Fed("{\"type\":\"runError\",\"data\":{\"error\":{\"message\":\"Model unavailable\"}}}").Outcome(0, false, "");
        Assert.That(r, Is.EqualTo(new KiroResult(KiroState.Failed, "Model unavailable", 0)));
    }

    [Test]
    public void An_interruption_record_or_a_stop_is_cancelled_not_failed()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Fed("{\"type\":\"runInterrupted\",\"data\":{}}").Outcome(1, false, "").State, Is.EqualTo(KiroState.Cancelled));
            Assert.That(Fed(Chunk("Half")).Outcome(-1, true, "").State, Is.EqualTo(KiroState.Cancelled));
            Assert.That(Fed().Outcome(-1, true, "").Text, Is.EqualTo("Stopped before Kiro finished."));
        });
    }

    [Test]
    public void Failures_read_as_what_to_do_next()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Fed().Outcome(1, false, "Failed to open browser for authentication.\nPlease try again with: kiro-cli login --use-device-flow").Text,
                Does.StartWith("Kiro needs you to sign in"));
            Assert.That(Fed().Outcome(3, false, "").Text, Does.Contain("MCP server"));
            Assert.That(Fed().Outcome(2, false, "\u001b[31merror:\u001b[0m unexpected argument '--bogus'\n").Text, Is.EqualTo("error: unexpected argument '--bogus'"));
            Assert.That(Fed().Outcome(7, false, "").Text, Is.EqualTo("kiro-cli stopped with exit code 7."));
            Assert.That(Fed("{\"type\":\"runFinished\",\"data\":{\"stopReason\":\"refusal\"}}").Outcome(0, false, "").State, Is.EqualTo(KiroState.Failed));
        });
    }
    [Test]
    public void The_answer_is_the_last_message_by_id_or_after_a_tool_call()
    {
        static string U(string u) => "{\"method\":\"session/update\",\"params\":{\"sessionId\":\"s\",\"update\":" + u + "}}";
        var byId = Fed(U("{\"sessionUpdate\":\"agent_message_chunk\",\"messageId\":\"m1\",\"content\":{\"type\":\"text\",\"text\":\"Warning: skills trimmed.\"}}"),
            U("{\"sessionUpdate\":\"agent_message_chunk\",\"messageId\":\"m2\",\"content\":{\"type\":\"text\",\"text\":\"hel\"}}"),
            U("{\"sessionUpdate\":\"agent_message_chunk\",\"messageId\":\"m2\",\"content\":{\"type\":\"text\",\"text\":\"lo\"}}"));
        var byTool = Fed(U("{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Let me look.\"}}"),
            U("{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"t\",\"kind\":\"read\"}"),
            U("{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Done.\"}}"));
        var byPhase = Fed(U("{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"Warning: skills trimmed.\"}}"),
            U("{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"hello\"},\"_meta\":{\"codex\":{\"phase\":\"final_answer\"}}}"));
        Assert.That((byId.Said, byTool.Said, byPhase.Said), Is.EqualTo(("hello", "Done.", "hello")));
    }
}

/// AcpHost against a stand-in agent that speaks ACP over in-memory pipes: it answers
/// initialize, session/new, load, set_config_option and prompt, and records what it got.
[NonParallelizable]
public sealed class AcpHostTests
{
    private string _dir = "";

    [SetUp]
    public void Folder()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-acp-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_dir);
    }

    [TearDown]
    public void Clean() => Directory.Delete(_dir, true);

    private sealed class Phases : IProgress<KiroPhase>
    {
        public readonly List<KiroPhase> Seen = new();
        public void Report(KiroPhase value) { lock (Seen) Seen.Add(value); }
    }

    private sealed class Events : IProgress<KiroEvent>
    {
        public readonly List<KiroEvent> Seen = new();
        public void Report(KiroEvent value) { lock (Seen) Seen.Add(value); }
    }

    /// The stand-in agent. Every message it gets is kept, by method.
    private sealed class Fake
    {
        public readonly List<(string Method, JsonElement Params)> Got = new();
        public int Starts;
        public bool HangPrompt, AskToEdit;
        public string? PermissionAnswer;
        /// The agent dies: its output closes, as when the process exits.
        public Action? Crash;
        private StreamWriter? _out;
        private long? _hanging;
        private string _model = "m1";
        private readonly SemaphoreSlim _w = new(1, 1);

        public AcpLink Connect()
        {
            Starts++;
            var toAgent = new AnonymousPipeServerStream(PipeDirection.Out);
            var agentIn = new AnonymousPipeClientStream(PipeDirection.In, toAgent.ClientSafePipeHandle);
            var fromAgent = new AnonymousPipeServerStream(PipeDirection.In);
            var agentOut = new AnonymousPipeClientStream(PipeDirection.Out, fromAgent.ClientSafePipeHandle);
            var w = new StreamWriter(agentOut, new UTF8Encoding(false)) { AutoFlush = true };
            _out = w;
            Crash = () => agentOut.Dispose();
            _ = Task.Run(() => Serve(new StreamReader(agentIn), w));
            return new AcpLink(toAgent, fromAgent, () => { agentOut.Dispose(); toAgent.Dispose(); });
        }

        private async Task Say(StreamWriter w, object m)
        {
            await _w.WaitAsync();
            try { await w.WriteLineAsync(JsonSerializer.Serialize(m)); } catch (Exception) { } finally { _w.Release(); }
        }

        private Task Update(StreamWriter w, string sid, object update) =>
            Say(w, new { jsonrpc = "2.0", method = "session/update", @params = new { sessionId = sid, update } });

        private static readonly object[] Models = { new { value = "m1", name = "Model one" }, new { value = "m2", name = "Model two" } };

        private async Task Serve(StreamReader r, StreamWriter w)
        {
            try
            {
                while (await r.ReadLineAsync() is { } line)
                {
                    using var doc = JsonDocument.Parse(line);
                    var m = doc.RootElement;
                    var method = m.TryGetProperty("method", out var mm) ? mm.GetString() : null;
                    var p = m.TryGetProperty("params", out var pp) ? pp.Clone() : default;
                    if (method is null)
                    {
                        // The answer to a permission request.
                        PermissionAnswer = m.GetProperty("result").GetProperty("outcome").GetProperty("optionId").GetString();
                        if (_hanging is { } h) await Say(w, new { jsonrpc = "2.0", id = h, result = new { stopReason = "cancelled" } });
                        continue;
                    }
                    lock (Got) Got.Add((method, p));
                    var id = m.TryGetProperty("id", out var idv) ? idv.GetInt64() : (long?)null;
                    object? result = null;
                    switch (method)
                    {
                        case "initialize": result = new { protocolVersion = 1, agentCapabilities = new { loadSession = true } }; break;
                        case "session/new":
                            result = new { sessionId = "s1", configOptions = new object[] { new { id = "model", category = "model", currentValue = "m1", options = Models } } };
                            break;
                        case "session/load":
                            // It replays the conversation before it answers.
                            await Update(w, "s1", new { sessionUpdate = "tool_call", toolCallId = "old", kind = "read", title = "Read", status = "completed" });
                            result = new { configOptions = new object[] { new { id = "model", category = "model", currentValue = "m1", options = Models } } };
                            break;
                        case "session/set_config_option":
                            if (p.GetProperty("configId").GetString() == "model") _model = p.GetProperty("value").GetString()!;
                            result = new
                            {
                                configOptions = new object[]
                                {
                                    new { id = "model", category = "model", currentValue = _model, options = Models },
                                    new { id = "effortLevel", category = "thought_level", currentValue = "medium", options = new object[] { new { value = "medium", name = "Medium" }, new { value = "high", name = "High" } } },
                                },
                            };
                            break;
                        case "session/cancel":
                            if (_hanging is { } h) await Say(w, new { jsonrpc = "2.0", id = h, result = new { stopReason = "cancelled" } });
                            break;
                        case "session/prompt":
                            var sid = p.GetProperty("sessionId").GetString()!;
                            if (AskToEdit)
                            {
                                _hanging = id;
                                await Say(w, new
                                {
                                    jsonrpc = "2.0", id = 900, method = "session/request_permission",
                                    @params = new
                                    {
                                        sessionId = sid, toolCall = new { toolCallId = "e", kind = "edit", title = "Write" },
                                        options = new object[] { new { optionId = "yes", name = "Accept", kind = "allow_once" }, new { optionId = "no", name = "Reject", kind = "reject_once" } },
                                    },
                                });
                                continue;
                            }
                            await Update(w, sid, new { sessionUpdate = "agent_message_chunk", content = new { type = "text", text = "Let me look." } });
                            await Update(w, sid, new { sessionUpdate = "tool_call", toolCallId = "t1", kind = "read", title = "Read", status = "in_progress", locations = new[] { new { path = "a.cs" } } });
                            await Update(w, sid, new { sessionUpdate = "tool_call", toolCallId = "t2", kind = "edit", title = "Edit", status = "completed" });
                            await Update(w, sid, new { sessionUpdate = "agent_message_chunk", content = new { type = "text", text = "Renamed it." } });
                            if (HangPrompt) { _hanging = id; continue; }
                            result = new { stopReason = "end_turn" };
                            break;
                    }
                    if (id is { } i && result is not null) await Say(w, new { jsonrpc = "2.0", id = i, result });
                }
            }
            catch (Exception) { /* the host went */ }
        }

        public List<string> Methods() { lock (Got) return Got.Select(g => g.Method).ToList(); }
    }

    private (AcpHost Host, Fake Fake) Make(AgentOptions? o = null)
    {
        var fake = new Fake();
        return (new AcpHost(AgentTool.Kiro, () => o ?? AgentOptions.Default, fake.Connect), fake);
    }

    [Test]
    public async Task A_turn_goes_over_the_pipe_and_a_reply_carries_on_in_the_same_session()
    {
        var (host, fake) = Make();
        var phases = new Phases();
        var events = new Events();
        var prompt = "Fix the \"failing\" tests & don't touch %PATH% ünïcode";
        var r = await host.Run(_dir, prompt, phases, CancellationToken.None, null, events);
        var again = await host.Run(_dir, "and the docs", null, CancellationToken.None, "s1", null);

        Assert.Multiple(() =>
        {
            Assert.That(r, Is.EqualTo(new KiroResult(KiroState.Completed, "Renamed it.", 0)), "the answer is the text after the last tool call");
            Assert.That(fake.Got.First(g => g.Method == "session/new").Params.GetProperty("cwd").GetString(), Is.EqualTo(_dir));
            Assert.That(fake.Got.First(g => g.Method == "session/prompt").Params.GetProperty("prompt")[0].GetProperty("text").GetString(), Is.EqualTo(prompt));
            Assert.That(again.State, Is.EqualTo(KiroState.Completed));
            Assert.That(fake.Methods().Count(x => x == "session/new"), Is.EqualTo(1), "the reply used the same session");
            Assert.That(fake.Starts, Is.EqualTo(1), "one process for both");
        });
        lock (phases.Seen) Assert.That(phases.Seen, Is.EqualTo(new[] { KiroPhase.Starting, KiroPhase.Writing, KiroPhase.Reading, KiroPhase.Editing, KiroPhase.Writing }));
        lock (events.Seen)
        {
            Assert.That(events.Seen.Any(e => e.SessionId == "s1"));
            Assert.That(events.Seen.Where(e => e.Step is not null).Select(e => e.Step!.Target).First(), Is.EqualTo("a.cs"));
        }
        host.Shutdown();
    }

    [Test]
    public async Task After_a_shutdown_a_reply_loads_the_conversation_and_ignores_its_replay()
    {
        var (host, fake) = Make();
        await host.Run(_dir, "first", null, CancellationToken.None);
        host.Shutdown("idle");
        var events = new Events();
        var r = await host.Run(_dir, "second", null, CancellationToken.None, "s1", events);
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Completed));
            Assert.That(fake.Starts, Is.EqualTo(2));
            Assert.That(fake.Methods(), Does.Contain("session/load"));
            lock (events.Seen) Assert.That(events.Seen.Where(e => e.Step is not null).Select(e => e.Step!.Id), Does.Not.Contain("old"));
        });
        host.Shutdown();
    }

    [Test]
    public async Task A_tool_that_dies_fails_its_run_and_the_next_run_starts_it_again()
    {
        var (host, fake) = Make();
        fake.HangPrompt = true;
        var run = host.Run(_dir, "long task", null, CancellationToken.None);
        while (!fake.Methods().Contains("session/prompt")) await Task.Delay(20);
        fake.Crash!();
        var r = await run.WaitAsync(TimeSpan.FromSeconds(15));
        fake.HangPrompt = false;
        var next = await host.Run(_dir, "again", null, CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(15));
        Assert.Multiple(() =>
        {
            Assert.That(r.State, Is.EqualTo(KiroState.Failed));
            Assert.That(r.Text, Does.StartWith("Kiro stopped unexpectedly"));
            Assert.That(next.State, Is.EqualTo(KiroState.Completed));
            Assert.That(fake.Starts, Is.EqualTo(2));
            Assert.That(host.Alive, Is.True);
        });
        host.Shutdown();
        Assert.That(host.Alive, Is.False);
    }

    [Test]
    public async Task Stopping_cancels_the_turn()    {
        var (host, fake) = Make();
        fake.HangPrompt = true;
        using var cts = new CancellationTokenSource();
        var run = host.Run(_dir, "long task", null, cts.Token);
        while (!fake.Methods().Contains("session/prompt")) await Task.Delay(20);
        cts.Cancel();
        var r = await run.WaitAsync(TimeSpan.FromSeconds(15));
        Assert.That(r.State, Is.EqualTo(KiroState.Cancelled));
        Assert.That(fake.Methods(), Does.Contain("session/cancel"));
        host.Shutdown();
    }

    [Test]
    public async Task Read_only_refuses_a_write_and_says_why()
    {
        var (host, fake) = Make(new AgentOptions(ReadOnly: true));
        fake.AskToEdit = true;
        var r = await host.Run(_dir, "change it", null, CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(15));
        Assert.Multiple(() =>
        {
            Assert.That(fake.PermissionAnswer, Is.EqualTo("no"));
            Assert.That(r.State, Is.EqualTo(KiroState.Failed));
            Assert.That(r.Text, Does.Contain("read only"));
        });
        host.Shutdown();
    }

    [Test]
    public async Task The_model_is_set_and_then_the_effort_it_offers()
    {
        var (host, fake) = Make(new AgentOptions(Model: "m2", Effort: "high"));
        IReadOnlyList<AcpOption>? seen = null;
        host.OptionsSeen += (_, o) => seen = o;
        await host.Run(_dir, "go", null, CancellationToken.None);
        var sets = fake.Got.Where(g => g.Method == "session/set_config_option")
            .Select(g => g.Params.GetProperty("configId").GetString() + "=" + g.Params.GetProperty("value").GetString()).ToList();
        Assert.That(sets.Take(2), Is.EqualTo(new[] { "model=m2", "effortLevel=high" }));
        Assert.That(seen?.First(o => o.Id == "model").Current, Is.EqualTo("m2"));
        host.Shutdown();
    }

    [Test]
    public async Task A_missing_folder_or_tool_never_starts_anything()
    {
        var (host, fake) = Make();
        var gone = await host.Run(Path.Combine(_dir, "gone"), "hi", null, CancellationToken.None);
        var none = await new AcpHost(AgentTool.Codex, () => AgentOptions.Default, () => null).Run(_dir, "hi", null, CancellationToken.None);
        Assert.Multiple(() =>
        {
            Assert.That(gone.Text, Does.Contain("Choose another"));
            Assert.That(fake.Starts, Is.Zero);
            Assert.That(none.State, Is.EqualTo(KiroState.Failed));
            Assert.That(none.Text, Does.StartWith("Codex isn’t installed"));
        });
    }

    [Test]
    public void Only_a_full_path_to_an_existing_folder_is_usable()
    {
        var file = Path.Combine(_dir, "f.txt");
        File.WriteAllText(file, "");
        Assert.Multiple(() =>
        {
            Assert.That(KiroRunner.UsableFolder(null), Is.False);
            Assert.That(KiroRunner.UsableFolder("  "), Is.False);
            Assert.That(KiroRunner.UsableFolder("relative" + Path.DirectorySeparatorChar + "dir"), Is.False);
            Assert.That(KiroRunner.UsableFolder(Path.Combine(_dir, "gone")), Is.False);
            Assert.That(KiroRunner.UsableFolder(file), Is.False, "a file is not a folder");
            Assert.That(KiroRunner.UsableFolder(_dir), Is.True);
        });
    }
}

/// The shared run state the Kiro page draws from, with the runner stubbed out.
public sealed class KiroSessionTests
{
    private string _folder = "";

    [SetUp]
    public void Folder()
    {
        _folder = Path.Combine(Path.GetTempPath(), "hover-kiro-session-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_folder);
    }

    [TearDown]
    public void Clean() => Directory.Delete(_folder, true);

    [Test]
    public async Task A_run_goes_from_idle_to_running_to_completed()
    {
        var release = new TaskCompletionSource<KiroResult>();
        (string Folder, string Prompt)? got = null;
        var s = new KiroSession((f, p, progress, ct, _, _) => { got = (f, p); return release.Task; });
        var ended = new List<KiroResult>();
        s.Ended += ended.Add;

        Assert.That(s.State, Is.EqualTo(KiroState.Idle));
        Assert.That(s.Start(_folder, "  Write the changelog  "), Is.True);
        Assert.That(s.State, Is.EqualTo(KiroState.Running));
        Assert.That(got, Is.EqualTo((_folder, "Write the changelog")));
        Assert.That(s.Start(_folder, "another"), Is.False, "a session runs one task, once");

        release.SetResult(new KiroResult(KiroState.Completed, "Wrote it.", 0));
        await WaitFor(() => s.State != KiroState.Running);
        Assert.That(s.State, Is.EqualTo(KiroState.Completed));
        Assert.That(s.Result!.Text, Is.EqualTo("Wrote it."));
        Assert.That(ended.Single().State, Is.EqualTo(KiroState.Completed));
        Assert.That(s.Title, Is.EqualTo("Write the changelog"));
    }

    [Test]
    public void It_will_not_start_without_a_usable_folder_or_a_prompt()
    {
        var s = new KiroSession((_, _, _, _, _, _) => throw new AssertionException("must not run"));
        Assert.Multiple(() =>
        {
            Assert.That(s.Start(Path.Combine(_folder, "missing"), "task"), Is.False);
            Assert.That(s.Start("", "task"), Is.False);
            Assert.That(s.Start(_folder, "   "), Is.False);
            Assert.That(s.State, Is.EqualTo(KiroState.Idle));
        });
    }

    [Test]
    public async Task Stop_cancels_the_run()
    {
        var s = new KiroSession(async (_, _, _, ct, _, _) =>
        {
            await Task.Delay(Timeout.Infinite, ct).ContinueWith(_ => { });
            return new KiroResult(KiroState.Failed, "killed", -1);
        });
        s.Start(_folder, "long");
        s.Stop();
        await WaitFor(() => s.State != KiroState.Running);
        Assert.That(s.State, Is.EqualTo(KiroState.Cancelled), "a stopped run reads as stopped, however it ended");
    }

    [Test]
    public async Task A_runner_that_throws_ends_as_failed()
    {
        var s = new KiroSession((_, _, _, _, _, _) => Task.FromException<KiroResult>(new InvalidOperationException("boom")));
        s.Start(_folder, "task");
        await WaitFor(() => s.State != KiroState.Running);
        Assert.That(s.Result, Is.EqualTo(new KiroResult(KiroState.Failed, "boom")));
    }

    [Test]
    public async Task A_reply_carries_on_with_kiros_id_and_waits_while_a_turn_runs()
    {
        var runs = new List<(string Prompt, string? Resume, TaskCompletionSource<KiroResult> Done)>();
        var s = new KiroSession((_, p, _, _, resume, events) =>
        {
            var tcs = new TaskCompletionSource<KiroResult>();
            lock (runs) runs.Add((p, resume, tcs));
            events?.Report(new KiroEvent(SessionId: "sess_9"));
            return tcs.Task;
        });
        Assert.That(s.Reply("too early"), Is.False, "nothing to reply to yet");
        s.Start(_folder, "first");
        await WaitFor(() => s.KiroId is not null);
        Assert.That(s.Reply("second"), Is.True);
        Assert.That(s.Turns.Last().Queued, Is.True, "it waits for the turn that runs");
        runs[0].Done.SetResult(new KiroResult(KiroState.Completed, "one", 0));
        await WaitFor(() => runs.Count == 2);
        Assert.Multiple(() =>
        {
            Assert.That(runs[1].Prompt, Is.EqualTo("second"));
            Assert.That(runs[1].Resume, Is.EqualTo("sess_9"));
            Assert.That(s.Busy && !s.Turns.Last().Queued, Is.True);
            Assert.That(s.Prompt, Is.EqualTo("first"), "the session keeps its first prompt as its title");
        });
        s.Reply("third");
        s.Stop();
        runs[1].Done.SetResult(new KiroResult(KiroState.Failed, "killed", -1));
        await WaitFor(() => !s.Busy);
        Assert.That(s.Turns.Last().Result!.State, Is.EqualTo(KiroState.Cancelled), "a stop drops the waiting reply");
        Assert.That(runs, Has.Count.EqualTo(2));
    }

    internal static async Task WaitFor(Func<bool> done)
    {
        var sw = Stopwatch.StartNew();
        while (!done() && sw.Elapsed < TimeSpan.FromSeconds(5)) await Task.Delay(20);
    }
}

/// Several tasks at once: a cap on how many run, and on how many are kept.
public sealed class KiroSessionsTests
{
    private string _folder = "";
    private readonly List<TaskCompletionSource<KiroResult>> _runs = new();

    [SetUp]
    public void Folder()
    {
        _folder = Path.Combine(Path.GetTempPath(), "hover-kiro-sessions-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_folder);
        _runs.Clear();
    }

    [TearDown]
    public void Clean() => Directory.Delete(_folder, true);

    private KiroSessions Make() => new(() => new KiroSession((_, _, _, ct, _, _) =>
    {
        var tcs = new TaskCompletionSource<KiroResult>();
        ct.Register(() => tcs.TrySetResult(new KiroResult(KiroState.Cancelled, "stopped", -1)));
        lock (_runs) _runs.Add(tcs);
        return tcs.Task;
    }));

    private void Finish(int i) => _runs[i].TrySetResult(new KiroResult(KiroState.Completed, $"done {i}", 0));

    [Test]
    public async Task Tasks_run_side_by_side_up_to_the_cap()
    {
        var k = Make();
        var a = k.Start(_folder, "one");
        var b = k.Start(_folder, "two");
        var c = k.Start(_folder, "three");
        Assert.Multiple(() =>
        {
            Assert.That(new[] { a, b, c }, Has.None.Null);
            Assert.That(k.Running, Is.EqualTo(KiroSessions.MaxRunning));
            Assert.That(k.Start(_folder, "four"), Is.Null, "no fourth kiro-cli while three work");
            Assert.That(k.Selected, Is.SameAs(c), "a new task is the one shown");
            Assert.That(k.All.Select(s => s.Prompt), Is.EqualTo(new[] { "one", "two", "three" }));
        });

        Finish(1);
        await KiroSessionTests.WaitFor(() => k.Running == 2);
        Assert.That(b!.State, Is.EqualTo(KiroState.Completed));
        Assert.That(a!.Busy && c!.Busy, Is.True, "the others carry on");
        Assert.That(k.Start(_folder, "four"), Is.Not.Null, "a free slot takes a new task");
        k.StopAll();
        await KiroSessionTests.WaitFor(() => k.Running == 0);
        Assert.That(k.All.Count(s => s.State == KiroState.Cancelled), Is.EqualTo(3));
    }

    [Test]
    public async Task Tools_share_the_cap_and_each_session_runs_on_its_own_tool()
    {
        var ran = new List<AgentTool>();
        var gate = new TaskCompletionSource<KiroResult>();
        var k = new KiroSessions(tool => new KiroSession((_, _, _, ct, _, _) =>
        {
            lock (ran) ran.Add(tool);
            ct.Register(() => gate.TrySetResult(new KiroResult(KiroState.Cancelled, "")));
            return gate.Task;
        }));
        var a = k.Start(AgentTool.Kiro, _folder, "one");
        var b = k.Start(AgentTool.Codex, _folder, "two");
        var c = k.Start(AgentTool.Cursor, _folder, "three");
        Assert.Multiple(() =>
        {
            Assert.That(new[] { a!.Tool, b!.Tool, c!.Tool }, Is.EqualTo(new[] { AgentTool.Kiro, AgentTool.Codex, AgentTool.Cursor }));
            lock (ran) Assert.That(ran, Is.EqualTo(new[] { AgentTool.Kiro, AgentTool.Codex, AgentTool.Cursor }));
            Assert.That(k.Start(AgentTool.Kiro, _folder, "four"), Is.Null, "three running in all, whatever the tools");
        });
        k.StopAll();
        await KiroSessionTests.WaitFor(() => k.Running == 0);
    }

    [Test]
    public async Task Only_the_newest_are_kept_and_running_ones_never_go()
    {
        var k = Make();
        for (var i = 0; i < KiroSessions.MaxKept + 2; i++)
        {
            k.Start(_folder, "task " + i);
            Finish(i);
            await KiroSessionTests.WaitFor(() => k.Running == 0);
        }
        Assert.That(k.All, Has.Count.EqualTo(KiroSessions.MaxKept));
        Assert.That(k.All.First().Prompt, Is.EqualTo("task 2"));
    }

    [Test]
    public async Task Dismiss_removes_a_finished_task()
    {
        var k = Make();
        var a = k.Start(_folder, "one")!;
        k.Start(_folder, "two");
        k.Dismiss(a);
        Assert.That(k.All, Has.Count.EqualTo(2), "not while it runs");
        Finish(0);
        await KiroSessionTests.WaitFor(() => !a.Busy);
        k.Select(a);
        k.Dismiss(a);
        Assert.That(k.All.Select(s => s.Prompt), Is.EqualTo(new[] { "two" }));
        Assert.That(k.Selected, Is.Null, "dismissing the shown task goes back to a new one");
        k.StopAll();
    }
}
