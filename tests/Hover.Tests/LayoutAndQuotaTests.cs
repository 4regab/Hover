using System.IO;
using System.Text;
using System.Text.Json;
using Hover.Core;
using NUnit.Framework;

namespace Hover.Tests;

public sealed class LayoutTests
{
    [Test]
    public void A_missing_layout_is_every_card_in_order()
    {
        Assert.That(CardLayout.Normalize(null).Select(c => c.Id),
            Is.EqualTo(new[] { "tasks", "timer", "notepad", "events", "shots" }));
    }

    [Test]
    public void A_saved_layout_is_repaired()
    {
        var saved = new[]
        {
            new CardSlot("events", true, 9), new CardSlot("bogus"), new CardSlot("events"),
            new CardSlot("tasks", true, 0.1), new CardSlot("timer", true, double.NaN),
        };
        var n = CardLayout.Normalize(saved);
        Assert.Multiple(() =>
        {
            Assert.That(n.Select(c => c.Id).Distinct().Count(), Is.EqualTo(5), "unknown and repeated cards dropped, missing ones added");
            Assert.That(n[0].Id, Is.EqualTo("events"), "the saved order is kept");
            Assert.That(n.Single(c => c.Id == "events").Width, Is.EqualTo(CardLayout.MaxWidth));
            Assert.That(n.Single(c => c.Id == "tasks").Width, Is.EqualTo(CardLayout.MinWidth));
            Assert.That(n.Single(c => c.Id == "timer").Width, Is.EqualTo(CardLayout.Default.Single(c => c.Id == "timer").Width), "a broken width falls back to the default");
        });
    }

    [Test]
    public void Every_card_hidden_brings_back_tasks()
    {
        var n = CardLayout.Normalize(CardLayout.Default.Select(c => c with { Visible = false }));
        Assert.That(n.Single(c => c.Visible).Id, Is.EqualTo(CardLayout.Tasks));
    }

    [Test]
    public void Cards_move_within_bounds()
    {
        Assert.Multiple(() =>
        {
            Assert.That(CardLayout.Move(CardLayout.Default, "timer", -1).Select(c => c.Id).Take(2), Is.EqualTo(new[] { "timer", "tasks" }));
            Assert.That(CardLayout.Move(CardLayout.Default, "tasks", -1)[0].Id, Is.EqualTo("tasks"));
            Assert.That(CardLayout.Move(CardLayout.Default, "shots", 1).Last().Id, Is.EqualTo("shots"));
        });
    }

    [Test]
    public void A_card_saved_without_a_width_gets_the_default()
    {
        var saved = JsonSerializer.Deserialize<List<CardSlot>>("[{\"Id\":\"shots\",\"Visible\":false}]")!;
        Assert.That(saved[0], Is.EqualTo(new CardSlot("shots", false, 1)));
    }
}

public sealed class QuotaTests
{
    [Test]
    public void Kiro_reads_the_bar_the_credits_the_plan_and_the_reset()
    {
        var output = "\u001b[1m┃  | KIRO FREE ┃\n┃ Monthly credits: ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃\n";
        var q = Quota.ParseKiro(output);
        Assert.Multiple(() =>
        {
            Assert.That(q.Used, Is.EqualTo(42));
            Assert.That(q.Detail, Is.EqualTo("KIRO FREE · 21 of 50 credits · resets 10/01"));
            Assert.That(Quota.ParseKiro("(10.5 of 50 covered in plan)").Used, Is.EqualTo(21).Within(0.001));
            Assert.That(Quota.ParseKiro("Not logged in. Run kiro-cli login.").Ok, Is.False);
            Assert.That(Quota.ParseKiro("anything else").Ok, Is.False);
        });
    }

    private static readonly DateTime Now = new(2026, 9, 26, 12, 0, 0, DateTimeKind.Local);

    private static string CodexLine(long primaryReset) =>
        "{\"timestamp\":\"" + new DateTimeOffset(Now.AddMinutes(-5)).ToString("o") + "\",\"type\":\"event_msg\"," +
        "\"payload\":{\"type\":\"token_count\",\"rate_limits\":{" +
        "\"primary\":{\"used_percent\":37.5,\"window_minutes\":300,\"resets_at\":" + primaryReset + "}," +
        "\"secondary\":{\"used_percent\":12,\"window_minutes\":10080,\"resets_in_seconds\":3600}}}}";

    [Test]
    public void Codex_shows_the_window_nearest_its_limit()
    {
        var q = Quota.ParseCodexLine(CodexLine(new DateTimeOffset(Now.AddHours(2)).ToUnixTimeSeconds()), Now);
        Assert.That(q!.Used, Is.EqualTo(37.5));
        Assert.That(q.Detail, Does.StartWith("5h 38% · week 12%"));
    }

    [Test]
    public void Codex_counts_a_window_that_has_reset_as_empty()
    {
        var q = Quota.ParseCodexLine(CodexLine(new DateTimeOffset(Now.AddHours(-1)).ToUnixTimeSeconds()), Now);
        Assert.That(q!.Used, Is.EqualTo(12));
    }

    [Test]
    public void Codex_skips_lines_without_limits()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Quota.ParseCodexLine("{\"payload\":{\"rate_limits\":null}}", Now), Is.Null);
            Assert.That(Quota.ParseCodexLine("{\"payload\":{\"rate_limits\":{\"primary\":{\"used_percent\":\"x\"}}}}", Now), Is.Null);
            Assert.That(Quota.ParseCodexLine("not json", Now), Is.Null);
        });
    }

    [Test]
    public void Codex_reads_the_newest_session_log()
    {
        var home = Path.Combine(TestEnvironment.Root, "codex");
        var day = Path.Combine(home, "sessions", "2026", "09", "26");
        Directory.CreateDirectory(day);
        File.WriteAllText(Path.Combine(day, "rollout-a.jsonl"),
            "{\"x\":1}\n" + CodexLine(new DateTimeOffset(Now.AddHours(2)).ToUnixTimeSeconds()) + "\n{\"type\":\"other\"}\n");
        var before = Environment.GetEnvironmentVariable("CODEX_HOME");
        Environment.SetEnvironmentVariable("CODEX_HOME", home);
        try { Assert.That(Quota.Codex(Now).Used, Is.EqualTo(37.5)); }
        finally { Environment.SetEnvironmentVariable("CODEX_HOME", before); }
    }

    private static string B64(string s) =>
        Convert.ToBase64String(Encoding.UTF8.GetBytes(s)).TrimEnd('=').Replace('+', '-').Replace('/', '_');

    [Test]
    public void Cursor_cookie_is_the_user_id_and_the_token()
    {
        var exp = DateTimeOffset.UtcNow.AddHours(1).ToUnixTimeSeconds();
        var token = B64("{\"alg\":\"HS256\"}") + "." + B64("{\"sub\":\"auth0|user_01ABC\",\"exp\":" + exp + "}") + ".sig";
        Assert.Multiple(() =>
        {
            Assert.That(Quota.CursorCookie(token, DateTime.UtcNow), Is.EqualTo("WorkosCursorSessionToken=user_01ABC%3A%3A" + token));
            Assert.That(Quota.CursorCookie(B64("{}") + "." + B64("{\"sub\":\"a|u1\",\"exp\":1}") + ".s", DateTime.UtcNow), Is.Null, "expired");
            Assert.That(Quota.CursorCookie(B64("{}") + "." + B64("{\"sub\":\"a|b c\"}") + ".s", DateTime.UtcNow), Is.Null, "bad id");
            Assert.That(Quota.CursorCookie("abc", DateTime.UtcNow), Is.Null, "not a JWT");
        });
    }

    [TestCase("{\"membershipType\":\"pro\",\"individualUsage\":{\"plan\":{\"used\":500,\"limit\":2000,\"totalPercentUsed\":33.4}}}", 33.4)]
    [TestCase("{\"individualUsage\":{\"plan\":{\"used\":500,\"limit\":2000}}}", 25)]
    [TestCase("{\"individualUsage\":{\"plan\":{\"autoPercentUsed\":10,\"apiPercentUsed\":30}}}", 20)]
    [TestCase("{\"teamUsage\":{\"pooled\":{\"used\":1,\"limit\":4}},\"membershipType\":5}", 25)]
    public void Cursor_reads_plan_usage(string json, double used) =>
        Assert.That(Quota.ParseCursorSummary(json).Used, Is.EqualTo(used).Within(0.001));

    [Test]
    public void Cursor_without_usage_is_a_readable_failure()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Quota.ParseCursorSummary("{}").Ok, Is.False);
            Assert.That(Quota.ParseCursorSummary("[").Ok, Is.False);
        });
    }
}
