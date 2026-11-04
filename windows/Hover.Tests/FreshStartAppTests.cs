using System.Diagnostics;
using System.Runtime.Versioning;
using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The owner's report on v5.0.18 ("cant scroll and start task"), in the real app with real mouse and keyboard input, on a data folder
/// that has never run: the note stands over the office until Got it; the folder list (30 folders, far more than fit) is reached with the
/// wheel and its last folder picked; a task is typed and sent by the round button and again with Enter; the stand-in agent asks and is
/// allowed, and its answer is in the chat. Once in the app window and once in the open notch, where the list is also copied off the screen.
/// </summary>
[SupportedOSPlatform("windows")]
public class FreshStartAppTests(ITestOutputHelper output)
{
    const int Projects = 30;

    static async Task<bool> Until(Func<bool> condition, int milliseconds = 30_000)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(100);
        }
        return condition();
    }

    /// <summary>A data folder that has never run, with 30 registered folders: the last is the sandbox's project.</summary>
    static string Settings(Sandbox sandbox, bool notch)
    {
        var projects = new JsonArray();
        for (var i = 1; i < Projects; i++)
            projects.Add(new JsonObject { ["Id"] = $"p{i:00}", ["Name"] = $"Project {i:00}", ["Folder"] = Path.Combine(sandbox.Root, $"project-{i:00}"), ["Aliases"] = new JsonArray() });
        projects.Add(new JsonObject { ["Id"] = "p99", ["Name"] = "zz-sandbox", ["Folder"] = sandbox.Project, ["Aliases"] = new JsonArray() });
        var settings = new JsonObject { ["HoverOpensWorkspace"] = false, ["Projects"] = projects };
        if (notch)
        {
            settings["NotchItems"] = new JsonArray("claude");
            // The box's folder chip opens the folder dialog at once while the box has no folder (as the Go box does), and a test cannot
            // answer that dialog: the box starts in a folder, so that the chip opens the list.
            var start = Path.Combine(sandbox.Root, "project-01");
            Directory.CreateDirectory(start);
            settings["KiroFolder"] = start;
        }
        return settings.ToJsonString();
    }

    static string OutFolder()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "build.ps1"))) return Path.Combine(d.FullName, "out", "notch-live");
        return Path.Combine(Path.GetTempPath(), "hover-notch-live");
    }

    sealed class Run : IDisposable
    {
        public readonly Sandbox Sandbox = new();
        public readonly AppDump Dump;
        public readonly Process App;

        public Run(bool notch, params string[] args)
        {
            Directory.CreateDirectory(Sandbox.Data);
            File.WriteAllText(Path.Combine(Sandbox.Data, "settings.json"), Settings(Sandbox, notch));
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

    // The stand-in says "Sandbox reply"; when the backend ends a turn without having heard it, it writes its own line.
    static bool IsAnswer(string t) => t == "Sandbox reply" || t.StartsWith("Done. ", StringComparison.Ordinal);

    async Task Check(Run run, Func<bool> condition, string what, int milliseconds = 30_000)
    {
        if (await Until(condition, milliseconds)) return;
        var log = DataFolder.LogFile(run.Sandbox.Data);
        var tail = File.Exists(log) ? string.Join("\n", File.ReadAllLines(log).TakeLast(45)) : "(no log)";
        Assert.Fail($"{what}\n--- the panels:\n{run.Dump.Summary()}\n--- the log:\n{tail}");
    }

    /// <summary>A place is clicked once two looks, a moment apart, found it at the same spot (the layout moves while the office settles).</summary>
    async Task ClickAt(Run run, string part, string id, string what, Func<JsonNode, bool>? pick = null)
    {
        await Check(run, () => run.Dump.Where(part, id, pick) is not null, $"no '{id}' to click ({what})");
        (int X, int Y) p = run.Dump.Where(part, id, pick)!.Value;
        for (var i = 0; i < 30; i++)
        {
            await Task.Delay(400);
            var again = run.Dump.Where(part, id, pick);
            if (again is { } q && q == p) break;
            if (again is { } moved) p = moved;
        }
        output.WriteLine($"click {what} at {p.X},{p.Y}");
        Input.Click(p.X, p.Y);
    }

    /// <summary>The wheel, a notch at a time, over the open menu until the folder is in view; then the folder is clicked.</summary>
    async Task PickFromFarDown(Run run, string part, string row)
    {
        await Check(run, () => run.Dump.Section(part)?["menu"]?.GetValue<string>() == "project", "the folder list opened");
        var box = AppDump.Items(run.Dump.Section(part), "targets").Last(t => (string?)t["id"] == "menuBox");
        Assert.True(run.Dump.Where(part, "menu", n => (string?)n["s"] == row) is null, $"'{row}' is the last of {Projects} folders and must not be in view when the list opens");
        var middle = run.Dump.Where(part, "menuBox")!.Value;
        output.WriteLine($"the list's box is at {middle.X},{middle.Y} ({(double)box["w"]!} x {(double)box["h"]!})");
        // One notch of the real wheel moves the list 60 DIPs, as in the Go app (the panels' units, so the display's scale does not enter).
        var probe = AppDump.Items(run.Dump.Section(part), "targets").Where(t => (string?)t["id"] == "menu" && ((string?)t["s"] ?? "").Length > 0).ElementAt(2);
        var probeName = (string?)probe["s"];
        var probeY = (double)probe["y"]!;
        Input.Wheel(middle.X, middle.Y, -1);
        await Check(run, () => AppDump.Items(run.Dump.Section(part), "targets").LastOrDefault(t => (string?)t["id"] == "menu" && (string?)t["s"] == probeName) is { } m && Math.Abs((double)m["y"]! - (probeY - 60)) <= 1.5,
            $"one notch of the wheel moved the list 60 DIPs ('{probeName}' was at {probeY})");
        for (var i = 0; i < 40 && run.Dump.Where(part, "menu", n => (string?)n["s"] == row) is null; i++)
        {
            Input.Wheel(middle.X, middle.Y, -3);
            await Task.Delay(200);
        }
        await Check(run, () => run.Dump.Where(part, "menu", n => (string?)n["s"] == row) is not null, $"the wheel brought '{row}' into view");
        await ClickAt(run, part, "menu", row, n => (string?)n["s"] == row);
        await Check(run, () => run.Dump.Section(part)?["menu"] is null, "the list closed after the pick");
    }

    [AppFact]
    public async Task A_data_folder_that_never_ran_starts_its_first_task_in_the_app_window_by_the_note_the_wheel_a_click_and_Enter()
    {
        using var run = new Run(false, "--dashboard");
        var dump = run.Dump;
        await AppTests.WaitForLog(run.Sandbox.Data, "agent backend is ready", TimeSpan.FromSeconds(60), run.App);
        await Check(run, () => dump.Section("window") is not null, "the app window drew its panels");
        await Input.Park(run.App);

        // The note first: the + circle is under it and does nothing; Got it takes it away.
        await Check(run, () => (bool?)dump.Section("window")?["notice"] == true, "the first-task note shows");
        await ClickAt(run, "window", "fab", "the + circle under the note");
        await Task.Delay(800);
        Assert.Null(dump.Where("window", "taskText"));
        await ClickAt(run, "window", "noticeOk", "Got it");
        await Check(run, () => (bool?)dump.Section("window")?["notice"] == false, "Got it took the note away");

        // The chat view's start screen: the folder list, the wheel, the last folder.
        await ClickAt(run, "window", "view", "the chat view", n => (int)n["n"]! == 1);
        await Check(run, () => (bool?)dump.Section("window")?["chatView"] == true, "the chat view opened");
        await ClickAt(run, "window", "pillFolder", "the folder chip");
        await PickFromFarDown(run, "window", "zz-sandbox");
        await Check(run, () => dump.Chrome("window").Contains("project"), "the chip names the folder that was picked");

        // The agent and what it may do; then the words, and the round button.
        await ClickAt(run, "window", "pillAgent", "the agent pill");
        await ClickAt(run, "window", "menu", "Codex", n => (string?)n["s"] == "Codex");
        await ClickAt(run, "window", "pillAccess", "the access pill");
        await ClickAt(run, "window", "menu", "Ask always", n => (string?)n["s"] == "Ask always");
        await ClickAt(run, "window", "taskText", "the task box");
        Input.Type("Tidy it");
        await Check(run, () => (string?)dump.Section("window")?["taskText"] == "Tidy it", "the words reached the box");
        await ClickAt(run, "window", "startTask", "the round start button");
        await Check(run, () => dump.Where("window", "askAllow") is not null, "the task started and its question shows in the chat");
        await ClickAt(run, "window", "askAllow", "Allow in the chat");
        await Check(run, () => dump.Texts("window").Any(IsAnswer), "the agent's answer shows in the chat");

        // A second task, sent with Enter.
        await ClickAt(run, "window", "newChat", "New chat");
        await ClickAt(run, "window", "taskText", "the task box");
        Input.Type("Once more");
        await Check(run, () => (string?)dump.Section("window")?["taskText"] == "Once more", "the words reached the box again");
        Input.Key(Input.Enter);
        await Check(run, () => dump.Where("window", "askAllow") is not null, "Enter started the second task and its question shows");
        await ClickAt(run, "window", "askAllow", "Allow the second task");
        await Check(run, () => dump.Texts("window").Any(t => t.Contains("Once more") || IsAnswer(t)), "the second task ran");
        // The refusal the owner met never showed.
        var toast = (string?)dump.Section("window")?["toast"];
        Assert.True(toast is null || !toast.Contains("Review agent access", StringComparison.Ordinal), $"the backend refused a task: {toast}");
    }

    [AppFact]
    public async Task The_open_notch_keeps_its_lists_inside_and_the_wheel_reaches_the_last_folder()
    {
        using var run = new Run(true);
        var dump = run.Dump;
        var report = OutFolder();
        Directory.CreateDirectory(report);
        await AppTests.WaitForLog(run.Sandbox.Data, "hotkey Alt+N registered", TimeSpan.FromSeconds(60), run.App);
        await Task.Delay(1500);
        var notch = Desktop.Notch();
        Assert.True(notch != 0, "no window of class HoverNotch");
        Desktop.AltN();
        await Check(run, () => dump.Section("notch") is not null && (bool?)dump.Section("notch")?["notice"] == true, "the open notch shows the first-task note");
        await Task.Delay(2500);
        var rect = Desktop.Rect(notch);
        DesktopPicture.Take(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top).SavePng(Path.Combine(report, "notice-open.png"));

        await ClickAt(run, "notch", "noticeOk", "Got it");
        await Check(run, () => (bool?)dump.Section("notch")?["notice"] == false, "Got it took the note away in the notch");
        await ClickAt(run, "notch", "fab", "the + circle");
        await Check(run, () => dump.Where("notch", "pillFolder") is not null, "the new-task box opened in the notch");
        await ClickAt(run, "notch", "pillFolder", "the folder pill");
        await Check(run, () => dump.Section("notch")?["menu"]?.GetValue<string>() == "project", "the folder list opened in the notch");
        await Task.Delay(1500);

        // The list is inside the window of the notch and no taller than its office less 16 px.
        var section = dump.Section("notch")!;
        double left = (double)section["left"]!, top = (double)section["top"]!, scale = (double)section["scale"]!, viewH = (double)section["viewH"]!;
        var box = AppDump.Items(section, "targets").Last(t => (string?)t["id"] == "menuBox");
        double bx = left + (double)box["x"]! * scale, by = top + (double)box["y"]! * scale, bw = (double)box["w"]! * scale, bh = (double)box["h"]! * scale;
        rect = Desktop.Rect(notch);
        output.WriteLine($"the list is at {bx:0},{by:0} {bw:0} x {bh:0} on the screen; the notch window is {rect.Left},{rect.Top} to {rect.Right},{rect.Bottom}; the office is {viewH:0} DIPs tall");
        DesktopPicture.Take(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top).SavePng(Path.Combine(report, "folder-list-open.png"));
        Assert.True(bx >= rect.Left && by >= rect.Top && bx + bw <= rect.Right && by + bh <= rect.Bottom, "the folder list runs off the notch's window");
        Assert.True((double)box["h"]! <= viewH - 16 + 0.01, $"the folder list is {(double)box["h"]!} DIPs tall in an office {viewH} tall");

        // The wheel, as the owner turned it, reaches the last folder, which is then picked.
        await PickFromFarDown(run, "notch", "zz-sandbox");
        await Check(run, () => dump.Chrome("notch").Contains("project"), "the folder pill names the folder that was picked");
        DesktopPicture.Take(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top).SavePng(Path.Combine(report, "folder-picked.png"));
        Desktop.AltN();
    }
}
