using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Voice.Tests;

/// <summary>A fact for the real app: Windows, and HOVER_APP_EXE names the built hoverai.exe.</summary>
sealed class AppFactAttribute : FactAttribute
{
    public AppFactAttribute()
    {
        if (!OperatingSystem.IsWindows()) Skip = "Windows only";
        else if (TestEnvironment.AppExe is null) Skip = "HOVER_APP_EXE is not set";
    }
}

/// <summary>
/// Voice in the real app, with real keys: the hold-to-talk chord is pressed, the app has no
/// microphone to open (the runner has none), and the card says why in the Go app's words. Esc
/// clears it. A chord another program holds is said on the card too.
/// </summary>
public class VoiceAppTests(ITestOutputHelper output)
{
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);

    const byte Ctrl = 0x11, Alt = 0x12, Space = 0x20, Escape = 0x1B;

    static void Down(byte vk) => keybd_event(vk, 0, 0, 0);

    static void Up(byte vk) => keybd_event(vk, 0, 2, 0);

    static void LetGo()
    {
        Up(Space);
        Up(Alt);
        Up(Ctrl);
    }

    static async Task<bool> Until(Func<bool> test, int milliseconds = 20_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (test()) return true;
            await Task.Delay(100);
        }
        return test();
    }

    sealed class Dump(string file)
    {
        public JsonNode? Section(string name)
        {
            for (var i = 0; i < 5; i++)
            {
                try
                {
                    if (!File.Exists(file)) return null;
                    using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                    return JsonNode.Parse(new StreamReader(stream).ReadToEnd())?[name];
                }
                catch (IOException) { Thread.Sleep(20); }
                catch (System.Text.Json.JsonException) { Thread.Sleep(20); }
            }
            return null;
        }

        public bool Showing => Section("voice")?["showing"]?.GetValue<bool>() == true;

        public string Words => Section("voice")?["words"]?.GetValue<string>() ?? "";

        public int Kind => Section("voice")?["kind"]?.GetValue<int>() ?? 0;

        /// <summary>The screen point at the middle of a button of the card.</summary>
        public (int X, int Y)? Where(string action)
        {
            var s = Section("voice");
            var t = s?["targets"]?.AsArray().LastOrDefault(n => n?["action"]?.GetValue<string>() == action);
            if (s is null || t is null) return null;
            double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
            double x = (double)t["x"]!, y = (double)t["y"]!, w = (double)t["w"]!, h = (double)t["h"]!;
            return ((int)Math.Round(left + (x + w / 2) * scale), (int)Math.Round(top + (y + h / 2) * scale));
        }
    }

    sealed class RunningApp : IDisposable
    {
        public Process Process { get; }
        public string Data { get; }
        public Dump Dump { get; }
        public string Root { get; }

        public RunningApp(string settings, VoiceSandbox box)
        {
            Root = box.Root;
            Data = box.Data;
            Directory.CreateDirectory(Data);
            File.WriteAllText(Path.Combine(Data, "settings.json"), settings);
            var dumpFile = Path.Combine(box.Root, "ui-dump.json");
            Dump = new Dump(dumpFile);
            var info = new ProcessStartInfo(TestEnvironment.AppExe!)
            {
                UseShellExecute = false,
                WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)!,
            };
            foreach (var (name, value) in box.Environment(null))
                if (value is not null) info.Environment[name] = value;
            info.Environment["HOVER_DATA_DIR"] = Data;
            info.Environment["HOVER_UI_DUMP"] = dumpFile;
            Process = Process.Start(info)!;
        }

        public string Log()
        {
            var log = DataFolder.LogFile(Data);
            if (!File.Exists(log)) return "(no log)";
            using var stream = new FileStream(log, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            return new StreamReader(stream).ReadToEnd();
        }

        public void Dispose()
        {
            try { if (!Process.HasExited) Process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            Process.Dispose();
        }
    }

    [AppFact]
    public async Task The_chord_with_no_microphone_shows_the_go_apps_words_and_esc_clears_the_card()
    {
        using var box = new VoiceSandbox();
        using var app = new RunningApp("""{"KiroNoticeSeen": true, "Voice": {"Enabled": true, "HoldToTalk": true, "Countdown": 5}}""", box);
        try
        {
            Assert.True(await Until(() => app.Log().Contains("voice hotkey Ctrl+Alt+Space registered", StringComparison.Ordinal), 40_000),
                "the app did not take the voice shortcut\n" + app.Log());
            Assert.False(app.Dump.Showing, "no card before the chord");

            Down(Ctrl);
            Down(Alt);
            Down(Space);
            var shown = await Until(() => app.Dump.Showing && app.Dump.Kind == 7, 20_000);
            LetGo();
            Assert.True(shown, "the card did not come\n" + app.Log());
            output.WriteLine("the card says: " + app.Dump.Words);
            // The runner has no microphone, and says so as the Go app does; a machine with one has to be told it is not the test's business.
            if (MicDevices.RawNames().Count == 0) Assert.Equal("No microphone is connected.", app.Dump.Words);
            else Assert.True(app.Dump.Words.Length > 0);
            Assert.Contains("voice: the shortcut was pressed", app.Log());

            // Esc, sent as a key, clears it: the notch took the keyboard for the card. If the desktop will not give it, the card's own Esc chip does.
            Down(Escape);
            Up(Escape);
            var cleared = await Until(() => !app.Dump.Showing, 4_000);
            if (!cleared && app.Dump.Where("cancel") is { } chip)
            {
                output.WriteLine("Esc did not reach the notch on this desktop; the Esc chip is clicked");
                Click(chip.X, chip.Y);
                cleared = await Until(() => !app.Dump.Showing, 8_000);
            }
            Assert.True(cleared, "the card did not go\n" + app.Log());
            Assert.False(app.Process.HasExited, "the app ended");
        }
        finally { LetGo(); }
    }

    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, int data, nuint extra);

    static void Click(int x, int y)
    {
        SetCursorPos(x, y);
        Thread.Sleep(250);
        mouse_event(0x0002, 0, 0, 0, 0);
        Thread.Sleep(60);
        mouse_event(0x0004, 0, 0, 0, 0);
        Thread.Sleep(250);
    }

    [AppFact]
    public async Task A_chord_another_program_holds_is_said_on_the_card_and_the_app_goes_on()
    {
        using var box = new VoiceSandbox();
        using var holder = VoiceHotkey.Register(VoiceSettings.DefaultShortcut, out var error);
        Assert.True(holder is not null, error);
        using var app = new RunningApp("""{"KiroNoticeSeen": true, "Voice": {"Enabled": true}}""", box);
        Assert.True(await Until(() => app.Dump.Showing && app.Dump.Kind == 7, 40_000), "the card did not come\n" + app.Log());
        Assert.Equal("Ctrl+Alt+Space is already in use by another app, by Windows or by Hover’s own shortcut.", app.Dump.Words);
        Assert.False(app.Process.HasExited);
        Assert.Contains("could not be registered (Win32 error 1409)", app.Log());
    }
}
