using Avalonia.Threading;
using HoverAvalonia.Core;
using HoverAvalonia.Ui;

namespace HoverAvalonia;

/// <summary>HOVER_BENCH=1: the same line channel on stdin/stdout as Hover's bench.rs, so tools/hover-measure's scenario runner drives this
/// prototype with the same scripts. Only the commands the comparison needs; `task`/`until-idle`/`chat` are acknowledged no-ops because the
/// simulated agents are already in the scene.</summary>
static class Bench
{
    static NotchWindow? w; static double waitStart = -1; static string waitWhat = "";
    public static void Start(NotchWindow win)
    {
        w = win;
        new Thread(() =>
        {
            string? line;
            while ((line = Console.In.ReadLine()) != null)
            {
                var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries); if (parts.Length == 0) continue;
                var a = parts;
                Dispatcher.UIThread.Post(() => Run(a), DispatcherPriority.Send);
            }
        }) { IsBackground = true, Name = "bench-stdin" }.Start();
    }

    public static void FirstFrame() { if (waitStart >= 0) { Console.WriteLine($"bench {waitWhat} {w!.Now - waitStart:F1}"); waitStart = -1; } }

    static void Run(string[] p)
    {
        var nw = w!;
        switch (p[0])
        {
            case "unfold": if (nw.State == HoverAvalonia.Core.NotchGeometry.State.Rest) { waitStart = nw.Now; waitWhat = "reopen"; nw.Expand(false, false); Console.WriteLine("bench unfolded"); } else Console.WriteLine("bench unfolded already"); break;
            case "toggle": { bool opening = nw.State == HoverAvalonia.Core.NotchGeometry.State.Rest; if (opening) { waitStart = nw.Now; waitWhat = "reopen"; } nw.Toggle(); Console.WriteLine($"bench toggled {(opening ? "open" : "rest")}"); break; }
            case "fold": nw.Collapse(); Console.WriteLine("bench folded"); break;
            case "frames":
            {
                double secs = p.Length > 1 ? double.Parse(p[1]) : 10; var oc = nw.OfficeCtl;
                var st = oc?.Stamps ?? []; double now = oc?.ClockMs ?? 0; var recent = st.Where(t => now - t <= secs * 1000).ToList();
                var iv = recent.Zip(recent.Skip(1), (x, y) => y - x).OrderBy(x => x).ToList();
                double Pc(double q) => iv.Count == 0 ? 0 : iv[(int)Math.Round((iv.Count - 1) * q)];
                Console.WriteLine($"bench frames {recent.Count} {Pc(.5):F1} {Pc(.95):F1} {Pc(.99):F1} {oc?.Rendered ?? 0}"); break;
            }
            case "task": Console.WriteLine("bench task"); break;
            case "until-idle": Console.WriteLine("bench idle"); break;
            case "chat" or "chat-last" or "chat-nth": if (nw.Scene.Sessions.Count > 0) { nw.Scene.Sel = 1; } Console.WriteLine("bench ok"); break;
            case "size": Console.WriteLine("bench size default 1120x440"); break;
            case "state": Console.WriteLine($"bench state {nw.State} {nw.OpennessNow:F2}"); break;
            case "quit": (Avalonia.Application.Current!.ApplicationLifetime as Avalonia.Controls.ApplicationLifetimes.IClassicDesktopStyleApplicationLifetime)!.Shutdown(); break;
            default: Console.WriteLine("bench unknown"); break;
        }
    }
}
