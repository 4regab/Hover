using System.IO;
using System.Text.Json;
using Hover.Core;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// The agents' sandbox (srt): what its settings allow and refuse, which folders a tool
/// started for some covers, and how a tool's start is wrapped. Nothing is run; srt is
/// an empty stand-in on a PATH of its own.
[TestFixture, NonParallelizable]
public sealed class SandboxTests
{
    private string _dir = "";
    private string? _path, _inside;
    private bool _was;

    [SetUp]
    public void SetUp()
    {
        if (!Sandbox.Supported) Assert.Ignore("The sandbox runs on macOS and Linux.");
        _dir = Path.Combine(Path.GetTempPath(), "hover-box-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(_dir, "project"));
        _path = Environment.GetEnvironmentVariable("PATH");
        _inside = Environment.GetEnvironmentVariable("HOVER_SANDBOXED");
        _was = Settings.Sandbox;
    }

    [TearDown]
    public void TearDown()
    {
        if (!Sandbox.Supported) return;
        Environment.SetEnvironmentVariable("PATH", _path);
        Environment.SetEnvironmentVariable("HOVER_SANDBOXED", _inside);
        Environment.SetEnvironmentVariable("HOVER_SANDBOX_TMP", null);
        Settings.Sandbox = _was;
        try { Directory.Delete(_dir, true); } catch (IOException) { }
    }

    private static JsonElement Parse(string json) => JsonDocument.Parse(json).RootElement;
    private static List<string> List(JsonElement e, string a, string b) => e.GetProperty(a).GetProperty(b).EnumerateArray().Select(x => x.GetString()!).ToList();

    [Test]
    public void The_settings_open_the_folders_and_close_the_rest()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var folder = Path.Combine(_dir, "project");
        var c = Parse(Sandbox.Config(AgentTool.Codex, new[] { folder }, "/private/tmp/claude/hover-codex", new[] { "/private/tmp/claude/hover-codex", Sandbox.CuaSocket }));
        var write = List(c, "filesystem", "allowWrite");
        Assert.That(write, Does.Contain(folder).And.Contain(Path.Combine(home, ".codex")).And.Contain("/private/tmp/claude/hover-codex"));
        Assert.That(write, Has.None.EqualTo(home), "the home folder itself isn't writable");
        Assert.That(write, Has.None.Contains("Library/Containers"));
        var deny = List(c, "filesystem", "denyRead");
        foreach (var p in new[] { ".ssh", "Library/Group Containers", "Library/Containers", "Library/Mail", "Library/Messages", "Library/Cookies" })
            Assert.That(deny, Does.Contain(Path.Combine(home, p)), p);
        Assert.That(deny, Does.Contain(Path.GetFullPath(Paths.Support).TrimEnd('/')), "Hover's own data");
        Assert.That(List(c, "filesystem", "allowRead"), Does.Contain(folder).And.Some.EndsWith("kiro-images"));
        var domains = List(c, "network", "allowedDomains");
        Assert.That(domains, Does.Contain("api.openai.com").And.Contain("registry.npmjs.org").And.Contain("github.com").And.Contain("localhost"));
        Assert.That(domains, Has.None.EqualTo("*.cursor.sh"), "another tool's service");
        Assert.That(List(c, "network", "allowUnixSockets"), Does.Contain(Sandbox.CuaSocket));
        Assert.That(c.GetProperty("allowAppleEvents").GetBoolean(), Is.False);
    }

    [Test]
    public void The_users_own_hosts_are_added_and_nonsense_is_dropped()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Sandbox.ExtraFile)!);
        File.WriteAllText(Sandbox.ExtraFile, "# mine\ndocs.example.com\n*.example.org  # wildcard\n*\nhttps://bad.example.com/x\n*.com\nlocalhost:8080\n");
        try
        {
            Assert.That(Sandbox.Extra(), Is.EqualTo(new[] { "docs.example.com", "*.example.org", "localhost:8080" }));
            var c = Parse(Sandbox.Config(AgentTool.Kiro, Array.Empty<string>(), "/tmp/x", Array.Empty<string>()));
            Assert.That(List(c, "network", "allowedDomains"), Does.Contain("docs.example.com").And.Contain("*.amazonaws.com"));
        }
        finally { File.Delete(Sandbox.ExtraFile); }
    }

    [Test]
    public void A_tool_covers_its_folders_and_what_is_inside_them()
    {
        var a = Path.Combine(_dir, "project");
        Assert.That(Sandbox.Covers(new[] { a }, a), Is.True);
        Assert.That(Sandbox.Covers(new[] { a }, Path.Combine(a, "sub")), Is.True);
        Assert.That(Sandbox.Covers(new[] { a }, a + "-other"), Is.False);
        Assert.That(Sandbox.Covers(new[] { a }, _dir), Is.False);
        Sandbox.Remember(a);
        Assert.That(Sandbox.Folders(), Does.Contain(Path.GetFullPath(a)));
    }

    [Test]
    public void A_start_is_wrapped_in_srt_with_the_tools_own_arguments()
    {
        var bin = Path.Combine(_dir, "bin");
        Directory.CreateDirectory(bin);
        // srt, and what it needs beside it, as stand-ins.
        foreach (var tool in new[] { "srt", "rg", "bwrap", "socat" })
        {
            File.WriteAllText(Path.Combine(bin, tool), "#!/bin/sh\n");
            if (!OperatingSystem.IsWindows()) File.SetUnixFileMode(Path.Combine(bin, tool), UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
        var srt = Path.Combine(bin, "srt");
        Environment.SetEnvironmentVariable("PATH", bin + Path.PathSeparator + _path);
        Environment.SetEnvironmentVariable("HOVER_SANDBOX_TMP", Path.Combine(_dir, "t"));
        Environment.SetEnvironmentVariable("HOVER_SANDBOXED", null);
        Settings.Sandbox = true;
        var psi = Quota.Hidden("/usr/local/bin/kiro-cli", "acp", "--agent-engine", "v3");
        psi.Environment["SECRET_FREE"] = "1";
        var folder = Path.Combine(_dir, "project");
        var boxed = Sandbox.Wrap(psi, AgentTool.Kiro, new[] { folder });
        Assert.That(boxed.FileName, Is.EqualTo(srt));
        var args = boxed.ArgumentList.ToList();
        Assert.That(args[0], Is.EqualTo("--settings"));
        var relay = File.Exists("/usr/bin/perl") ? new[] { "/usr/bin/perl", Path.Combine(_dir, "t", "hover-kiro", "relay.pl") } : Array.Empty<string>();
        Assert.That(args.Skip(2).ToList(), Is.EqualTo(new[] { "--", "/usr/bin/env", args[4] }.Concat(relay).Concat(new[] { "/usr/local/bin/kiro-cli", "acp", "--agent-engine", "v3" })));
        Assert.That(args[4], Is.EqualTo("TMPDIR=" + Path.Combine(_dir, "t", "hover-kiro") + "/"));
        Assert.That(boxed.Environment["SECRET_FREE"], Is.EqualTo("1"));
        Assert.That(boxed.Environment["HOVER_SANDBOXED"], Is.EqualTo("1"));
        var written = Parse(File.ReadAllText(args[1]));
        Assert.That(List(written, "filesystem", "allowWrite"), Does.Contain(folder));
        if (!OperatingSystem.IsWindows()) Assert.That(File.GetUnixFileMode(args[1]), Is.EqualTo(UnixFileMode.UserRead | UnixFileMode.UserWrite));

        // Switched off, or inside another sandbox already: left as it was.
        Settings.Sandbox = false;
        Assert.That(Sandbox.Wrap(psi, AgentTool.Kiro, new[] { folder }), Is.SameAs(psi));
        Settings.Sandbox = true;
        Environment.SetEnvironmentVariable("HOVER_SANDBOXED", "1");
        Assert.That(Sandbox.Wrap(psi, AgentTool.Kiro, new[] { folder }), Is.SameAs(psi));
        Assert.That(Sandbox.Missing(), Is.Null);
    }

    /// srt's stdio is non-blocking: a tool's write past the pipe's 64 KB buffer failed
    /// with EAGAIN and the tool died (Kiro with Cua Driver's 67 KB tool list). Through
    /// the relay the same write arrives whole, and the tool's stdin closes with ours.
    [Test]
    public async Task The_relay_carries_a_big_write_through_a_nonblocking_pipe()
    {
        if (OperatingSystem.IsWindows() || !File.Exists("/usr/bin/perl") || !File.Exists("/usr/bin/python3")) { Assert.Ignore("Needs perl and python3."); return; }
        var relay = Path.Combine(_dir, "relay.pl");
        File.WriteAllText(relay, Sandbox.Relay);
        // Like srt: stdout made non-blocking, then 200 KB written in one go, after a line read from stdin.
        var psi = new System.Diagnostics.ProcessStartInfo("/usr/bin/perl") { RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false };
        foreach (var a in new[] { "-e", "use Fcntl; for (0,1) { open(my $h,'+<&=',$_) or next; fcntl($h,F_SETFL,fcntl($h,F_GETFL,0)|O_NONBLOCK) } exec @ARGV", "/usr/bin/perl", relay,
            "/usr/bin/python3", "-c", "import sys; sys.stdin.readline(); sys.stdout.write('x'*200000+'\\n'); sys.stdout.flush(); sys.stdin.read(); print('eof')" })
            psi.ArgumentList.Add(a);
        using var p = System.Diagnostics.Process.Start(psi)!;
        await p.StandardInput.WriteLineAsync("go");
        await p.StandardInput.FlushAsync();
        // Read slowly, so the pipe fills up while the tool writes.
        await Task.Delay(300);
        var line = await p.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(10));
        p.StandardInput.Close();
        var rest = await p.StandardOutput.ReadToEndAsync().WaitAsync(TimeSpan.FromSeconds(10));
        await p.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Multiple(() =>
        {
            Assert.That(line?.Length, Is.EqualTo(200000), "the whole write arrived");
            Assert.That(rest.Trim(), Is.EqualTo("eof"), "our stdin's end reached the tool");
            Assert.That(p.ExitCode, Is.EqualTo(0));
        });
    }

    /// Writes each tool's real settings to HOVER_LIVE_OUT, for trying the installed tools
    /// under srt by hand (srt --settings <file> -- kiro-cli whoami). Run on request:
    /// dotnet test … --filter "TestCategory=LiveSandbox".
    [Test, Explicit, Category("LiveSandbox")]
    public void Write_the_real_settings_for_a_hand_check()
    {
        if (Environment.GetEnvironmentVariable("HOVER_LIVE_OUT") is not { Length: > 0 } output) { Assert.Ignore("Set HOVER_LIVE_OUT."); return; }
        var folder = Environment.GetEnvironmentVariable("HOVER_LIVE_FOLDER") ?? Path.Combine(_dir, "project");
        Directory.CreateDirectory(output);
        foreach (var t in Agents.All)
        {
            var temp = $"/private/tmp/claude/hover-{Agents.Id(t)}";
            File.WriteAllText(Path.Combine(output, Agents.Id(t) + ".json"), Sandbox.Config(t, new[] { folder }, temp, new[] { temp, Sandbox.CuaSocket }));
        }
    }

    [Test]
    public void A_missing_sandbox_is_said_and_set_up_with_the_tool()
    {
        Environment.SetEnvironmentVariable("PATH", Path.Combine(_dir, "empty"));
        Environment.SetEnvironmentVariable("HOVER_SANDBOXED", null);
        Settings.Sandbox = true;
        // A Mac with srt or ripgrep in Homebrew's folder still finds them there.
        if (Sandbox.Exe() is null) Assert.That(Sandbox.Missing(), Does.Contain(Sandbox.Package + "@" + Sandbox.Version));
        Settings.Sandbox = false;
        Assert.That(Sandbox.Missing(), Is.Null);
    }
}
