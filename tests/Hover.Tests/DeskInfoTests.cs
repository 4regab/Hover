using System.IO;
using System.Text.Json;
using Hover.Owl;
using Hover.Services;
using NUnit.Framework;

namespace Hover.Tests;

/// The desk menu's panels: how a diff, a status and a step are read, which calls
/// count as subagents and computer use, and that a file request can't leave the
/// session's folder. One test runs the real git in a folder of its own.
[TestFixture, NonParallelizable]
public sealed class DeskInfoTests
{
    private string _dir = "";

    [SetUp]
    public void SetUp()
    {
        _dir = Path.Combine(Path.GetTempPath(), "hover-desk-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(_dir, "src"));
    }

    [TearDown]
    public void TearDown()
    {
        try { Directory.Delete(_dir, true); } catch (IOException) { }
    }

    private static DeskInfo.Snap Snap(string folder, params KiroStep[] steps) =>
        new(folder, false, steps.Select(s => new DeskInfo.Item(0, s)).ToList(), new[] { "Fix it", "Opened https://github.com/acme/app/pull/7 for review." });

    [Test]
    public void A_unified_diff_becomes_one_entry_per_file()
    {
        var patch = string.Join("\n",
            "diff --git a/src/a.cs b/src/a.cs", "index 1..2 100644", "--- a/src/a.cs", "+++ b/src/a.cs",
            "@@ -1,3 +1,3 @@", " keep", "-old", "+new", " end",
            "diff --git a/new.txt b/new.txt", "new file mode 100644", "--- /dev/null", "+++ b/new.txt", "@@ -0,0 +1,2 @@", "+one", "+two",
            "diff --git a/old.md b/renamed.md", "similarity index 90%", "rename from old.md", "rename to renamed.md",
            "diff --git a/logo.png b/logo.png", "Binary files a/logo.png and b/logo.png differ");
        var files = DeskInfo.ParseDiff(patch);
        Assert.That(files.Select(f => (f.Path, f.Status, f.Add, f.Del)), Is.EqualTo(new[]
        {
            ("src/a.cs", "M", 1, 1), ("new.txt", "A", 2, 0), ("renamed.md", "R", 0, 0), ("logo.png", "M", 0, 0),
        }));
        Assert.That(files[0].Patch, Does.StartWith("@@ -1,3 +1,3 @@").And.Contain("-old\n+new"));
        Assert.That(files[2].Old, Is.EqualTo("old.md"));
        Assert.That(files[3].Binary, Is.True);
    }

    [Test]
    public void Status_paths_are_relative_to_the_folder_in_the_repository()
    {
        var z = " M app/src/a.cs\0?? app/notes.txt\0R  app/b.cs\0app/a_old.cs\0 D app/gone.cs\0";
        var list = DeskInfo.ParseStatus(z, "app/");
        Assert.That(list.Select(c => (c.Path, c.Status, c.Old)), Is.EqualTo(new (string, string, string?)[]
        {
            ("src/a.cs", "M", null), ("notes.txt", "?", null), ("b.cs", "R", "a_old.cs"), ("gone.cs", "D", null),
        }));
    }

    [Test]
    public void A_file_request_stays_inside_the_folder()
    {
        File.WriteAllText(Path.Combine(_dir, "src", "ok.txt"), "hello");
        var outside = Path.Combine(Path.GetTempPath(), "hover-desk-outside-" + Guid.NewGuid().ToString("N") + ".txt");
        File.WriteAllText(outside, "secret");
        try
        {
            Assert.That(DeskInfo.Inside(_dir, "src/ok.txt"), Is.Not.Null);
            foreach (var bad in new[] { "../x.txt", "src/../../x.txt", "/etc/passwd", "", "a\0b" })
                Assert.That(DeskInfo.Inside(_dir, bad), Is.Null, bad);
            // A link in the folder that points out of it is refused too.
            File.CreateSymbolicLink(Path.Combine(_dir, "leak.txt"), outside);
            Directory.CreateSymbolicLink(Path.Combine(_dir, "up"), Path.GetTempPath());
            Assert.That(DeskInfo.Inside(_dir, "leak.txt"), Is.Null);
            Assert.That(DeskInfo.Inside(_dir, "up/" + Path.GetFileName(outside)), Is.Null);
            var json = JsonSerializer.Serialize(DeskInfo.FileText(_dir, "leak.txt"));
            Assert.That(json, Does.Not.Contain("secret"));
            Assert.That(JsonSerializer.Serialize(DeskInfo.FileText(_dir, "src/ok.txt")), Does.Contain("hello"));
        }
        finally { File.Delete(outside); }
    }

    [Test]
    public void Subagents_and_computer_use_are_told_apart_from_other_calls()
    {
        var task = new KiroStep("1", "other", "Find the notch code", null, "completed", Input: "{\"description\":\"Find it\",\"prompt\":\"Look\",\"subagent_type\":\"explore\"}");
        var spawn = new KiroStep("2", "other", "spawn_agent", null, "in_progress");
        var click = new KiroStep("3", "other", "mcp__cua-driver__click", null, "completed");
        var shot = new KiroStep("4", "other", "screenshot", null, "completed");
        var read = new KiroStep("5", "read", "Read src/a.cs", "src/a.cs", "completed");
        var clicked = new KiroStep("6", "other", "Clicked through the docs", null, "completed");
        Assert.That(new[] { task, spawn, click, shot, read, clicked }.Select(DeskInfo.IsSubagent), Is.EqualTo(new[] { true, true, false, false, false, false }));
        Assert.That(new[] { task, spawn, click, shot, read, clicked }.Select(DeskInfo.IsScreen), Is.EqualTo(new[] { false, false, true, true, false, false }));
        var json = JsonSerializer.Serialize(DeskInfo.Subagents(Snap(_dir, task, spawn, read)));
        Assert.That(json, Does.Contain("\"name\":\"explore\"").And.Contain("\"task\":\"Find it\"").And.Contain("\"running\":1"));
    }

    [Test]
    public void Commands_pages_and_linked_pull_requests_come_from_the_steps()
    {
        var dev = new KiroStep("1", "execute", "Run", null, "completed", Exit: 0, Input: "{\"command\":[\"bash\",\"-lc\",\"npm run dev\"]}",
            Log: "VITE ready\n  Local:   http://localhost:5173/\n  Network: http://192.168.1.4:5173/");
        var fetch = new KiroStep("2", "fetch", "three.js docs", "https://threejs.org/docs/", "completed");
        var pr = new KiroStep("3", "execute", "Run", "gh pr create", "completed", Log: "https://github.com/acme/app/pull/12");
        Assert.That(DeskInfo.CommandOf(dev), Is.EqualTo("npm run dev"));
        var snap = Snap(_dir, dev, fetch, pr);
        var term = JsonSerializer.Serialize(DeskInfo.Terminal(snap));
        Assert.That(term, Does.Contain("npm run dev").And.Contain("VITE ready"));
        var pages = DeskInfo.Pages(snap).Select(p => JsonSerializer.Serialize(p)).ToList();
        // The fetch is newest; the dev server's local address counts, its network one doesn't.
        Assert.That(pages, Has.Count.EqualTo(2));
        Assert.That(pages[0], Does.Contain("threejs.org"));
        Assert.That(pages[1], Does.Contain("http://localhost:5173/").And.Contain("\"local\":true"));
        Assert.That(DeskInfo.LinkedUrls(snap).Select(u => (u.Repo, u.Number)), Is.EqualTo(new[] { ("acme/app", 12), ("acme/app", 7) }));
    }

    [Test]
    public void A_tool_calls_input_and_longer_output_are_kept()
    {
        using var doc = JsonDocument.Parse("{\"rawInput\":{\"url\":\"https://example.com\"},\"content\":[{\"type\":\"content\",\"content\":{\"type\":\"text\",\"text\":\"page text\"}}]}");
        Assert.That(KiroStream.InputOf(doc.RootElement), Is.EqualTo("{\"url\":\"https://example.com\"}"));
        Assert.That(KiroStream.LogOf(doc.RootElement, "fetch"), Is.EqualTo("page text"));
        Assert.That(KiroStream.LogOf(doc.RootElement, "read"), Is.Null);
        var big = string.Join("\n", Enumerable.Range(0, 5000).Select(i => "line " + i));
        var tail = KiroStream.Tail(big)!;
        Assert.That(tail.Length, Is.LessThanOrEqualTo(KiroStream.LogLimit));
        Assert.That(tail, Does.EndWith("line 4999").And.StartWith("line "));
    }

    private static string? GitExe() =>
        (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator).Select(d => Path.Combine(d, OperatingSystem.IsWindows() ? "git.exe" : "git")).FirstOrDefault(File.Exists);

    [Test]
    public void The_diff_and_files_of_a_real_repository()
    {
        if (GitExe() is null) Assert.Ignore("git isn't installed.");
        Environment.SetEnvironmentVariable("GIT_CONFIG_GLOBAL", "/dev/null");
        Environment.SetEnvironmentVariable("GIT_CONFIG_NOSYSTEM", "1");
        void Git(params string[] args) => Assert.That(DeskInfo.Run(GitExe()!, _dir, 20000, 65536, args).Code, Is.Zero, string.Join(" ", args));
        File.WriteAllText(Path.Combine(_dir, "src", "a.txt"), "one\ntwo\n");
        Git("init", "-q");
        Git("add", ".");
        Git("-c", "user.email=t@example.com", "-c", "user.name=Test", "commit", "-q", "-m", "first");
        File.WriteAllText(Path.Combine(_dir, "src", "a.txt"), "one\nTWO\n");
        File.WriteAllText(Path.Combine(_dir, "new.txt"), "fresh\n");
        var snap = Snap(_dir, new KiroStep("1", "edit", "Edit", Path.Combine(_dir, "src", "a.txt"), "completed", 1, 1));
        var diff = JsonSerializer.Serialize(DeskInfo.Diff(snap));
        // ("+" is written \u002B by the default encoder.)
        Assert.That(diff, Does.Contain("\"git\":true").And.Contain("src/a.txt").And.Contain("-two\\n\\u002BTWO").And.Contain("new.txt").And.Contain("\\u002Bfresh"));
        var files = JsonSerializer.Serialize(DeskInfo.Files(snap));
        Assert.That(files, Does.Contain("\"tree\":[\"new.txt\",\"src/a.txt\"]"));
        Assert.That(files, Does.Contain("\"path\":\"src/a.txt\",\"status\":\"M\""));
        Assert.That(files, Does.Contain("\"touched\":[{\"path\":\"src/a.txt\",\"read\":0,\"edit\":1}]"));
    }
}
