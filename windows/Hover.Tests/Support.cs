using System.Diagnostics;
using System.Text;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;

[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace Hover.Tests;

/// <summary>Where the programs the tests start are. A missing one fails the test: a skipped check proves nothing.</summary>
static class TestEnvironment
{
    public static string Backend => Require("HOVER_BACKEND");
    public static string DataTool => Require("HOVER_DATA_TOOL");
    public static string GoFixture => Require("HOVER_GOFIXTURE");

    /// <summary>hoverai.exe, for the checks of the app itself; null when they are not asked for.</summary>
    public static string? AppExe => Environment.GetEnvironmentVariable("HOVER_APP_EXE") is { Length: > 0 } p ? p : null;

    static string Require(string variable)
    {
        var path = Environment.GetEnvironmentVariable(variable);
        if (string.IsNullOrEmpty(path) || !File.Exists(path))
            throw new InvalidOperationException($"{variable} must name a built program (it is '{path}'). See docs/development/windows.md.");
        return path;
    }
}

/// <summary>A fact that runs on Windows only, and says so in the report where it does not.</summary>
sealed class WindowsFactAttribute : FactAttribute
{
    public WindowsFactAttribute()
    {
        if (!OperatingSystem.IsWindows()) Skip = "Windows only";
    }
}

/// <summary>How the history key is kept: DPAPI on Windows, a plain file where the Go app has no keyring.</summary>
static class KeyGuards
{
    public static IKeyGuard ForThisSystem() => OperatingSystem.IsWindows() ? new DpapiKeyGuard() : new PlainFileGuard();
}

/// <summary>The Go app's fallback where there is no Secret Service: note.key holds the 32 bytes. Not for Windows.</summary>
sealed class PlainFileGuard : IKeyGuard
{
    public byte[] Wrap(byte[] key) => key;
    public byte[] Unwrap(byte[] stored) => stored;
}

/// <summary>Programs that stand in for the tools Hover starts, found by PATH and the file's own name.</summary>
static class FakeTools
{
    static readonly Lazy<string> Folder = new(Install);

    public static string Directory => Folder.Value;

    public static string Exe(string name) => Path.Combine(Directory, name + (OperatingSystem.IsWindows() ? ".exe" : ""));

    /// <summary>
    /// Hover.TestTools, and what it needs, copied once beside each other with a file per
    /// stand-in name. A .NET program finds its dll next to itself, so the copies keep them close.
    /// </summary>
    static string Install()
    {
        var dir = System.IO.Directory.CreateTempSubdirectory("hover-fake-bin.").FullName;
        foreach (var file in System.IO.Directory.EnumerateFiles(AppContext.BaseDirectory))
        {
            var name = Path.GetFileName(file);
            if (name.StartsWith("Hover.", StringComparison.Ordinal) || name.StartsWith("System.Security.Cryptography.ProtectedData", StringComparison.Ordinal))
                File.Copy(file, Path.Combine(dir, name), overwrite: true);
        }
        var main = Path.Combine(dir, "Hover.TestTools" + (OperatingSystem.IsWindows() ? ".exe" : ""));
        if (!File.Exists(main)) throw new FileNotFoundException("Hover.TestTools was not built beside the tests.", main);
        foreach (var tool in new[] { "codex-acp", "codex" })
            File.Copy(main, Path.Combine(dir, tool + (OperatingSystem.IsWindows() ? ".exe" : "")), overwrite: true);
        return dir;
    }
}

/// <summary>
/// One test's own world, as the Mac smoke test makes one: a project folder that is a git
/// repository with a change in it, a data folder, and a PATH where the stand-in tools come first.
/// </summary>
sealed class Sandbox : IDisposable
{
    public string Root { get; } = System.IO.Directory.CreateTempSubdirectory("hover-sandbox.").FullName;
    public string Project => Path.Combine(Root, "project");
    public string Data => Path.Combine(Root, "integration-data");

    public Sandbox()
    {
        System.IO.Directory.CreateDirectory(Project);
        File.WriteAllText(Path.Combine(Project, "app.js"), "a\n");
        Run("git", "init", "-q");
        Run("git", "add", ".");
        Run("git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init");
        File.WriteAllText(Path.Combine(Project, "app.js"), "a\nb\n");
    }

    void Run(string file, params string[] args)
    {
        var result = ProcessRunner.Run(file, args, workingDirectory: Project);
        if (result.ExitCode != 0) throw new InvalidOperationException($"{file} {string.Join(' ', args)} failed: {result.Error}");
    }

    public IReadOnlyDictionary<string, string?> Environment => new Dictionary<string, string?>
    {
        ["PATH"] = FakeTools.Directory + Path.PathSeparator + System.Environment.GetEnvironmentVariable("PATH"),
        ["HOVER_SANDBOX_ROOT"] = Root,
        // A sandbox inside a sandbox is not made: the machine running the tests is the sandbox.
        ["HOVER_SANDBOXED"] = "1",
    };

    public BackendClient Start(int maxLineBytes = 64 * 1024 * 1024) => BackendClient.Start(new BackendOptions
    {
        ExecutablePath = TestEnvironment.Backend,
        DataDir = Data,
        Environment = Environment,
        MaxLineBytes = maxLineBytes,
    });

    public void Dispose()
    {
        try { System.IO.Directory.Delete(Root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }
}

readonly record struct ProcessResult(int ExitCode, string Output, string Error);

static class ProcessRunner
{
    /// <summary>Runs a program to its end and returns what it wrote.</summary>
    public static ProcessResult Run(string file, IEnumerable<string> args, IDictionary<string, string?>? environment = null,
        string? workingDirectory = null, TimeSpan? timeout = null)
    {
        var info = new ProcessStartInfo(file)
        {
            UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardOutput = true, RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (var a in args) info.ArgumentList.Add(a);
        if (workingDirectory is not null) info.WorkingDirectory = workingDirectory;
        foreach (var (name, value) in environment ?? new Dictionary<string, string?>())
        {
            if (value is null) info.Environment.Remove(name);
            else info.Environment[name] = value;
        }
        using var process = Process.Start(info)!;
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        if (!process.WaitForExit(timeout ?? TimeSpan.FromSeconds(60)))
        {
            process.Kill(entireProcessTree: true);
            throw new TimeoutException($"{file} did not finish.");
        }
        return new ProcessResult(process.ExitCode, output.Result, error.Result);
    }
}

/// <summary>Waiting for backend messages the way the Mac smoke test does: a toast is a failure.</summary>
sealed class Conversation(BackendClient client)
{
    readonly EventQueue queue = client.Subscribe();

    public BackendClient Client => client;

    public void Send(BackendCommand command) => client.Send(command);

    public async Task<T> Until<T>(Func<T, bool>? predicate = null) where T : BackendEvent
    {
        var found = await queue.Until<BackendEvent>(e =>
        {
            if (e is ToastEvent toast && typeof(T) != typeof(ToastEvent)) throw new Xunit.Sdk.XunitException("The backend sent a toast: " + toast.Text);
            return e is T match && (predicate is null || predicate(match));
        });
        return (T)found;
    }

    /// <summary>Starts a backend in the sandbox with the key the Mac test uses (bytes 0 to 31).</summary>
    public static async Task<Conversation> Open(Sandbox sandbox, byte[]? key = null)
    {
        var client = sandbox.Start();
        var talk = new Conversation(client);
        await client.InitializeAsync(key ?? Key);
        return talk;
    }

    public static readonly byte[] Key = Enumerable.Range(0, 32).Select(i => (byte)i).ToArray();

    /// <summary>Allows what the stand-in tools need to be started: the first-run notice, and the tool ready.</summary>
    public async Task GetReady()
    {
        Send(new SaveSettingsCommand { NoticeSeen = true });
        await Until<PreferencesEvent>();
        Send(new ReadyCommand());
        await Until<StateEvent>(s => s.Tools.Any(t => t.Id == "codex" && t.Ready));
    }
}
