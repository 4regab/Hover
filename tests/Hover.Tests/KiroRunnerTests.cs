using System.Diagnostics;
using System.IO;
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
}

/// The runner against a stand-in kiro-cli: a script that records its stdin, working
/// folder and arguments, prints canned events and exits with a given code.
[NonParallelizable]
public sealed class KiroRunnerTests
{
    private string _dir = "";
    private string _project = "";
    private string _exe = "";

    [SetUp]
    public void MakeFake()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-kiro-" + Guid.NewGuid().ToString("N"));
        _project = Path.Combine(_dir, "my project");
        Directory.CreateDirectory(_project);
        if (OperatingSystem.IsWindows())
        {
            _exe = Path.Combine(_dir, "kiro-cli.cmd");
            File.WriteAllText(_exe,
                "@echo off\r\n" +
                // findstr copies the bytes as they are; more would recode them.
                "findstr \"^\" > \"%~dp0stdin.txt\"\r\n" +
                "cd > \"%~dp0cwd.txt\"\r\n" +
                "echo %*> \"%~dp0args.txt\"\r\n" +
                "type \"%~dp0events.jsonl\"\r\n" +
                "if exist \"%~dp0hang\" ping -n 60 127.0.0.1 > nul\r\n" +
                "set /p CODE=<\"%~dp0code.txt\"\r\n" +
                "exit /b %CODE%\r\n");
        }
        else
        {
            _exe = Path.Combine(_dir, "kiro-cli");
            File.WriteAllText(_exe,
                "#!/bin/sh\nd=$(dirname \"$0\")\ncat > \"$d/stdin.txt\"\npwd > \"$d/cwd.txt\"\necho \"$@\" > \"$d/args.txt\"\n" +
                "cat \"$d/events.jsonl\"\n[ -f \"$d/hang\" ] && sleep 60\nexit $(cat \"$d/code.txt\")\n");
            File.SetUnixFileMode(_exe, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
        Script(0,
            "{\"type\":\"runStarted\",\"data\":{}}",
            "{\"type\":\"sessionUpdate\",\"data\":{\"update\":{\"sessionUpdate\":\"tool_call\",\"kind\":\"read\",\"title\":\"Reading\"}}}",
            "{\"type\":\"sessionUpdate\",\"data\":{\"update\":{\"sessionUpdate\":\"tool_call\",\"kind\":\"edit\",\"title\":\"Editing\"}}}",
            "{\"type\":\"runFinished\",\"data\":{\"stopReason\":\"end_turn\",\"finalText\":\"Renamed the helper and updated 3 callers.\"}}");
    }

    [TearDown]
    public void Clean()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { /* a killed child may still hold a file for a moment */ }
    }

    private void Script(int code, params string[] events)
    {
        File.WriteAllText(Path.Combine(_dir, "code.txt"), code.ToString());
        File.WriteAllText(Path.Combine(_dir, "events.jsonl"), string.Join("\n", events) + "\n");
    }

    private sealed class Phases : IProgress<KiroPhase>
    {
        public readonly List<KiroPhase> Seen = new();
        public void Report(KiroPhase value) { lock (Seen) Seen.Add(value); }
    }

    [Test]
    public void The_command_is_headless_with_full_tool_access_and_no_prompt_in_it() =>
        Assert.That(KiroRunner.Arguments, Is.EqualTo(new[]
        {
            "chat", "--no-interactive", "--trust-all-tools", "--agent-engine", "v3", "--output-format", "stream-json",
        }));

    [Test]
    public void Only_a_full_path_to_an_existing_folder_is_usable()
    {
        Assert.Multiple(() =>
        {
            Assert.That(KiroRunner.UsableFolder(null), Is.False);
            Assert.That(KiroRunner.UsableFolder("  "), Is.False);
            Assert.That(KiroRunner.UsableFolder("relative" + Path.DirectorySeparatorChar + "dir"), Is.False);
            Assert.That(KiroRunner.UsableFolder(Path.Combine(_dir, "gone")), Is.False);
            Assert.That(KiroRunner.UsableFolder(_exe), Is.False, "a file is not a folder");
            Assert.That(KiroRunner.UsableFolder(_project), Is.True);
        });
    }

    [Test]
    public async Task The_prompt_goes_in_on_stdin_and_the_folder_is_the_working_directory()
    {
        var prompt = "Fix the \"failing\" tests & keep the API; don't touch %PATH% or $HOME ünïcode";
        var phases = new Phases();
        var r = await KiroRunner.Run(_project, prompt, phases, CancellationToken.None, _exe);

        Assert.Multiple(() =>
        {
            Assert.That(r, Is.EqualTo(new KiroResult(KiroState.Completed, "Renamed the helper and updated 3 callers.", 0)));
            Assert.That(File.ReadAllText(Path.Combine(_dir, "stdin.txt")).Trim(), Is.EqualTo(prompt));
            Assert.That(File.ReadAllText(Path.Combine(_dir, "cwd.txt")).Trim(), Is.EqualTo(_project));
            var args = File.ReadAllText(Path.Combine(_dir, "args.txt")).Trim();
            Assert.That(args, Is.EqualTo(string.Join(" ", KiroRunner.Arguments)));
            Assert.That(args, Does.Not.Contain("failing"));
        });
        lock (phases.Seen) Assert.That(phases.Seen, Is.EqualTo(new[] { KiroPhase.Starting, KiroPhase.Reading, KiroPhase.Editing }));
    }

    [Test]
    public async Task A_non_zero_exit_fails_with_the_reason()
    {
        Script(1);
        File.AppendAllText(Path.Combine(_dir, "events.jsonl"), "Please try again with: kiro-cli login --use-device-flow\n");
        var r = await KiroRunner.Run(_project, "hello", null, CancellationToken.None, _exe);
        Assert.That(r.State, Is.EqualTo(KiroState.Failed));
        Assert.That(r.ExitCode, Is.EqualTo(1));
        Assert.That(r.Text, Does.StartWith("Kiro needs you to sign in"));
    }

    [Test]
    public async Task Stopping_kills_the_run_and_it_ends_as_cancelled()
    {
        Script(0, "{\"type\":\"sessionUpdate\",\"data\":{\"update\":{\"sessionUpdate\":\"tool_call\",\"kind\":\"execute\"}}}");
        File.WriteAllText(Path.Combine(_dir, "hang"), "");
        var phases = new Phases();
        using var cts = new CancellationTokenSource();
        var sw = Stopwatch.StartNew();
        var run = KiroRunner.Run(_project, "long task", phases, cts.Token, _exe);
        while (sw.Elapsed < TimeSpan.FromSeconds(20))
        {
            lock (phases.Seen) if (phases.Seen.Contains(KiroPhase.Running)) break;
            await Task.Delay(50);
        }
        cts.Cancel();
        var r = await run.WaitAsync(TimeSpan.FromSeconds(15));
        Assert.That(r.State, Is.EqualTo(KiroState.Cancelled));
        Assert.That(sw.Elapsed, Is.LessThan(TimeSpan.FromSeconds(30)), "the 60 second sleep was killed with it");
    }

    [Test]
    public async Task A_missing_folder_or_tool_never_starts_anything()
    {
        var gone = await KiroRunner.Run(Path.Combine(_dir, "gone"), "hi", null, CancellationToken.None, _exe);
        var missing = await KiroRunner.Run(_project, "hi", null, CancellationToken.None, Path.Combine(_dir, "no-such-kiro"));
        Assert.Multiple(() =>
        {
            Assert.That(gone.State, Is.EqualTo(KiroState.Failed));
            Assert.That(gone.Text, Does.Contain("Choose another"));
            Assert.That(File.Exists(Path.Combine(_dir, "stdin.txt")), Is.False);
            Assert.That(missing.State, Is.EqualTo(KiroState.Failed));
            Assert.That(missing.Text, Does.StartWith("kiro-cli couldn’t start"));
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
        var s = new KiroSession((f, p, progress, ct) => { got = (f, p); return release.Task; });
        var ended = new List<KiroResult>();
        s.Ended += ended.Add;

        Assert.That(s.State, Is.EqualTo(KiroState.Idle));
        Assert.That(s.Start(_folder, "  Write the changelog  "), Is.True);
        Assert.That(s.State, Is.EqualTo(KiroState.Running));
        Assert.That(got, Is.EqualTo((_folder, "Write the changelog")));
        Assert.That(s.Start(_folder, "another"), Is.False, "one task at a time");

        release.SetResult(new KiroResult(KiroState.Completed, "Wrote it.", 0));
        await WaitFor(() => s.State != KiroState.Running);
        Assert.That(s.State, Is.EqualTo(KiroState.Completed));
        Assert.That(s.Result!.Text, Is.EqualTo("Wrote it."));
        Assert.That(ended.Single().State, Is.EqualTo(KiroState.Completed));

        s.Reset();
        Assert.That(s.State, Is.EqualTo(KiroState.Idle));
        Assert.That(s.Prompt, Is.EqualTo("Write the changelog"), "the last task is still there to run again");
    }

    [Test]
    public void It_will_not_start_without_a_usable_folder_or_a_prompt()
    {
        var s = new KiroSession((_, _, _, _) => throw new AssertionException("must not run"));
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
        var s = new KiroSession(async (_, _, _, ct) =>
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
        var s = new KiroSession((_, _, _, _) => Task.FromException<KiroResult>(new InvalidOperationException("boom")));
        s.Start(_folder, "task");
        await WaitFor(() => s.State != KiroState.Running);
        Assert.That(s.Result, Is.EqualTo(new KiroResult(KiroState.Failed, "boom")));
    }

    private static async Task WaitFor(Func<bool> done)
    {
        var sw = Stopwatch.StartNew();
        while (!done() && sw.Elapsed < TimeSpan.FromSeconds(5)) await Task.Delay(20);
    }
}
