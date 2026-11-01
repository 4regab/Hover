using Hover.TestTools;

// One program, several jobs. The tests copy this exe under the names of the tools Hover
// starts (codex-acp, codex), so the first word below picks the job, and with none the file's
// own name does.
//
//   sleep <seconds>                               the child that the stand-in agent leaves behind
//   spawn                                         starts a sleep child of its own and waits (a process with a child)
//   fake-backend                                  a backend that misbehaves when told to
//   host-harness <backend> <data> <project> <pid> a host to be killed
//   codex-acp / codex                             the stand-in agent / its login check
var name = Path.GetFileNameWithoutExtension(Environment.ProcessPath)!.ToLowerInvariant();
var job = args.Length > 0 && args[0] is "sleep" or "spawn" or "fake-backend" or "host-harness" ? args[0] : name;

switch (job)
{
    case "sleep":
        Thread.Sleep(TimeSpan.FromSeconds(double.Parse(args[1], System.Globalization.CultureInfo.InvariantCulture)));
        return 0;
    case "spawn":
        using (System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(Environment.ProcessPath!, ["sleep", "120"]) { UseShellExecute = false, CreateNoWindow = true })!)
            Thread.Sleep(TimeSpan.FromSeconds(120));
        return 0;
    case "fake-backend":
        return FakeBackend.Run();
    case "host-harness":
        return await HostHarness.Run(args[1..]);
    case "codex":
        // `codex login status`, which Hover runs to see whether the tool is signed in.
        Console.WriteLine("Logged in (sandbox fixture)");
        return 0;
    case "codex-acp":
        return AcpAgent.Run();
    default:
        Console.Error.WriteLine($"Hover.TestTools: nothing to do for '{job}'.");
        return 2;
}
