using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text.Json.Nodes;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>Mouse and keyboard input as a person makes it, for the checks of the real app.</summary>
[SupportedOSPlatform("windows")]
static class Input
{
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, flags, time; public nuint extra; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public nuint extra; }
    [StructLayout(LayoutKind.Explicit)] struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public InputUnion u; }

    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, int dx, int dy, int data, nuint extra);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);

    public static void Move(int x, int y) => SetCursorPos(x, y);

    public static void Click(int x, int y)
    {
        SetCursorPos(x, y);
        Thread.Sleep(250);
        mouse_event(0x0002, 0, 0, 0, 0);
        Thread.Sleep(60);
        mouse_event(0x0004, 0, 0, 0, 0);
        Thread.Sleep(250);
    }

    public static void Wheel(int x, int y, int notches)
    {
        SetCursorPos(x, y);
        Thread.Sleep(250);
        mouse_event(0x0800, 0, 0, notches * 120, 0);
        Thread.Sleep(250);
    }

    /// <summary>Types text as a keyboard would, any character, one after the other.</summary>
    public static void Type(string text)
    {
        foreach (var c in text)
        {
            var down = new INPUT { type = 1, u = new InputUnion { ki = new KEYBDINPUT { scan = c, flags = 0x0004 } } };
            var up = new INPUT { type = 1, u = new InputUnion { ki = new KEYBDINPUT { scan = c, flags = 0x0004 | 0x0002 } } };
            SendInput(2, [down, up], Marshal.SizeOf<INPUT>());
            Thread.Sleep(15);
        }
        Thread.Sleep(200);
    }

    public static void Key(byte vk)
    {
        keybd_event(vk, 0, 0, 0);
        Thread.Sleep(30);
        keybd_event(vk, 0, 2, 0);
        Thread.Sleep(250);
    }

    public const byte Enter = 0x0D, Escape = 0x1B;
}

/// <summary>The app's UI dump (HOVER_UI_DUMP) as a test reads it.</summary>
sealed class AppDump(string file)
{
    public JsonNode? Read()
    {
        for (var i = 0; i < 5; i++)
        {
            try
            {
                if (!File.Exists(file)) return null;
                // Shared so that the app can replace the file while it is read.
                using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                return JsonNode.Parse(new StreamReader(stream).ReadToEnd());
            }
            catch (IOException) { Thread.Sleep(20); }
            catch (System.Text.Json.JsonException) { Thread.Sleep(20); }
        }
        return null;
    }

    public JsonNode? Section(string name) => Read()?[name];

    public static IEnumerable<JsonNode> Items(JsonNode? section, string array) => section?[array]?.AsArray().Select(n => n!) ?? [];

    /// <summary>The screen point at the middle of a place of the panels (window or notch section), or null.</summary>
    public (int X, int Y)? Where(string part, string id, Func<JsonNode, bool>? pick = null)
    {
        var s = Section(part);
        if (s is null) return null;
        var t = Items(s, "targets").LastOrDefault(n => (string?)n["id"] == id && (pick is null || pick(n)));
        return t is null ? null : Screen(s, t);
    }

    public (int X, int Y)? Act(string part, string kind, int section)
    {
        var s = Section(part);
        var t = Items(s, "acts").LastOrDefault(n => (string?)n["kind"] == kind && (int?)n["section"] == section);
        return t is null || s is null ? null : Screen(s, t);
    }

    static (int X, int Y) Screen(JsonNode s, JsonNode t)
    {
        double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
        double x = (double)t["x"]!, y = (double)t["y"]!, w = (double)t["w"]!, h = (double)t["h"]!;
        return ((int)Math.Round(left + (x + w / 2) * scale), (int)Math.Round(top + (y + h / 2) * scale));
    }

    /// <summary>The place on the notch bar (its island or card) for an action, as a screen point.</summary>
    public (int X, int Y)? Bar(string action)
    {
        var s = Section("bar");
        var t = Items(s, "targets").LastOrDefault(n => (string?)n["action"] == action);
        if (t is null || s is null) return null;
        double left = (double)s["left"]!, top = (double)s["top"]!, scale = (double)s["scale"]!;
        double x = (double)t["x"]!, y = (double)t["y"]!, w = (double)t["w"]!, h = (double)t["h"]!;
        return ((int)Math.Round(left + (x + w / 2) * scale), (int)Math.Round(top + (y + h / 2) * scale));
    }

    public IReadOnlyList<string> Texts(string part) => Items(Section(part), "texts").Select(n => (string)n!).ToList();

    public IReadOnlyList<string> Chrome(string part) => Items(Section(part), "chrome").Select(n => (string)n!).ToList();

    public string Summary()
    {
        var all = Read()?.ToJsonString() ?? "(no dump)";
        return all.Length > 3000 ? all[..3000] + "…" : all;
    }
}

/// <summary>
/// The whole flow, in the real app with the stand-in agent and real mouse and keyboard input: a task is
/// started from the new-task flow, a permission is answered from the chat and another from the notch's
/// card, a reply is queued while the agent works, the run is paused and resumed, and a checkpoint is
/// restored and a message tried again.
/// </summary>
[SupportedOSPlatform("windows")]
public class PanelsAppTests(ITestOutputHelper output)
{
    static async Task<bool> Until(Func<bool> condition, int milliseconds = 30_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(100);
        }
        return condition();
    }

    [AppFact]
    public async Task A_task_is_run_from_the_new_task_flow_with_its_permissions_answered_queued_paused_and_restored()
    {
        using var sandbox = new Sandbox();
        var data = sandbox.Data;
        Directory.CreateDirectory(data);
        // The folder the last task ran in is the one the new-task box starts with.
        var folder = System.Text.Json.JsonSerializer.Serialize(sandbox.Project);
        File.WriteAllText(Path.Combine(data, "settings.json"), $$"""{"KiroNoticeSeen": true, "HoverOpensWorkspace": false, "NotchItems": ["claude"], "KiroFolder": {{folder}}}""");
        var dumpFile = Path.Combine(sandbox.Root, "ui-dump.json");
        var dump = new AppDump(dumpFile);
        var info = new ProcessStartInfo(TestEnvironment.AppExe!)
        {
            UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)!,
        };
        info.ArgumentList.Add("--dashboard");
        foreach (var (name, value) in sandbox.Environment)
            if (value is not null) info.Environment[name] = value;
        info.Environment["HOVER_DATA_DIR"] = data;
        info.Environment["HOVER_UI_DUMP"] = dumpFile;
        using var app = Process.Start(info)!;

        // The stand-in says "Sandbox reply"; when the backend ends a turn without having heard it, it writes its own line.
        static bool IsAnswer(string t) => t == "Sandbox reply" || t.StartsWith("Done. ", StringComparison.Ordinal);

        async Task Check(Func<bool> condition, string what, int milliseconds = 30_000)
        {
            if (await Until(condition, milliseconds)) return;
            var log = DataFolder.LogFile(data);
            var tail = File.Exists(log) ? string.Join("\n", File.ReadAllLines(log).TakeLast(45)) : "(no log)";
            var agent = Path.Combine(sandbox.Root, "agent-log.txt");
            var said = File.Exists(agent) ? string.Join("\n", File.ReadAllLines(agent).TakeLast(30)) : "(the stand-in agent wrote nothing)";
            Assert.Fail($"{what}\n--- the panels:\n{dump.Summary()}\n--- the log:\n{tail}\n--- the stand-in agent:\n{said}");
        }

        // The thread opens at its end: scroll up with the wheel until the button is in view.
        async Task<(int X, int Y)> FindAct(string kind, int section, string what)
        {
            for (var i = 0; i < 12; i++)
            {
                if (dump.Act("window", kind, section) is { } p)
                {
                    // Looked at twice, a moment apart, so a layout that is still moving is not clicked.
                    for (var k = 0; k < 20; k++)
                    {
                        await Task.Delay(400);
                        var again = dump.Act("window", kind, section);
                        if (again is { } q && q == p) return p;
                        if (again is { } moved) p = moved;
                    }
                    return p;
                }
                var spot = dump.Where("window", "compose") ?? dump.Where("window", "composer");
                if (spot is { } w) Input.Wheel(w.X, w.Y, 4);
                await Task.Delay(300);
            }
            Assert.Fail($"{what}: no {kind} button in view\n{dump.Summary()}");
            return default;
        }

        // The layout moves while the agent works (a question goes, the answer comes), so a place is clicked only
        // once two looks, a moment apart, found it at the same spot.
        async Task ClickAt(string part, string id, string what, Func<JsonNode, bool>? pick = null)
        {
            await Check(() => dump.Where(part, id, pick) is not null, $"no '{id}' to click ({what})");
            (int X, int Y) p = dump.Where(part, id, pick)!.Value;
            for (var i = 0; i < 30; i++)
            {
                await Task.Delay(400);
                var again = dump.Where(part, id, pick);
                if (again is { } q && q == p) break;
                if (again is { } moved) p = moved;
            }
            output.WriteLine($"click {what} at {p.X},{p.Y}");
            Input.Click(p.X, p.Y);
        }

        try
        {
            await AppTests.WaitForLog(data, "agent backend is ready", TimeSpan.FromSeconds(60), app);
            await Check(() => dump.Section("window") is not null, "the app window drew its panels");

            // The chat view, and the start screen's new-task box.
            await ClickAt("window", "view", "the chat view", n => (int)n["n"]! == 1);
            await Check(() => (bool?)dump.Section("window")?["chatView"] == true, "the chat view opened");
            await ClickAt("window", "pillAgent", "the agent pill");
            await Check(() => dump.Section("window")?["menu"]?.GetValue<string>() == "agent", "the agent menu opened");
            await Check(() => dump.Where("window", "menu", n => (string?)n["s"] == "Codex") is not null, "Codex is in the agent menu (the stand-in is ready)");
            await ClickAt("window", "menu", "Codex", n => (string?)n["s"] == "Codex");
            await ClickAt("window", "pillAccess", "the access pill");
            await ClickAt("window", "menu", "Ask always", n => (string?)n["s"] == "Ask always");
            await ClickAt("window", "taskText", "the task box");
            Input.Type("Tidy it");
            await Check(() => (string?)dump.Section("window")?["taskText"] == "Tidy it", "the words reached the box");
            await ClickAt("window", "startTask", "the round start button");

            // The permission comes up in the chat, and is answered there.
            await Check(() => dump.Where("window", "askAllow") is not null, "the permission question shows in the chat");
            await ClickAt("window", "askAllow", "Allow in the chat");
            await Check(() => dump.Texts("window").Any(IsAnswer), "the agent's answer shows after the permission is allowed from the chat");
            await Check(() => dump.Section("window")!["acts"]!.AsArray().Any(a => (string?)a!["kind"] == "Copy"), "the finished turn offers Copy");
            Assert.Equal("a\nb\n", File.ReadAllText(Path.Combine(sandbox.Project, "app.js")).Replace("\r\n", "\n"));

            // A reply in the same chat gives a second turn, which asks too: answered from the notch's card.
            await ClickAt("window", "compose", "the reply circle");
            Input.Type("And once more");
            await Check(() => (string?)dump.Section("window")?["replyText"] == "And once more", "the words reached the reply box");
            Input.Key(Input.Enter);
            await Check(() => dump.Section("bar") is { } bar && (bool?)bar["asking"] == true, "the notch's island shows the second question");
            await Check(() => dump.Section("bar")!["targets"]!.AsArray().Any(t => (string?)t!["action"] == "review"), "the island offers Review");
            var review = dump.Bar("review")!.Value;
            output.WriteLine($"click Review at {review.X},{review.Y}");
            Input.Click(review.X, review.Y);
            await Check(() => (bool?)dump.Section("bar")?["card"] == true, "the notch's card opened");
            await Check(() => dump.Bar("cardAllow") is not null, "the card has its allow button");
            var allow = dump.Bar("cardAllow")!.Value;
            output.WriteLine($"click the card's allow at {allow.X},{allow.Y}");
            Input.Click(allow.X, allow.Y);
            await Check(() => dump.Texts("window").Count(IsAnswer) >= 2, "the second answer shows after the permission is allowed from the notch");

            // Restore the first turn's checkpoint: the second turn goes.
            Assert.Contains("And once more", dump.Texts("window"));
            var restore = await FindAct("Restore", 0, "the first turn offers Restore");
            Input.Click(restore.X, restore.Y);
            await ClickAt("window", "confirmYes", "confirm the restore");
            await Check(() => !dump.Texts("window").Contains("And once more"), "the second turn is gone after the restore");

            // Try the first message again: it goes again, and asks again.
            var again = await FindAct("TryAgain", 0, "the first turn offers Try again");
            Input.Click(again.X, again.Y);
            await ClickAt("window", "confirmYes", "confirm trying again");
            await Check(() => dump.Where("window", "askAllow") is not null, "the message went again and asks again");
            await ClickAt("window", "askAllow", "Allow again");
            await Check(() => dump.Texts("window").Any(IsAnswer), "the answer shows again");

            // A new task that never ends by itself: a reply is queued behind it, the run is paused, then resumed.
            await ClickAt("window", "newChat", "New chat");
            await ClickAt("window", "taskText", "the task box");
            Input.Type("[slow] work");
            await ClickAt("window", "startTask", "the round start button");
            await Check(() => dump.Texts("window").Any(t => t.Contains("[slow] work")), "the slow task opened its chat");
            await ClickAt("window", "compose", "the reply circle");
            Input.Type("Then do this");
            Input.Key(Input.Enter);
            await Check(() => dump.Texts("window").Any(t => t.Contains("Queued")), "the reply is queued behind the run");
            await ClickAt("window", "compose", "the reply circle");
            await ClickAt("window", "pause", "the pause button");
            // Pause ends the run and sends the queued reply once; the agent asks about it.
            await Check(() => dump.Where("window", "askAllow") is not null, "the queued reply went after the pause and asks");
            await ClickAt("window", "askAllow", "Allow the queued reply");
            await Check(() => dump.Texts("window").Any(IsAnswer), "the queued reply's answer shows");
        }
        finally
        {
            try { if (!app.HasExited) app.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
            output.WriteLine(dump.Summary());
        }
    }
}
