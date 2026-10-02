using System.Diagnostics;
using System.IO;
using System.Text.Json;
using Hover.Owl;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// Hover's browser for agents (its MCP server over the socket and relay, and how its
/// steps read), the screen panel's agent apps, the GitHub CLI's setup output, and
/// Create pull request against a real git repository with a stand-in gh.
[TestFixture, NonParallelizable]
public sealed class BrowserAndGitHubTests
{
    private string _dir = "";

    [SetUp]
    public void SetUp()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-bg-" + Guid.NewGuid().ToString("N")[..8]);
        Directory.CreateDirectory(_dir);
    }

    [TearDown]
    public void TearDown()
    {
        try { Directory.Delete(_dir, true); } catch (IOException) { }
    }

    // MARK: Hover's browser

    [Test]
    public void Browser_tool_calls_are_named_whatever_the_tool_calls_them()
    {
        Assert.That(BrowserTool.Op("hover-browser/browser_open"), Is.EqualTo("open"));
        Assert.That(BrowserTool.Op("mcp__hover-browser__browser_click"), Is.EqualTo("click"));
        Assert.That(BrowserTool.Op("browser_screenshot"), Is.EqualTo("screenshot"));
        Assert.That(BrowserTool.Op("Ran npm test"), Is.Null);
        Assert.That(BrowserTool.Op("browser_prepare"), Is.Null);
    }

    [Test]
    public void Browser_clicks_are_not_computer_use()
    {
        var click = new KiroStep("1", "other", "mcp__hover-browser__browser_click", null, "completed", Input: "{\"ref\":3}");
        var cua = new KiroStep("2", "other", "mcp__cua-driver__click", null, "completed", Input: "{\"pid\":4242,\"window_id\":7}");
        Assert.That(DeskInfo.IsScreen(click), Is.False);
        Assert.That(DeskInfo.IsScreen(cua), Is.True);
    }

    [Test]
    public void The_screen_shows_the_apps_computer_use_opened_and_no_others()
    {
        var launch = new KiroStep("1", "other", "cua-driver/launch_app", null, "completed", Input: "{\"bundle_id\":\"com.example.Demo\",\"name\":\"Demo\"}",
            Log: "{\"pid\": 5150, \"window_id\": 12}");
        var click = new KiroStep("2", "other", "cua-driver/click", null, "completed", Input: "{\"pid\":5150,\"element_token\":\"t1\"}");
        var read = new KiroStep("3", "read", "Read a.cs", "a.cs", "completed", Input: "{\"pid\":99}");
        var apps = DeskInfo.Apps(new[] { launch, click, read });
        Assert.That(apps, Is.Not.Null);
        Assert.That(apps!.Pids, Is.EqualTo(new[] { 5150 }));
        Assert.That(apps.Bundles, Is.EqualTo(new[] { "com.example.Demo" }));
        Assert.That(apps.Names, Is.EqualTo(new[] { "Demo" }));
        Assert.That(DeskInfo.Apps(new[] { read }), Is.Null, "no computer use, no apps: the desktop alone");
    }

    [Test]
    public async Task The_MCP_server_answers_over_the_relay_and_passes_calls_to_the_host()
    {
        if (OperatingSystem.IsWindows() || !File.Exists("/usr/bin/perl")) Assert.Ignore("Needs perl and Unix sockets.");
        // Unix sockets take short paths, and srt allows them only in its temp folder.
        Environment.SetEnvironmentVariable("HOVER_BROWSER_SOCKET", Path.Combine(Path.GetTempPath(), $"hb-{Environment.ProcessId}.sock"));
        var sent = new List<JsonElement>();
        BrowserTool.Available = true;
        BrowserTool.Resolve = tag => Task.FromResult<int?>(tag == "key-1" ? 7 : null);
        BrowserTool.ToHost = m =>
        {
            var e = JsonSerializer.SerializeToElement(m);
            lock (sent) sent.Add(e);
            // The host's answer: text and a screenshot.
            BrowserTool.Complete(JsonSerializer.SerializeToElement(new { type = "browserResult", call = e.GetProperty("call").GetInt64(), ok = true, text = "Opened Demo — http://localhost:5173/", image = "AAAA" }));
        };
        try
        {
            var server = BrowserTool.Servers(AgentTool.Codex, "key-1").Single();
            Assert.That(server.Name, Is.EqualTo("hover-browser"));
            var psi = new ProcessStartInfo(server.Command) { RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false };
            foreach (var a in server.Args) psi.ArgumentList.Add(a);
            using var p = Process.Start(psi)!;
            // Killed whatever happens, so a failed test never leaves the relay behind.
            using var kill = new Disposer(() => { try { if (!p.HasExited) p.Kill(); } catch { } });
            async Task<JsonElement> Ask(object m)
            {
                await p.StandardInput.WriteLineAsync(JsonSerializer.Serialize(m));
                await p.StandardInput.FlushAsync();
                var line = await p.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(10));
                Assert.That(line, Is.Not.Null, "the relay closed its output");
                return JsonDocument.Parse(line!).RootElement.Clone();
            }
            var init = await Ask(new { jsonrpc = "2.0", id = 1, method = "initialize", @params = new { protocolVersion = "2025-06-18", capabilities = new { }, clientInfo = new { name = "test", version = "1" } } });
            Assert.That(init.GetProperty("result").GetProperty("serverInfo").GetProperty("name").GetString(), Is.EqualTo("hover-browser"));
            Assert.That(init.GetProperty("result").GetProperty("instructions").GetString(), Does.Contain("browser_snapshot"));
            await p.StandardInput.WriteLineAsync("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}");
            var list = await Ask(new { jsonrpc = "2.0", id = 2, method = "tools/list" });
            var names = list.GetProperty("result").GetProperty("tools").EnumerateArray().Select(t => t.GetProperty("name").GetString()).ToList();
            Assert.That(names, Does.Contain("browser_open").And.Contain("browser_click").And.Contain("browser_screenshot").And.Contain("browser_snapshot"));
            var call = await Ask(new { jsonrpc = "2.0", id = 3, method = "tools/call", @params = new { name = "browser_open", arguments = new { url = "localhost:5173" } } });
            var content = call.GetProperty("result").GetProperty("content").EnumerateArray().ToList();
            Assert.That(call.GetProperty("result").GetProperty("isError").GetBoolean(), Is.False);
            Assert.That(content[0].GetProperty("text").GetString(), Does.Contain("Opened Demo"));
            Assert.That(content[1].GetProperty("type").GetString(), Is.EqualTo("image"));
            lock (sent)
            {
                Assert.That(sent, Has.Count.EqualTo(1));
                Assert.That(sent[0].GetProperty("id").GetInt32(), Is.EqualTo(7), "the call goes to the session the token names");
                Assert.That(sent[0].GetProperty("op").GetString(), Is.EqualTo("open"));
                Assert.That(sent[0].GetProperty("args").GetProperty("url").GetString(), Is.EqualTo("localhost:5173"));
            }
            var unknown = await Ask(new { jsonrpc = "2.0", id = 4, method = "tools/call", @params = new { name = "rm_rf", arguments = new { } } });
            Assert.That(unknown.TryGetProperty("error", out _), Is.True);
            p.StandardInput.Close();
            Assert.That(p.WaitForExit(5000), Is.True);
        }
        finally
        {
            BrowserTool.Stop();
            BrowserTool.Available = false; BrowserTool.ToHost = null; BrowserTool.Resolve = null;
            Environment.SetEnvironmentVariable("HOVER_BROWSER_SOCKET", null);
        }
    }

    [Test]
    public void Computer_use_steps_read_as_what_was_done_on_the_desktop()
    {
        string Said(string title, string input) => DeskInfo.ScreenAction(new KiroStep("1", "other", title, null, "completed", Input: input)) is var (d, o) ? $"{d}|{o}" : "";
        Assert.That(Said("cua-driver/launch_app", "{\"bundle_id\":\"dev.demo\",\"name\":\"Demo\"}"), Is.EqualTo("Opened|Demo"));
        Assert.That(Said("mcp__cua-driver__click", "{\"pid\":1,\"x\":120,\"y\":80}"), Is.EqualTo("Clicked|at 120, 80"));
        Assert.That(Said("cua-driver: type_text", "{\"pid\":1,\"text\":\"Ada\"}"), Is.EqualTo("Typed|“Ada”"));
        Assert.That(Said("cua-driver/hotkey", "{\"pid\":1,\"keys\":[\"cmd\",\"s\"]}"), Is.EqualTo("Pressed|cmd+s"));
        Assert.That(Said("cua-driver/get_window_state", "{\"pid\":1}"), Is.EqualTo("Read the window|"));
        // A Cua Space's own tool names.
        Assert.That(Said("mcp__cua-space__computer_type", "{\"text\":\"Ada\"}"), Is.EqualTo("Typed|“Ada”"));
        Assert.That(Said("mcp__cua-space__computer_key", "{\"key\":\"return\"}"), Is.EqualTo("Pressed|return"));
        Assert.That(Said("mcp__cua-space__computer_launch", "{\"app\":\"Safari\"}"), Is.EqualTo("Opened|Safari"));
        Assert.That(Said("mcp__cua-space__computer_screenshot", "{}"), Is.EqualTo("Looked at the screen|"));
    }

    [Test]
    public void Each_project_has_its_own_space_and_the_list_is_read_loosely()
    {
        // One desktop per project: the same folder (however it is written) gives one Space,
        // two folders with the same name give two.
        var app = Path.Combine(_dir, "My App");
        var other = Path.Combine(_dir, "x", "My App");
        Assert.That(Spaces.NameFor(app), Does.Match("^hover-my-app-[0-9a-f]{6}$"));
        Assert.That(Spaces.NameFor(app + Path.DirectorySeparatorChar), Is.EqualTo(Spaces.NameFor(app)));
        Assert.That(Spaces.NameFor(Path.Combine(_dir, "x", "..", "My App")), Is.EqualTo(Spaces.NameFor(app)));
        Assert.That(Spaces.NameFor(other), Is.Not.EqualTo(Spaces.NameFor(app)));
        Assert.That(Spaces.IdFor(app), Is.EqualTo("local:" + Spaces.NameFor(app)));
        Assert.That(Spaces.Title(app), Is.EqualTo("My App"));
        var list = Spaces.ParseList("note: signed out\n[{\"id\":\"local:hover-1\",\"name\":\"hover-1\",\"os\":\"macos\",\"power_state\":\"running\"},{\"id\":\"local:x\",\"power_state\":\"stopped\"}]");
        Assert.That(list.Select(x => (x.Id, x.Name, x.Running)), Is.EqualTo(new[] { ("local:hover-1", "hover-1", true), ("local:x", "x", false) }));
        Assert.That(Spaces.ParseList("{\"spaces\":[{\"id\":\"local:a\",\"name\":\"a\"}]}"), Has.Count.EqualTo(1));
        Assert.That(Spaces.ParseList("not json"), Is.Empty);
        // What cua 0.2 prints: telemetry notice, then the list, with no power state.
        var real = Spaces.ParseList("Cua collects anonymous usage data…\n{\"relay_error\":null,\"spaces\":[{\"id\":\"local:hover-hover-9a332d\",\"name\":\"Apple-Virtual-Machine-1.local\",\"os\":\"macos\",\"kind\":\"vm\"}]}");
        Assert.That(real.Single().Id, Is.EqualTo("local:hover-hover-9a332d"));
        // Lume knows whether it is on, and its size.
        var vm = Spaces.ParseVm("{\"name\":\"hover-hover-9a332d\",\"status\":\"stopped\",\"cpuCount\":2,\"memorySize\":4294967296,\"display\":\"1024x768\"}");
        Assert.That(vm, Is.EqualTo(new Spaces.VmInfo(true, false, 2, 4, "1024x768")));
        Assert.That(Spaces.ParseVm("[{\"status\":\"running\",\"cpuCount\":4,\"memorySize\":8589934592}]")!.Running, Is.True);
        var size = Spaces.Target();
        Assert.That(size.Cpus, Is.InRange(2, 6)); Assert.That(size.MemoryGb, Is.InRange(4, 8)); 
        // The agent's tools in its Space: computer use, never its shell or Spaces' admin.
        Assert.That(Spaces.Permissions, Does.Contain("computer:click").And.Not.Contain("shell").And.Not.Contain("spaces:"));
    }

    [Test]
    public void An_app_is_unpacked_and_opened_with_every_name_quoted()
    {
        var script = Spaces.InstallScript("/Users/lume/Downloads/.hover-x.zip", "It's; rm -rf ~.app");
        Assert.That(script, Does.Contain("a='It'\\''s; rm -rf ~.app'"), "a quote in a name can't end the string");
        Assert.That(script, Does.Contain("/usr/bin/ditto -x -k \"$z\" \"$d\""));
        Assert.That(script, Does.EndWith("/usr/bin/open \"$d/$a\""));
    }

    [Test]
    public void A_wrong_token_gets_nothing()
    {
        Assert.That(BrowserTool.Servers(AgentTool.Codex, "x"), Is.Empty, "no host browser, no server");
    }

    // MARK: The GitHub CLI

    [Test]
    public void Gh_output_gives_the_device_code_and_the_account()
    {
        Assert.That(GitHubCli.ParseCode("! First copy your one-time code: 1A2B-3C4D"), Is.EqualTo("1A2B-3C4D"));
        Assert.That(GitHubCli.ParseCode("Open this URL to continue in your web browser: https://github.com/login/device"), Is.Null);
        Assert.That(GitHubCli.ParseUser("github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Active account: true"), Is.EqualTo("octocat"));
        Assert.That(GitHubCli.ParseUser("You are not logged into any GitHub hosts."), Is.Null);
    }

    [Test]
    public void Branch_names_from_the_form_cannot_be_options()
    {
        Assert.That(DeskInfo.ValidRef("hover/fix-notch"), Is.True);
        Assert.That(DeskInfo.ValidRef("--upload-pack=evil"), Is.False);
        Assert.That(DeskInfo.ValidRef("a..b"), Is.False);
        Assert.That(DeskInfo.ValidRef("has space"), Is.False);
        Assert.That(DeskInfo.Slug("Fix the notch flicker on resize!"), Is.EqualTo("fix-the-notch-flicker-on-resize"));
    }

    private static string Git(string dir, params string[] args)
    {
        var psi = new ProcessStartInfo("git") { WorkingDirectory = dir, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false };
        foreach (var a in args) psi.ArgumentList.Add(a);
        psi.Environment["GIT_CONFIG_GLOBAL"] = "/dev/null";
        using var p = Process.Start(psi)!;
        var o = p.StandardOutput.ReadToEnd(); var e = p.StandardError.ReadToEnd(); p.WaitForExit();
        Assert.That(p.ExitCode, Is.EqualTo(0), $"git {string.Join(' ', args)}: {e}");
        return o.Trim();
    }

    [Test]
    public async Task Create_pull_request_branches_commits_pushes_and_opens_it()
    {
        if (OperatingSystem.IsWindows()) Assert.Ignore("Uses a shell script for gh.");
        var remote = Path.Combine(_dir, "remote.git"); var repo = Path.Combine(_dir, "repo"); var bin = Path.Combine(_dir, "bin");
        Directory.CreateDirectory(repo); Directory.CreateDirectory(bin);
        Git(_dir, "init", "-q", "--bare", "-b", "main", remote);
        Git(repo, "init", "-q", "-b", "main");
        Git(repo, "config", "user.email", "t@example.com"); Git(repo, "config", "user.name", "T"); Git(repo, "config", "commit.gpgsign", "false");
        File.WriteAllText(Path.Combine(repo, "a.txt"), "one\n");
        Git(repo, "add", "-A"); Git(repo, "commit", "-q", "-m", "first");
        Git(repo, "remote", "add", "origin", remote); Git(repo, "push", "-q", "-u", "origin", "main");
        File.WriteAllText(Path.Combine(repo, "a.txt"), "two\n");
        // A stand-in gh: says where it opened the pull request, and keeps its arguments.
        var log = Path.Combine(_dir, "gh.log");
        File.WriteAllText(Path.Combine(bin, "gh"), $"#!/bin/sh\nprintf '%s\\n' \"$@\" > '{log}'\necho https://github.com/acme/app/pull/12\n");
        File.SetUnixFileMode(Path.Combine(bin, "gh"), UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        var path = Environment.GetEnvironmentVariable("PATH");
        Environment.SetEnvironmentVariable("PATH", bin + Path.PathSeparator + path);
        Environment.SetEnvironmentVariable("GIT_CONFIG_GLOBAL", "/dev/null");
        try
        {
            var s = new KiroSession();
            s.Restore(new SavedSession("k1", AgentTool.Codex, repo, "Change a", null, null, Array.Empty<SavedTurn>(), DateTime.Now));
            var args = JsonSerializer.SerializeToElement(new { title = "Change a", body = "Made a two.", branch = "hover/change-a", commit = true, draft = true });
            var r = JsonSerializer.SerializeToElement(await DeskInfo.CreatePr(s, args));
            Assert.That(r.TryGetProperty("error", out var err) ? err.GetString() : null, Is.Null);
            Assert.That(r.GetProperty("url").GetString(), Is.EqualTo("https://github.com/acme/app/pull/12"));
            Assert.That(Git(remote, "branch", "--list", "hover/change-a"), Does.Contain("hover/change-a"), "the branch was pushed");
            Assert.That(Git(repo, "status", "--porcelain"), Is.Empty, "the change was committed");
            var gh = File.ReadAllLines(log);
            Assert.That(gh, Is.EqualTo(new[] { "pr", "create", "--title", "Change a", "--body", "Made a two.", "--base", "main", "--head", "hover/change-a", "--draft" }));
            var bad = JsonSerializer.SerializeToElement(await DeskInfo.CreatePr(s, JsonSerializer.SerializeToElement(new { title = "x", branch = "--evil" })));
            Assert.That(bad.GetProperty("error").GetString(), Does.Contain("isn’t valid"));
        }
        finally
        {
            Environment.SetEnvironmentVariable("PATH", path);
            Environment.SetEnvironmentVariable("GIT_CONFIG_GLOBAL", null);
        }
    }

    private sealed class Disposer(Action a) : IDisposable { public void Dispose() => a(); }
}
