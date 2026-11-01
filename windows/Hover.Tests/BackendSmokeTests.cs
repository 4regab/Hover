using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The flows of tests/macos/backend-smoke.py, run by the C# host against a real hover-backend
/// and the stand-in tools in place of Codex: no account, no network, no real agent. The Mac
/// script does them as one long story; here each flow has its own sandbox and its own name,
/// so a failure says which one. Computer use is Mac only and is not part of these flows.
/// </summary>
public class BackendSmokeTests(ITestOutputHelper output)
{
    /// <summary>A task from start to its first finished turn, through the permission question.</summary>
    static async Task<SessionInfo> RunFirstTask(Conversation talk, Sandbox sandbox)
    {
        await talk.GetReady();
        talk.Send(new NewTaskCommand("codex", sandbox.Project, "Sandbox secret", "always"));
        var waiting = await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "waiting");
        var session = waiting.Sessions[0];
        talk.Send(new AnswerCommand(session.Id, session.Ask!.Id, "allow"));
        var done = await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "done");
        return done.Sessions[0];
    }

    [Fact]
    public async Task Settings_save_and_read_back()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;

        talk.Send(new GetSettingsCommand());
        Assert.False((await talk.Until<PreferencesEvent>()).NoticeSeen);

        talk.Send(new SaveSettingsCommand { NoticeSeen = false, Hover = true });
        var first = await talk.Until<PreferencesEvent>();
        Assert.False(first.NoticeSeen);
        Assert.False(first.ComputerUse);
        Assert.False(first.KiroAutoCompact);
        Assert.Equal(80, first.KiroCompactAt);

        talk.Send(new SaveSettingsCommand
        {
            NoticeSeen = true, Hover = false, MaxRunning = 4, QuotaItems = [], ComputerUse = true,
            KiroAutoCompact = true, KiroCompactAt = 60,
            Tools = [new ToolSetting("codex", "always", 15, true)],
        });
        var saved = await talk.Until<PreferencesEvent>();
        Assert.True(saved.NoticeSeen);
        Assert.False(saved.Hover);
        Assert.Equal(4, saved.MaxRunning);
        Assert.True(saved.ComputerUse);
        Assert.True(saved.KiroAutoCompact);
        Assert.Equal(60, saved.KiroCompactAt);
        var codex = saved.Tools.Single(t => t.Id == "codex");
        Assert.Equal(("always", 15, true), (codex.Access, codex.Idle, codex.HideSteps));

        talk.Send(new ComputerUseCommand());
        var cu = await talk.Until<ComputerUseEvent>(c => c.Checked);
        Assert.True(cu.On);
        output.WriteLine($"Computer use is switched on and checked here: installed={cu.Installed} ready={cu.Ready} ({cu.Hint})");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Task_runs_asks_permission_and_answers()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;

        var session = await RunFirstTask(talk, sandbox);
        Assert.Equal("Sandbox reply", session.Turns[0].Answer);
        var kinds = session.Turns[0].Steps.Select(x => x.K).ToList();
        Assert.Contains("thought", kinds);
        Assert.Contains("agent", kinds);
        Assert.Contains("run", kinds);
        Assert.Contains("edit", kinds);

        // The servers the backend hands every session. The host runs no agent browser yet and
        // computer use is a Mac feature, so there are none.
        var servers = JsonDocument.Parse(File.ReadAllText(Path.Combine(sandbox.Root, "mcp-servers.json"))).RootElement;
        Assert.Equal(JsonValueKind.Array, servers.ValueKind);
        Assert.DoesNotContain(servers.EnumerateArray(), x => x.GetProperty("name").GetString() == "cua-driver");
        Assert.DoesNotContain(servers.EnumerateArray(), x => x.GetProperty("name").GetString() == "hover-browser");

        // A reply starts a second turn, with its own permission question.
        talk.Send(new ReplyCommand("Continue", Id: session.Id));
        var waiting = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting");
        talk.Send(new AnswerCommand(session.Id, waiting.Sessions[0].Ask!.Id, "allow"));
        var second = await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 2 && s.Sessions[0].Stage == "done");
        Assert.Equal("Sandbox reply", second.Sessions[0].Turns[1].Answer);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Desk_panels_read_the_project()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var session = await RunFirstTask(talk, sandbox);

        // The backend writes + and < as \uXXXX; the text below is what they say.
        static string Text(JsonElement x) => JsonSerializer.Serialize(x, new JsonSerializerOptions { Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping });

        async Task<JsonElement> Desk(string what)
        {
            talk.Send(new DeskCommand(session.Id, what));
            var panel = await talk.Until<DeskEvent>(d => d.What == what);
            Assert.False(panel.Data.TryGetProperty("error", out var error), $"{what}: {error}");
            return panel.Data;
        }

        var probe = await Desk("probe");
        Assert.Equal(JsonValueKind.True, probe.GetProperty("folder").ValueKind);
        Assert.True(probe.GetProperty("git").ValueKind is not (JsonValueKind.False or JsonValueKind.Null));
        Assert.True(probe.GetProperty("commands").GetInt32() >= 1);
        Assert.Equal(1, probe.GetProperty("agents").GetInt32());
        Assert.True(probe.GetProperty("pages").GetInt32() >= 2);
        Assert.Contains("localhost:5173", Text(await Desk("terminal")));
        Assert.Contains("app.js", Text(await Desk("files")));
        Assert.Contains("+b", Text(await Desk("diff")));
        var agent = (await Desk("agents")).GetProperty("agents")[0];
        Assert.Equal("explore", agent.GetProperty("name").GetString());
        Assert.Contains("Found config", agent.GetProperty("out").GetString());
        var pages = Text(await Desk("browser"));
        Assert.Contains("example.com/docs", pages);
        Assert.Contains("localhost:5173", pages);
        output.WriteLine("Desk panels passed: probe, terminal, files, diff, subagents, browser pages and the thought step.");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task History_is_sealed_and_a_wrong_key_changes_nothing()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using (talk.Client)
        {
            await RunFirstTask(talk, sandbox);
            Assert.Equal(0, await talk.Client.StopAsync());
        }

        byte[] History() => Directory.EnumerateFiles(Path.Combine(sandbox.Data, "agents"), "*.dat").Order()
            .SelectMany(File.ReadAllBytes).ToArray();
        var before = History();
        Assert.NotEmpty(before);
        Assert.DoesNotContain("Sandbox secret", Encoding.Latin1.GetString(before));

        // The right folder with a wrong key: the backend refuses, and the history is not touched.
        await using var wrong = sandbox.Start();
        var queue = wrong.Subscribe();
        wrong.Send(new InitializeCommand(Convert.ToBase64String(Enumerable.Repeat((byte)255, 32).ToArray())));
        var failure = await Assert.ThrowsAsync<BackendException>(() => queue.Until<InitializedEvent>());
        Assert.Contains("cannot decrypt", failure.Message);
        await wrong.WaitForExitAsync(TimeSpan.FromSeconds(15));
        Assert.Equal(before, History());
    }

    [Fact]
    public async Task No_key_means_the_backend_runs_with_the_history_off()
    {
        // HistoryKey rule 4: a key that cannot be read now. The host starts the backend with no
        // key; tasks work, and nothing is sealed that the next run could not open.
        using var sandbox = new Sandbox();
        var client = sandbox.Start();
        var talk = new Conversation(client);
        await using var cleanup = client;
        await client.InitializeAsync(null);

        var session = await RunFirstTask(talk, sandbox);
        Assert.Equal("Sandbox reply", session.Turns[0].Answer);
        talk.Send(new ReadyCommand());
        Assert.Empty((await talk.Until<StateEvent>()).History);
        Assert.Equal(0, await client.StopAsync());

        var sealedFiles = Directory.Exists(Path.Combine(sandbox.Data, "agents"))
            ? Directory.GetFiles(Path.Combine(sandbox.Data, "agents"), "*.dat") : [];
        Assert.Empty(sealedFiles);
        Assert.False(File.Exists(DataFolder.KeyFile(sandbox.Data)), "the host makes note.key, not the backend");
    }

    [Fact]
    public async Task History_and_settings_survive_a_restart()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        string key;
        PreferencesEvent preferences;
        await using (talk.Client)
        {
            var session = await RunFirstTask(talk, sandbox);
            key = session.Key;
            talk.Send(new SaveSettingsCommand { MaxRunning = 4, KiroCompactAt = 60, Tools = [new ToolSetting("codex", "always", 15, true)] });
            preferences = await talk.Until<PreferencesEvent>();
            talk.Send(new ReplyCommand("Continue", Id: session.Id));
            var waiting = await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "waiting");
            talk.Send(new AnswerCommand(session.Id, waiting.Sessions[0].Ask!.Id, "allow"));
            await talk.Until<StateEvent>(s => s.Sessions[0].Turns.Count == 2 && s.Sessions[0].Stage == "done");
            Assert.Equal(0, await talk.Client.StopAsync());
        }

        var again = await Conversation.Open(sandbox);
        await using var restarted = again.Client;
        again.Send(new GetSettingsCommand());
        var persisted = await again.Until<PreferencesEvent>();
        Assert.Equal(
            (preferences.MaxRunning, preferences.KiroCompactAt, preferences.NoticeSeen, preferences.Tools.Single(t => t.Id == "codex")),
            (persisted.MaxRunning, persisted.KiroCompactAt, persisted.NoticeSeen, persisted.Tools.Single(t => t.Id == "codex")));

        again.Send(new ReadyCommand());
        var state = await again.Until<StateEvent>(s => s.History.Count == 1);
        Assert.Equal(key, state.History[0].Key);
        again.Send(new HistoryCommand(key));
        var transcript = await again.Until<TranscriptEvent>();
        Assert.Equal(2, transcript.Session.Turns.Count);
        Assert.Equal("Sandbox reply", transcript.Session.Turns[1].Answer);

        again.Send(new DeleteCommand(Key: key));
        await again.Until<StateEvent>(s => s.History.Count == 0);
        Assert.Equal(0, await again.Client.StopAsync());
    }

    [Fact]
    public async Task Stop_ends_a_waiting_turn()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        await talk.GetReady();

        talk.Send(new NewTaskCommand("codex", sandbox.Project, "Cancel me", "always"));
        var waiting = await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "waiting");
        talk.Send(new StopCommand(waiting.Sessions[0].Id));
        await talk.Until<StateEvent>(s => s.Sessions[0].Stage == "stopped");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Closing_stdin_stops_the_backend_and_its_child()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        await talk.GetReady();
        talk.Send(new NewTaskCommand("codex", sandbox.Project, "UI crash", "always"));
        await talk.Until<StateEvent>(s => s.Sessions.Any(x => x.Stage == "waiting"));
        var child = int.Parse(File.ReadAllText(Path.Combine(sandbox.Root, "fixture-child.pid")));
        Assert.True(IsRunning(child), "the stand-in agent's child should be running before the host goes");

        // The end of the input is how a host that died looks to the backend.
        talk.Client.CloseInput();
        Assert.Equal(0, await talk.Client.WaitForExitAsync(TimeSpan.FromSeconds(15)));
        Assert.True(await Gone(child), "the stand-in agent's child survived the host's end");
    }

    internal static bool IsRunning(int pid)
    {
        try { using var p = Process.GetProcessById(pid); return !p.HasExited; }
        catch (ArgumentException) { return false; }
    }

    internal static async Task<bool> Gone(int pid)
    {
        for (var i = 0; i < 100; i++)
        {
            if (!IsRunning(pid)) return true;
            await Task.Delay(100);
        }
        return false;
    }
}
