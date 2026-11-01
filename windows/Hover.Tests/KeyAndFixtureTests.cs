using System.Runtime.Versioning;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>The data folder, the history key and the sealed files, against what the Go app does with them.</summary>
public class DataFolderTests
{
    [Fact]
    public void An_override_wins_and_a_blank_one_does_not()
    {
        using var temp = new TempDirectory();
        var chosen = Path.Combine(temp.Path, "mine");
        Assert.Equal(chosen, DataFolder.Resolve(chosen, appData: Path.Combine(temp.Path, "roaming")));
        Assert.True(Directory.Exists(chosen));

        var roaming = Path.Combine(temp.Path, "roaming");
        Assert.Equal(Path.Combine(roaming, "Hover"), DataFolder.Resolve("  ", roaming));
        Assert.True(Directory.Exists(Path.Combine(roaming, "Hover")));
    }

    [Fact]
    public void The_old_Noty_folder_comes_across_once_when_there_is_no_Hover_folder()
    {
        using var temp = new TempDirectory();
        Directory.CreateDirectory(Path.Combine(temp.Path, "Noty"));
        File.WriteAllText(Path.Combine(temp.Path, "Noty", "settings.json"), "{}");

        Assert.Equal(Path.Combine(temp.Path, "Hover"), DataFolder.Resolve("", temp.Path));
        Assert.True(File.Exists(Path.Combine(temp.Path, "Hover", "settings.json")));
        Assert.False(Directory.Exists(Path.Combine(temp.Path, "Noty")));

        // With a Hover folder already, Noty is left where it is.
        Directory.CreateDirectory(Path.Combine(temp.Path, "Noty"));
        DataFolder.Resolve("", temp.Path);
        Assert.True(Directory.Exists(Path.Combine(temp.Path, "Noty")));
    }

    [Fact]
    public void The_file_names_are_the_Go_apps()
    {
        Assert.Equal(Path.Combine("d", "note.key"), DataFolder.KeyFile("d"));
        Assert.Equal(Path.Combine("d", "settings.json"), DataFolder.SettingsFile("d"));
        Assert.Equal(Path.Combine("d", "secrets.dat"), DataFolder.SecretsFile("d"));
        Assert.Equal(Path.Combine("d", "hover.log"), DataFolder.LogFile("d"));
        Assert.Equal(Path.Combine("d", "agents", "index.dat"), DataFolder.AgentsIndex("d"));
        Assert.Equal(Path.Combine("d", "old-service-removed"), DataFolder.OldServiceMarker("d"));
    }
}

/// <summary>The key rules of docs/development/windows-csharp.md, section C. (Rule 6 is the backend's, in BackendSmokeTests.)</summary>
public class HistoryKeyTests
{
    /// <summary>Wraps by reversing the bytes, and refuses a key it did not wrap when told to.</summary>
    sealed class FakeGuard : IKeyGuard
    {
        public bool Refuse { get; set; }
        public byte[] Wrap(byte[] key) => Enumerable.Reverse(key).ToArray();
        public byte[] Unwrap(byte[] stored) => Refuse
            ? throw new CryptographicException("Key not valid for use in specified state.")
            : Enumerable.Reverse(stored).ToArray();
    }

    static string[] SetAside(string dir) => Directory.GetFiles(dir, "note.key.unreadable-*");

    [Fact]
    public void Rule_1_a_key_that_opens_to_32_bytes_is_used()
    {
        using var temp = new TempDirectory();
        var guard = new FakeGuard();
        var first = HistoryKey.LoadOrCreate(temp.Path, guard)!;
        Assert.Equal(32, first.Length);
        var stored = File.ReadAllBytes(DataFolder.KeyFile(temp.Path));
        Assert.Equal(Enumerable.Reverse(first).ToArray(), stored);

        Assert.Equal(first, HistoryKey.LoadOrCreate(temp.Path, guard));
        Assert.Equal(stored, File.ReadAllBytes(DataFolder.KeyFile(temp.Path)));
        Assert.Empty(SetAside(temp.Path));
        Assert.Empty(Directory.GetFiles(temp.Path, "*.tmp"));
    }

    [Fact]
    public void Rule_2_a_key_of_another_length_is_set_aside_and_a_new_one_made()
    {
        using var temp = new TempDirectory();
        var file = DataFolder.KeyFile(temp.Path);
        File.WriteAllBytes(file, new byte[16]);

        var key = HistoryKey.LoadOrCreate(temp.Path, new FakeGuard())!;
        Assert.Equal(32, key.Length);
        Assert.Equal(new byte[16], File.ReadAllBytes(Assert.Single(SetAside(temp.Path))));
        Assert.Equal(Enumerable.Reverse(key).ToArray(), File.ReadAllBytes(file));
    }

    [Fact]
    public void Rule_3_a_key_the_system_refuses_is_set_aside_with_its_bytes_and_a_new_one_made()
    {
        using var temp = new TempDirectory();
        var file = DataFolder.KeyFile(temp.Path);
        var stored = RandomNumberGenerator.GetBytes(48);
        File.WriteAllBytes(file, stored);
        var guard = new FakeGuard { Refuse = true };

        var key = HistoryKey.LoadOrCreate(temp.Path, guard)!;
        var aside = Assert.Single(SetAside(temp.Path));
        Assert.Matches(@"note\.key\.unreadable-\d{14}$", aside);
        Assert.Equal(stored, File.ReadAllBytes(aside));

        // The new key is the one now in note.key (the guard opens it once it stops refusing).
        guard.Refuse = false;
        Assert.Equal(key, HistoryKey.LoadOrCreate(temp.Path, guard));
    }

    [Fact]
    public void Rule_4_a_key_that_cannot_be_read_now_means_no_key_and_the_file_stays()
    {
        using var temp = new TempDirectory();
        var file = DataFolder.KeyFile(temp.Path);
        var stored = RandomNumberGenerator.GetBytes(32);
        File.WriteAllBytes(file, stored);

        using (new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
        {
            Assert.Null(HistoryKey.LoadOrCreate(temp.Path, new PlainFileGuard()));
        }
        Assert.Equal(stored, File.ReadAllBytes(file));
        Assert.Empty(SetAside(temp.Path));
        Assert.Equal([file], Directory.GetFiles(temp.Path));
    }

    [Fact]
    public void Rule_5_with_no_key_file_a_new_key_is_made_and_an_existing_history_is_not_touched()
    {
        using var temp = new TempDirectory();
        Directory.CreateDirectory(Path.Combine(temp.Path, "agents"));
        var index = DataFolder.AgentsIndex(temp.Path);
        File.WriteAllBytes(index, [1, 2, 3]);

        var key = HistoryKey.LoadOrCreate(temp.Path, KeyGuards.ForThisSystem());
        Assert.Equal(32, key!.Length);
        Assert.True(File.Exists(DataFolder.KeyFile(temp.Path)));
        Assert.Equal<byte>([1, 2, 3], File.ReadAllBytes(index));
    }

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public void DPAPI_wraps_for_this_user_and_gives_the_key_back()
    {
        var guard = new DpapiKeyGuard();
        var key = RandomNumberGenerator.GetBytes(32);
        var wrapped = guard.Wrap(key);
        Assert.NotEqual(key, wrapped);
        Assert.True(wrapped.Length > 32);
        Assert.Equal(key, guard.Unwrap(wrapped));
        Assert.ThrowsAny<CryptographicException>(() => guard.Unwrap([1, 2, 3, 4]));
    }
}

public class SealedTests
{
    [Fact]
    public void Sealed_text_opens_with_its_key_and_only_with_it()
    {
        var key = RandomNumberGenerator.GetBytes(32);
        var sealedText = Sealed.Seal(key, "héllo \u2713");
        Assert.Equal(12 + Encoding.UTF8.GetByteCount("héllo \u2713") + 16, sealedText.Length);
        Assert.Equal("héllo \u2713", Sealed.Open(key, sealedText));
        Assert.Null(Sealed.Open(RandomNumberGenerator.GetBytes(32), sealedText));
        Assert.Null(Sealed.Open(key, sealedText.AsSpan(0, 20)));
        sealedText[^1] ^= 1;
        Assert.Null(Sealed.Open(key, sealedText));
    }
}

/// <summary>
/// The folder the Go app writes, read by the C# host, and the folder the C# host writes,
/// read by the Go app. On Windows the key is DPAPI-wrapped on both sides, so each side opens
/// the other's note.key; elsewhere it is the Go app's plain-file fallback.
/// </summary>
public class GoCompatibilityTests(ITestOutputHelper output)
{
    static Dictionary<string, string?> DataEnv(string data) => new() { ["HOVER_DATA_DIR"] = data };

    static string GoSecret(string data, params string[] args)
    {
        var result = ProcessRunner.Run(TestEnvironment.GoFixture, args, DataEnv(data));
        Assert.True(result.ExitCode == 0, $"gofixture {args[0]} failed ({result.ExitCode}): {result.Error}");
        return result.Output.TrimEnd('\r', '\n');
    }

    /// <summary>secrets.dat, opened by the test's own sealing code: a JSON object of name to value.</summary>
    static string? SecretOf(string data, byte[] key, string name)
    {
        var plain = Sealed.Open(key, File.ReadAllBytes(DataFolder.SecretsFile(data)));
        return plain is null ? null : JsonDocument.Parse(plain).RootElement.GetProperty(name).GetString();
    }

    static void Copy(string from, string to)
    {
        Directory.CreateDirectory(to);
        foreach (var file in Directory.GetFiles(from)) File.Copy(file, Path.Combine(to, Path.GetFileName(file)));
        foreach (var dir in Directory.GetDirectories(from)) Copy(dir, Path.Combine(to, Path.GetFileName(dir)));
    }

    [Fact]
    public async Task The_folder_the_Go_app_wrote_is_read_by_the_C_sharp_host()
    {
        // Made in this job by cmd/hover-data write (settings, note.key, one sealed session of two turns).
        var made = Environment.GetEnvironmentVariable("HOVER_FIXTURE_GO");
        Assert.True(Directory.Exists(made), "HOVER_FIXTURE_GO must name the folder hover-data write made (see docs/development/windows.md).");
        using var temp = new TempDirectory();
        var data = Path.Combine(temp.Path, "go-data");
        Copy(made!, data);
        var answer = (await File.ReadAllTextAsync(Path.Combine(data, "answer.md"))).Trim();
        var settingsBefore = File.ReadAllBytes(DataFolder.SettingsFile(data));

        // The host opens note.key (DPAPI on Windows) and gets the key the Go app sealed with.
        var key = HistoryKey.LoadOrCreate(data, KeyGuards.ForThisSystem())!;
        Assert.Equal(32, key.Length);
        Assert.Empty(Directory.GetFiles(data, "note.key.unreadable-*"));

        // The history, opened by this test's own sealing code: the index and the session file.
        var index = JsonDocument.Parse(Sealed.Open(key, File.ReadAllBytes(DataFolder.AgentsIndex(data)))!).RootElement;
        Assert.Equal(1, index.GetArrayLength());
        var sessionKey = index[0].GetProperty("Key").GetString()!;
        var sealedSession = Sealed.Open(key, File.ReadAllBytes(Path.Combine(data, "agents", sessionKey + ".dat")))!;
        var session = JsonDocument.Parse(sealedSession).RootElement;
        Assert.Equal("A long rich conversation", session.GetProperty("Title").GetString());
        Assert.Equal(2, session.GetProperty("Turns").GetArrayLength());
        output.WriteLine($"C# opened the Go history {sessionKey[..8]}… with the key from note.key");

        // A secret written by the Go code, opened by this test.
        GoSecret(data, "secrets-set", "voice.groq", "gsk_from_go_\u00e9");
        Assert.Equal("gsk_from_go_\u00e9", SecretOf(data, key, "voice.groq"));

        // The Go backend, started by the C# host with that key: a run that is asked for nothing
        // leaves settings.json as the same bytes. (A getSettings fills in the notch items, as
        // it does for the Mac app, and the file is then written with them.)
        await using (var first = BackendClient.Start(new BackendOptions { ExecutablePath = TestEnvironment.Backend, DataDir = data }))
        {
            await first.InitializeAsync(key);
            Assert.Equal(0, await first.StopAsync());
        }
        Assert.Equal(settingsBefore, File.ReadAllBytes(DataFolder.SettingsFile(data)));

        // A second run lists the history and gives the transcript.
        await using (var second = BackendClient.Start(new BackendOptions { ExecutablePath = TestEnvironment.Backend, DataDir = data }))
        {
            var events = second.Subscribe();
            await second.InitializeAsync(key);
            second.Send(new ReadyCommand());
            var state = await events.Until<StateEvent>(s => s.History.Count == 1);
            Assert.Equal((sessionKey, "A long rich conversation", 2), (state.History[0].Key, state.History[0].Title, state.History[0].Turns));
            second.Send(new HistoryCommand(sessionKey));
            var transcript = await events.Until<TranscriptEvent>();
            Assert.Equal(2, transcript.Session.Turns.Count);
            Assert.Equal(answer, transcript.Session.Turns[0].Answer.Trim());
            output.WriteLine("The Go backend started by the C# host listed the Go session and gave its transcript.");
            Assert.Equal(0, await second.StopAsync());
        }

        // The Go tool still reads everything, and would save every file as the bytes it is.
        var check = ProcessRunner.Run(TestEnvironment.DataTool, ["check", data], DataEnv(data));
        output.WriteLine(check.Output);
        Assert.True(check.ExitCode == 0, check.Output + check.Error);
    }

    [Fact]
    public async Task The_folder_the_C_sharp_host_wrote_is_read_by_the_Go_app()
    {
        using var sandbox = new Sandbox();
        var data = sandbox.Data;
        Directory.CreateDirectory(data);

        // The host makes note.key; the Go backend it starts writes the settings and a sealed
        // history from a task of the stand-in agent.
        var key = HistoryKey.LoadOrCreate(data, KeyGuards.ForThisSystem())!;
        var talk = await Conversation.Open(sandbox, key);
        await using (talk.Client)
        {
            await talk.GetReady();
            talk.Send(new NewTaskCommand("codex", sandbox.Project, "A task from the C# host", "always"));
            var waiting = await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "waiting");
            talk.Send(new AnswerCommand(waiting.Sessions[0].Id, waiting.Sessions[0].Ask!.Id, "allow"));
            await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "done");
            Assert.Equal(0, await talk.Client.StopAsync());
        }

        // A secret sealed by the test's own code with that key.
        File.WriteAllBytes(DataFolder.SecretsFile(data), Sealed.Seal(key, """{"voice.groq":"gsk_from_csharp_\u00e9"}"""));

        // Now the Go side: its own key reading (DPAPI on Windows) opens the history, and every
        // file would be saved back as the bytes it is.
        var dump = ProcessRunner.Run(TestEnvironment.DataTool, ["dump", data], DataEnv(data));
        output.WriteLine(dump.Output);
        Assert.Contains("history: 1 sessions", dump.Output);
        Assert.Contains("A task from the C# host", dump.Output);
        var check = ProcessRunner.Run(TestEnvironment.DataTool, ["check", data], DataEnv(data));
        output.WriteLine(check.Output);
        Assert.True(check.ExitCode == 0, check.Output + check.Error);
        Assert.Equal("gsk_from_csharp_\u00e9", GoSecret(data, "secrets-get", "voice.groq"));
    }
}

sealed class TempDirectory : IDisposable
{
    public string Path { get; } = Directory.CreateTempSubdirectory("hover-test.").FullName;

    public void Dispose()
    {
        try { Directory.Delete(Path, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }
}
