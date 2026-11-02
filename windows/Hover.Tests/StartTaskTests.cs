using System.Collections.Concurrent;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Tests;

/// <summary>
/// "cant scroll and start task" (owner, v5.0.18), end to end. The panels the app shows (<see cref="OfficeUi"/>, with a shaper of fixed
/// letters) are joined to a real hover-backend with a data folder that has never run, and the stand-in agent. Everything is done through
/// the panels: the note shows, Got it, an agent and a folder are picked, a task is typed and sent (once with a click, once with Enter),
/// the agent asks to edit and is allowed, and the answer is in the chat.
/// </summary>
public class StartTaskTests
{
    /// <summary>The panels as the app joins them to the backend: its messages go in on this thread, between frames.</summary>
    sealed class LiveRig : IAsyncDisposable
    {
        readonly Sandbox sandbox = new();
        readonly ConcurrentQueue<BackendEvent> inbox = new();
        readonly FakeClipboard clipboard = new();
        double now = 1000;

        public BackendClient Client { get; }
        public OfficeUi Ui { get; }
        public readonly List<BackendCommand> Sent = [];
        public RecordingCanvas Canvas = new();
        public float W { get; }
        public float H { get; }
        public string Project => sandbox.Project;

        /// <summary>What the folder dialog answers.</summary>
        public string? Chosen;

        LiveRig(float w, float h)
        {
            (W, H) = (w, h);
            Client = sandbox.Start();
            Client.Event += inbox.Enqueue;
            Ui = new OfficeUi(new FakeShaper(), new UiEnv
            {
                Send = c => { lock (Sent) Sent.Add(c); Client.Send(c); },
                Clipboard = clipboard,
                PickFolder = () => Chosen,
            });
        }

        public static async Task<LiveRig> Open(float w, float h)
        {
            var rig = new LiveRig(w, h);
            await rig.Client.InitializeAsync(Conversation.Key);
            // What the app asks first: the settings (the note is theirs to say), and a look at the tools.
            rig.Client.Send(new SettingsCommand());
            rig.Client.Send(new ReadyCommand());
            return rig;
        }

        /// <summary>Hands the backend's messages to the panels and draws until nothing more is waiting.</summary>
        public void Frame()
        {
            for (var round = 0; round < 20; round++)
            {
                var took = false;
                while (inbox.TryDequeue(out var e))
                {
                    Ui.Accept(e);
                    took = true;
                }
                now += 16;
                Canvas = new RecordingCanvas();
                Ui.Paint(Canvas, W, H, now);
                Assert.True(Canvas.Balanced);
                if (!took && !Ui.Dirty) break;
            }
        }

        public async Task Until(Func<bool> done, string what, int seconds = 25)
        {
            var end = DateTime.UtcNow.AddSeconds(seconds);
            while (true)
            {
                Frame();
                if (done()) return;
                if (DateTime.UtcNow > end)
                    throw new Xunit.Sdk.XunitException($"Timed out waiting for {what}. Sessions: {string.Join(", ", Ui.State?.Sessions.Select(s => s.Id + ":" + s.Stage) ?? [])}; note shown: {Ui.NoticeShown}; toast: {Ui.ToastText}");
                await Task.Delay(25);
            }
        }

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

        public void Key(UiKey key)
        {
            Ui.Key(key, UiMods.None);
            Frame();
        }

        public async ValueTask DisposeAsync()
        {
            try { await Client.StopAsync(TimeSpan.FromSeconds(10)); } catch (Exception e) when (e is BackendException or InvalidOperationException) { }
            await Client.DisposeAsync();
            sandbox.Dispose();
        }
    }

    [Theory]
    [InlineData(828f, 346f)]
    [InlineData(1200f, 720f)]
    public async Task A_first_task_is_started_from_the_office_with_a_click_and_with_Enter_and_finishes_with_its_answer_in_the_chat(float w, float h)
    {
        await using var rig = await LiveRig.Open(w, h);
        var ui = rig.Ui;

        // A data folder that has never run: the backend has not been told the note was seen, and says so.
        await rig.Until(() => ui.NoticeShown && ui.State?.Tools.Any(t => t.Id == "codex" && t.Ready) == true, "the note and the tools");
        Assert.Contains("Before an agent starts", rig.Canvas.Texts);
        Assert.Contains("Got it", rig.Canvas.Texts);
        // Under the note nothing answers: the + circle is there to see but a press on it opens nothing.
        Assert.True(rig.Click("fab"));
        Assert.Null(ui.Find("taskText"));

        // Got it. The backend has the setting (it answers a settings request with it) and the card is gone.
        var queue = rig.Client.Subscribe();
        Assert.True(rig.Click("noticeOk"));
        Assert.False(ui.NoticeShown);
        // The panels asked for the settings when they started: that reply may still be on its way, and says the note is not seen. The one
        // that counts is the first that says it is (the run 41440945992 re-run took the early one on a slow machine).
        rig.Client.Send(new SettingsCommand());
        var settings = await queue.Until<SettingsEvent>(e => e.Model.TryGetProperty("KiroNoticeSeen", out var seen) && seen.ValueKind == System.Text.Json.JsonValueKind.True);
        Assert.True(settings.Model.GetProperty("KiroNoticeSeen").GetBoolean());
        Assert.DoesNotContain("Before an agent starts", rig.Canvas.Texts);

        // The box: the stand-in agent (Codex), "Ask always" so that it asks, and a folder from the dialog.
        Assert.True(rig.Click("fab"));
        Assert.NotNull(ui.Find("taskText"));
        Assert.True(rig.Click("pillAgent"));
        Assert.True(rig.Click("menu", s: "Codex"));
        Assert.True(rig.Click("pillAccess"));
        Assert.True(rig.Click("menu", s: "Ask always"));
        // No folder yet: the chip says so, and a click opens the folder dialog at once (as the Go box does). Cancelled, nothing is picked.
        Assert.Contains("Choose a folder", rig.Canvas.Texts);
        rig.Chosen = null;
        Assert.True(rig.Click("pillFolder"));
        Assert.Null(ui.OpenMenuKind);
        Assert.Contains("Choose a folder", rig.Canvas.Texts);
        rig.Chosen = rig.Project;
        Assert.True(rig.Click("pillFolder"));
        Assert.Contains("project", rig.Canvas.Texts);

        // An empty box does not send. Typed words do, by the round button.
        Assert.True(rig.Click("startOff"));
        Assert.DoesNotContain(rig.Sent, c => c is NewTaskCommand);
        Assert.True(rig.Click("taskText"));
        rig.Type("Sandbox secret");
        Assert.True(rig.Click("startTask"));
        var first = rig.Sent.OfType<NewTaskCommand>().Single();
        Assert.Equal(("codex", rig.Project, "Sandbox secret", "always"), (first.Tool, first.Folder, first.Prompt, first.Access));
        await rig.Until(() => ui.State?.Sessions.Count == 1, "the session");
        Assert.Null(ui.ToastText);

        // It runs, asks to edit, is allowed from the panels, and finishes; its chat opened by itself and holds the answer.
        await rig.Until(() => ui.State!.Sessions[0].Stage == "waiting" && ui.Find("askAllow") is not null, "the question");
        Assert.True(rig.Click("askAllow"));
        await rig.Until(() => ui.State!.Sessions[0].Stage == "done", "the end of the task");
        Assert.Equal(1, ui.OpenId);
        await rig.Until(() => ui.ThreadTexts.Any(t => t.Contains("Sandbox reply", StringComparison.Ordinal)), "the answer in the chat");

        // A second task, sent with Enter. The picks are kept: no second note, no second folder dialog.
        ui.CloseChat();
        rig.Frame();
        Assert.True(rig.Click("fab"));
        Assert.True(rig.Click("taskText"));
        rig.Type("Second one");
        rig.Key(UiKey.Enter);
        Assert.Equal(2, rig.Sent.OfType<NewTaskCommand>().Count());
        Assert.Equal(("codex", rig.Project, "Second one"), (rig.Sent.OfType<NewTaskCommand>().Last().Tool, rig.Sent.OfType<NewTaskCommand>().Last().Folder, rig.Sent.OfType<NewTaskCommand>().Last().Prompt));
        await rig.Until(() => ui.State?.Sessions.Count == 2, "the second session");
        await rig.Until(() => ui.State!.Sessions.Any(s => s.Stage == "waiting") && ui.Find("askAllow") is not null, "the second question");
        Assert.True(rig.Click("askAllow"));
        await rig.Until(() => ui.State!.Sessions.All(s => s.Stage == "done"), "both tasks done");
        Assert.All(ui.State!.Sessions, s => Assert.Equal("Sandbox reply", s.Turns[0].Answer));
        Assert.Null(ui.ToastText);
    }

    [Fact]
    public async Task The_backend_still_refuses_a_task_until_the_note_is_seen_and_the_panels_say_why_before_it_comes_to_that()
    {
        await using var rig = await LiveRig.Open(1200, 720);
        await rig.Until(() => rig.Ui.NoticeShown && rig.Ui.State?.Tools.Any(t => t.Id == "codex" && t.Ready) == true, "the note and the tools");
        // What the owner met: a task sent before the note was answered is refused by the backend, and nothing starts.
        rig.Client.Send(new NewTaskCommand("codex", rig.Project, "too early", "always"));
        await rig.Until(() => rig.Ui.ToastText is { } t && t.Contains("Review agent access in Settings before starting your first task", StringComparison.Ordinal), "the refusal");
        Assert.Empty(rig.Ui.State!.Sessions);
        // Settings > Kiro > "Show the note again" goes through setSetting: the card comes back after Got it.
        Assert.True(rig.Click("noticeOk"));
        Assert.False(rig.Ui.NoticeShown);
        rig.Client.Send(new SetSettingCommand("KiroNoticeSeen", System.Text.Json.Nodes.JsonValue.Create(false)));
        await rig.Until(() => rig.Ui.NoticeShown, "the note again");
        Assert.Contains("Before an agent starts", rig.Canvas.Texts);
        Assert.True(rig.Click("noticeOk"));
        await rig.Until(() => !rig.Ui.NoticeShown, "the note gone");
    }
}
