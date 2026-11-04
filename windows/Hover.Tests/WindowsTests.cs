using System.Diagnostics;
using System.Runtime.Versioning;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>The job object: what ties the backend and everything it starts to the host's life.</summary>
public class JobObjectTests
{
    static Process Sleeper() => Process.Start(new ProcessStartInfo(FakeTools.Exe("Hover.TestTools"), ["sleep", "120"])
    {
        UseShellExecute = false, CreateNoWindow = true,
    })!;

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public async Task Closing_the_job_kills_what_is_in_it_and_only_that()
    {
        using var inside = Sleeper();
        using var outside = Sleeper();
        try
        {
            using (var job = JobObject.CreateKillOnClose())
            {
                job.Assign(inside);
                Assert.False(inside.HasExited);
            }
            Assert.True(inside.WaitForExit(10_000), "the process in the job should die when the job's handle closes");
            await Task.Delay(300);
            Assert.False(outside.HasExited, "a process that was never in the job must not be touched");
        }
        finally
        {
            outside.Kill();
        }
    }

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public async Task A_child_of_a_process_in_the_job_is_in_the_job_too()
    {
        // The backend starts agents, and they start more: all must go with the host. The copy
        // has a name of its own, so the child can be found among the processes.
        var name = "hover-parent-" + Guid.NewGuid().ToString("N")[..8];
        File.Copy(FakeTools.Exe("Hover.TestTools"), FakeTools.Exe(name));
        using var parent = Process.Start(new ProcessStartInfo(FakeTools.Exe(name), ["spawn"]) { UseShellExecute = false, CreateNoWindow = true })!;
        using var job = JobObject.CreateKillOnClose();
        job.Assign(parent);

        List<Process> children = [];
        for (var i = 0; i < 100 && children.Count == 0; i++)
        {
            await Task.Delay(100);
            children = Process.GetProcessesByName(name).Where(p => p.Id != parent.Id).ToList();
        }
        Assert.NotEmpty(children);

        job.Dispose();
        Assert.True(parent.WaitForExit(10_000));
        foreach (var child in children) Assert.True(child.WaitForExit(10_000), "the child of a process in the job should die with it");
    }
}

/// <summary>The host is killed the hard way: the backend and the agent's child must be gone.</summary>
public class HostDeathTests
{
    [Fact]
    public async Task Killing_the_host_kills_the_backend_and_its_children()
    {
        using var sandbox = new Sandbox();
        var pidFile = Path.Combine(sandbox.Root, "host.pids");
        var info = new ProcessStartInfo(FakeTools.Exe("Hover.TestTools"))
        {
            UseShellExecute = false, CreateNoWindow = true,
            ArgumentList = { "host-harness", TestEnvironment.Backend, sandbox.Data, sandbox.Project, pidFile },
        };
        foreach (var (name, value) in sandbox.Environment) info.Environment[name] = value;
        using var host = Process.Start(info)!;
        try
        {
            for (var i = 0; i < 400 && !File.Exists(pidFile); i++)
            {
                Assert.False(host.HasExited, "the harness host ended before it was ready");
                await Task.Delay(100);
            }
            Assert.True(File.Exists(pidFile), "the harness host never got to the waiting task");
            var pids = File.ReadAllLines(pidFile).Select(int.Parse).ToArray();
            var (backend, child) = (pids[0], pids[1]);
            Assert.True(BackendSmokeTests.IsRunning(backend));
            Assert.True(BackendSmokeTests.IsRunning(child));

            // TerminateProcess: no chance to say goodbye, as a crash.
            host.Kill(entireProcessTree: false);
            Assert.True(host.WaitForExit(10_000));

            Assert.True(await BackendSmokeTests.Gone(backend), "the backend survived the host");
            Assert.True(await BackendSmokeTests.Gone(child), "the agent's child survived the host");
        }
        finally
        {
            if (!host.HasExited) host.Kill(entireProcessTree: true);
        }
    }
}

/// <summary>One copy of Hover per user, by the names the Go app uses.</summary>
public class InstanceLockTests
{
    [Fact]
    [SupportedOSPlatform("windows")]
    public void The_names_are_the_ones_the_Go_app_uses()
    {
        Assert.Equal(@"Local\HoverRunningInstance", InstanceLock.MutexName);
        Assert.Equal(@"Local\HoverShowApp", InstanceLock.ShowEventName);
        Assert.Equal(@"Local\HoverQuitApp", InstanceLock.QuitEventName);
    }

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public async Task A_request_to_quit_reaches_the_running_copy_and_waits_until_it_is_gone()
    {
        var id = Guid.NewGuid().ToString("N");
        var (mutex, show, quit) = ($@"Local\HoverTest-{id}", $@"Local\HoverTestShow-{id}", $@"Local\HoverTestQuit-{id}");

        // Nothing runs: there is nothing to wait for.
        Assert.True(InstanceLock.AskToQuit(TimeSpan.FromSeconds(1), mutex, quit));

        var asked = new TaskCompletionSource();
        var first = InstanceLock.Claim(() => { }, mutex, show, () => asked.TrySetResult(), quit);
        Assert.NotNull(first);
        // A copy that does not end by the timeout: false, and it was asked.
        Assert.False(InstanceLock.AskToQuit(TimeSpan.FromMilliseconds(600), mutex, quit));
        await asked.Task.WaitAsync(TimeSpan.FromSeconds(10));

        // One that ends when it is asked: true, once its mutex is released.
        first!.Dispose();
        var gone = new TaskCompletionSource();
        InstanceLock? second = null;
        second = InstanceLock.Claim(() => { }, mutex, show, () => Task.Run(() => { second!.Dispose(); gone.TrySetResult(); }), quit);
        Assert.NotNull(second);
        Assert.True(InstanceLock.AskToQuit(TimeSpan.FromSeconds(10), mutex, quit));
        await gone.Task.WaitAsync(TimeSpan.FromSeconds(10));
    }

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public async Task A_second_claim_fails_and_tells_the_first_to_open_its_dashboard()
    {
        var id = Guid.NewGuid().ToString("N");
        var (mutex, show) = ($@"Local\HoverTest-{id}", $@"Local\HoverTestShow-{id}");
        var asked = new TaskCompletionSource();

        var first = InstanceLock.Claim(() => asked.TrySetResult(), mutex, show);
        Assert.NotNull(first);

        Assert.Null(InstanceLock.Claim(() => Assert.Fail("the second copy must not run"), mutex, show));
        await asked.Task.WaitAsync(TimeSpan.FromSeconds(10));

        // Asked twice, told twice: the event is not a one-time latch.
        var again = new TaskCompletionSource();
        var second = InstanceLock.Claim(() => again.TrySetResult(), mutex, show);
        Assert.Null(second);

        first!.Dispose();
        using var next = InstanceLock.Claim(() => { }, mutex, show);
        Assert.NotNull(next);
    }
}

/// <summary>Row 6: the scheduled task of earlier versions is removed once.</summary>
public class OldServiceTests(ITestOutputHelper output)
{
    static int Schtasks(params string[] args) => ProcessRunner.Run("schtasks", args).ExitCode;

    [WindowsFact]
    [SupportedOSPlatform("windows")]
    public async Task The_task_is_removed_and_the_marker_is_written_only_when_it_is_gone()
    {
        using var temp = new TempDirectory();
        var task = "Hover Service Test " + Guid.NewGuid().ToString("N")[..8];

        Assert.Equal(0, Schtasks("/Create", "/TN", task, "/TR", "cmd.exe /c exit", "/SC", "DAILY", "/ST", "03:00", "/F"));
        try
        {
            await OldService.RemoveOnceAsync(temp.Path, task);
            Assert.NotEqual(0, Schtasks("/Query", "/TN", task));
            Assert.True(File.Exists(DataFolder.OldServiceMarker(temp.Path)));

            // With the marker there, nothing is done: a task made afterwards stays.
            Assert.Equal(0, Schtasks("/Create", "/TN", task, "/TR", "cmd.exe /c exit", "/SC", "DAILY", "/ST", "03:00", "/F"));
            await OldService.RemoveOnceAsync(temp.Path, task);
            Assert.Equal(0, Schtasks("/Query", "/TN", task));
        }
        finally
        {
            Schtasks("/Delete", "/TN", task, "/F");
        }

        // No task at all: "not installed", and that counts as gone.
        var (said, gone) = await OldService.RemoveAsync(task);
        output.WriteLine(said);
        Assert.True(gone);
        Assert.Equal("not installed", said);
    }
}

/// <summary>A fact that needs the built app, hoverai.exe, beside a hover-backend.exe.</summary>
sealed class AppFactAttribute : FactAttribute
{
    public AppFactAttribute()
    {
        if (!OperatingSystem.IsWindows()) Skip = "Windows only";
        else if (TestEnvironment.AppExe is null) Skip = "HOVER_APP_EXE is not set";
    }
}

/// <summary>hoverai.exe itself, started the way a user starts it.</summary>
public class AppTests(ITestOutputHelper output)
{
    internal static Process Launch(string dataDir, params string[] args)
    {
        var info = new ProcessStartInfo(TestEnvironment.AppExe!)
        {
            UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)!,
        };
        foreach (var a in args) info.ArgumentList.Add(a);
        info.Environment["HOVER_DATA_DIR"] = dataDir;
        return Process.Start(info)!;
    }

    internal static async Task<string> WaitForLog(string dataDir, string text, TimeSpan timeout, Process? mustRun = null)
    {
        var file = DataFolder.LogFile(dataDir);
        var end = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < end)
        {
            if (File.Exists(file))
            {
                using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                var log = await new StreamReader(stream).ReadToEndAsync();
                if (log.Contains(text)) return log;
            }
            if (mustRun is { HasExited: true }) Assert.Fail($"the app ended ({mustRun.ExitCode}) before the log said '{text}'. {Describe(dataDir)}");
            await Task.Delay(200);
        }
        Assert.Fail($"the log never said '{text}'");
        return "";
    }

    /// <summary>What the app wrote to its log, for a failure message.</summary>
    static string Describe(string dataDir)
    {
        var file = DataFolder.LogFile(dataDir);
        return File.Exists(file) ? "hover.log:\n" + File.ReadAllText(file) : "There is no hover.log.";
    }

    /// <summary>
    /// The app window is open: a visible window of the process titled "Hover". The notch is a window
    /// of the process too, so the process's "main window" can no longer say.
    /// </summary>
    [SupportedOSPlatform("windows")]
    static bool HasWindow(Process p) => Desktop.WindowOf(p, "Hover") != 0;

    [AppFact]
    [SupportedOSPlatform("windows")]
    public async Task A_second_launch_exits_and_the_first_opens_its_dashboard_and_dies_with_its_backend()
    {
        using var temp = new TempDirectory();
        using var first = Launch(temp.Path, "--background");
        try
        {
            var log = await WaitForLog(temp.Path, "agent backend is ready", TimeSpan.FromSeconds(60), first);
            output.WriteLine(log);
            Assert.DoesNotContain("styles did not load", log);
            Assert.False(HasWindow(first), "a background start has no window");
            var backendPid = int.Parse(System.Text.RegularExpressions.Regex.Match(log, @"backend started \(pid (\d+)\)").Groups[1].Value);
            Assert.True(BackendSmokeTests.IsRunning(backendPid));

            // The second launch asks and leaves.
            using var second = Launch(temp.Path);
            Assert.True(second.WaitForExit(20_000), "the second launch should exit by itself");
            Assert.Equal(0, second.ExitCode);

            log = await WaitForLog(temp.Path, "opening the dashboard", TimeSpan.FromSeconds(20), first);
            Assert.Contains("a second launch asked for the dashboard", log);
            for (var i = 0; i < 100 && !HasWindow(first); i++) await Task.Delay(100);
            Assert.True(HasWindow(first), "the first copy should have its dashboard open now");

            // Only one copy runs: the second one left no process behind.
            Assert.Single(Process.GetProcessesByName("hoverai"));

            // Killed the hard way, the app takes its backend with it.
            first.Kill(entireProcessTree: false);
            Assert.True(first.WaitForExit(10_000));
            Assert.True(await BackendSmokeTests.Gone(backendPid), "the backend survived the app");
        }
        finally
        {
            if (!first.HasExited) first.Kill(entireProcessTree: true);
        }
    }

    [AppFact]
    [SupportedOSPlatform("windows")]
    public async Task Shots_draws_the_dashboard_to_a_jpeg_no_wider_than_1400()
    {
        using var temp = new TempDirectory();
        var shots = Path.Combine(temp.Path, "shots");
        using var app = Launch(Path.Combine(temp.Path, "data"), "--shots", shots);
        // The job's own picture step waits five minutes: the pictures of the notch, the chat, the desk, Settings and the voice card take about three.
        Assert.True(app.WaitForExit(300_000), "--shots should finish by itself");
        Assert.True(app.ExitCode == 0, $"--shots exited with {app.ExitCode}. {Describe(Path.Combine(temp.Path, "data"))}");

        Assert.DoesNotContain("styles did not load", Describe(Path.Combine(temp.Path, "data")));
        // The app quits after the pictures: a frame of the panels drawn after the graphics device was disposed is logged as "frame failed".
        Assert.DoesNotContain("frame failed", Describe(Path.Combine(temp.Path, "data")));
        var picture = await File.ReadAllBytesAsync(Path.Combine(shots, "dashboard.jpg"));
        var (width, height) = Jpeg.Size(picture);
        output.WriteLine($"dashboard.jpg: {picture.Length} bytes, {width} x {height}");
        Assert.InRange(width, 200, 1400);
        Assert.InRange(height, 100, 4000);
        Assert.True(picture.Length > 2000, "a picture of a window with text is not this small");
    }
}

static class Jpeg
{
    /// <summary>The width and height from the first start-of-frame marker.</summary>
    public static (int Width, int Height) Size(byte[] jpeg)
    {
        Assert.True(jpeg is [0xFF, 0xD8, ..], "not a JPEG");
        for (var i = 2; i + 9 < jpeg.Length;)
        {
            if (jpeg[i] != 0xFF) { i++; continue; }
            var marker = jpeg[i + 1];
            if (marker is >= 0xC0 and <= 0xCF and not (0xC4 or 0xC8 or 0xCC))
                return ((jpeg[i + 7] << 8) | jpeg[i + 8], (jpeg[i + 5] << 8) | jpeg[i + 6]);
            i += 2 + ((jpeg[i + 2] << 8) | jpeg[i + 3]);
        }
        throw new InvalidDataException("no frame in the JPEG");
    }
}
