using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Text.Json.Nodes;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Settings.Tests;

/// <summary>
/// Settings in the real app (hoverai.exe, HOVER_APP_EXE; Windows only): the page is opened with the command line, and the real pointer and
/// keyboard change settings the backend writes to settings.json. The page's places come from the app's own dump (HOVER_UI_DUMP).
/// </summary>
[SupportedOSPlatform("windows")]
public class SettingsAppTests(ITestOutputHelper output)
{
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, int data, nuint extra);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);
    [DllImport("user32.dll")] static extern bool SetWindowPos(nint hwnd, nint after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(nint hwnd, out Rect rect);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, nint param);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(nint hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(nint hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(nint hwnd);

    delegate bool EnumProc(nint hwnd, nint param);

    [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }

    static string? AppExe => Environment.GetEnvironmentVariable("HOVER_APP_EXE") is { Length: > 0 } p && File.Exists(p) ? p : null;

    sealed class Session : IDisposable
    {
        public readonly string Data, Dump;
        public readonly Process App;

        public Session(params string[] args)
        {
            Data = Directory.CreateTempSubdirectory("hover-settings-app.").FullName;
            Dump = Path.Combine(Data, "ui-dump.json");
            var info = new ProcessStartInfo(AppExe!, args) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(AppExe!)! };
            info.Environment["HOVER_DATA_DIR"] = Data;
            info.Environment["HOVER_UI_DUMP"] = Dump;
            App = Process.Start(info)!;
        }

        public JsonNode? Read()
        {
            for (var i = 0; i < 5; i++)
            {
                try
                {
                    if (!File.Exists(Dump)) return null;
                    using var stream = new FileStream(Dump, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                    return JsonNode.Parse(new StreamReader(stream).ReadToEnd());
                }
                catch (IOException) { Thread.Sleep(20); }
                catch (System.Text.Json.JsonException) { Thread.Sleep(20); }
            }
            return null;
        }

        public string Settings() => File.Exists(Path.Combine(Data, "settings.json")) ? ReadShared(Path.Combine(Data, "settings.json")) : "";

        public string Log() => File.Exists(Path.Combine(Data, "hover.log")) ? ReadShared(Path.Combine(Data, "hover.log")) : "";

        static string ReadShared(string file)
        {
            try
            {
                using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                return new StreamReader(stream).ReadToEnd();
            }
            catch (IOException) { return ""; }
        }

        public void Dispose()
        {
            try { if (!App.HasExited) App.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            App.WaitForExit(5000);
            try { Directory.Delete(Data, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
    }

    // MARK: Input as a person makes it

    static void Click(int x, int y)
    {
        SetCursorPos(x, y);
        Thread.Sleep(250);
        mouse_event(0x0002, 0, 0, 0, 0);
        Thread.Sleep(60);
        mouse_event(0x0004, 0, 0, 0, 0);
        Thread.Sleep(300);
    }

    static void Wheel(int x, int y, int notches)
    {
        SetCursorPos(x, y);
        Thread.Sleep(250);
        mouse_event(0x0800, 0, 0, notches * 120, 0);
        Thread.Sleep(300);
    }

    static void Key(byte vk)
    {
        keybd_event(vk, 0, 0, 0);
        Thread.Sleep(30);
        keybd_event(vk, 0, 2, 0);
        Thread.Sleep(250);
    }

    static void Chord(byte modifier1, byte modifier2, byte key)
    {
        keybd_event(modifier1, 0, 0, 0);
        keybd_event(modifier2, 0, 0, 0);
        Thread.Sleep(60);
        keybd_event(key, 0, 0, 0);
        Thread.Sleep(60);
        keybd_event(key, 0, 2, 0);
        keybd_event(modifier2, 0, 2, 0);
        keybd_event(modifier1, 0, 2, 0);
        Thread.Sleep(300);
    }

    static void AltN()
    {
        keybd_event(0x12, 0, 0, 0);
        Thread.Sleep(60);
        keybd_event(0x4E, 0, 0, 0);
        Thread.Sleep(60);
        keybd_event(0x4E, 0, 2, 0);
        keybd_event(0x12, 0, 2, 0);
        Thread.Sleep(300);
    }

    static void TypeText(string text)
    {
        foreach (var c in text)
        {
            var vk = c is >= 'a' and <= 'z' ? (byte)(c - 'a' + 'A') : c == '-' ? (byte)0xBD : (byte)c;
            Key(vk);
        }
    }

    static nint WindowOf(Process app, string title)
    {
        nint found = 0;
        EnumWindows((hwnd, _) =>
        {
            GetWindowThreadProcessId(hwnd, out var pid);
            if (pid != app.Id || !IsWindowVisible(hwnd)) return true;
            var text = new StringBuilder(64);
            GetWindowTextW(hwnd, text, 64);
            if (text.ToString() == title) found = hwnd;
            return true;
        }, 0);
        return found;
    }

    /// <summary>Puts the app's window near the screen's top left corner and one pixel wider, so that the app draws again and its dump says where the window is now.</summary>
    static async Task Park(Process app)
    {
        for (var end = Environment.TickCount64 + 20_000; Environment.TickCount64 < end;)
        {
            var window = WindowOf(app, "Hover");
            if (window != 0 && GetWindowRect(window, out var r))
            {
                SetWindowPos(window, 0, 10, 40, r.Right - r.Left + 1, r.Bottom - r.Top, 0x0004 | 0x0010);
                await Task.Delay(900);
                return;
            }
            await Task.Delay(100);
        }
    }

    static (int X, int Y)? Where(JsonNode? dump, string section, string kind, string id, int? n = null)
    {
        var s = dump?[section];
        if (s is null) return null;
        foreach (var t in (s["targets"]?.AsArray() ?? []).Reverse())
        {
            if ((string?)t?["kind"] != kind || (string?)t["id"] != id) continue;
            double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
            double x = (double)t["x"]!, y = (double)t["y"]!, w = (double)t["w"]!, h = (double)t["h"]!;
            return ((int)Math.Round(left + (x + w / 2) * scale), (int)Math.Round(top + (y + h / 2) * scale));
        }
        return null;
    }

    /// <summary>A place on the office panels' dump (<c>notch</c> or <c>window</c>), by id and the text it carries.</summary>
    static (int X, int Y)? WherePanel(JsonNode? dump, string section, string id, string? text = null)
    {
        var s = dump?[section];
        if (s is null) return null;
        foreach (var t in (s["targets"]?.AsArray() ?? []).Reverse())
        {
            if ((string?)t?["id"] != id || (text is not null && (string?)t["s"] != text)) continue;
            double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
            double x = (double)t["x"]!, y = (double)t["y"]!, w = (double)t["w"]!, h = (double)t["h"]!;
            return ((int)Math.Round(left + (x + w / 2) * scale), (int)Math.Round(top + (y + h / 2) * scale));
        }
        return null;
    }

    /// <summary>The middle of a place of the dump, in screen pixels.</summary>
    static (int X, int Y) Center(JsonNode section, JsonNode t)
    {
        double left = (double)section["left"]!, top = (double)section["top"]!, scale = (double)section["scale"]!;
        return ((int)Math.Round(left + ((double)t["x"]! + (double)t["w"]! / 2) * scale), (int)Math.Round(top + ((double)t["y"]! + (double)t["h"]! / 2) * scale));
    }

    static async Task<T> Until<T>(Func<T?> find, string what, ITestOutputHelper output, Func<string>? why = null, int ms = 40_000) where T : struct
    {
        for (var end = Environment.TickCount64 + ms; Environment.TickCount64 < end;)
        {
            if (find() is { } v) return v;
            await Task.Delay(150);
        }
        output.WriteLine(why?.Invoke() ?? "");
        throw new Xunit.Sdk.XunitException("Timed out waiting for " + what);
    }

    static async Task UntilTrue(Func<bool> condition, string what, ITestOutputHelper output, Func<string>? why = null, int ms = 40_000)
    {
        for (var end = Environment.TickCount64 + ms; Environment.TickCount64 < end;)
        {
            if (condition()) return;
            await Task.Delay(150);
        }
        output.WriteLine(why?.Invoke() ?? "");
        throw new Xunit.Sdk.XunitException("Timed out waiting for " + what);
    }

    [WindowsFact]
    public async Task The_app_window_shows_Settings_and_a_click_a_key_and_a_wheel_change_settings_the_backend_writes()
    {
        if (AppExe is null) { output.WriteLine("HOVER_APP_EXE is not set: the real-app checks are not asked for."); return; }
        using var s = new Session("--settings");
        string Why() => "log:\n" + s.Log() + "\ndump: " + s.Read()?["settings-window"]?["texts"]?.ToJsonString();
        await Park(s.App);

        // The page is there with the General rows, and the way back.
        await UntilTrue(() => s.Read()?["settings-window"]?["texts"]?.AsArray().Any(t => (string?)t == "Launch at login") == true, "the General page", output, Why);
        Assert.Equal("General", (string?)s.Read()!["settings-window"]!["section"]);

        // A click on a switch: Open on hover goes off, and settings.json says so (the backend writes it).
        var hover = await Until(() => Where(s.Read(), "settings-window", "Switch", "HoverOpens"), "the Open on hover switch", output, Why);
        Click(hover.X, hover.Y);
        await UntilTrue(() => s.Settings().Contains("\"HoverOpensWorkspace\": false"), "settings.json with Open on hover off", output, Why);

        // Segments: Appearance Light, and the page wears Hover's light colours. The row is below the window's bottom edge: the wheel brings it up.
        (int X, int Y)? light = null;
        for (var i = 0; i < 25 && light is null; i++)
        {
            var d = s.Read()?["settings-window"];
            var segs = (d?["targets"]?.AsArray() ?? []).Where(t => (string?)t!["kind"] == "Seg" && (string?)t["id"] == "Appearance" && (double)t["h"]! >= 20).OrderBy(t => (double)t!["x"]!).ToList();
            if (segs.Count == 3) light = Center(d!, segs[1]!);
            else
            {
                Wheel((int)Math.Round((double)d!["left"]! + 600 * (double)d["scale"]!), (int)Math.Round((double)d["top"]! + 300 * (double)d["scale"]!), -3);
                await Task.Delay(200);
            }
        }
        Assert.NotNull(light);
        Click(light.Value.X, light.Value.Y);
        await UntilTrue(() => s.Settings().Contains("\"Appearance\": \"Light\"") && (bool?)s.Read()?["settings-window"]?["dark"] == false, "the light appearance", output, Why);

        // The sidebar: Voice. Its page is long, so the wheel scrolls to the box for the cleanup model.
        var sections = s.Read()!["settings-window"]!["targets"]!.AsArray().Where(t => (string?)t!["kind"] == "Section").OrderBy(t => (double)t!["y"]!).ToList();
        Assert.True(sections.Count >= 5, "the sidebar has its sections");
        double left = (double)s.Read()!["settings-window"]!["left"]!, top = (double)s.Read()!["settings-window"]!["top"]!, sc = (double)s.Read()!["settings-window"]!["scale"]!;
        var voice = Center(s.Read()!["settings-window"]!, sections[3]!);
        Click(voice.X, voice.Y);
        await UntilTrue(() => (string?)s.Read()?["settings-window"]?["section"] == "Voice", "the Voice page", output, Why);

        (int X, int Y)? field = null;
        for (var i = 0; i < 25 && field is null; i++)
        {
            field = Where(s.Read(), "settings-window", "Field", "VoiceCleanupModel");
            if (field is null)
            {
                Wheel((int)(left + 600 * sc), (int)(top + 300 * sc), -3);
                await Task.Delay(200);
            }
        }
        Assert.NotNull(field);
        Click(field.Value.X, field.Value.Y);
        await UntilTrue(() => (string?)s.Read()?["settings-window"]?["focus"] == "VoiceCleanupModel", "the box having the keyboard", output, Why);
        TypeText("my-model");
        await UntilTrue(() => (int?)s.Read()?["settings-window"]?["focusLength"] == 8, "eight characters typed", output, Why);
        Key(0x0D);
        await UntilTrue(() => s.Settings().Contains("\"CleanupModel\": \"my-model\""), "the model saved", output, Why);
    }

    [WindowsFact]
    public async Task A_shortcut_is_recorded_from_the_real_keyboard_and_saved()
    {
        if (AppExe is null) { output.WriteLine("HOVER_APP_EXE is not set: the real-app checks are not asked for."); return; }
        using var s = new Session("--settings");
        string Why() => "log:\n" + s.Log();
        await Park(s.App);
        var pill = await Until(() => Where(s.Read(), "settings-window", "Shortcut", "WorkspaceShortcut"), "the notch shortcut", output, Why);
        Click(pill.X, pill.Y);
        await UntilTrue(() => (bool?)s.Read()?["settings-window"]?["recording"] == true, "recording", output, Why);
        // Ctrl+Alt+M: the notch's own Alt+N was let go for the recording, and the chord reaches the page.
        Chord(0x11, 0x12, 0x4D);
        await UntilTrue(() => s.Settings().Contains("\"ScWorkspace\"") && s.Settings().Contains("\"Key\": \"M\""), "the chord saved", output, Why);
        Assert.False((bool?)s.Read()?["settings-window"]?["recording"]);
        Assert.Contains("Ctrl+Alt+M", ((JsonArray)s.Read()!["settings-window"]!["texts"]!).Select(t => (string?)t).ToList());
    }

    [WindowsFact]
    public async Task Settings_open_in_the_open_notch_from_its_menu_and_a_click_changes_a_setting()
    {
        if (AppExe is null) { output.WriteLine("HOVER_APP_EXE is not set: the real-app checks are not asked for."); return; }
        SetCursorPos(5, 5);
        using var s = new Session();
        string Why() => "log:\n" + s.Log() + "\nnotch: " + s.Read()?["notch"]?["texts"]?.ToJsonString();
        await UntilTrue(() => s.Log().Contains("hotkey Alt+N registered"), "the notch shortcut", output, Why, 60_000);
        await Task.Delay(500);

        // Alt+N opens the notch on the office, with the menu button at the top.
        AltN();
        var menu = await Until(() => WherePanel(s.Read(), "notch", "topMenu"), "the notch's menu button", output, Why);
        Click(menu.X, menu.Y);
        var row = await Until(() => WherePanel(s.Read(), "notch", "menu", "Settings"), "the Settings row in the menu", output, Why);
        Click(row.X, row.Y);

        // The page is drawn in the notch, on General, and a switch there is the real one.
        await UntilTrue(() => s.Read()?["settings-notch"]?["texts"]?.AsArray().Any(t => (string?)t == "Launch at login") == true, "the page in the notch", output, Why);
        Assert.Equal("General", (string?)s.Read()!["settings-notch"]!["section"]);
        var hover = await Until(() => Where(s.Read(), "settings-notch", "Switch", "HoverOpens"), "the Open on hover switch", output, Why);
        Click(hover.X, hover.Y);
        await UntilTrue(() => s.Settings().Contains("\"HoverOpensWorkspace\": false"), "settings.json with Open on hover off", output, Why);
    }

    [WindowsFact]
    public async Task The_Settings_menu_of_the_bar_opens_the_page_it_names()
    {
        if (AppExe is null) { output.WriteLine("HOVER_APP_EXE is not set: the real-app checks are not asked for."); return; }
        using var s = new Session("--dashboard");
        string Why() => "log:\n" + s.Log() + "\nmenubar: " + s.Read()?["menubar"]?.ToJsonString() + "\nsettings-window: " + ((string?)s.Read()?["settings-window"]?["section"] ?? "(none)");
        await Park(s.App);

        // The bar has File, Settings and Help in this order, each with a place on the screen.
        await UntilTrue(() => (s.Read()?["menubar"]?["items"]?.AsArray().Select(i => (string?)i!["title"]).ToList() ?? []).SequenceEqual(["File", "Settings", "Help"]), "the three menus", output, Why);
        var title = s.Read()!["menubar"]!["items"]![1]!;
        Click((int)((double)title["x"]! + (double)title["w"]! / 2), (int)((double)title["y"]! + (double)title["h"]! / 2));
        await Task.Delay(800);
        // The menu lists General, Integrations, Projects, Voice, then the agents. One key down and Enter takes the first or the second
        // of them (whether the menu starts with its first entry chosen is the system's); either way a page of Settings opens by name.
        Key(0x28);
        Key(0x0D);
        var named = new[] { "General", "Integrations", "Projects", "Voice" };
        await UntilTrue(() => named.Contains((string?)s.Read()?["settings-window"]?["section"]), "a page of Settings from the menu", output, Why);
        output.WriteLine("the menu opened " + (string?)s.Read()!["settings-window"]!["section"]);
    }
}
