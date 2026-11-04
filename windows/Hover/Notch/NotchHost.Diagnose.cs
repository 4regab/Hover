using System.Diagnostics;
using Hover.Chatting;
using Hover.Host;
using Hover.Notch;
using static Hover.Windowing.Native;

namespace Hover.Windowing;

/// <summary>
/// For the checks of the real app (<c>HOVER_UI_DUMP</c>): lines in the log that say why the open notch has no panels.
/// Without the switch nothing here writes anything.
/// </summary>
sealed partial class NotchHost
{
    double diagAt, lastFrameEndedAt = -1;

    /// <summary>A frame that took long is named, with the scene it drew and the one the clock has moved on to.</summary>
    void DiagFrame(long began)
    {
        var ms = Stopwatch.GetElapsedTime(began).TotalMilliseconds;
        lastFrameEndedAt = Now;
        if (ms < 80 || ctl.State == NotchState.Rest) return;
        HostLog.Line($"notch: a frame took {ms:0} ms; it drew openness {scene.Openness:0.000} and view opacity {scene.ViewOpacity:0.00}, the clock is at {ctl.OpennessNow:0.000} and {ctl.CurrentFrame.ViewOpacity:0.00} now");
    }

    /// <summary>The frame timer stops: the last frame drawn is the last one until something asks for another.</summary>
    void DiagStopped()
    {
        if (!UiDump.On || ctl.State == NotchState.Rest) return;
        HostLog.Line($"notch: frame timer stopped; the last frame drew openness {scene.Openness:0.000} and view opacity {scene.ViewOpacity:0.00}, the clock is at {ctl.OpennessNow:0.000} and {ctl.CurrentFrame.ViewOpacity:0.00} now, panels open {PanelsOpen}");
    }

    /// <summary>Once a second, while the notch is open and the panels are not: every term of <see cref="PanelsOpen"/> and what is around it.</summary>
    void DiagWatch()
    {
        if (!UiDump.On || ctl.State == NotchState.Rest || PanelsOpen || Now - diagAt < 1000) return;
        diagAt = Now;
        var (cx, cy) = window.Cursor();
        var ago = lastFrameEndedAt < 0 ? "never" : $"{Now - lastFrameEndedAt:0} ms ago";
        HostLog.Line($"notch: no panels - panels {(panels is null ? "missing" : "made")}, Settings wanted {CfgWanted}, state {ctl.State}, " +
            $"view shown {scene.ViewVisible}, drawn at opacity {scene.ViewOpacity:0.00} (clock {ctl.CurrentFrame.ViewOpacity:0.00}), openness {scene.Openness:0.000} (clock {ctl.OpennessNow:0.000}); " +
            $"frame {FrameMs:0} ms, timer {(animTimer.IsRunning ? "running" : "stopped")}, moving {Moving}, render queued {drawQueued}, last frame ended {ago}; " +
            $"office: {office?.Diagnose() ?? "none"}; keyboard: {Describe(GetForegroundWindow())}; under the pointer ({cx},{cy}): {Describe(WindowFromPoint(new POINT { X = cx, Y = cy }))}");
    }

    string Describe(nint hwnd)
    {
        if (hwnd == 0) return "no window";
        var text = new char[128];
        var n = GetClassNameW(hwnd, text, text.Length);
        var ours = hwnd == window.Handle || GetAncestor(hwnd, GA_ROOTOWNER) == window.Handle;
        return $"0x{hwnd:X} {new string(text, 0, Math.Max(n, 0))}{(ours ? " (the notch's)" : "")}";
    }
}
