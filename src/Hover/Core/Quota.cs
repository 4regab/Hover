using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.Data.Sqlite;

namespace Hover.Core;

/// One reading of how much of a plan is used. Used is null when there is nothing to
/// show; Detail then says why ("kiro-cli not found", "Sign in to Cursor").
public sealed record QuotaReading(double? Used, string Detail)
{
    public bool Ok => Used is not null;
    public static QuotaReading Fail(string why) => new(null, why);
}

/// Usage for the AI tools the notch can show. None of them publishes a quota API, so
/// each is read the way the tool itself exposes it, read-only, and nothing leaves
/// the PC except each tool's own usage request with its own sign-in:
///
///   Kiro CLI     `kiro-cli chat --no-interactive /usage`, the CLI's printed report.
///   Codex        the rate-limit snapshot Codex writes into its session logs.
///   Cursor       cursor.com/api/usage-summary, with the token Cursor keeps locally.
///   Claude Code  api.anthropic.com/api/oauth/usage, with Claude Code's own sign-in.
///
/// No WPF in here, so the parsers can be checked on their own.
public static class Quota
{
    // MARK: Kiro CLI

    private static readonly Regex Ansi = new(@"\x1B\[[0-9;?]*[A-Za-z]|\x1B\][^\x07]*\x07", RegexOptions.Compiled);

    public static async Task<QuotaReading> Kiro(CancellationToken ct)
    {
        var exe = OnPath("kiro-cli");
        if (exe is null) return QuotaReading.Fail("kiro-cli isn’t installed or isn’t on PATH.");
        // A .cmd shim has to go through cmd; an .exe runs directly.
        var shim = exe.EndsWith(".cmd", StringComparison.OrdinalIgnoreCase) || exe.EndsWith(".bat", StringComparison.OrdinalIgnoreCase);
        var psi = new ProcessStartInfo(shim ? "cmd.exe" : exe)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            RedirectStandardInput = true,
            UseShellExecute = false,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        if (shim) { psi.ArgumentList.Add("/d"); psi.ArgumentList.Add("/c"); psi.ArgumentList.Add(exe); }
        foreach (var a in new[] { "chat", "--no-interactive", "/usage" }) psi.ArgumentList.Add(a);
        psi.Environment["NO_COLOR"] = "1";
        psi.Environment["TERM"] = "dumb";

        using var p = new Process { StartInfo = psi };
        try
        {
            // One deadline for the exit and both pipes: a grandchild that inherits
            // stdout can hold it open after kiro-cli itself has gone.
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
            timeout.CancelAfter(TimeSpan.FromSeconds(25));
            p.Start();
            p.StandardInput.Close();
            var stdout = p.StandardOutput.ReadToEndAsync(timeout.Token);
            var stderr = p.StandardError.ReadToEndAsync(timeout.Token);
            await p.WaitForExitAsync(timeout.Token);
            await Task.WhenAll(stdout, stderr).WaitAsync(timeout.Token);
            return ParseKiro(await stdout + "\n" + await stderr);
        }
        catch (OperationCanceledException)
        {
            try { p.Kill(entireProcessTree: true); } catch { /* already gone */ }
            return QuotaReading.Fail("kiro-cli didn’t answer in time.");
        }
        catch (Exception e)
        {
            return QuotaReading.Fail($"kiro-cli failed: {e.Message}");
        }
    }

    /// The report is a box: "████ 42% (resets on 10/01)" and "(21.00 of 50 covered
    /// in plan)". The bar's percentage is the share used; the credit line is the
    /// fallback when a release drops the bar.
    public static QuotaReading ParseKiro(string output)
    {
        var text = Ansi.Replace(output, "");
        var lower = text.ToLowerInvariant();
        if (lower.Contains("not logged in") || lower.Contains("login required") || lower.Contains("kiro-cli login"))
            return QuotaReading.Fail("Run “kiro-cli login” first.");
        if (lower.Contains("could not retrieve usage"))
            return QuotaReading.Fail("Kiro couldn’t retrieve usage right now.");

        var reset = Regex.Match(text, @"resets on (\d{4}-\d{2}-\d{2}|\d{2}/\d{2})");
        var resetText = reset.Success ? $" · resets {reset.Groups[1].Value}" : "";
        var plan = Regex.Match(text, @"\b(KIRO(?:[ \t]+[A-Z]+)+)\b");
        var planText = plan.Success ? plan.Groups[1].Value.Trim() + " · " : "";

        var bar = Regex.Match(text, @"█+\s*(\d+(?:\.\d+)?)\s*%");
        var credits = Regex.Match(text, @"\((\d+(?:\.\d+)?)\s+of\s+(\d+(?:\.\d+)?)\s+covered");
        double? used = null;
        var detail = "";
        if (credits.Success && Num(credits.Groups[1].Value) is { } u && Num(credits.Groups[2].Value) is { } l && l > 0)
        {
            detail = $"{u:0.##} of {l:0.##} credits";
            used = u / l * 100;
        }
        if (bar.Success && Num(bar.Groups[1].Value) is { } pct) used = pct;
        if (used is null) return QuotaReading.Fail("Couldn’t read kiro-cli’s usage report.");
        if (detail.Length == 0) detail = $"{used:0}% used";
        return new QuotaReading(Math.Clamp(used.Value, 0, 100), planText + detail + resetText);
    }

    // MARK: Codex

    public static QuotaReading Codex(DateTime now)
    {
        var home = Environment.GetEnvironmentVariable("CODEX_HOME");
        if (string.IsNullOrWhiteSpace(home))
            home = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".codex");
        var sessions = Path.Combine(home, "sessions");
        if (!Directory.Exists(sessions)) return QuotaReading.Fail("No Codex sessions on this PC yet.");
        try
        {
            // ponytail: walks every rollout file to find the newest; fine for thousands
            // of sessions. If that ever gets slow, walk only the last few date folders.
            var files = new DirectoryInfo(sessions)
                .EnumerateFiles("rollout-*.jsonl", SearchOption.AllDirectories)
                .OrderByDescending(f => f.LastWriteTimeUtc)
                .Take(8);
            foreach (var f in files)
            {
                // Codex may be writing to it right now.
                using var s = new FileStream(f.FullName, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
                using var r = new StreamReader(s);
                QuotaReading? last = null;
                while (r.ReadLine() is { } line)
                    if (line.Contains("\"rate_limits\"") && ParseCodexLine(line, now) is { } q) last = q;
                if (last is not null) return last;
            }
        }
        catch (Exception e)
        {
            return QuotaReading.Fail($"Couldn’t read Codex’s logs: {e.Message}");
        }
        return QuotaReading.Fail("Codex hasn’t recorded any limits yet — use it once.");
    }

    /// One token_count event from a Codex session log. Its rate_limits carry the
    /// five-hour window (primary) and the weekly one (secondary). The notch shows
    /// whichever is closer to the limit; a window whose reset has passed since the
    /// snapshot counts as empty again.
    public static QuotaReading? ParseCodexLine(string line, DateTime now)
    {
        try
        {
            using var doc = JsonDocument.Parse(line);
            var root = doc.RootElement;
            if (!root.TryGetProperty("payload", out var payload) ||
                !payload.TryGetProperty("rate_limits", out var limits) || limits.ValueKind != JsonValueKind.Object) return null;
            var at = root.TryGetProperty("timestamp", out var ts) && ts.ValueKind == JsonValueKind.String &&
                     DateTimeOffset.TryParse(ts.GetString(), out var t) ? t.LocalDateTime : now;

            (double Used, string Label)? Window(string name, string fallback)
            {
                if (!limits.TryGetProperty(name, out var w) || w.ValueKind != JsonValueKind.Object ||
                    N(w, "used_percent") is not { } used) return null;
                DateTime? reset = null;
                if (N(w, "resets_at") is { } epoch) reset = DateTimeOffset.FromUnixTimeSeconds((long)epoch).LocalDateTime;
                else if (N(w, "resets_in_seconds") is { } secs) reset = at.AddSeconds(secs);
                if (reset is { } rs && rs <= now) used = 0;
                var mins = N(w, "window_minutes") ?? 0;
                var label = mins switch { >= 10000 => "week", >= 60 => $"{Math.Round(mins / 60)}h", > 0 => $"{mins}m", _ => fallback };
                return (Math.Clamp(used, 0, 100), label);
            }

            var windows = new[] { Window("primary", "5h"), Window("secondary", "week") }.OfType<(double Used, string Label)>().ToList();
            if (windows.Count == 0) return null;
            var detail = string.Join(" · ", windows.Select(w => $"{w.Label} {w.Used:0}%")) + $" · as of {at:d MMM HH:mm}";
            return new QuotaReading(windows.Max(w => w.Used), detail);
        }
        catch (Exception e) when (e is JsonException or ArgumentOutOfRangeException or InvalidOperationException) { return null; }
    }

    // MARK: Cursor

    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(15) };

    public static async Task<QuotaReading> Cursor(CancellationToken ct)
    {
        var db = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "Cursor", "User", "globalStorage", "state.vscdb");
        if (!File.Exists(db)) return QuotaReading.Fail("Cursor isn’t installed, or hasn’t been signed in to.");
        string? token;
        try { token = CursorToken(db); }
        catch (Exception e) { return QuotaReading.Fail($"Couldn’t read Cursor’s sign-in: {e.Message}"); }
        if (token is null) return QuotaReading.Fail("Sign in to Cursor first.");
        var cookie = CursorCookie(token, DateTime.UtcNow);
        if (cookie is null) return QuotaReading.Fail("Cursor’s sign-in has expired — open Cursor to renew it.");
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, "https://cursor.com/api/usage-summary");
            req.Headers.TryAddWithoutValidation("Cookie", cookie);
            req.Headers.TryAddWithoutValidation("Accept", "application/json");
            req.Headers.TryAddWithoutValidation("User-Agent", "Hover");
            using var res = await Http.SendAsync(req, ct);
            if ((int)res.StatusCode is 401 or 403) return QuotaReading.Fail("cursor.com refused Cursor’s sign-in — open Cursor to renew it.");
            if (!res.IsSuccessStatusCode) return QuotaReading.Fail($"cursor.com answered {(int)res.StatusCode}.");
            return ParseCursorSummary(await res.Content.ReadAsStringAsync(ct));
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException)
        {
            return QuotaReading.Fail("Couldn’t reach cursor.com.");
        }
    }

    private static string? CursorToken(string db)
    {
        // Read-only and unpooled, so Hover never holds Cursor's database open.
        using var c = new SqliteConnection(new SqliteConnectionStringBuilder
        {
            DataSource = db, Mode = SqliteOpenMode.ReadOnly, Pooling = false,
            // Cursor may be writing; give up quickly rather than wait out the lock.
            DefaultTimeout = 2,
        }.ToString());
        c.Open();
        using var cmd = c.CreateCommand();
        cmd.CommandText = "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'";
        return cmd.ExecuteScalar() switch
        {
            string s => CleanToken(s),
            byte[] b => CleanToken(b.Length > 1 && b[1] == 0 ? Encoding.Unicode.GetString(b) : Encoding.UTF8.GetString(b)),
            _ => null,
        };
    }

    private static string? CleanToken(string s)
    {
        s = s.Trim().Trim('"');
        return s.Length == 0 ? null : s;
    }

    /// Cursor's web session cookie is "user id::token"; the user id is the last part
    /// of the token's subject. Null when the token is malformed or about to expire.
    public static string? CursorCookie(string token, DateTime utcNow)
    {
        var parts = token.Split('.');
        if (parts.Length != 3) return null;
        try
        {
            var body = parts[1].Replace('-', '+').Replace('_', '/');
            body = body.PadRight(body.Length + (4 - body.Length % 4) % 4, '=');
            using var doc = JsonDocument.Parse(Convert.FromBase64String(body));
            var root = doc.RootElement;
            if (N(root, "exp") is { } e && DateTimeOffset.FromUnixTimeSeconds((long)e).UtcDateTime <= utcNow.AddSeconds(60)) return null;
            if (!root.TryGetProperty("sub", out var sub) || sub.ValueKind != JsonValueKind.String || sub.GetString() is not { } s) return null;
            var id = s.Split('|', StringSplitOptions.RemoveEmptyEntries).LastOrDefault();
            if (id is null || !id.All(ch => char.IsAsciiLetterOrDigit(ch) || ch is '.' or '_' or '-')) return null;
            return $"WorkosCursorSessionToken={id}%3A%3A{token}";
        }
        catch (Exception ex) when (ex is FormatException or JsonException or InvalidOperationException or ArgumentOutOfRangeException)
        {
            return null;
        }
    }

    /// usage-summary: the plan's percentages are already in percent. Older and team
    /// accounts report cents used against a limit instead.
    public static QuotaReading ParseCursorSummary(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            JsonElement? Get(params string[] path)
            {
                var cur = root;
                foreach (var p in path)
                {
                    if (cur.ValueKind != JsonValueKind.Object || !cur.TryGetProperty(p, out var next)) return null;
                    cur = next;
                }
                return cur;
            }
            double? D(params string[] path) => Get(path) is { ValueKind: JsonValueKind.Number } e ? e.GetDouble() : null;
            string? S(string name) => Get(name) is { ValueKind: JsonValueKind.String } e ? e.GetString() : null;
            double? Ratio(string a, string b) =>
                D(a, b, "used") is { } u && D(a, b, "limit") is { } l && l > 0 ? u / l * 100 : null;

            var auto = D("individualUsage", "plan", "autoPercentUsed");
            var api = D("individualUsage", "plan", "apiPercentUsed");
            var used = D("individualUsage", "plan", "totalPercentUsed")
                ?? (auto is { } x && api is { } y ? (x + y) / 2 : auto ?? api)
                ?? Ratio("individualUsage", "plan")
                ?? Ratio("individualUsage", "overall")
                ?? Ratio("teamUsage", "pooled");
            if (used is null) return QuotaReading.Fail("Cursor didn’t report plan usage.");

            var membership = S("membershipType");
            var end = S("billingCycleEnd");
            var detail = (membership is { Length: > 0 } ? char.ToUpperInvariant(membership[0]) + membership[1..] + " · " : "")
                         + $"{used:0}% of plan"
                         + (DateTimeOffset.TryParse(end, out var e) ? $" · resets {e.LocalDateTime:d MMM}" : "");
            return new QuotaReading(Math.Clamp(used.Value, 0, 100), detail);
        }
        catch (Exception e) when (e is JsonException or InvalidOperationException) { return QuotaReading.Fail("Cursor’s answer couldn’t be read."); }
    }

    // MARK: Claude Code

    /// Claude Code's plan limits, as its /usage shows them. Claude Code keeps no usage
    /// file; it asks api.anthropic.com/api/oauth/usage (undocumented, found by the
    /// community) with its own sign-in. Hover asks the same way, read-only: it never
    /// refreshes that sign-in, which would rotate Claude Code's tokens underneath it.
    public static async Task<QuotaReading> Claude(DateTime now, CancellationToken ct)
    {
        var home = Environment.GetEnvironmentVariable("CLAUDE_CONFIG_DIR");
        if (string.IsNullOrWhiteSpace(home))
            home = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".claude");
        var file = Path.Combine(home, ".credentials.json");
        if (!File.Exists(file)) return QuotaReading.Fail("Sign in to Claude Code with a Claude plan (Pro or Max) first.");
        (string? Token, DateTime? Expires, string? Plan) sign;
        try { sign = ClaudeSignIn(File.ReadAllText(file)); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return QuotaReading.Fail($"Couldn’t read Claude Code’s sign-in: {e.Message}"); }
        if (sign.Token is null) return QuotaReading.Fail("Sign in to Claude Code with a Claude plan (Pro or Max) first.");
        if (sign.Expires is { } exp && exp <= now.ToUniversalTime().AddSeconds(60))
            return QuotaReading.Fail("Claude Code’s sign-in has expired — run claude to renew it.");
        try
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, "https://api.anthropic.com/api/oauth/usage");
            req.Headers.TryAddWithoutValidation("Authorization", "Bearer " + sign.Token);
            req.Headers.TryAddWithoutValidation("anthropic-beta", "oauth-2025-04-20");
            req.Headers.TryAddWithoutValidation("Accept", "application/json");
            req.Headers.TryAddWithoutValidation("User-Agent", "Hover");
            using var res = await Http.SendAsync(req, ct);
            if ((int)res.StatusCode == 401) return QuotaReading.Fail("Anthropic refused Claude Code’s sign-in — run claude to renew it.");
            if ((int)res.StatusCode == 403) return QuotaReading.Fail("This sign-in can’t read plan usage — run claude and sign in again.");
            if ((int)res.StatusCode == 429) return QuotaReading.Fail("Anthropic is limiting usage checks; Hover tries again in five minutes.");
            if (!res.IsSuccessStatusCode) return QuotaReading.Fail($"api.anthropic.com answered {(int)res.StatusCode}.");
            return ParseClaudeUsage(await res.Content.ReadAsStringAsync(ct), sign.Plan, now);
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException)
        {
            return QuotaReading.Fail("Couldn’t reach api.anthropic.com.");
        }
    }

    /// .credentials.json: {"claudeAiOauth": {"accessToken", "expiresAt" (ms), "subscriptionType"}}.
    public static (string? Token, DateTime? ExpiresUtc, string? Plan) ClaudeSignIn(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            if (!doc.RootElement.TryGetProperty("claudeAiOauth", out var o) || o.ValueKind != JsonValueKind.Object) return (null, null, null);
            string? S(string name) => o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
            var token = S("accessToken") is { Length: > 0 } t ? t : null;
            DateTime? exp = N(o, "expiresAt") is { } ms ? DateTimeOffset.FromUnixTimeMilliseconds((long)ms).UtcDateTime : null;
            return (token, exp, S("subscriptionType"));
        }
        catch (Exception e) when (e is JsonException or ArgumentOutOfRangeException) { return (null, null, null); }
    }

    /// The usage answer: five_hour and seven_day, each a utilization in percent and an
    /// ISO resets_at (null while the window hasn't begun). The notch shows whichever
    /// is closer to the limit, as it does for Codex.
    public static QuotaReading ParseClaudeUsage(string json, string? plan, DateTime now)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            (double Used, string Label, DateTime? Reset)? Window(string name, string label)
            {
                if (!root.TryGetProperty(name, out var w) || w.ValueKind != JsonValueKind.Object || N(w, "utilization") is not { } used) return null;
                DateTime? reset = w.TryGetProperty("resets_at", out var r) && r.ValueKind == JsonValueKind.String &&
                                  DateTimeOffset.TryParse(r.GetString(), out var t) ? t.LocalDateTime : null;
                if (reset is { } rs && rs <= now) used = 0;
                return (Math.Clamp(used, 0, 100), label, reset);
            }
            var windows = new[] { Window("five_hour", "5h"), Window("seven_day", "week") }.OfType<(double Used, string Label, DateTime? Reset)>().ToList();
            if (windows.Count == 0) return QuotaReading.Fail("Anthropic didn’t report plan usage.");
            var top = windows.MaxBy(w => w.Used);
            var planText = plan is { Length: > 0 } ? char.ToUpperInvariant(plan[0]) + plan[1..] + " · " : "";
            var resetText = top.Reset is { } when ? $" · resets {(when.Date == now.Date ? when.ToString("HH:mm") : when.ToString("d MMM HH:mm"))}" : "";
            return new QuotaReading(top.Used, planText + string.Join(" · ", windows.Select(w => $"{w.Label} {w.Used:0}%")) + resetText);
        }
        catch (Exception e) when (e is JsonException or InvalidOperationException) { return QuotaReading.Fail("Anthropic’s answer couldn’t be read."); }
    }

    // MARK: Bits

    private static double? N(JsonElement o, string name) =>
        o.ValueKind == JsonValueKind.Object && o.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetDouble() : null;

    private static double? Num(string s) =>
        double.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var v) ? v : null;

    /// The first match for a command on PATH, trying Windows' executable suffixes.
    internal static string? OnPath(string name)
    {
        var exts = OperatingSystem.IsWindows()
            ? (Environment.GetEnvironmentVariable("PATHEXT") ?? ".EXE;.CMD;.BAT").Split(';', StringSplitOptions.RemoveEmptyEntries)
            : new[] { "" };
        foreach (var dir in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
            foreach (var ext in exts)
            {
                try
                {
                    var p = Path.Combine(dir.Trim('"'), name + ext);
                    if (File.Exists(p)) return p;
                }
                catch (ArgumentException) { /* a malformed PATH entry */ }
            }
        return null;
    }
}
