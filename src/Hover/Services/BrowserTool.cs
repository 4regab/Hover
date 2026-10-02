using System.Collections.Concurrent;
using System.IO;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Hover.Core;

namespace Hover.Services;

/// Hover's built-in browser, handed to every session as an MCP server, as T3 Code
/// hands its agents their preview tools: the agent opens pages, reads them, clicks,
/// types and takes screenshots in a browser the host owns (a WKWebView per session
/// on a Mac), and the user watches the same page in the desk's Browser panel.
///
/// The agent's tool starts the MCP server itself, inside its sandbox: a small relay
/// (perl, as Cua's guard) that joins its stdio to one Unix socket Hover listens on,
/// sending the session's token first. Hover answers MCP here (initialize, tools/list,
/// tools/call) and passes each call to the host, which drives the session's browser
/// and answers with text or a screenshot. The browser has no cookies of the user's
/// (its own, non-persistent store), and opens http(s) pages only. Each call is an MCP
/// tool call under the session's access, so Ask first asks about it and Read only
/// turns down what the tool counts as a change. No WPF in here.
public static class BrowserTool
{
    public const string ServerName = "hover-browser";

    /// The host can drive a browser (the Mac app says so by starting the backend).
    public static volatile bool Available;

    /// The session a token's tool is working for: its office id, or null. Called off
    /// the backend's loop, which it hops to itself.
    public static Func<string, Task<int?>>? Resolve;

    /// Sends a message to the host.
    public static Action<object>? ToHost;

    private static readonly ConcurrentDictionary<string, string> Tokens = new(), Tags = new();
    private static readonly ConcurrentDictionary<long, TaskCompletionSource<JsonElement>> Pending = new();
    private static long _calls;
    private static Socket? _listener;
    private static readonly object Lock = new();

    /// Where the socket is: a short path (Unix sockets take 104 bytes) in srt's temp
    /// folder, the place sandboxed tools may connect to sockets, in a folder only the
    /// user can open.
    public static string SocketPath
    {
        get
        {
            if (Environment.GetEnvironmentVariable("HOVER_BROWSER_SOCKET") is { Length: > 0 } s) return s;
            var root = Environment.GetEnvironmentVariable("HOVER_SANDBOX_TMP") is { Length: > 0 } r ? r
                : OperatingSystem.IsMacOS() ? "/private/tmp/claude" : Path.Combine(Path.GetTempPath(), "claude");
            var user = Regex.Replace(Environment.UserName, "[^A-Za-z0-9_-]", "");
            return Path.Combine(root, $"hover-browser-{(user.Length > 16 ? user[..16] : user)}", "b.sock");
        }
    }

    /// Where the relay is written: Hover's own folder, readable but not writable to
    /// the sandboxed tool, so it can't change what it runs.
    internal static string Dir => Path.Combine(Paths.Support, "browser");

    private const string Perl = "/usr/bin/perl";

    /// The browser's MCP server for a session's tool, or none (no host browser,
    /// switched off in Settings, or no perl). The tag names the session (its key);
    /// OpenCode's one server for all its sessions passes "opencode".
    public static IReadOnlyList<McpServer> Servers(AgentTool tool, string? tag)
    {
        if (!Available || !Settings.AgentBrowser || string.IsNullOrEmpty(tag) || OperatingSystem.IsWindows() || !File.Exists(Perl))
            return Array.Empty<McpServer>();
        try
        {
            Listen();
            Directory.CreateDirectory(Dir);
            var relay = Path.Combine(Dir, "relay.pl");
            if (!File.Exists(relay) || File.ReadAllText(relay) != Relay) File.WriteAllText(relay, Relay);
            var token = Tokens.GetOrAdd(tag, _ => Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant());
            Tags[token] = tag;
            return new[] { new McpServer(ServerName, Perl, new[] { relay, SocketPath, token }) };
        }
        catch (Exception e) when (e is IOException or SocketException or UnauthorizedAccessException)
        {
            Log.Line($"browser: couldn't listen - {e.Message}");
            return Array.Empty<McpServer>();
        }
    }

    /// The tool's MCP command joins its stdio to the socket, after its token line.
    internal const string Relay = """
#!/usr/bin/perl
# Hover's built-in browser for agents: joins this MCP server's stdio to Hover's socket.
# See BrowserTool in Hover's source.
use strict; use warnings;
use IO::Socket::UNIX;
use IO::Select;
use POSIX qw(EAGAIN EINTR);
die "usage: relay.pl socket token\n" unless @ARGV == 2;
my $s = IO::Socket::UNIX->new(Type => SOCK_STREAM(), Peer => $ARGV[0]) or die "Hover's browser isn't reachable ($ARGV[0]): $!\n";
$SIG{PIPE} = 'IGNORE';
sub put {
    my ($fh, $b) = @_;
    while (length $b) {
        my $n = syswrite($fh, $b);
        if (!defined $n) { return 0 if $! != EAGAIN && $! != EINTR; IO::Select->new($fh)->can_write(1); next; }
        substr($b, 0, $n) = '';
    }
    return 1;
}
put($s, "HELLO $ARGV[1]\n") or exit 1;
my $sel = IO::Select->new(\*STDIN, $s);
my $parent = getppid();
while ($sel->count) {
    exit 0 if getppid() != $parent;
    for my $fh ($sel->can_read(1)) {
        my $n = sysread($fh, my $chunk, 65536);
        if (!defined $n) { next if $! == EAGAIN || $! == EINTR; $n = 0; }
        exit 0 if $n == 0;
        put($fh == $s ? \*STDOUT : $s, $chunk) or exit 1;
    }
}
""";

    // MARK: The socket

    private static void Listen()
    {
        lock (Lock)
        {
            if (_listener is not null) return;
            var path = SocketPath;
            var dir = Path.GetDirectoryName(path)!;
            Directory.CreateDirectory(dir);
            if (!OperatingSystem.IsWindows()) File.SetUnixFileMode(dir, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
            if (File.Exists(path)) File.Delete(path);
            var s = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
            s.Bind(new UnixDomainSocketEndPoint(path));
            if (!OperatingSystem.IsWindows()) File.SetUnixFileMode(path, UnixFileMode.UserRead | UnixFileMode.UserWrite);
            s.Listen(16);
            _listener = s;
            _ = Task.Run(() => Accept(s));
            Log.Line($"browser: listening at {path}");
        }
    }

    private static async Task Accept(Socket listener)
    {
        while (true)
        {
            Socket client;
            try { client = await listener.AcceptAsync(); }
            catch (Exception e) when (e is SocketException or ObjectDisposedException) { return; }
            _ = Task.Run(() => Serve(client));
        }
    }

    /// Stops listening (Hover quits), and fails the calls still waiting on the host.
    public static void Stop()
    {
        lock (Lock)
        {
            try { _listener?.Dispose(); } catch { }
            _listener = null;
            try { if (File.Exists(SocketPath)) File.Delete(SocketPath); } catch { }
        }
        foreach (var p in Pending.Values) p.TrySetCanceled();
    }

    private static async Task Serve(Socket client)
    {
        using var stream = new NetworkStream(client, ownsSocket: true);
        using var reader = new StreamReader(stream, new UTF8Encoding(false));
        var write = new SemaphoreSlim(1, 1);
        async Task Send(object reply)
        {
            var bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(reply) + "\n");
            await write.WaitAsync();
            try { await stream.WriteAsync(bytes); await stream.FlushAsync(); }
            catch (IOException) { }
            finally { write.Release(); }
        }
        try
        {
            var hello = await reader.ReadLineAsync();
            if (hello is null || !hello.StartsWith("HELLO ", StringComparison.Ordinal) || !Tags.TryGetValue(hello[6..].Trim(), out var tag)) return;
            while (await reader.ReadLineAsync() is { } line)
            {
                if (line.Length == 0 || line.Length > 4 * 1024 * 1024) continue;
                JsonElement m;
                try { using var doc = JsonDocument.Parse(line); m = doc.RootElement.Clone(); }
                catch (JsonException) { continue; }
                // Calls run side by side: a slow page doesn't hold up a ping.
                _ = Task.Run(async () =>
                {
                    if (await Answer(tag, m) is { } reply) await Send(reply);
                });
            }
        }
        catch (Exception e) when (e is IOException or SocketException or ObjectDisposedException) { }
    }

    // MARK: MCP

    internal const string Instructions =
        "Hover's built-in browser. Use it whenever you need to see a web page: the local dev server you started, a site " +
        "you are building or testing, or documentation. The user watches the same browser in Hover, so prefer it over curl " +
        "for pages and over computer use for anything in a browser. Open a page with browser_open, read it with " +
        "browser_snapshot (interactive elements get [ref] numbers), act with browser_click, browser_type and browser_press " +
        "using those refs, and check the result with browser_screenshot and browser_console. Take a new snapshot after the " +
        "page changes: refs belong to the last snapshot. It has no cookies or sign-ins of the user's.";

    private static object Tool(string name, string description, object properties, params string[] required) => new
    {
        name,
        description,
        inputSchema = new { type = "object", properties, required, additionalProperties = false },
    };

    private static readonly object Target = new { type = "integer", description = "The element's [ref] number from the last browser_snapshot." };
    private static readonly object Selector = new { type = "string", description = "A CSS selector, when there is no ref." };
    private static readonly object Text = new { type = "string", description = "Visible text or label of the element, when there is no ref or selector." };

    internal static readonly object[] Tools =
    {
        Tool("browser_open", "Open a URL in Hover's browser (http or https; \"localhost:3000\" works) and wait for it to load. Returns the title, the final URL and the HTTP status.",
            new { url = new { type = "string", description = "The address to open." } }, "url"),
        Tool("browser_snapshot", "Read the open page as text: its title, URL, headings, text and every interactive element with a [ref] number to act on.",
            new { max_chars = new { type = "integer", description = "Longest answer (default 12000)." } }),
        Tool("browser_click", "Click an element on the page, then wait for any navigation it starts.",
            new { @ref = Target, selector = Selector, text = Text }),
        Tool("browser_type", "Type into a text field (replacing what is there unless append is true), optionally submitting its form.",
            new { @ref = Target, selector = Selector, label = Text, text = new { type = "string", description = "What to type." }, append = new { type = "boolean" }, submit = new { type = "boolean", description = "Press Enter / submit the form afterwards." } }, "text"),
        Tool("browser_press", "Press a key in the focused element: Enter, Escape, Tab, ArrowDown, ArrowUp, Backspace, or a character.",
            new { key = new { type = "string" } }, "key"),
        Tool("browser_scroll", "Scroll the page by a number of pixels, to its top or bottom, or to an element.",
            new { @ref = Target, dy = new { type = "integer", description = "Pixels down (negative: up). Default 600." }, to = new { type = "string", @enum = new[] { "top", "bottom" } } }),
        Tool("browser_screenshot", "A screenshot of what the page shows now, as an image.", new { }),
        Tool("browser_evaluate", "Run JavaScript in the page and return its result as JSON. The script is a function body: use return.",
            new { script = new { type = "string" } }, "script"),
        Tool("browser_wait", "Wait until some text or an element is on the page, up to a timeout.",
            new { text = new { type = "string" }, selector = Selector, timeout_ms = new { type = "integer", description = "Default 5000, at most 20000." } }),
        Tool("browser_console", "The page's console messages and errors since it loaded (newest last).",
            new { clear = new { type = "boolean", description = "Empty the log afterwards." } }),
        Tool("browser_back", "Go back to the previous page.", new { }),
        Tool("browser_reload", "Reload the page and wait for it to load.", new { }),
    };

    private static readonly HashSet<string> Names = Tools.Select(t => (string)t.GetType().GetProperty("name")!.GetValue(t)!).ToHashSet();

    /// One message from the agent: the reply to send, or null for a notification.
    internal static async Task<object?> Answer(string tag, JsonElement m)
    {
        if (m.ValueKind != JsonValueKind.Object || !m.TryGetProperty("id", out var id)) return null;
        var method = m.TryGetProperty("method", out var me) && me.ValueKind == JsonValueKind.String ? me.GetString() : null;
        switch (method)
        {
            case "initialize":
                var version = m.TryGetProperty("params", out var p) && p.TryGetProperty("protocolVersion", out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : "2025-06-18";
                return Ok(id, new { protocolVersion = version, capabilities = new { tools = new { listChanged = false } }, serverInfo = new { name = ServerName, title = "Hover browser", version = "1.0" }, instructions = Instructions });
            case "ping": return Ok(id, new { });
            case "tools/list": return Ok(id, new { tools = Tools });
            case "tools/call":
                var prm = m.TryGetProperty("params", out var pp) ? pp : default;
                var name = prm.ValueKind == JsonValueKind.Object && prm.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String ? n.GetString()! : "";
                var args = prm.ValueKind == JsonValueKind.Object && prm.TryGetProperty("arguments", out var a) && a.ValueKind == JsonValueKind.Object ? a : JsonDocument.Parse("{}").RootElement;
                if (!Names.Contains(name)) return Fail(id, -32602, $"Unknown tool {name}.");
                return Ok(id, await Call(tag, name, args));
            default:
                return Fail(id, -32601, $"Method {method} isn't supported.");
        }
    }

    private static object Ok(JsonElement id, object result) => new { jsonrpc = "2.0", id, result };
    private static object Fail(JsonElement id, int code, string message) => new { jsonrpc = "2.0", id, error = new { code, message } };
    private static object Said(string text, bool error = false) => new { content = new[] { new { type = "text", text } }, isError = error };

    /// A tool call, driven by the host in the session's browser.
    internal static async Task<object> Call(string tag, string name, JsonElement args)
    {
        var session = Resolve is null ? null : await Resolve(tag);
        if (session is not { } sid) return Said("Hover's browser isn't open for this session right now.", true);
        if (ToHost is not { } send) return Said("Hover's browser isn't available here.", true);
        var call = Interlocked.Increment(ref _calls);
        var done = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        Pending[call] = done;
        try
        {
            send(new { type = "browser", call, id = sid, op = name["browser_".Length..], args });
            var r = await done.Task.WaitAsync(TimeSpan.FromSeconds(name == "browser_wait" ? 40 : 90));
            return Result(r);
        }
        catch (TimeoutException) { return Said("The browser didn’t answer in time.", true); }
        catch (TaskCanceledException) { return Said("Hover is closing.", true); }
        finally { Pending.TryRemove(call, out _); }
    }

    /// The host's answer as MCP content: its text, and a screenshot when it took one.
    internal static object Result(JsonElement r)
    {
        var ok = r.TryGetProperty("ok", out var o) && o.ValueKind == JsonValueKind.True;
        var text = r.TryGetProperty("text", out var t) && t.ValueKind == JsonValueKind.String ? t.GetString() ?? "" : "";
        var content = new List<object>();
        if (text.Length > 0 || !r.TryGetProperty("image", out _)) content.Add(new { type = "text", text = text.Length > 0 ? text : ok ? "Done." : "That didn’t work." });
        if (r.TryGetProperty("image", out var img) && img.ValueKind == JsonValueKind.String)
            content.Add(new { type = "image", data = img.GetString(), mimeType = r.TryGetProperty("mime", out var mt) && mt.ValueKind == JsonValueKind.String ? mt.GetString() : "image/jpeg" });
        return new { content, isError = !ok };
    }

    /// The host's answer to a call ({type:'browserResult', call, ok, text, image}).
    public static void Complete(JsonElement m)
    {
        if (m.TryGetProperty("call", out var c) && c.TryGetInt64(out var call) && Pending.TryGetValue(call, out var done))
            done.TrySetResult(m.Clone());
    }

    // MARK: Steps

    private static readonly Regex Step = new(@"(?:^|[^a-z])browser_(open|snapshot|click|type|press|scroll|screenshot|evaluate|wait|console|back|reload)(?:$|[^a-z_])",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    /// The browser tool a step called ("open", "click"…), or null when it isn't one.
    public static string? Op(string? title) => title is null ? null : Step.Match(title) is { Success: true } m ? m.Groups[1].Value.ToLowerInvariant() : null;
}
