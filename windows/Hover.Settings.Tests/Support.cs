using System.Security.Cryptography;
using System.Text.Json;
using Hover.Chat;
using Hover.Chat.Ui;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;

[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace Hover.Settings.Tests;

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

    /// <summary>hover-data, the Go tool that checks a data folder; null when it is not given.</summary>
    public static string? DataTool => Environment.GetEnvironmentVariable("HOVER_DATA_TOOL") is { Length: > 0 } p && File.Exists(p) ? p : null;
}

/// <summary>A fact that runs on Windows only, and says so in the report where it does not.</summary>
sealed class WindowsFactAttribute : FactAttribute
{
    public WindowsFactAttribute()
    {
        if (!OperatingSystem.IsWindows()) Skip = "Windows only";
    }
}

/// <summary>Go's own <c>hover-data</c>, which reads a folder the way the Go app does (Windows: the key in note.key is wrapped for the user).</summary>
static class GoTool
{
    static string Tool => TestEnvironment.DataTool ?? throw new InvalidOperationException("HOVER_DATA_TOOL must name a built hover-data. See docs/development/windows.md.");

    /// <summary>The tool's output, whatever its exit code.</summary>
    public static string Run(params string[] args)
    {
        var p = System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Tool, args) { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false })!;
        var text = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
        p.WaitForExit();
        return text;
    }

    /// <summary><c>check</c> reads every file of the folder and would save the same bytes; it exits 0.</summary>
    public static void Check(string dir)
    {
        var p = System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Tool, ["check", dir]) { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false })!;
        var text = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
        p.WaitForExit();
        Assert.True(p.ExitCode == 0, "hover-data check said: " + text);
    }
}

sealed class TempDir : IDisposable
{
    public string Path { get; } = Directory.CreateTempSubdirectory("hover-settings.").FullName;

    public void Dispose()
    {
        try { Directory.Delete(Path, recursive: true); }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}

// MARK: A shaper with fixed-width letters

/// <summary>Letters half the font size wide, wrapped at spaces, so the painter's layout runs on any system without DirectWrite.</summary>
sealed class FixedShaper : IShaper
{
    public ShapedText Shape(IReadOnlyList<Span> spans, Look look, float width, Align align)
    {
        var (text, runs, links) = Runs.Build(spans, look);
        return new ShapedText(new FixedLayout(text, runs, width), text, links, runs);
    }

    public (float Min, float Max) Widths(IReadOnlyList<Span> spans, Look look)
    {
        var (text, runs, _) = Runs.Build(spans, look);
        var l = new FixedLayout(text, runs, 0);
        return (l.Width, l.Width);
    }
}

sealed class FixedLayout : IParaLayout
{
    readonly string text;
    readonly float size, lineH;
    readonly List<LineBox> lines = [];

    public FixedLayout(string text, IReadOnlyList<StyleRun> runs, float width)
    {
        this.text = text;
        size = runs.Count > 0 ? runs[0].Size : 12;
        lineH = size * 1.3f;
        var advance = size * 0.5f;
        var pos = 0;
        float top = 0, widest = 0;
        while (true)
        {
            int start = pos, end = start, lastSpace = -1;
            var hard = false;
            while (end < text.Length)
            {
                if (text[end] == '\n') { hard = true; break; }
                if (width > 0 && (end - start + 1) * advance > width && end > start)
                {
                    if (lastSpace >= start) end = lastSpace + 1;
                    break;
                }
                if (text[end] == ' ') lastSpace = end;
                end++;
            }
            lines.Add(new LineBox(start, end, top, lineH, lineH * 0.8f));
            widest = Math.Max(widest, (end - start) * advance);
            top += lineH;
            pos = hard ? end + 1 : end;
            if (!hard && end >= text.Length) break;
            if (hard && pos >= text.Length)
            {
                lines.Add(new LineBox(pos, pos, top, lineH, lineH * 0.8f));
                top += lineH;
                break;
            }
        }
        Width = widest;
        Height = top;
    }

    public float Width { get; }
    public float Height { get; }
    public IReadOnlyList<LineBox> Lines => lines;
    float Advance => size * 0.5f;

    int LineAt(float y)
    {
        for (var i = 0; i < lines.Count; i++) if (y < lines[i].Top + lines[i].Height) return i;
        return lines.Count - 1;
    }

    public float SnapX(float x) => (float)Math.Floor(x / Advance) * Advance;

    public int IndexAt(float x, float y)
    {
        var l = lines[LineAt(y)];
        var i = l.Start + (int)Math.Round(x / Advance);
        return Math.Clamp(i, l.Start, l.End);
    }

    public int? ClusterAt(float x, float y)
    {
        if (y < 0 || y >= Height) return null;
        var l = lines[LineAt(y)];
        var i = l.Start + (int)Math.Floor(x / Advance);
        return i >= l.Start && i < l.End ? i : null;
    }

    public bool SoftLineEnd(int index) => lines.Any(l => l.End == index && index < text.Length && text[index] != '\n');

    public IReadOnlyList<RectF> SelectionRects(int start, int end)
    {
        var rects = new List<RectF>();
        foreach (var l in lines)
        {
            int a = Math.Max(start, l.Start), z = Math.Min(end, l.End);
            if (z > a) rects.Add(new RectF((a - l.Start) * Advance, l.Top, (z - l.Start) * Advance, l.Top + l.Height));
        }
        return rects;
    }

    public (float X, float Y, float H) CaretAt(int index)
    {
        foreach (var l in lines)
            if (index >= l.Start && index <= l.End) return ((index - l.Start) * Advance, l.Top, l.Height);
        var last = lines[^1];
        return ((last.End - last.Start) * Advance, last.Top, last.Height);
    }
}

// MARK: A canvas that writes down what it was asked

sealed class RecordingCanvas : ICanvas
{
    public readonly List<(Rect4 R, Rgba C)> Fills = [];
    public int Texts;
    public int Svgs;
    public readonly Stack<float> Opacity = new();
    public float Dimmed;

    public void PushClip(Rect4 r) { }
    public void PopClip() { }
    public void PushMove(float dx, float dy) { }
    public void PopMove() { }
    public void PushOpacity(float opacity) { }
    public void PopOpacity() { }
    public void Fill(Rect4 r, float[] radius, Rgba c) => Fills.Add((r, c));
    public void Stroke(Rect4 r, float[] radius, float width, Rgba c) { }
    public void Line(IReadOnlyList<(float X, float Y)> points, float width, Rgba c) { }
    public void Polygon(IReadOnlyList<(float X, float Y)> points, Rgba c) { }
    public void Glow(float cx, float cy, float radius, Rgba c) { }
    public void Text(IParaLayout layout, IReadOnlyList<StyleRun> runs, float x, float y) => Texts++;
    public void Svg(string svg, Rect4 r) => Svgs++;
    public void Image(string src, Rect4 r, float radius, bool cover) { }
    public void BrokenImage(float x, float y) { }
    public void Path(IReadOnlyList<Hover.Notch.PathCmd> path, Rgba? fill, Rgba? stroke, float strokeWidth, bool evenOdd, float[]? dash) { }
    public void Label(string text, float x, float y, float size, float weight, Rgba c, Align anchor, FontFamily family) { }
}

sealed class Clip : IClipboard
{
    public string? Held;
    public string? Text() => Held;
    public void Set(string text) => Held = text;
}

// MARK: The app's side of Settings, written down

sealed class FakeEnv : ISettingsEnv
{
    public readonly List<BackendCommand> Sent = [];
    public Action<BackendCommand>? Forward;
    public bool Login;
    public bool Dark = true;
    public string? NextFolder;
    public string? NextThemeFile;
    public bool Quit_;
    public string? Opened;
    public int TrialDown, TrialUp;
    public IReadOnlyList<string> Mics = ["Microphone (USB)", "Line In"];

    public void Send(BackendCommand command)
    {
        lock (Sent) Sent.Add(command);
        Forward?.Invoke(command);
    }

    public bool LaunchAtLogin() => Login;
    public void SetLaunchAtLogin(bool on) => Login = on;
    public bool SystemDark() => Dark;
    public void Quit() => Quit_ = true;
    public void ChooseFolder(Action<string?> done) => done(NextFolder);
    public void ChooseThemeFile(Action<string?> done) => done(NextThemeFile);
    public void OpenInEditor(string path) => Opened = path;
    public IReadOnlyList<string> Microphones() => Mics;
    public void TrialPress() => TrialDown++;
    public void TrialRelease() => TrialUp++;

    public IEnumerable<T> Of<T>() where T : BackendCommand
    {
        lock (Sent) return Sent.OfType<T>().ToList();
    }

    public string[] Lines()
    {
        lock (Sent) return Sent.Select(c => System.Text.Encoding.UTF8.GetString(ProtocolJson.Serialize(c))).ToArray();
    }
}

// MARK: The controller on a real backend

/// <summary>The settings controller wired to a real hover-backend in a data folder of its own, as the app wires it.</summary>
sealed class Rig : IAsyncDisposable
{
    readonly object gate = new();
    BackendClient? client;
    public SettingsController Ctl { get; }
    public FakeEnv Env { get; } = new();
    public string Dir { get; }
    public byte[] Key { get; }
    public List<BackendEvent> Seen { get; } = [];

    Rig(string dir, byte[] key)
    {
        Dir = dir;
        Key = key;
        Ctl = new SettingsController(Env);
    }

    /// <summary>
    /// The key a test hands the backend. On Windows it is the one in <c>note.key</c>, wrapped for the user as the app makes it,
    /// so that Go's own tool can open the folder afterwards; elsewhere it is a new one.
    /// </summary>
    public static byte[] KeyFor(string dir)
    {
        Directory.CreateDirectory(dir);
        return OperatingSystem.IsWindows() ? Hover.Host.HistoryKey.LoadOrCreate(dir, new Hover.Host.DpapiKeyGuard())! : RandomNumberGenerator.GetBytes(32);
    }

    public static async Task<Rig> StartAsync(string dir, byte[]? key = null, Action<string>? prepare = null, string? home = null)
    {
        key ??= RandomNumberGenerator.GetBytes(32);
        prepare?.Invoke(dir);
        var rig = new Rig(dir, key);
        // The editors' themes are found under the home folder: a test can give the backend one of its own.
        var env = home is null ? new Dictionary<string, string?>() : new Dictionary<string, string?> { ["HOME"] = home, ["USERPROFILE"] = home };
        rig.client = BackendClient.Start(new BackendOptions { ExecutablePath = TestEnvironment.Backend, DataDir = dir, Environment = env });
        rig.client.Event += e => { lock (rig.gate) { rig.Seen.Add(e); rig.Ctl.Accept(e); } };
        rig.Env.Forward = c => rig.client!.Send(c);
        await rig.client.InitializeAsync(key, credits: true, hostDiscord: true);
        lock (rig.gate) rig.Ctl.Start();
        rig.client.Send(new ReadyCommand());
        await rig.Until(c => c.Data.Appearance != "" && rig.Seen.OfType<SettingsEvent>().Any(), "the settings");
        return rig;
    }

    public T Read<T>(Func<SettingsController, T> f)
    {
        lock (gate) return f(Ctl);
    }

    public void Do(Action<SettingsController> act)
    {
        lock (gate) act(Ctl);
    }

    public async Task Until(Func<SettingsController, bool> condition, string what, int ms = 20000)
    {
        var end = DateTime.UtcNow.AddMilliseconds(ms);
        while (DateTime.UtcNow < end)
        {
            lock (gate) if (condition(Ctl)) return;
            await Task.Delay(25);
        }
        string seen;
        lock (gate) seen = string.Join(", ", Seen.TakeLast(12).Select(e => e.GetType().Name));
        throw new Xunit.Sdk.XunitException($"Timed out waiting for {what}. The last messages: {seen}");
    }

    /// <summary>The orderly end: the backend writes the settings and the history, then exits.</summary>
    public async Task StopAsync()
    {
        if (client is null) return;
        await client.StopAsync(TimeSpan.FromSeconds(20));
        await client.DisposeAsync();
        client = null;
    }

    public async ValueTask DisposeAsync() => await StopAsync();
}

static class Json
{
    public static JsonElement Parse(string text)
    {
        using var doc = JsonDocument.Parse(text);
        return doc.RootElement.Clone();
    }
}
