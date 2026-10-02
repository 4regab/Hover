using System.IO;
using System.Text.Json;
using Hover.Core;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// Computer use through Cua Driver: which servers a session gets, how they are
/// written for ACP and OpenCode, and how cua-driver's permission report is read.
/// Nothing is run: cua-driver is an empty stand-in file on a PATH of its own.
[TestFixture, NonParallelizable]
public sealed class ComputerUseTests
{
    private string _dir = "";
    private string? _path;
    private bool _was;

    [SetUp]
    public void SetUp()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-cua-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_dir);
        _path = Environment.GetEnvironmentVariable("PATH");
        Environment.SetEnvironmentVariable("PATH", _dir);
        _was = Settings.ComputerUse;
    }

    [TearDown]
    public void TearDown()
    {
        Settings.ComputerUse = _was;
        Environment.SetEnvironmentVariable("PATH", _path);
        Directory.Delete(_dir, true);
    }

    private string Driver()
    {
        var exe = Path.Combine(_dir, OperatingSystem.IsWindows() ? "cua-driver.exe" : "cua-driver");
        File.WriteAllText(exe, "");
        return exe;
    }

    [Test]
    public void Off_by_default_and_off_means_no_servers()
    {
        Driver();
        Settings.ComputerUse = false;
        Assert.That(ComputerUse.Servers(), Is.Empty);
    }

    [Test]
    public void On_hands_out_cua_drivers_mcp_command_from_path()
    {
        var exe = Driver();
        Settings.ComputerUse = true;
        var s = ComputerUse.Servers().Single();
        Assert.That(s.Name, Is.EqualTo("cua-driver"));
        if (OperatingSystem.IsWindows() || !File.Exists("/usr/bin/perl"))
        {
            Assert.That(s.Command, Is.EqualTo(exe));
            // Never Cua's approval bypass: the session's tool access decides.
            Assert.That(s.Args, Is.EqualTo(new[] { "mcp" }));
            return;
        }
        // Behind the guard, written to Hover's own folder; still no approval bypass.
        Assert.Multiple(() =>
        {
            Assert.That(s.Command, Is.EqualTo("/usr/bin/perl"));
            Assert.That(s.Args, Is.EqualTo(new[] { Path.Combine(ComputerUse.GuardDir, "guard.pl"), exe, "mcp" }));
            Assert.That(File.ReadAllText(s.Args[0]), Is.EqualTo(ComputerUse.Guard));
        });
    }

    /// The guard against a stand-in cua-driver that echoes what reaches it: foreground
    /// becomes background, input with no app or on the desktop and the tools that take
    /// over the user's screen are answered by the guard and never reach the driver, and
    /// the tool list and initialize answer are fixed on the way back.
    [Test]
    public async Task The_guard_keeps_computer_use_in_the_background()
    {
        if (OperatingSystem.IsWindows() || !File.Exists("/usr/bin/perl") || !File.Exists("/usr/bin/python3")) { Assert.Ignore("Needs perl and python3."); return; }
        Environment.SetEnvironmentVariable("PATH", _path);
        var guard = Path.Combine(_dir, "guard.pl");
        File.WriteAllText(guard, ComputerUse.Guard);
        const string fake = """
import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get('method') == 'initialize':
        r = {'protocolVersion': '2025-06-18', 'instructions': 'Cua.'}
    elif m.get('method') == 'tools/list':
        r = {'tools': [{'name': 'click', 'inputSchema': {'properties': {'pid': {}, 'delivery_mode': {'enum': ['background', 'foreground']}, 'scope': {'enum': ['window', 'desktop']}}}},
                       {'name': 'bring_to_front', 'inputSchema': {'properties': {}}}, {'name': 'get_window_state', 'inputSchema': {'properties': {}}}]}
    else:
        r = {'got': m['params']}
    print(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}), flush=True)
""";
        var psi = new System.Diagnostics.ProcessStartInfo("/usr/bin/perl") { RedirectStandardInput = true, RedirectStandardOutput = true, UseShellExecute = false };
        foreach (var a in new[] { guard, "/usr/bin/python3", "-c", fake }) psi.ArgumentList.Add(a);
        using var p = System.Diagnostics.Process.Start(psi)!;
        async Task<JsonElement> Ask(string line)
        {
            await p.StandardInput.WriteLineAsync(line); await p.StandardInput.FlushAsync();
            var answer = await p.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(10));
            return JsonDocument.Parse(answer!).RootElement.GetProperty("result");
        }
        var init = await Ask("""{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}""");
        var tools = await Ask("""{"jsonrpc":"2.0","id":2,"method":"tools/list"}""");
        var fg = await Ask("""{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"click","arguments":{"pid":7,"window_id":1,"x":5,"y":5,"delivery_mode":"foreground"}}}""");
        var front = await Ask("""{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"bring_to_front","arguments":{"pid":7}}}""");
        var nowhere = await Ask("""{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"type_text","arguments":{"text":"hi"}}}""");
        var desk = await Ask("""{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"click","arguments":{"scope":"desktop","x":5,"y":5}}}""");
        var pointer = await Ask("""{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"move_cursor","arguments":{"scope":"desktop","x":5,"y":5}}}""");
        var look = await Ask("""{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"get_desktop_state","arguments":{"scope":"desktop"}}}""");
        p.StandardInput.Close();
        await p.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(10));
        var names = tools.GetProperty("tools").EnumerateArray().Select(t => t.GetProperty("name").GetString()).ToList();
        var click = tools.GetProperty("tools")[0].GetProperty("inputSchema").GetProperty("properties");
        Assert.Multiple(() =>
        {
            Assert.That(init.GetProperty("instructions").GetString(), Does.StartWith("Cua.").And.Contain("background"));
            Assert.That(names, Is.EqualTo(new[] { "click", "get_window_state" }), "what is off isn't offered");
            Assert.That(click.TryGetProperty("delivery_mode", out _), Is.False);
            Assert.That(click.GetProperty("scope").GetProperty("enum").EnumerateArray().Select(e => e.GetString()), Is.EqualTo(new[] { "window" }));
            Assert.That(fg.GetProperty("got").GetProperty("arguments").GetProperty("delivery_mode").GetString(), Is.EqualTo("background"));
            foreach (var r in new[] { front, nowhere, desk, pointer })
            {
                Assert.That(r.GetProperty("isError").GetBoolean(), Is.True, "answered by the guard");
                Assert.That(r.TryGetProperty("got", out _), Is.False, "never reached the driver");
            }
            Assert.That(look.GetProperty("got").GetProperty("name").GetString(), Is.EqualTo("get_desktop_state"), "looking is fine");
            Assert.That(p.ExitCode, Is.EqualTo(0));
        });
    }

    [Test]
    public void The_acp_entry_is_a_stdio_server_with_an_env_list()
    {
        var json = JsonSerializer.Serialize(ComputerUse.Acp(new[] { new McpServer("cua-driver", "/x/cua-driver", new[] { "mcp" }) }));
        Assert.That(json, Is.EqualTo("[{\"name\":\"cua-driver\",\"command\":\"/x/cua-driver\",\"args\":[\"mcp\"],\"env\":[]}]"));
    }

    [Test]
    public void OpenCodes_inline_config_adds_a_local_server_and_keeps_what_was_there()
    {
        var servers = new[] { new McpServer("cua-driver", "/x/cua-driver", new[] { "mcp" }) };
        Assert.That(ComputerUse.OpenCodeConfig(Array.Empty<McpServer>(), "{\"a\":1}"), Is.EqualTo("{\"a\":1}"), "nothing to add: left alone");
        using var doc = JsonDocument.Parse(ComputerUse.OpenCodeConfig(servers, "{\"model\":\"p/m\",\"mcp\":{\"mine\":{\"type\":\"remote\",\"url\":\"https://x\"}}}")!);
        var mcp = doc.RootElement.GetProperty("mcp");
        var cua = mcp.GetProperty("cua-driver");
        Assert.Multiple(() =>
        {
            Assert.That(doc.RootElement.GetProperty("model").GetString(), Is.EqualTo("p/m"));
            Assert.That(mcp.TryGetProperty("mine", out _), "the user's own server stays");
            Assert.That(cua.GetProperty("type").GetString(), Is.EqualTo("local"));
            Assert.That(cua.GetProperty("command").EnumerateArray().Select(x => x.GetString()), Is.EqualTo(new[] { "/x/cua-driver", "mcp" }));
            Assert.That(cua.GetProperty("enabled").GetBoolean());
        });
        // A config that isn't JSON is replaced rather than breaking the start.
        Assert.That(ComputerUse.OpenCodeConfig(servers, "not json"), Does.Contain("\"cua-driver\""));
    }

    [Test]
    public void A_change_in_servers_changes_the_signature()
    {
        var a = ComputerUse.Signature(Array.Empty<McpServer>());
        var b = ComputerUse.Signature(new[] { new McpServer("cua-driver", "/x", new[] { "mcp" }) });
        Assert.That(a, Is.Not.EqualTo(b));
        Assert.That(b, Is.EqualTo(ComputerUse.Signature(new[] { new McpServer("cua-driver", "/x", new[] { "mcp" }) })));
    }

    [Test]
    public void Permission_reports_are_read_only_when_cua_driver_vouches_for_them()
    {
        // As cua-driver 0.31 prints them: no booleans at all when it can't say.
        Assert.That(ComputerUse.ParsePermissions("{\n  \"daemon_running\": true,\n  \"reason\": \"…\",\n  \"status\": \"unknown\"\n}"), Is.Null);
        Assert.That(ComputerUse.ParsePermissions("{\"accessibility\":true,\"screen_recording\":true,\"source\":{\"attribution\":\"driver-daemon\"}}"), Is.EqualTo((true, true)));
        Assert.That(ComputerUse.ParsePermissions("note: proxying\n{\"accessibility\":true,\"screen_recording\":false}"), Is.EqualTo((true, false)));
        Assert.That(ComputerUse.ParsePermissions("{\"accessibility\":false}"), Is.EqualTo((false, false)));
        Assert.That(ComputerUse.ParsePermissions("garbage"), Is.Null);
    }

    [Test]
    public void Ready_needs_it_installed_and_at_least_accessibility()
    {
        Assert.Multiple(() =>
        {
            Assert.That(new ComputerUseStatus(true, "1", "granted", "").Ready);
            Assert.That(new ComputerUseStatus(true, "1", "partial", "").Ready);
            Assert.That(new ComputerUseStatus(true, "1", "missing", "").Ready, Is.False);
            Assert.That(new ComputerUseStatus(false, "", "granted", "").Ready, Is.False);
        });
    }
}
