using System.Diagnostics;
using System.IO;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// Against the real OpenCode on this PC, with a real model: run by hand only.
///   dotnet test .\tests\Hover.Tests -c Release --filter "TestCategory=LiveOpenCode"
/// HOVER_LIVE_MODEL picks the model (default: OpenCode's free opencode/big-pickle).
/// Each test works in a new git folder with a space and a non-ASCII letter in its name.
[Explicit("Starts the real opencode serve and spends model calls.")]
[Category("LiveOpenCode")]
[NonParallelizable]
public sealed class OpenCodeLiveTests
{
    private static readonly string Model = Environment.GetEnvironmentVariable("HOVER_LIVE_MODEL") ?? "opencode/big-pickle";
    private string _dir = "";

    [SetUp]
    public void Folder()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover live ü " + Guid.NewGuid().ToString("N")[..6]);
        Directory.CreateDirectory(_dir);
        Git("init", "-q");
        File.WriteAllText(Path.Combine(_dir, "README.md"), "demo\n");
        Git("add", "."); Git("-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "start");
    }

    private readonly List<OpenCodeHost> _hosts = new();

    /// A host that is shut down after the test, whether it passed or not.
    private OpenCodeHost Make(AgentOptions o)
    {
        var h = new OpenCodeHost(() => o);
        _hosts.Add(h);
        return h;
    }

    [TearDown]
    public void Clean()
    {
        foreach (var h in _hosts) h.Shutdown("test end");
        _hosts.Clear();
        try { Directory.Delete(_dir, true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }

    private void Git(params string[] args)
    {
        var psi = new ProcessStartInfo("git") { WorkingDirectory = _dir, UseShellExecute = false, CreateNoWindow = true };
        foreach (var a in args) psi.ArgumentList.Add(a);
        using var p = Process.Start(psi)!;
        p.WaitForExit();
    }

    private sealed class Steps : IProgress<KiroEvent>
    {
        public readonly List<KiroEvent> Seen = new();
        public string? Sid;
        public void Report(KiroEvent e) { lock (Seen) { Seen.Add(e); if (e.SessionId is { } s) Sid = s; } }
    }

    private static long OpenCodeMemory(out int count)
    {
        var ps = Process.GetProcessesByName("opencode");
        count = ps.Length;
        long sum = 0;
        foreach (var p in ps) { try { p.Refresh(); sum += p.PrivateMemorySize64; } catch { } p.Dispose(); }
        return sum;
    }

    [Test]
    public async Task Edits_a_file_carries_on_the_conversation_after_idle_shutdown_and_cleans_up()
    {
        var before = Process.GetProcessesByName("opencode").Select(p => p.Id).ToHashSet();
        var host = Make(new AgentOptions(Model: Model));
        var steps = new Steps();
        var sw = Stopwatch.StartNew();
        var r = await host.Run(_dir, "Create a file named hello.txt containing exactly the word hi. Then reply with one short sentence.", null, CancellationToken.None, events: steps);
        TestContext.Out.WriteLine($"turn 1: {r.State} in {sw.Elapsed.TotalSeconds:0.0}s: {r.Text}");
        var active = OpenCodeMemory(out var n);
        TestContext.Out.WriteLine($"opencode processes: {n}, private memory {active / 1048576} MB");
        Assert.That(r.State, Is.EqualTo(KiroState.Completed), r.Text);
        Assert.That(File.ReadAllText(Path.Combine(_dir, "hello.txt")).Trim(), Is.EqualTo("hi"));
        Assert.That(steps.Seen.Select(e => e.Step).OfType<KiroStep>().Any(s => s.Kind == "edit"), Is.True);
        var sid = steps.Sid!;

        // As the idle timer does: the server goes; a reply starts it again and resumes.
        host.Shutdown("test idle");
        await Task.Delay(1500);
        Assert.That(Process.GetProcessesByName("opencode").Count(p => !before.Contains(p.Id)), Is.Zero, "the server and its children are gone");
        var steps2 = new Steps();
        var r2 = await host.Run(_dir, "What is the name of the file you created a moment ago? Answer with just the file name.", null, CancellationToken.None, sid, steps2);
        TestContext.Out.WriteLine($"turn 2: {r2.State}: {r2.Text}");
        Assert.That(r2.State, Is.EqualTo(KiroState.Completed), r2.Text);
        Assert.That(steps2.Sid, Is.EqualTo(sid), "the same OpenCode conversation");
        Assert.That(r2.Text, Does.Contain("hello.txt"), "it remembers the first turn");
        host.Shutdown("test done");
    }

    [Test]
    public async Task Asks_before_a_command_takes_a_refusal_and_answers_a_question()
    {
        var host = Make(new AgentOptions(Model: Model, Approval: AgentApproval.Always));
        var asks = new List<AgentAsk>();
        var questions = new List<AgentAsk>();
        host.Asking = (_, a, _) => { lock (asks) asks.Add(a); return Task.FromResult(AskAnswer.Deny); };
        host.Questioning = (_, a, _) => { lock (questions) questions.Add(a); return Task.FromResult<IReadOnlyList<IReadOnlyList<string>>?>(new[] { new[] { a.Questions![0].Options[0].Label } }); };
        var r = await host.Run(_dir, "First use your question tool to ask me whether I prefer Tabs or Spaces (those two options). " +
            "Then run the shell command: git status. If it is refused, don't retry; just say so.", null, CancellationToken.None);
        TestContext.Out.WriteLine($"{r.State}: {r.Text}\nasks: {string.Join(", ", asks.Select(a => a.Kind + " " + a.Command))}\nquestions: {questions.Count}");
        Assert.That(r.State, Is.EqualTo(KiroState.Completed), r.Text);
        Assert.That(questions, Has.Count.GreaterThanOrEqualTo(1));
        Assert.That(asks.Any(a => a.Kind == "execute"), Is.True);
        host.Shutdown("test done");
    }

    [Test]
    public async Task Read_only_leaves_the_folder_unchanged_even_through_the_shell_and_outside_it()
    {
        var outside = Path.Combine(Path.GetTempPath(), "hover-live-outside-" + Guid.NewGuid().ToString("N")[..6] + ".txt");
        // The model can take the shell route to get around an edit rule; the server has to stop both.
        var host = Make(new AgentOptions(Model: Model, ReadOnly: true));
        host.Asking = (_, _, _) => throw new AssertionException("read only never asks");
        var steps = new Steps();
        var r = await host.Run(_dir, $"Create a file named a.txt with the text x. Also run the shell command: echo x > b.txt. Also write the text x to the file {outside}. Then say what happened.",
            null, CancellationToken.None, events: steps);
        TestContext.Out.WriteLine($"{r.State}: {r.Text}\nsteps: {string.Join(", ", steps.Seen.Select(e => e.Step).OfType<KiroStep>().Select(s => s.Kind + "/" + s.Status))}");
        Assert.That(r.Text, Does.Not.Contain("didn’t start"), "the run has to have happened");
        Assert.That(r.State, Is.Not.EqualTo(KiroState.Failed).Or.Property("Text").Contains("read only"), r.Text);
        Git("status", "--porcelain");
        Assert.Multiple(() =>
        {
            Assert.That(File.Exists(Path.Combine(_dir, "a.txt")), Is.False);
            Assert.That(File.Exists(Path.Combine(_dir, "b.txt")), Is.False);
            Assert.That(File.Exists(outside), Is.False);
        });
        host.Shutdown("test done");
    }

    [Test]
    public async Task Stop_ends_a_run_and_a_second_folder_keeps_working()
    {
        var other = Path.Combine(Path.GetTempPath(), "hover live two " + Guid.NewGuid().ToString("N")[..6]);
        Directory.CreateDirectory(other);
        try
        {
            var host = Make(new AgentOptions(Model: Model));
            using var cts = new CancellationTokenSource();
            var steps = new Steps();
            var slow = host.Run(_dir, "Count slowly from 1 to 200, one number per line, then create done.txt.", null, cts.Token, events: steps);
            var fast = host.Run(other, "Create a file named two.txt containing the word two. Reply done.", null, CancellationToken.None);
            await Task.Delay(TimeSpan.FromSeconds(6));
            var sw = Stopwatch.StartNew();
            cts.Cancel();
            var stopped = await slow;
            TestContext.Out.WriteLine($"stopped: {stopped.State} after {sw.Elapsed.TotalSeconds:0.0}s");
            var done = await fast;
            TestContext.Out.WriteLine($"other: {done.State}: {done.Text}");
            Assert.That(stopped.State, Is.EqualTo(KiroState.Cancelled));
            Assert.That(done.State, Is.EqualTo(KiroState.Completed), done.Text);
            Assert.That(File.Exists(Path.Combine(other, "two.txt")), Is.True);
            host.Shutdown("test done");
        }
        finally { try { Directory.Delete(other, true); } catch (IOException) { } }
    }
}
