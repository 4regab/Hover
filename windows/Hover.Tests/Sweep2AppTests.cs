using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Text.Json.Nodes;
using Microsoft.Win32;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// Sweep 2, in the real app on the runner (hoverai.exe, HOVER_APP_EXE; Windows only): what only the real machine can show. The tray icon
/// when Explorer restarts, the app window's icon and place, Windows switching between light and dark while the app runs, a backend that
/// is killed, the system's folder picker, Settings' pointer shape and keyboard in both hosts, and the panels stepping aside under Settings.
/// </summary>
[SupportedOSPlatform("windows")]
public class Sweep2AppTests(ITestOutputHelper output)
{
    [DllImport("user32.dll")] static extern nint SendMessageW(nint hwnd, uint msg, nint w, nint l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, nint data);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(nint parent, EnumProc proc, nint data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(nint hwnd, char[] text, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(nint hwnd, char[] text, int max);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(nint hwnd);
    [DllImport("user32.dll")] static extern nint LoadCursorW(nint instance, nint name);
    [DllImport("user32.dll")] static extern bool GetCursorInfo(ref CURSORINFO info);
    [StructLayout(LayoutKind.Sequential)] struct CURSORINFO { public int cbSize, flags; public nint hCursor; public int x, y; }

    delegate bool EnumProc(nint hwnd, nint data);

    const int Hand = 32649;

    static nint Shape() { var info = new CURSORINFO { cbSize = Marshal.SizeOf<CURSORINFO>() }; GetCursorInfo(ref info); return info.hCursor; }

    // MARK: A run of the app

    sealed class Run : IDisposable
    {
        public readonly Sandbox Sandbox = new();
        public readonly AppDump Dump;
        public readonly Process App;
        readonly string logFile;

        public Run(string[] args, Action<JsonObject>? settings = null, Dictionary<string, string>? env = null)
        {
            Directory.CreateDirectory(Sandbox.Data);
            var model = new JsonObject { ["KiroNoticeSeen"] = true, ["KiroFolder"] = Sandbox.Project };
            settings?.Invoke(model);
            File.WriteAllText(Path.Combine(Sandbox.Data, "settings.json"), model.ToJsonString());
            var dumpFile = Path.Combine(Sandbox.Root, "ui-dump.json");
            Dump = new AppDump(dumpFile);
            logFile = Path.Combine(Sandbox.Data, "hover.log");
            var info = new ProcessStartInfo(TestEnvironment.AppExe!) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)! };
            foreach (var a in args) info.ArgumentList.Add(a);
            foreach (var (name, value) in Sandbox.Environment)
                if (value is not null) info.Environment[name] = value;
            foreach (var (name, value) in env ?? []) info.Environment[name] = value;
            info.Environment["HOVER_DATA_DIR"] = Sandbox.Data;
            info.Environment["HOVER_UI_DUMP"] = dumpFile;
            Desktop.MovePointer(5, 5);
            App = Process.Start(info)!;
        }

        public string Log()
        {
            try
            {
                if (!File.Exists(logFile)) return "";
                using var stream = new FileStream(logFile, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                return new StreamReader(stream).ReadToEnd();
            }
            catch (IOException) { return ""; }
        }

        public string Settings()
        {
            try
            {
                var file = Path.Combine(Sandbox.Data, "settings.json");
                if (!File.Exists(file)) return "";
                using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                return new StreamReader(stream).ReadToEnd();
            }
            catch (IOException) { return ""; }
        }

        public void Dispose()
        {
            try { if (!App.HasExited) App.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
            App.WaitForExit(5000);
            Sandbox.Dispose();
        }
    }

    static async Task<bool> Until(Func<bool> condition, int milliseconds = 30_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(150);
        }
        return condition();
    }

    static int Count(string text, string part)
    {
        var n = 0;
        for (var i = text.IndexOf(part, StringComparison.Ordinal); i >= 0; i = text.IndexOf(part, i + part.Length, StringComparison.Ordinal)) n++;
        return n;
    }

    static bool Skip(ITestOutputHelper output)
    {
        if (TestEnvironment.AppExe is not null) return false;
        output.WriteLine("HOVER_APP_EXE is not set: the real-app checks are not asked for.");
        return true;
    }

    // MARK: The tray

    /// <summary>
    /// Explorer is ended and starts again, as it does after a crash or when a user restarts it: every icon in the notification area is gone
    /// and Windows tells each program (the TaskbarCreated message) to add its own again. The shell is asked whether Hover's icon is there.
    /// </summary>
    [WindowsFact]
    public async Task The_tray_icon_comes_back_when_Explorer_restarts()
    {
        if (Skip(output)) return;
        Assert.True(Desktop.HasTaskbar(), "the machine has no taskbar to put an icon in");
        using var run = new Run(["--background"]);
        string Why() => "log:\n" + run.Log();
        Assert.True(await Until(() => run.Log().Contains("notch ready")), "the notch came up\n" + Why());
        var notch = Desktop.Notch();
        Assert.NotEqual(0, notch);
        // The app asks the shell when it adds the icon, and says so (a change to an icon works only on one the shell has). Where the icon sits
        // can also be asked from outside, but some shells answer "not shown" for an icon in the overflow, so that is only written down.
        Assert.Contains("tray icon added (the shell has it: True)", run.Log());
        var added = Count(run.Log(), "tray icon added (the shell has it: True)");
        output.WriteLine($"before: Shell_NotifyIconGetRect says {(Desktop.TrayIconRect(notch, out var first) is { } r0 ? $"{r0.Left},{r0.Top}" : $"no icon (0x{first:X8})")}");

        try
        {
            foreach (var p in Process.GetProcessesByName("explorer"))
            {
                try { p.Kill(); } catch (Exception e) when (e is InvalidOperationException or System.ComponentModel.Win32Exception) { }
            }
            // Windows starts the shell again by itself; if it has not after a while, it is started here so that the machine is left as it was.
            var restarted = await Until(() => Process.GetProcessesByName("explorer").Length > 0 && Desktop.HasTaskbar(), 40_000);
            if (!restarted)
            {
                output.WriteLine("Explorer did not restart by itself; starting it");
                Process.Start(new ProcessStartInfo("explorer.exe") { UseShellExecute = true });
                await Until(() => Desktop.HasTaskbar(), 30_000);
            }
            // The icon is there again, and the app says it put it back.
            var back = await Until(() => Count(run.Log(), "tray icon added (the shell has it: True)") > added, 60_000);
            output.WriteLine(Why());
            output.WriteLine($"after: Shell_NotifyIconGetRect says {(Desktop.TrayIconRect(notch, out var second) is { } r1 ? $"{r1.Left},{r1.Top}" : $"no icon (0x{second:X8})")}");
            Assert.True(back, "Hover's tray icon did not come back after Explorer restarted\n" + Why());
            Assert.Contains("the taskbar was made again", run.Log());
        }
        finally
        {
            if (Process.GetProcessesByName("explorer").Length == 0) Process.Start(new ProcessStartInfo("explorer.exe") { UseShellExecute = true });
        }
    }

    // MARK: The app window

    [WindowsFact]
    public async Task The_window_has_its_icon_and_opens_in_the_middle_of_the_work_area()
    {
        if (Skip(output)) return;
        using var run = new Run(["--dashboard"]);
        nint window = 0;
        Assert.True(await Until(() => (window = Desktop.WindowOf(run.App, "Hover")) != 0), "the app window\n" + run.Log());
        await Task.Delay(1500);
        var small = SendMessageW(window, 0x7F, 0, 0);
        var big = SendMessageW(window, 0x7F, 1, 0);
        output.WriteLine(string.Join("\n", run.Log().Split('\n').Where(l => l.Contains("window icon"))));
        Assert.NotEqual(0, small);
        Assert.NotEqual(0, big);

        var work = Desktop.WorkArea();
        var r = Desktop.Rect(window);
        int cx = (r.Left + r.Right) / 2, cy = (r.Top + r.Bottom) / 2, wx = (work.Left + work.Right) / 2, wy = (work.Top + work.Bottom) / 2;
        output.WriteLine($"window {r.Left},{r.Top} to {r.Right},{r.Bottom}; work area {work.Left},{work.Top} to {work.Right},{work.Bottom}");
        Assert.InRange(Math.Abs(cx - wx), 0, 4);
        Assert.InRange(Math.Abs(cy - wy), 0, 4);
    }

    // MARK: The setup asks Hover to quit

    /// <summary>`hoverai.exe --quit`, as the setup runs it before it replaces the files: the running copy ends in order, and the command waits.</summary>
    [WindowsFact]
    public async Task Hover_quit_ends_the_running_copy_in_order_and_its_backend_with_it()
    {
        if (Skip(output)) return;
        using var run = new Run(["--background"]);
        string Why() => "log:\n" + run.Log();
        Assert.True(await Until(() => run.Log().Contains("notch ready")), "the notch came up\n" + Why());
        Assert.True(await Until(() => run.Log().Contains("backend: The agent backend is ready")), "the backend ready\n" + Why());

        var ask = Process.Start(new ProcessStartInfo(TestEnvironment.AppExe!, "--quit") { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)! })!;
        Assert.True(ask.WaitForExit(40_000), "hoverai.exe --quit did not return\n" + Why());
        Assert.Equal(0, ask.ExitCode);
        Assert.True(run.App.WaitForExit(15_000), "the running copy ended\n" + Why());
        Assert.Equal(0, run.App.ExitCode);
        var log = run.Log();
        Assert.Contains("a quit was asked for", log);
        Assert.Matches(@"backend stopped \(exit code 0\)", log);
    }

    // MARK: Windows light and dark

    sealed class SystemTheme : IDisposable
    {
        const string Key = @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
        readonly object? before;

        public SystemTheme()
        {
            using var key = Registry.CurrentUser.CreateSubKey(Key);
            before = key.GetValue("AppsUseLightTheme");
        }

        public void Set(bool dark)
        {
            using var key = Registry.CurrentUser.CreateSubKey(Key);
            key.SetValue("AppsUseLightTheme", dark ? 0 : 1, RegistryValueKind.DWord);
        }

        public void Dispose()
        {
            using var key = Registry.CurrentUser.CreateSubKey(Key);
            if (before is null) key.DeleteValue("AppsUseLightTheme", throwOnMissingValue: false);
            else key.SetValue("AppsUseLightTheme", before, RegistryValueKind.DWord);
        }
    }

    [WindowsFact]
    public async Task The_System_appearance_follows_Windows_switching_between_light_and_dark_while_the_app_runs()
    {
        if (Skip(output)) return;
        using var theme = new SystemTheme();
        theme.Set(dark: false);
        using var run = new Run(["--settings"]);
        string Why() => "log:\n" + run.Log();
        bool? Dark() => (bool?)run.Dump.Section("settings-window")?["dark"];
        Assert.True(await Until(() => Dark() == false, 40_000), "the page in Windows' light colours\n" + Why());
        theme.Set(dark: true);
        Assert.True(await Until(() => Dark() == true, 15_000), "the page in dark colours after Windows went dark\n" + Why());
        theme.Set(dark: false);
        Assert.True(await Until(() => Dark() == false, 15_000), "the page in light colours after Windows went light again\n" + Why());
        Assert.Contains("windows is now dark", run.Log());
    }

    // MARK: A backend that is gone

    [WindowsFact]
    public async Task A_killed_backend_is_said_in_the_bar_and_from_the_tray_with_what_to_do()
    {
        if (Skip(output)) return;
        using var run = new Run(["--dashboard"]);
        string Why() => "log:\n" + run.Log();
        Assert.True(await Until(() => run.Log().Contains("backend started (pid ")), "the backend\n" + Why());
        Assert.True(await Until(() => run.Log().Contains("backend: The agent backend is ready")), "the backend ready\n" + Why());
        var text = run.Log();
        var at = text.IndexOf("backend started (pid ", StringComparison.Ordinal) + "backend started (pid ".Length;
        var pid = int.Parse(text[at..text.IndexOf(')', at)]);
        Process.GetProcessById(pid).Kill(entireProcessTree: true);

        string? Bar() => (string?)run.Dump.Section("bar-problem")?["text"];
        Assert.True(await Until(() => Bar() is { Length: > 0 }, 20_000), "a line in the title bar\n" + Why());
        output.WriteLine("the bar says: " + Bar());
        Assert.Contains("The agent backend stopped", Bar());
        Assert.Contains("Quit Hover from its tray menu and open it again", Bar());
        Assert.True(await Until(() => run.Log().Contains("tray balloon: Hover")), "the tray said so too\n" + Why());
    }

    // MARK: The system's folder picker

    static bool IsPicker(Process app, out string classes)
    {
        string? found = null;
        EnumWindows((hwnd, _) =>
        {
            GetWindowThreadProcessId(hwnd, out var pid);
            if (pid != app.Id || !IsWindowVisible(hwnd)) return true;
            var cls = new char[64];
            var n = GetClassNameW(hwnd, cls, cls.Length);
            if (new string(cls, 0, n) != "#32770") return true;
            var kids = new List<string>();
            EnumChildWindows(hwnd, (child, _) =>
            {
                var c = new char[96];
                var m = GetClassNameW(child, c, c.Length);
                kids.Add(new string(c, 0, m));
                return true;
            }, 0);
            found = string.Join(",", kids.Distinct());
            return false;
        }, 0);
        classes = found ?? "";
        return found is not null;
    }

    [WindowsFact]
    public async Task Add_a_project_opens_the_systems_folder_picker_and_Escape_closes_it()
    {
        if (Skip(output)) return;
        using var run = new Run(["--settings"]);
        string Why() => "log:\n" + run.Log() + "\ndump: " + run.Dump.Summary();
        await Input.Park(run.App);
        Assert.True(await Until(() => run.Dump.Texts("settings-window").Contains("Launch at login")), "Settings\n" + Why());
        var sections = AppDump.Items(run.Dump.Section("settings-window"), "targets").Where(t => (string?)t["kind"] == "Section").OrderBy(t => (double)t["y"]!).ToList();
        Assert.True(sections.Count >= 3);
        var s = run.Dump.Section("settings-window")!;
        double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
        Input.Click((int)Math.Round(left + ((double)sections[2]["x"]! + (double)sections[2]["w"]! / 2) * scale), (int)Math.Round(top + ((double)sections[2]["y"]! + (double)sections[2]["h"]! / 2) * scale));
        Assert.True(await Until(() => (string?)run.Dump.Section("settings-window")?["section"] == "Projects"), "the Projects page\n" + Why());
        Assert.True(await Until(() => run.Dump.Where("settings-window", "ProjectAdd") is not null), "the Add a project link\n" + Why());
        var add = run.Dump.Where("settings-window", "ProjectAdd")!.Value;
        Input.Click(add.X, add.Y);

        string classes = "";
        Assert.True(await Until(() => IsPicker(run.App, out classes), 20_000), "a dialog window\n" + Why());
        output.WriteLine("the dialog holds: " + classes);
        // The Explorer-style picker is drawn by DirectUI and has the address bar (breadcrumbs) and the search box; the old window that
        // SHBrowseForFolder shows is a tree in a small dialog, with neither. (This picker's side list is a tree view too.)
        Assert.Contains("DirectUIHWND", classes);
        Assert.Contains("Address Band Root", classes);
        Assert.Contains("Search Box", classes);

        Input.Key(Input.Escape);
        Assert.True(await Until(() => !IsPicker(run.App, out _), 10_000), "the dialog closing on Escape");
        Assert.DoesNotContain("\"Projects\"", run.Settings());
    }

    // MARK: Settings, the pointer and the keyboard

    [WindowsFact]
    public async Task Settings_in_the_app_window_shows_the_hand_over_a_switch_and_works_from_the_keyboard()
    {
        if (Skip(output)) return;
        using var run = new Run(["--settings"]);
        string Why() => "log:\n" + run.Log() + "\ndump: " + run.Dump.Summary();
        await Input.Park(run.App);
        Assert.True(await Until(() => run.Dump.Texts("settings-window").Contains("Launch at login")), "Settings\n" + Why());
        Assert.True(await Until(() => run.Dump.Where("settings-window", "HoverOpens") is not null), "the Open on hover switch\n" + Why());

        // The pointer, moved as a mouse moves, over the switch: the hand.
        var sw = run.Dump.Where("settings-window", "HoverOpens")!.Value;
        nint last = 0;
        foreach (var dx in new[] { 3, -3, 2, -2, 0 })
        {
            Input.MoveLikeMouse(sw.X + dx, sw.Y + (dx & 1));
            await Task.Delay(300);
            last = Shape();
        }
        output.WriteLine($"the pointer over the switch shows 0x{last:X}; the hand is 0x{LoadCursorW(0, Hand):X}");
        Assert.Equal(LoadCursorW(0, Hand), last);

        // The keyboard alone: a press on the page's empty top gives it the keyboard, then Tab walks to the switch and Space works it.
        var pane = AppDump.Items(run.Dump.Section("settings-window"), "targets").Last(t => (string?)t["kind"] == "Pane" && (string?)t["id"] == "pane");
        var p = run.Dump.Section("settings-window")!;
        double l = (double)p["left"]!, t0 = (double)p["top"]!, sc = (double)p["scale"]!;
        Input.Click((int)Math.Round(l + ((double)pane["x"]! + (double)pane["w"]! * 0.8) * sc), (int)Math.Round(t0 + ((double)pane["y"]! + 12) * sc));
        for (var i = 0; i < 12 && (string?)run.Dump.Section("settings-window")?["keyboard"] != "Switch:HoverOpens#0"; i++)
        {
            Input.Key(0x09);
            await Task.Delay(150);
        }
        Assert.Equal("Switch:HoverOpens#0", (string?)run.Dump.Section("settings-window")?["keyboard"]);
        Assert.True((bool?)run.Dump.Section("settings-window")?["ring"]);
        Input.Key(0x20);
        Assert.True(await Until(() => run.Settings().Contains("\"HoverOpensWorkspace\": false")), "Space switching Open on hover off\n" + Why());
    }

    [WindowsFact]
    public async Task Settings_in_the_open_notch_shows_the_hand_over_a_switch()
    {
        if (Skip(output)) return;
        using var run = new Run([]);
        string Why() => "log:\n" + run.Log() + "\nnotch: " + run.Dump.Summary();
        Assert.True(await Until(() => run.Log().Contains("hotkey Alt+N registered"), 60_000), "the notch shortcut\n" + Why());
        await Task.Delay(500);
        Desktop.AltN();
        Assert.True(await Until(() => run.Dump.Where("notch", "topMenu") is not null), "the notch's menu button\n" + Why());
        var menu = run.Dump.Where("notch", "topMenu")!.Value;
        Input.Click(menu.X, menu.Y);
        Assert.True(await Until(() => run.Dump.Where("notch", "menu", n => (string?)n["s"] == "Settings") is not null), "the Settings row\n" + Why());
        var row = run.Dump.Where("notch", "menu", n => (string?)n["s"] == "Settings")!.Value;
        Input.Click(row.X, row.Y);
        Assert.True(await Until(() => run.Dump.Texts("settings-notch").Contains("Launch at login")), "the page in the notch\n" + Why());
        Assert.True(await Until(() => run.Dump.Where("settings-notch", "HoverOpens") is not null), "the Open on hover switch\n" + Why());

        var sw = run.Dump.Where("settings-notch", "HoverOpens")!.Value;
        nint last = 0;
        foreach (var dx in new[] { 3, -3, 2, -2, 0 })
        {
            Input.MoveLikeMouse(sw.X + dx, sw.Y + (dx & 1));
            await Task.Delay(300);
            last = Shape();
        }
        output.WriteLine($"the pointer over the switch in the notch shows 0x{last:X}; the hand is 0x{LoadCursorW(0, Hand):X}");
        Assert.Equal(LoadCursorW(0, Hand), last);

        // A press on the page gives it the keyboard: Tab then walks the controls and Space works one.
        Input.Click(sw.X, sw.Y);
        await Task.Delay(300);
        Assert.True(await Until(() => run.Settings().Contains("\"HoverOpensWorkspace\": false")), "the press on the switch\n" + Why());
        for (var i = 0; i < 12 && (string?)run.Dump.Section("settings-notch")?["keyboard"] != "Switch:HoverOpens#0"; i++)
        {
            Input.Key(0x09);
            await Task.Delay(150);
        }
        Assert.Equal("Switch:HoverOpens#0", (string?)run.Dump.Section("settings-notch")?["keyboard"]);
        Input.Key(0x20);
        Assert.True(await Until(() => run.Settings().Contains("\"HoverOpensWorkspace\": true")), "Space switching it on again\n" + Why());
    }

    // MARK: The panels under Settings

    [WindowsFact]
    public async Task The_panels_step_aside_while_Settings_shows_in_the_app_window_and_come_back()
    {
        if (Skip(output)) return;
        using var run = new Run(["--settings"]);
        string Why() => "log:\n" + run.Log() + "\ndump: " + run.Dump.Summary();
        await Input.Park(run.App);
        Assert.True(await Until(() => run.Dump.Texts("settings-window").Contains("Launch at login")), "Settings\n" + Why());
        Assert.True(await Until(() => (bool?)run.Dump.Section("panels-away")?["away"] == true), "the panels stepping aside\n" + Why());
        var back = run.Dump.Where("settings-window", "back")!.Value;
        Input.Click(back.X, back.Y);
        Assert.True(await Until(() => (bool?)run.Dump.Section("panels-away")?["away"] == false), "the panels coming back\n" + Why());
    }

    // MARK: The menu bar from the keyboard

    [WindowsFact]
    public async Task The_menu_bar_opens_with_Alt_and_a_letter_and_a_page_is_picked_with_the_keys()
    {
        if (Skip(output)) return;
        using var run = new Run(["--dashboard"]);
        string Why() => "log:\n" + run.Log() + "\nmenubar: " + run.Dump.Section("menubar")?.ToJsonString();
        await Input.Park(run.App);
        Assert.True(await Until(() => (run.Dump.Section("menubar")?["items"]?.AsArray().Count ?? 0) == 3), "the three menus\n" + Why());
        // Give the window the keyboard with a press on its title bar's empty part, then Alt+S.
        var items = run.Dump.Section("menubar")!["items"]!;
        var help = items[2]!;
        Input.Click((int)((double)help["x"]! + (double)help["w"]! + 120), (int)((double)help["y"]! + (double)help["h"]! / 2));
        // Alt on its own shows the keys of the menus; the letter then opens one.
        Input.Key(0x12);
        await Task.Delay(500);
        Input.Key((byte)'S');
        await Task.Delay(800);
        Input.Key(0x28);
        Input.Key(0x0D);
        var named = new[] { "General", "Integrations", "Projects", "Voice" };
        Assert.True(await Until(() => named.Contains((string?)run.Dump.Section("settings-window")?["section"]), 15_000), "a page of Settings from Alt+S, Down and Enter\n" + Why());
    }
}
