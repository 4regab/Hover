using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text.Json.Nodes;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The pointer's shape in the real app, read from the system (GetCursorInfo) with the pointer put on places the panels drew: the hand over a
/// button, the text cursor over a box of words, the resize cursor on the chat list's edge, the normal shape over a plain place. Once in the
/// app window (the overlay's cursor) and once in the open notch (its WM_SETCURSOR).
/// </summary>
[SupportedOSPlatform("windows")]
public class PointerShapeAppTests(ITestOutputHelper output)
{
    [StructLayout(LayoutKind.Sequential)] struct CURSORINFO { public int cbSize, flags; public nint hCursor; public int x, y; }

    [DllImport("user32.dll")] static extern bool GetCursorInfo(ref CURSORINFO info);
    [DllImport("user32.dll")] static extern nint LoadCursorW(nint instance, nint name);

    const int Normal = 32512, Text = 32513, ResizeColumn = 32644, Hand = 32649;

    static nint Shape() { var info = new CURSORINFO { cbSize = Marshal.SizeOf<CURSORINFO>() }; GetCursorInfo(ref info); return info.hCursor; }

    static async Task<bool> Until(Func<bool> condition, int milliseconds = 30_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(100);
        }
        return condition();
    }

    sealed class Run : IDisposable
    {
        public readonly Sandbox Sandbox = new();
        public readonly AppDump Dump;
        public readonly Process App;

        public Run(bool notch, params string[] args)
        {
            Directory.CreateDirectory(Sandbox.Data);
            var settings = new JsonObject { ["KiroNoticeSeen"] = true, ["HoverOpensWorkspace"] = false, ["KiroFolder"] = Sandbox.Project };
            if (notch) settings["NotchItems"] = new JsonArray("claude");
            File.WriteAllText(Path.Combine(Sandbox.Data, "settings.json"), settings.ToJsonString());
            var dumpFile = Path.Combine(Sandbox.Root, "ui-dump.json");
            Dump = new AppDump(dumpFile);
            var info = new ProcessStartInfo(TestEnvironment.AppExe!) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)! };
            foreach (var a in args) info.ArgumentList.Add(a);
            foreach (var (name, value) in Sandbox.Environment)
                if (value is not null) info.Environment[name] = value;
            info.Environment["HOVER_DATA_DIR"] = Sandbox.Data;
            info.Environment["HOVER_UI_DUMP"] = dumpFile;
            Desktop.MovePointer(5, 5);
            App = Process.Start(info)!;
        }

        public void Dispose()
        {
            try { if (!App.HasExited) App.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
            Sandbox.Dispose();
        }
    }

    /// <summary>Puts the pointer on a place (moving a little first, as a hand does) and says what shape the system shows after a moment.</summary>
    async Task<nint> ShapeOn(Run run, string part, string id, string what, Func<JsonNode, bool>? pick = null)
    {
        Assert.True(await Until(() => run.Dump.Where(part, id, pick) is not null), $"no '{id}' to point at ({what})\n{run.Dump.Summary()}");
        (int X, int Y) p = run.Dump.Where(part, id, pick)!.Value;
        for (var i = 0; i < 20; i++)
        {
            await Task.Delay(300);
            var again = run.Dump.Where(part, id, pick);
            if (again is { } q && q == p) break;
            if (again is { } moved) p = moved;
        }
        // A hand moves: the system hears several moves, a little apart, and the shape follows the last of them. The shape after each is logged.
        var seen = new List<string>();
        nint last = 0;
        foreach (var dx in new[] { 2, -2, 1, -1, 2, 0 })
        {
            Input.Move(p.X + dx, p.Y + (dx & 1));
            await Task.Delay(350);
            last = Shape();
            seen.Add($"0x{last:X}");
        }
        output.WriteLine($"the pointer on {what} at {p.X},{p.Y} shows {string.Join(", ", seen)}");
        return last;
    }

    static nint Of(int id) => LoadCursorW(0, id);

    [AppFact]
    public async Task In_the_app_window_the_pointer_is_the_hand_over_a_button_the_text_cursor_over_the_box_and_the_resize_cursor_on_the_lists_edge()
    {
        using var run = new Run(false, "--dashboard");
        await AppTests.WaitForLog(run.Sandbox.Data, "agent backend is ready", TimeSpan.FromSeconds(60), run.App);
        await Input.Park(run.App);
        Assert.True(await Until(() => run.Dump.Section("window") is not null), "the app window drew its panels");

        Assert.Equal(Of(Hand), await ShapeOn(run, "window", "view", "the chat view switch", n => (int)n["n"]! == 1));
        // Into the chat view: its start screen has a box of words, and the list has an edge.
        var view = run.Dump.Where("window", "view", n => (int)n["n"]! == 1)!.Value;
        Input.Click(view.X, view.Y);
        Assert.True(await Until(() => (bool?)run.Dump.Section("window")?["chatView"] == true), "the chat view opened");
        Assert.Equal(Of(Text), await ShapeOn(run, "window", "taskText", "the box of words"));
        Assert.Equal(Of(Hand), await ShapeOn(run, "window", "pillAgent", "the agent pill"));
        Assert.Equal(Of(ResizeColumn), await ShapeOn(run, "window", "sideEdge", "the list's edge"));
        // A plain place: the corner of the office, away from anything the panels drew.
        var area = run.Dump.Section("window")!;
        double left = (double)area["left"]!, top = (double)area["top"]!, scale = (double)area["scale"]!;
        Input.Move((int)(left + 600 * scale), (int)(top + 60 * scale));
        await Task.Delay(1500);
        Assert.Equal(Of(Normal), Shape());
    }

    [AppFact]
    public async Task In_the_open_notch_the_pointer_is_the_hand_over_a_button_and_the_text_cursor_over_the_box()
    {
        using var run = new Run(true);
        await AppTests.WaitForLog(run.Sandbox.Data, "hotkey Alt+N registered", TimeSpan.FromSeconds(60), run.App);
        await Task.Delay(1500);
        Assert.True(Desktop.Notch() != 0, "no window of class HoverNotch");
        Desktop.AltN();
        Assert.True(await Until(() => run.Dump.Section("notch") is not null), "the open notch drew its panels");
        await Task.Delay(2500);
        Assert.Equal(Of(Hand), await ShapeOn(run, "notch", "topMenu", "the menu button"));
        Assert.Equal(Of(Hand), await ShapeOn(run, "notch", "fab", "the + circle"));
        var fab = run.Dump.Where("notch", "fab")!.Value;
        Input.Click(fab.X, fab.Y);
        Assert.True(await Until(() => run.Dump.Where("notch", "taskText") is not null), "the new-task box opened in the notch");
        Assert.Equal(Of(Text), await ShapeOn(run, "notch", "taskText", "the box of words"));
        Desktop.AltN();
    }
}
