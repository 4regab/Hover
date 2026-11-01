using System.Security.Cryptography;
using Hover.Host;
using Hover.Host.Protocol;

namespace Hover.TestTools;

/// <summary>
/// A host with no window: starts the real backend through Hover.Host, starts a task whose
/// stand-in agent has a child, writes the process ids it should be judged by, and then waits
/// to be killed. The test kills it the hard way and looks for the survivors.
/// </summary>
static class HostHarness
{
    /// <param name="args">backend exe, data folder, project folder, pid file</param>
    public static async Task<int> Run(string[] args)
    {
        var (backend, data, project, pidFile) = (args[0], args[1], args[2], args[3]);
        var root = Environment.GetEnvironmentVariable("HOVER_SANDBOX_ROOT")!;

        var client = BackendClient.Start(new BackendOptions { ExecutablePath = backend, DataDir = data });
        var events = client.Subscribe();
        await client.InitializeAsync(RandomNumberGenerator.GetBytes(32));
        client.Send(new SaveSettingsCommand { NoticeSeen = true });
        client.Send(new ReadyCommand());
        await events.Until<StateEvent>(s => s.Tools.Any(t => t.Id == "codex" && t.Ready));
        client.Send(new NewTaskCommand("codex", project, "Harness task", "always"));
        await events.Until<StateEvent>(s => s.Sessions.Any(x => x.Stage == "waiting"));

        var child = int.Parse(File.ReadAllText(Path.Combine(root, "fixture-child.pid")));
        File.WriteAllText(pidFile + ".tmp", $"{client.ProcessId}\n{child}\n");
        File.Move(pidFile + ".tmp", pidFile);
        await Task.Delay(Timeout.Infinite);
        return 0;
    }
}
