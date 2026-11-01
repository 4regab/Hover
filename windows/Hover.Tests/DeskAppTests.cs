using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text.Json.Nodes;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>Keys held with Control, and the clipboard, for the checks of the real app.</summary>
[SupportedOSPlatform("windows")]
static class Chords
{
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, nuint extra);
    [DllImport("user32.dll", SetLastError = true)] static extern bool OpenClipboard(nint owner);
    [DllImport("user32.dll", SetLastError = true)] static extern bool CloseClipboard();
    [DllImport("user32.dll", SetLastError = true)] static extern bool EmptyClipboard();
    [DllImport("user32.dll", SetLastError = true)] static extern nint SetClipboardData(uint format, nint data);
    [DllImport("kernel32.dll")] static extern nint GlobalAlloc(uint flags, nuint bytes);
    [DllImport("kernel32.dll")] static extern nint GlobalLock(nint block);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(nint block);

    public static void Ctrl(byte vk)
    {
        keybd_event(0x11, 0, 0, 0);
        Thread.Sleep(40);
        keybd_event(vk, 0, 0, 0);
        Thread.Sleep(40);
        keybd_event(vk, 0, 2, 0);
        Thread.Sleep(40);
        keybd_event(0x11, 0, 2, 0);
        Thread.Sleep(300);
    }

    public const byte V = 0x56;

    /// <summary>A small bitmap (CF_DIB) on the clipboard, as a screenshot tool leaves it: 32 bits, the fourth byte of each pixel 0.</summary>
    public static void SetBitmap(int w, int h)
    {
        var pixels = new byte[w * h * 4];
        for (var i = 0; i < w * h; i++)
        {
            pixels[i * 4] = (byte)(40 + i * 7);
            pixels[i * 4 + 1] = 120;
            pixels[i * 4 + 2] = 200;
        }
        var dib = new byte[40 + pixels.Length];
        BitConverter.GetBytes(40).CopyTo(dib, 0);
        BitConverter.GetBytes(w).CopyTo(dib, 4);
        BitConverter.GetBytes(h).CopyTo(dib, 8);
        BitConverter.GetBytes((short)1).CopyTo(dib, 12);
        BitConverter.GetBytes((short)32).CopyTo(dib, 14);
        BitConverter.GetBytes(pixels.Length).CopyTo(dib, 20);
        pixels.CopyTo(dib, 40);
        for (var i = 0; i < 20; i++)
        {
            if (OpenClipboard(0)) break;
            Thread.Sleep(50);
        }
        try
        {
            EmptyClipboard();
            var block = GlobalAlloc(0x0002, (nuint)dib.Length);
            var ptr = GlobalLock(block);
            Marshal.Copy(dib, 0, ptr, dib.Length);
            GlobalUnlock(block);
            SetClipboardData(8, block);
        }
        finally { CloseClipboard(); }
    }
}

/// <summary>
/// The desk in the real app, with the stand-in agent and real mouse and keyboard input: a desk's card opens by a click on the
/// desk, its terminal runs a command, a file is opened, edited and saved, the diff is read, a run is stopped, a picture is
/// pasted and a file is attached with @; and the pull request tab with the GitHub CLI hidden, and not signed in.
/// </summary>
[SupportedOSPlatform("windows")]
public class DeskAppTests(ITestOutputHelper output)
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

    sealed class Session : IDisposable
    {
        public required Sandbox Sandbox;
        public required Process App;
        public required AppDump Dump;
        public string Data => Sandbox.Data;

        public void Dispose()
        {
            try { if (!App.HasExited) App.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
            App.Dispose();
            Sandbox.Dispose();
        }
    }

    static Session Launch(bool hideGh = false)
    {
        var sandbox = new Sandbox();
        var data = sandbox.Data;
        Directory.CreateDirectory(data);
        var folder = System.Text.Json.JsonSerializer.Serialize(sandbox.Project);
        File.WriteAllText(Path.Combine(data, "settings.json"), $$"""{"KiroNoticeSeen": true, "HoverOpensWorkspace": false, "NotchItems": ["claude"], "KiroFolder": {{folder}}}""");
        var dumpFile = Path.Combine(sandbox.Root, "ui-dump.json");
        var info = new ProcessStartInfo(TestEnvironment.AppExe!) { UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)! };
        info.ArgumentList.Add("--dashboard");
        foreach (var (name, value) in sandbox.Environment)
            if (value is not null) info.Environment[name] = value;
        info.Environment["HOVER_DATA_DIR"] = data;
        info.Environment["HOVER_UI_DUMP"] = dumpFile;
        if (hideGh)
        {
            // gh is looked for on PATH and in Program Files: neither has it.
            var empty = Directory.CreateTempSubdirectory("hover-nogh.").FullName;
            foreach (var v in new[] { "ProgramFiles", "ProgramW6432", "ProgramFiles(x86)", "LOCALAPPDATA" }) info.Environment[v] = empty;
            info.Environment["PATH"] = string.Join(Path.PathSeparator, (info.Environment["PATH"] ?? "").Split(Path.PathSeparator).Where(p => !p.Contains("GitHub CLI", StringComparison.OrdinalIgnoreCase)));
        }
        return new Session { Sandbox = sandbox, App = Process.Start(info)!, Dump = new AppDump(dumpFile) };
    }

    async Task Check(Session s, Func<bool> condition, string what, int milliseconds = 30_000)
    {
        if (await Until(condition, milliseconds)) return;
        var log = DataFolder.LogFile(s.Data);
        var tail = File.Exists(log) ? string.Join("\n", File.ReadAllLines(log).TakeLast(45)) : "(no log)";
        var agent = Path.Combine(s.Sandbox.Root, "agent-log.txt");
        var said = File.Exists(agent) ? string.Join("\n", File.ReadAllLines(agent).TakeLast(20)) : "(the stand-in agent wrote nothing)";
        Assert.Fail($"{what}\n--- the panels:\n{s.Dump.Summary()}\n--- the log:\n{tail}\n--- the stand-in agent:\n{said}");
    }

    // A place is clicked only once two looks, a moment apart, found it at the same spot (the layout moves while the agent works).
    async Task ClickAt(Session s, string id, string what, Func<JsonNode, bool>? pick = null)
    {
        await Check(s, () => s.Dump.Where("window", id, pick) is not null, $"no '{id}' to click ({what})");
        (int X, int Y) p = s.Dump.Where("window", id, pick)!.Value;
        for (var i = 0; i < 30; i++)
        {
            await Task.Delay(400);
            var again = s.Dump.Where("window", id, pick);
            if (again is { } q && q == p) break;
            if (again is { } moved) p = moved;
        }
        output.WriteLine($"click {what} at {p.X},{p.Y}");
        Input.Click(p.X, p.Y);
    }

    // Clicked again until what it should have done shows.
    async Task ClickUntil(Session s, string id, string what, Func<bool> done, Func<JsonNode, bool>? pick = null)
    {
        for (var attempt = 1; attempt <= 4; attempt++)
        {
            await ClickAt(s, id, $"{what} (try {attempt})", pick);
            if (await Until(done, 3000)) return;
        }
        await Check(s, done, $"{what}: the click did not do its work");
    }

    // What the stand-in agent wrote, read while it may still be writing.
    static string AgentSaid(string file)
    {
        try
        {
            using var f = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            return new StreamReader(f).ReadToEnd();
        }
        catch (IOException) { return ""; }
    }

    static Func<JsonNode, bool> Of(string text) => n => (string?)n["s"] == text;

    static bool IsAnswer(string t) => t == "Sandbox reply" || t.StartsWith("Done. ", StringComparison.Ordinal);

    string Panel(Session s, string key) => s.Dump.Section("window")?[key]?.ToString() ?? "";

    bool ChromeHas(Session s, string text) => s.Dump.Chrome("window").Any(t => t.Contains(text, StringComparison.Ordinal));

    /// <summary>A task is started from the chat view, its permission allowed, and its answer seen.</summary>
    async Task Finished(Session s, string prompt = "Tidy it")
    {
        await AppTests.WaitForLog(s.Data, "agent backend is ready", TimeSpan.FromSeconds(60), s.App);
        await Check(s, () => s.Dump.Section("window") is not null, "the app window drew its panels");
        await Input.Park(s.App);
        await ClickAt(s, "view", "the chat view", n => (int)n["n"]! == 1);
        await Check(s, () => (bool?)s.Dump.Section("window")?["chatView"] == true, "the chat view opened");
        await ClickAt(s, "pillAgent", "the agent pill");
        await Check(s, () => s.Dump.Where("window", "menu", Of("Codex")) is not null, "Codex is in the agent menu");
        await ClickAt(s, "menu", "Codex", Of("Codex"));
        await ClickAt(s, "pillAccess", "the access pill");
        await ClickAt(s, "menu", "Ask always", Of("Ask always"));
        await ClickAt(s, "taskText", "the task box");
        Input.Type(prompt);
        await Check(s, () => (string?)s.Dump.Section("window")?["taskText"] == prompt, "the words reached the box");
        await ClickAt(s, "startTask", "the round start button");
        await Check(s, () => s.Dump.Where("window", "askAllow") is not null, "the permission question shows in the chat");
        await ClickAt(s, "askAllow", "Allow");
        await Check(s, () => s.Dump.Texts("window").Any(IsAnswer), "the agent's answer shows");
    }

    [AppFact]
    public async Task A_desk_card_opens_by_a_click_on_the_desk_and_its_panels_work()
    {
        using var s = Launch();
        try
        {
            await Finished(s);

            // The office: the bot sits at its desk; a click on the desk opens the card, a click on the bot opens the chat.
            await ClickUntil(s, "view", "the office view", () => (bool?)s.Dump.Section("window")?["chatView"] == false, n => (int)n["n"]! == 0);
            await ClickTheDesk(s);
            await Check(s, () => s.Dump.Section("window")?["deskCard"] is not null, "a click on the desk opened its card");
            await Check(s, () => s.Dump.Where("window", "cardTile", Of("terminal")) is not null, "the card has its tiles");
            Assert.NotNull(s.Dump.Where("window", "cardChat"));

            // The terminal: PowerShell in the session's folder runs a command and shows what it printed.
            await ClickAt(s, "cardTile", "the Terminal tile", Of("terminal"));
            await Check(s, () => (string?)s.Dump.Section("window")?["deskTab"] == "terminal" && s.Dump.Section("window")?["deskPanel"] is not null, "the terminal opened");
            await Check(s, () => s.Dump.Where("window", "termBox") is not null, "the terminal has its prompt");
            await ClickAt(s, "termBox", "the terminal's prompt");
            Input.Type("echo desk-terminal-ok");
            Input.Key(Input.Enter);
            await Check(s, () => ChromeHas(s, "desk-terminal-ok"), "the command's output shows in the terminal");

            // Files: the app.js the stand-in changed is listed as changed; it opens, is edited and saved to disk.
            await ClickAt(s, "deskTab", "the Files tab", Of("files"));
            await Check(s, () => s.Dump.Where("window", "drow", Of("file:app.js")) is not null, "app.js is in the folder's list");
            await ClickAt(s, "drow", "app.js", Of("file:app.js"));
            await Check(s, () => s.Dump.Where("window", "fEdit") is not null, "the file opened with its edit button");
            await ClickAt(s, "fEdit", "Edit");
            await Check(s, () => s.Dump.Where("window", "fSave") is not null, "the file is in the editor");
            Input.Type("// saved from the desk");
            await ClickAt(s, "fSave", "Save");
            var appJs = Path.Combine(s.Sandbox.Project, "app.js");
            await Check(s, () => File.ReadAllText(appJs).Contains("// saved from the desk", StringComparison.Ordinal), "the file on disk has the edit");
            output.WriteLine("app.js: " + File.ReadAllText(appJs).Replace("\n", "\\n"));

            // The diff names the file and shows the change with its numbers.
            await ClickAt(s, "deskTab", "the Diff tab", Of("diff"));
            await Check(s, () => ChromeHas(s, "app.js") && ChromeHas(s, "saved from the desk"), "the diff shows the saved line");
            Assert.Contains(s.Dump.Chrome("window"), t => t.Contains("file", StringComparison.Ordinal) && t.Contains("changed", StringComparison.Ordinal));

            // Screen: the desktop is taken for real (the wallpaper under the app's windows) and shown; Watch live starts and stops. The panel's words are in the dump's chrome.
            await ClickAt(s, "deskTab", "the Screen tab", Of("screen"));
            await Check(s, () => s.Dump.Where("window", "sWatch") is not null && ChromeHas(s, "Desktop"), "the Screen tab has its Watch live button");
            await Check(s, () => !ChromeHas(s, "Reading…"), "the picture of the desktop arrived");
            await ClickAt(s, "sWatch", "Watch live");
            await Check(s, () => ChromeHas(s, "Stop watching") && ChromeHas(s, "Live"), "the tab is live");
            await ClickAt(s, "sWatch", "Stop watching");
            await Check(s, () => ChromeHas(s, "Watch live") && ChromeHas(s, "Desktop"), "the tab stopped watching");

            // The panel closes, and the chat is the way on: a picture is pasted into the reply, and a file attached with @.
            await ClickAt(s, "deskClose", "the panel's close button");
            await Check(s, () => s.Dump.Section("window")?["deskPanel"] is null, "the panel closed");
            await ClickUntil(s, "view", "the chat view", () => (bool?)s.Dump.Section("window")?["chatView"] == true, n => (int)n["n"]! == 1);
            // The chat view lists the sessions down the left: the one that ran is opened from there.
            await ClickUntil(s, "row", "the session in the list", () => s.Dump.Section("window")?["openId"] is not null, n => (int)n["n"]! == 0);
            await ClickUntil(s, "compose", "the reply circle", () => (bool?)s.Dump.Section("window")?["composing"] == true);
            Chords.SetBitmap(24, 16);
            Chords.Ctrl(Chords.V);
            await Check(s, () => (int?)s.Dump.Section("window")?["shots"] == 1, "the pasted picture is on the reply");
            Input.Type("Look at @app");
            await Check(s, () => (int?)s.Dump.Section("window")?["popRows"] > 0, "the @ list offers files");
            Input.Key(Input.Enter);
            await Check(s, () => (int?)s.Dump.Section("window")?["chips"] == 1, "the picked file is a chip");
            Input.Type("please");
            Input.Key(Input.Enter);
            // The picture and the chip reached the agent: the stand-in takes no images, so its prompt names the picture's file ("Attached image")
            // and has the "Attached by Hover" note for the chip.
            var agentLog = Path.Combine(s.Sandbox.Root, "agent-log.txt");
            await Check(s, () => AgentSaid(agentLog).Contains("Attached image (read it from this file)", StringComparison.Ordinal), "the agent was sent the picture");
            await Check(s, () => AgentSaid(agentLog).Contains("Attached by Hover", StringComparison.Ordinal), "the agent was sent the attached file");
            await Check(s, () => s.Dump.Where("window", "askAllow") is not null, "the reply asks");
            await ClickAt(s, "askAllow", "Allow the reply");
            await Check(s, () => s.Dump.Texts("window").Count(IsAnswer) >= 2, "the second answer shows");

            // A run is stopped with the square.
            await ClickAt(s, "newChat", "New chat");
            await ClickAt(s, "taskText", "the task box");
            Input.Type("[slow] work");
            await ClickAt(s, "startTask", "the round start button");
            await Check(s, () => s.Dump.Texts("window").Any(t => t.Contains("[slow] work")), "the slow task opened its chat");
            await ClickUntil(s, "compose", "the reply circle", () => (bool?)s.Dump.Section("window")?["composing"] == true);
            await ClickAt(s, "pause", "the stop square");
            await Check(s, () => s.Dump.Where("window", "pause") is null, "the run stopped and the square went");
        }
        finally { output.WriteLine(s.Dump.Summary()); }
    }

    [AppFact]
    public async Task The_pull_request_tab_offers_the_GitHub_CLI_setup_when_gh_is_missing()
    {
        using var s = Launch(hideGh: true);
        try
        {
            await Finished(s);
            await OpenPanelFromTheChatMenu(s, "Files & changes");
            await ClickAt(s, "deskTab", "the Pull request tab", Of("pr"));
            await Check(s, () => ChromeHas(s, "Set up the GitHub CLI"), "the pull request tab offers to set up the GitHub CLI");
            Assert.True(ChromeHas(s, "Install and sign in"));
        }
        finally { output.WriteLine(s.Dump.Summary()); }
    }

    [AppFact]
    public async Task The_GitHub_sign_in_can_be_started_and_cancelled_when_gh_is_not_signed_in()
    {
        using var s = Launch();
        try
        {
            await Finished(s);
            await OpenPanelFromTheChatMenu(s, "Files & changes");
            await ClickAt(s, "deskTab", "the Pull request tab", Of("pr"));
            // gh is on the runner and not signed in: the card offers the sign-in. (Where gh is missing, it offers to set it up.)
            await Check(s, () => ChromeHas(s, "Sign in with GitHub") || ChromeHas(s, "Install and sign in") || ChromeHas(s, "Create pull request"), "the pull request tab answered");
            if (!ChromeHas(s, "Sign in with GitHub")) return;
            await ClickAt(s, "ghStart", "Sign in with GitHub");
            // Busy (with GitHub's code), or it failed at once for want of a network; Cancel ends the first.
            await Check(s, () => s.Dump.Where("window", "ghCancel") is not null || s.Dump.Section("window")?["ghBusy"]?.GetValue<bool>() == false && ChromeHas(s, "Sign in with GitHub"), "the sign-in started or ended");
            // A Cancel that comes while gh is still starting can be followed by the sign-in showing itself once more: Cancel is pressed
            // again until the card is back at its start.
            for (var attempt = 1; attempt <= 3 && s.Dump.Where("window", "ghCancel") is not null; attempt++)
            {
                await ClickAt(s, "ghCancel", $"Cancel (try {attempt})");
                await Until(() => s.Dump.Where("window", "ghCancel") is null, 8000);
            }
            await Check(s, () => s.Dump.Where("window", "ghCancel") is null && s.Dump.Where("window", "ghStart") is not null, "Cancel ended the sign-in");
        }
        finally { output.WriteLine(s.Dump.Summary()); }
    }

    // The office says where the bot is (its box, in the panels' pixels). The desk is in front of it, to its right: the points
    // are fractions of that box, and the first is the middle of the part of the desk the bot does not cover (found with the
    // office's own picking). A click on the bot opens its chat instead: that is put away and the next point tried.
    async Task ClickTheDesk(Session s)
    {
        JsonNode? Bot() => s.Dump.Section("window")?["bots"]?.AsArray().FirstOrDefault();
        double[]? seen = null;
        var since = Environment.TickCount64;
        bool Still()
        {
            if (Bot() is not { } b) return false;
            double[] now = [(double)b["x"]!, (double)b["y"]!, (double)b["w"]!, (double)b["h"]!];
            if (seen is null || now.Zip(seen, (a, c) => Math.Abs(a - c) > 0.5).Any(moved => moved)) { seen = now; since = Environment.TickCount64; }
            return Environment.TickCount64 - since >= 1500;
        }
        await Check(s, () => Bot() is not null, "the office says where the bot is", 60_000);
        await Check(s, Still, "the bot stands still at its desk", 60_000);
        var spots = new List<(double Fx, double Fy)> { (1.3, 0.75), (1.45, 0.6), (1.2, 0.9), (1.5, 0.85), (1.6, 0.7), (1.1, 1.0), (-0.3, 0.75), (-0.45, 0.6) };
        for (var fy = 0.2; fy <= 1.5; fy += 0.15)
            for (var fx = -0.8; fx <= 2.2; fx += 0.2) spots.Add((fx, fy));
        foreach (var (fx, fy) in spots.Take(80))
        {
            var window = s.Dump.Section("window")!;
            var bot = Bot()!;
            double left = (double)window["left"]!, top = (double)window["top"]!, scale = (double)window["scale"]!;
            var x = (int)Math.Round(left + ((double)bot["x"]! + fx * (double)bot["w"]!) * scale);
            var y = (int)Math.Round(top + ((double)bot["y"]! + fy * (double)bot["h"]!) * scale);
            Input.Click(x, y);
            await Task.Delay(300);
            if (s.Dump.Section("window")?["deskCard"] is not null) { output.WriteLine($"the desk was at {fx:0.00} of the bot's width and {fy:0.00} of its height"); return; }
            if (s.Dump.Where("window", "close") is { } close)
            {
                Input.Click(close.X, close.Y);
                await Check(s, () => s.Dump.Section("window")?["openId"] is null, "the chat the click opened was put away");
                seen = null;
                await Check(s, Still, "the office stands still again", 30_000);
            }
        }
    }

    async Task OpenPanelFromTheChatMenu(Session s, string row)
    {
        await ClickUntil(s, "more", "the chat's menu", () => (string?)s.Dump.Section("window")?["menu"] == "more");
        await ClickAt(s, "menu", row, Of(row));
        await Check(s, () => s.Dump.Section("window")?["deskPanel"] is not null, $"{row} opened the panel");
    }
}
