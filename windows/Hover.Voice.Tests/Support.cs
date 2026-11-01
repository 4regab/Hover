using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json.Nodes;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;

[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace Hover.Voice.Tests;

/// <summary>Where the programs the tests start are. A missing one fails the test: a skipped check proves nothing.</summary>
static class TestEnvironment
{
    public static string Backend
    {
        get
        {
            var path = Environment.GetEnvironmentVariable("HOVER_BACKEND");
            if (string.IsNullOrEmpty(path) || !File.Exists(path))
                throw new InvalidOperationException($"HOVER_BACKEND must name a built hover-backend (it is '{path}'). See docs/development/windows.md.");
            return path;
        }
    }

    /// <summary>hoverai.exe, for the checks of the app itself; null when they are not asked for.</summary>
    public static string? AppExe => Environment.GetEnvironmentVariable("HOVER_APP_EXE") is { Length: > 0 } p ? p : null;
}

/// <summary>A fact that runs on Windows only, and says so in the report where it does not.</summary>
sealed class WindowsFactAttribute : FactAttribute
{
    public WindowsFactAttribute()
    {
        if (!OperatingSystem.IsWindows()) Skip = "Windows only";
    }
}

/// <summary>Programs that stand in for the tools Hover starts (<c>codex</c>, <c>codex-acp</c>), found by PATH.</summary>
static class FakeTools
{
    static readonly Lazy<string> Folder = new(Install);

    public static string Directory => Folder.Value;

    static string Install()
    {
        var exe = OperatingSystem.IsWindows() ? ".exe" : "";
        var dir = System.IO.Directory.CreateTempSubdirectory("hover-voice-bin.").FullName;
        foreach (var file in System.IO.Directory.EnumerateFiles(AppContext.BaseDirectory))
        {
            var name = Path.GetFileName(file);
            if (name.StartsWith("Hover.", StringComparison.Ordinal) || name.StartsWith("System.Security.Cryptography.ProtectedData", StringComparison.Ordinal))
                File.Copy(file, Path.Combine(dir, name), overwrite: true);
        }
        var main = Path.Combine(dir, "Hover.TestTools" + exe);
        if (!File.Exists(main)) throw new FileNotFoundException("Hover.TestTools was not built beside the tests.", main);
        foreach (var tool in new[] { "codex-acp", "codex" }) File.Copy(main, Path.Combine(dir, tool + exe), overwrite: true);
        return dir;
    }
}

/// <summary>A fake Groq on this computer's loopback: answers every transcription with a text and keeps what it was sent.</summary>
sealed class FakeGroq : IDisposable
{
    readonly TcpListener listener = new(IPAddress.Loopback, 0);
    readonly CancellationTokenSource stop = new();
    readonly Task accepting;
    readonly object gate = new();

    /// <summary>What the next transcription answers: a status and, for 200, the words.</summary>
    public int Status { get; set; } = 200;
    public string Text { get; set; } = "fix the build";

    public string? Auth { get; private set; }
    public int WavBytes { get; private set; }
    public bool Riff { get; private set; }
    public int Transcriptions { get; private set; }

    /// <summary>HOVER_GROQ_BASE: a plain-HTTP loopback address, the only kind the backend takes.</summary>
    public string Base => $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}/openai/v1";

    public FakeGroq()
    {
        listener.Start();
        accepting = Task.Run(AcceptAsync);
    }

    async Task AcceptAsync()
    {
        try
        {
            while (!stop.IsCancellationRequested)
            {
                var client = await listener.AcceptTcpClientAsync(stop.Token);
                _ = Task.Run(() => HandleAsync(client));
            }
        }
        catch (OperationCanceledException) { }
        catch (ObjectDisposedException) { }
    }

    async Task HandleAsync(TcpClient client)
    {
        using var _ = client;
        try
        {
            var stream = client.GetStream();
            var head = new List<byte>();
            var one = new byte[1];
            while (!(head.Count >= 4 && head[^4] == '\r' && head[^3] == '\n' && head[^2] == '\r' && head[^1] == '\n'))
            {
                if (await stream.ReadAsync(one) == 0) return;
                head.Add(one[0]);
            }
            var lines = Encoding.ASCII.GetString([.. head]).Split("\r\n", StringSplitOptions.RemoveEmptyEntries);
            var request = lines[0].Split(' ');
            var length = 0;
            string? auth = null;
            foreach (var line in lines.Skip(1))
            {
                var colon = line.IndexOf(':');
                if (colon < 0) continue;
                var name = line[..colon].Trim();
                var value = line[(colon + 1)..].Trim();
                if (name.Equals("Content-Length", StringComparison.OrdinalIgnoreCase)) length = int.Parse(value);
                if (name.Equals("Authorization", StringComparison.OrdinalIgnoreCase)) auth = value;
            }
            var body = new byte[length];
            for (var got = 0; got < length;)
            {
                var n = await stream.ReadAsync(body.AsMemory(got));
                if (n == 0) return;
                got += n;
            }

            int status;
            string json;
            lock (gate)
            {
                if (request[0] == "POST" && request[1].EndsWith("/audio/transcriptions", StringComparison.Ordinal))
                {
                    Transcriptions++;
                    Auth = auth;
                    // The file part is a WAV: "RIFF....WAVE" with 44 bytes of header before the samples.
                    var at = IndexOf(body, "RIFF"u8);
                    Riff = at >= 0 && IndexOf(body, "WAVE"u8) == at + 8;
                    WavBytes = at < 0 ? 0 : EndOfFile(body) - at;
                    status = Status;
                    json = status == 200
                        ? new JsonObject { ["text"] = Text, ["language"] = "en", ["duration"] = 1.0 }.ToJsonString()
                        : """{"error":{"message":"Invalid API Key"}}""";
                }
                else if (request[0] == "GET" && request[1].EndsWith("/models", StringComparison.Ordinal))
                {
                    Auth = auth;
                    status = auth is not null && auth.Contains("gsk_fake", StringComparison.Ordinal) && Status == 200 ? 200 : 401;
                    json = status == 200 ? """{"data":[]}""" : """{"error":{"message":"Invalid API Key"}}""";
                }
                else
                {
                    status = 404;
                    json = "{}";
                }
            }
            var payload = Encoding.UTF8.GetBytes(json);
            var reply = $"HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {payload.Length}\r\nConnection: close\r\n\r\n";
            await stream.WriteAsync(Encoding.ASCII.GetBytes(reply));
            await stream.WriteAsync(payload);
            await stream.FlushAsync();
        }
        catch (Exception e) when (e is IOException or SocketException or ObjectDisposedException) { }
    }

    static int IndexOf(byte[] data, ReadOnlySpan<byte> needle) => data.AsSpan().IndexOf(needle);

    /// <summary>The file part ends at the closing boundary: the last "\r\n--" in the body.</summary>
    static int EndOfFile(byte[] body)
    {
        var end = body.AsSpan().LastIndexOf("\r\n--"u8);
        return end < 0 ? body.Length : end;
    }

    public void Dispose()
    {
        stop.Cancel();
        listener.Stop();
        try { accepting.Wait(1000); } catch (AggregateException) { }
    }
}

/// <summary>A microphone that is not there: the test says what it hears, and when.</summary>
sealed class FakeMic
{
    Action<short[]>? block;
    Action<string>? failed;

    /// <summary>When set, the next open fails with these words.</summary>
    public string? Error { get; set; }

    public List<string?> Asked { get; } = [];
    public int Stops { get; private set; }
    public bool IsOpen { get; private set; }

    public MicOpen Open(string? device, Action<short[]> onBlock, Action<string> onFailed)
    {
        Asked.Add(device);
        if (Error is not null) return new MicOpen(null, Error);
        block = onBlock;
        failed = onFailed;
        IsOpen = true;
        return new MicOpen(new Handle(this), null);
    }

    /// <summary>What the microphone hears: a block of samples, as the device hands them.</summary>
    public void Hear(short[] samples) => block!(samples);

    public void Break(string why) => failed!(why);

    /// <summary>A second of a 440 Hz tone, in blocks of 50 ms, as the real device delivers it.</summary>
    public static IEnumerable<short[]> Tone(double seconds, double amplitude = 8000)
    {
        var total = (int)(16_000 * seconds);
        for (var at = 0; at < total; at += 800)
        {
            var n = Math.Min(800, total - at);
            var block = new short[n];
            for (var i = 0; i < n; i++) block[i] = (short)(amplitude * Math.Sin(2 * Math.PI * 440 * (at + i) / 16_000.0));
            yield return block;
        }
    }

    sealed class Handle(FakeMic owner) : IMicrophone
    {
        public void Stop()
        {
            if (!owner.IsOpen) return;
            owner.IsOpen = false;
            owner.Stops++;
        }

        public void Dispose() => Stop();
    }
}

/// <summary>One test's own world: a data folder, a workspace, and a PATH where the stand-in agent comes first.</summary>
sealed class VoiceSandbox : IDisposable
{
    public string Root { get; } = System.IO.Directory.CreateTempSubdirectory("hover-voice.").FullName;
    public string Data => Path.Combine(Root, "data");
    public string Workspace => Path.Combine(Root, "workspace");

    public VoiceSandbox()
    {
        System.IO.Directory.CreateDirectory(Workspace);
    }

    public IReadOnlyDictionary<string, string?> Environment(string? groqBase) => new Dictionary<string, string?>
    {
        ["PATH"] = FakeTools.Directory + Path.PathSeparator + System.Environment.GetEnvironmentVariable("PATH"),
        ["HOVER_SANDBOX_ROOT"] = Root,
        ["HOVER_SANDBOXED"] = "1",
        ["HOVER_GROQ_BASE"] = groqBase,
    };

    public void Dispose()
    {
        try { System.IO.Directory.Delete(Root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }
}

/// <summary>
/// The host's side of voice against a real hover-backend: a client, the voice service with a fake
/// microphone, a fake Groq, and a log of every `voice` event so a test can wait for a stage it may
/// have missed.
/// </summary>
sealed class Rig : IAsyncDisposable
{
    public VoiceSandbox Box { get; } = new();
    public FakeGroq Groq { get; } = new();
    public FakeMic Mic { get; } = new();
    public BackendClient Client { get; private set; } = null!;
    public VoiceService Service { get; private set; } = null!;
    public List<VoiceSnapshot> States { get; } = [];
    public List<StateEvent> Tasks { get; } = [];
    public List<BackendEvent> Others { get; } = [];
    public const string FakeKey = "gsk_fake_key_for_tests";
    readonly object gate = new();
    readonly List<(Func<VoiceSnapshot, bool> Test, TaskCompletionSource<VoiceSnapshot> Done)> waiting = [];

    /// <summary>Starts the backend with the voice settings given as JSON members of the Voice setting.</summary>
    public static async Task<Rig> Start(JsonObject? voice = null, bool key = true)
    {
        var rig = new Rig();
        rig.Client = BackendClient.Start(new BackendOptions
        {
            ExecutablePath = TestEnvironment.Backend,
            DataDir = rig.Box.Data,
            Environment = rig.Box.Environment(rig.Groq.Base),
        });
        rig.Service = new VoiceService(rig.Client.Send, rig.Mic.Open) { PollEvery = TimeSpan.FromMilliseconds(15) };
        rig.Client.Event += rig.OnEvent;
        var events = rig.Client.Subscribe();
        await rig.Client.InitializeAsync(Enumerable.Range(0, 32).Select(i => (byte)i).ToArray());
        rig.Client.Send(new SaveSettingsCommand { NoticeSeen = true });
        rig.Client.Send(new ReadyCommand());
        await events.Until<StateEvent>(s => s.Tools.Any(t => t.Id == "codex" && t.Ready));

        // The default workspace is where a task with no project goes.
        rig.Client.Send(new RawCommand("projects", new JsonObject { ["op"] = "default", ["folder"] = rig.Box.Workspace, ["access"] = "risky", ["make"] = true }));
        await events.Until<ProjectsEvent>(p => p.Op == "default" && p.Ok);
        if (key)
        {
            rig.Client.Send(new RawCommand("secret", new JsonObject { ["op"] = "set", ["name"] = "voice.groq", ["value"] = FakeKey }));
            await events.Until<UnknownEvent>(u => u.Type == "secret");
        }
        var settings = new JsonObject { ["Enabled"] = true, ["Agent"] = "codex", ["Countdown"] = 1, ["Speech"] = "Cloud" };
        foreach (var (name, value) in voice ?? []) settings[name] = value?.DeepClone();
        rig.Client.Send(new RawCommand("setSetting", new JsonObject { ["key"] = "Voice", ["value"] = settings }));
        await events.Until<SettingsEvent>(s => s.Model.TryGetProperty("Voice", out var v) && v.TryGetProperty("Agent", out var a) && a.GetString() == "codex");
        for (var i = 0; i < 200 && rig.Service.Settings.Enabled == false; i++) await Task.Delay(10);
        return rig;
    }

    void OnEvent(BackendEvent e)
    {
        Service.OnBackend(e);
        if (e is StateEvent s)
        {
            lock (gate) Tasks.Add(s);
            return;
        }
        if (VoiceSnapshot.From(e) is { } v)
        {
            List<(Func<VoiceSnapshot, bool> Test, TaskCompletionSource<VoiceSnapshot> Done)> hit;
            lock (gate)
            {
                States.Add(v);
                hit = waiting.Where(w => w.Test(v)).ToList();
                waiting.RemoveAll(w => hit.Contains(w));
            }
            foreach (var h in hit) h.Done.TrySetResult(v);
        }
        else lock (gate) Others.Add(e);
    }

    public int Mark { get { lock (gate) return States.Count; } }

    /// <summary>The first `voice` event after <paramref name="after"/> that the test accepts.</summary>
    public async Task<VoiceSnapshot> Wait(Func<VoiceSnapshot, bool> test, int after = 0, int seconds = 25)
    {
        TaskCompletionSource<VoiceSnapshot> done = new(TaskCreationOptions.RunContinuationsAsynchronously);
        lock (gate)
        {
            var seen = States.Skip(after).FirstOrDefault(test);
            if (seen is not null) return seen;
            waiting.Add((test, done));
        }
        var winner = await Task.WhenAny(done.Task, Task.Delay(TimeSpan.FromSeconds(seconds)));
        if (winner != done.Task)
        {
            string stages;
            lock (gate) stages = string.Join(", ", States.Skip(after).Select(s => s.Stage + (s.Message.Length > 0 ? $" \"{s.Message}\"" : "")));
            throw new TimeoutException($"No such `voice` event in {seconds} s. Saw: {stages}");
        }
        return await done.Task;
    }

    public Task<VoiceSnapshot> Stage(VoiceStage stage, int after = 0) => Wait(s => s.Stage == stage, after);

    /// <summary>The newest `state` message that has a session the test wants.</summary>
    public async Task<SessionInfo> Session(int id, Func<SessionInfo, bool>? test = null, int seconds = 25)
    {
        var end = DateTime.UtcNow.AddSeconds(seconds);
        while (DateTime.UtcNow < end)
        {
            lock (gate)
            {
                var found = Tasks.AsEnumerable().Reverse().SelectMany(s => s.Sessions).FirstOrDefault(x => x.Id == id && (test is null || test(x)));
                if (found is not null) return found;
            }
            await Task.Delay(25);
        }
        throw new TimeoutException($"The backend never listed session {id} as the test wanted.");
    }

    public async ValueTask DisposeAsync()
    {
        Service.Dispose();
        try { await Client.StopAsync(TimeSpan.FromSeconds(10)); } catch (Exception) { }
        await Client.DisposeAsync();
        Groq.Dispose();
        Box.Dispose();
    }
}
