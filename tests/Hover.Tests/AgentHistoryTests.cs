using System.IO;
using Hover.Owl;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// The sealed session history, and waking a saved session back into the office.
public sealed class AgentHistoryTests
{
    private string _dir = "", _folder = "";

    [SetUp]
    public void Folders()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-history-" + Guid.NewGuid().ToString("N"));
        _folder = Path.Combine(_dir, "project");
        Directory.CreateDirectory(_folder);
    }

    [TearDown]
    public void Clean() => Directory.Delete(_dir, true);

    /// A session lives on the UI thread in Hover, where its runs' news and their ends
    /// arrive in order. Without one, Progress posts to the thread pool and the session
    /// id can land after the turn has ended and been saved. This is that thread.
    private static void OnOneThread(Func<Task> body)
    {
        var queue = new System.Collections.Concurrent.BlockingCollection<(SendOrPostCallback, object?)>();
        var ctx = new OneThread(queue);
        Exception? failed = null;
        var t = new Thread(() =>
        {
            SynchronizationContext.SetSynchronizationContext(ctx);
            var task = body();
            task.ContinueWith(_ => queue.CompleteAdding(), TaskScheduler.Default);
            foreach (var (cb, state) in queue.GetConsumingEnumerable()) cb(state);
            failed = task.Exception?.InnerException;
        });
        t.Start();
        if (!t.Join(TimeSpan.FromSeconds(60))) throw new TimeoutException("the test didn't finish");
        if (failed is not null) System.Runtime.ExceptionServices.ExceptionDispatchInfo.Throw(failed);
    }

    private sealed class OneThread(System.Collections.Concurrent.BlockingCollection<(SendOrPostCallback, object?)> queue) : SynchronizationContext
    {
        public override void Post(SendOrPostCallback d, object? state) { try { queue.Add((d, state)); } catch (InvalidOperationException) { } }
        public override SynchronizationContext CreateCopy() => this;
    }

    /// Sessions whose runs answer at once, saying which conversation they resumed.
    private (KiroSessions Sessions, List<string?> Resumed) Make(AgentHistory history)
    {
        var resumed = new List<string?>();
        var k = new KiroSessions(tool => new KiroSession(async (_, p, _, _, resume, events) =>
        {
            lock (resumed) resumed.Add(resume);
            events?.Report(new KiroEvent(SessionId: "acp-1"));
            // A real run's end comes back through the UI thread's queue, after its news.
            await Task.Yield();
            return new KiroResult(KiroState.Completed, "answer to " + p);
        })) { History = history };
        return (k, resumed);
    }

    [Test]
    public void A_session_is_sealed_on_disk_and_comes_back_whole() => OnOneThread(async () =>
    {
        var history = new AgentHistory(Path.Combine(_dir, "agents"));
        var (k, _) = Make(history);
        var s = k.Start(AgentTool.Codex, _folder, "Fix the secret thing")!;
        await KiroSessionTests.WaitFor(() => !s.Busy);
        history.Flush();

        var again = new AgentHistory(Path.Combine(_dir, "agents"));
        var saved = again.Load(s.Key);
        Assert.Multiple(() =>
        {
            Assert.That(again.Entries.Single().Title, Is.EqualTo("Fix the secret thing"));
            Assert.That(again.Entries.Single().State, Is.EqualTo(KiroState.Completed));
            Assert.That(saved!.Tool, Is.EqualTo(AgentTool.Codex));
            Assert.That(saved.AcpId, Is.EqualTo("acp-1"));
            Assert.That(saved.Turns.Single().Text, Is.EqualTo("answer to Fix the secret thing"));
            var raw = string.Concat(Directory.GetFiles(Path.Combine(_dir, "agents")).Select(File.ReadAllText));
            Assert.That(raw, Does.Not.Contain("secret"), "sealed, not plain text");
        });
    });

    [Test]
    public void A_session_that_left_its_desk_wakes_on_a_reply_and_carries_on_its_conversation() => OnOneThread(async () =>
    {
        var history = new AgentHistory(Path.Combine(_dir, "agents"));
        var (k, resumed) = Make(history);
        var first = k.Start(AgentTool.Cursor, _folder, "first")!;
        await KiroSessionTests.WaitFor(() => !first.Busy);
        // Six more push it off its desk; it stays in the history.
        for (var i = 0; i < KiroSessions.MaxKept; i++)
        {
            var s = k.Start(AgentTool.Kiro, _folder, "task " + i)!;
            await KiroSessionTests.WaitFor(() => !s.Busy);
        }
        Assert.That(k.All.Any(x => x.Key == first.Key), Is.False);
        Assert.That(history.Entries.Any(e => e.Key == first.Key), Is.True);

        var woken = k.Wake(first.Key)!;
        Assert.That(woken.Turns.Single().Result!.Text, Is.EqualTo("answer to first"));
        Assert.That(k.Reply(woken, "and then?"), Is.True);
        await KiroSessionTests.WaitFor(() => !woken.Busy);
        Assert.Multiple(() =>
        {
            Assert.That(woken.Tool, Is.EqualTo(AgentTool.Cursor));
            lock (resumed) Assert.That(resumed.Last(), Is.EqualTo("acp-1"), "the reply resumed the saved conversation");
            Assert.That(woken.Turns, Has.Count.EqualTo(2));
            Assert.That(k.All, Has.Count.EqualTo(KiroSessions.MaxKept));
        });
    });

    [Test]
    public async Task Delete_takes_a_session_out_of_the_office_and_the_history()
    {
        var history = new AgentHistory(Path.Combine(_dir, "agents"));
        var (k, _) = Make(history);
        var s = k.Start(AgentTool.Kiro, _folder, "one")!;
        await KiroSessionTests.WaitFor(() => !s.Busy);
        k.Delete(s.Key);
        history.Flush();
        Assert.Multiple(() =>
        {
            Assert.That(k.All, Is.Empty);
            Assert.That(history.Entries, Is.Empty);
            Assert.That(history.Load(s.Key), Is.Null);
            Assert.That(new AgentHistory(Path.Combine(_dir, "agents")).Entries, Is.Empty);
        });
    }

    [Test]
    public void A_key_that_isnt_hovers_never_becomes_a_path()
    {
        var history = new AgentHistory(Path.Combine(_dir, "agents"));
        Assert.That(history.Load(@"..\..\x"), Is.Null);
        Assert.DoesNotThrow(() => history.Delete(@"..\x"));
    }
}
