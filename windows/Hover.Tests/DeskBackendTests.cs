using System.Text.Json;
using Hover.Chat.Desk;
using Hover.Host;
using Hover.Host.Protocol;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>
/// The desk's commands against a real hover-backend and the stand-in agent: the panels' data from a real git folder,
/// the user's shell, saving a file, Open in, attachments on a reply, and the GitHub CLI's check.
/// </summary>
public class DeskBackendTests(ITestOutputHelper output)
{
    const string OnePixel = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    static async Task<SessionInfo> Finished(Conversation talk, Sandbox sandbox, string prompt = "Tidy it")
    {
        await talk.GetReady();
        talk.Send(new NewTaskCommand("codex", sandbox.Project, prompt, "always"));
        var started = (await talk.Until<StateEvent>(x => x.Sessions.Count > 0)).Sessions[0];
        var waiting = started.Stage == "waiting" ? started : (await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "waiting")).Sessions[0];
        talk.Send(new AnswerCommand(waiting.Id, waiting.Ask!.Id, "allow"));
        return (await talk.Until<StateEvent>(s => s.Sessions.Count > 0 && s.Sessions[0].Stage == "done")).Sessions[0];
    }

    static async Task<Jv> Panel(Conversation talk, int id, string what, string? arg = null)
    {
        talk.Send(new DeskCommand(id, what, arg));
        var e = await talk.Until<DeskEvent>(d => d.Id == id && d.What == what);
        return new Jv(e.Data);
    }

    // MARK: The panels

    [Fact]
    public async Task The_desk_panels_read_the_folder_the_session_works_in()
    {
        using var sandbox = new Sandbox();
        File.WriteAllBytes(Path.Combine(sandbox.Project, "pic.bin"), [1, 2, 0, 3, 4]);
        Directory.CreateDirectory(Path.Combine(sandbox.Project, "docs"));
        File.WriteAllText(Path.Combine(sandbox.Project, "docs", "guide.md"), "# Guide\n\nHello.\n");
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);

        var probe = await Panel(talk, s.Id, "probe");
        Assert.True(probe["folder"].Bool());
        Assert.True(probe["git"].Bool());
        Assert.True(probe["changed"].Long() >= 1);
        Assert.True(probe["add"].Long() >= 1);
        Assert.Equal(1, probe["agents"].Int());
        // The tiles the card draws from it.
        var tiles = DeskTiles.Of(probe, s, null, 0);
        Assert.True(tiles.First(t => t.Id == "diff").Enabled);
        Assert.True(tiles.First(t => t.Id == "terminal").Enabled);
        Assert.False(tiles.First(t => t.Id == "browser").Enabled);

        var files = await Panel(talk, s.Id, "files");
        var tree = files["tree"].Strings();
        Assert.Contains("app.js", tree);
        Assert.Contains("docs/guide.md", tree);
        Assert.Contains(files["changed"].Items(), c => c["path"].Str() == "app.js" && c["status"].Str() == "M");
        var rows = DeskRows.FilesRows(files, new HashSet<string>(), new HashSet<string>());
        Assert.Contains(rows.Rows, r => r.Text == "app.js" && r.Badge == "M");

        var file = await Panel(talk, s.Id, "file", "app.js");
        Assert.Equal("a\nb\n", file["text"].Str().Replace("\r\n", "\n"));
        // A binary file is said to be one; a file that is not there says so.
        Assert.True((await Panel(talk, s.Id, "file", "pic.bin"))["binary"].Bool());
        Assert.True((await Panel(talk, s.Id, "file", "gone.txt"))["error"].Has);
        // A path out of the folder is not read.
        Assert.True((await Panel(talk, s.Id, "file", "../outside.txt"))["error"].Has);

        var diff = await Panel(talk, s.Id, "diff");
        var change = diff["files"].Items().Single(f => f["path"].Str() == "app.js");
        Assert.Contains("+b", change["patch"].Str());
        var laid = DeskRows.DiffRows(diff, new Dictionary<string, bool>(), false);
        Assert.Contains(laid.Rows, r => r.Kind == 10 && r.Tone == 1 && r.Text == "b" && r.Num2 == "2");

        var terminal = await Panel(talk, s.Id, "terminal");
        Assert.Contains(terminal["commands"].Items(), c => c["cmd"].Str() == "npm run dev");
        var agents = await Panel(talk, s.Id, "agents");
        Assert.Equal(1, agents["agents"].Count);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task The_pull_request_panel_answers_without_a_remote_and_with_the_GitHub_CLI_hidden()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        var pr = await Panel(talk, s.Id, "pr");
        output.WriteLine(pr.Raw.GetRawText());
        // No remote and no pull request: the panel offers the form or the setup, and never an empty answer.
        Assert.True(pr["setup"].Has || pr["none"].Bool() || pr["error"].Has);
        talk.Send(new GhCommand());
        var gh = await talk.Until<GhEvent>();
        output.WriteLine($"gh checked={gh.Checked} installed={gh.Installed} signedIn={gh.SignedIn}");
        Assert.False(gh.Busy);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task The_GitHub_CLI_is_found_missing_when_it_is_not_there_and_its_setup_can_be_cancelled()
    {
        // Only where gh can be hidden by the environment: Windows looks for it on PATH and in Program Files.
        if (!OperatingSystem.IsWindows()) return;
        using var sandbox = new Sandbox();
        var empty = Directory.CreateTempSubdirectory("hover-nogh.").FullName;
        var env = new Dictionary<string, string?>(sandbox.Environment)
        {
            ["ProgramFiles"] = empty, ["ProgramW6432"] = empty, ["ProgramFiles(x86)"] = empty, ["LOCALAPPDATA"] = empty,
            ["PATH"] = string.Join(Path.PathSeparator, (sandbox.Environment["PATH"] ?? "").Split(Path.PathSeparator).Where(p => !p.Contains("GitHub CLI", StringComparison.OrdinalIgnoreCase))),
        };
        var client = BackendClient.Start(new BackendOptions { ExecutablePath = TestEnvironment.Backend, DataDir = sandbox.Data, Environment = env });
        var talk = new Conversation(client);
        await using var _ = client;
        await client.InitializeAsync(Conversation.Key);
        var s = await Finished(talk, sandbox);
        var pr = await Panel(talk, s.Id, "pr");
        Assert.Equal("install", pr["setup"].Str());
        talk.Send(new GhCommand());
        var gh = await talk.Until<GhEvent>(g => g.Checked);
        Assert.False(gh.Installed);
        // Cancelling when nothing runs changes nothing and answers nothing that breaks.
        talk.Send(new GhCommand("cancel"));
        talk.Send(new GhCommand());
        Assert.False((await talk.Until<GhEvent>()).Busy);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    // MARK: The user's shell

    [Fact]
    public async Task The_shell_runs_a_command_in_the_folder_and_keeps_its_output()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        talk.Send(new TermCommand(s.Id));
        var view = await talk.Until<TermEvent>(t => t.Id == s.Id);
        Assert.True(view.Ok);
        Assert.Empty(view.Entries);
        Assert.False(view.Running);

        talk.Send(new TermCommand(s.Id, "run", "echo hello-from-the-shell"));
        var done = await talk.Until<TermEvent>(t => t.Id == s.Id && t.Entries.Any(e => e.Run.Kind != "running" && e.Lines.Any(l => l.Text.Contains("hello-from-the-shell"))));
        var entry = done.Entries.Last();
        Assert.Equal("echo hello-from-the-shell", entry.Cmd);
        Assert.Equal(0, entry.Run.Code);
        Assert.False(done.Running);
        // The shell works in the session's folder.
        Assert.Equal(Path.GetFullPath(sandbox.Project).TrimEnd('\\', '/'), Path.GetFullPath(done.Cwd).TrimEnd('\\', '/'), ignoreCase: OperatingSystem.IsWindows());

        // A command that fails says so, with its error output apart.
        talk.Send(new TermCommand(s.Id, "run", "definitely-not-a-command-here"));
        var failed = await talk.Until<TermEvent>(t => t.Id == s.Id && t.Entries.Any(e => e.Cmd == "definitely-not-a-command-here" && e.Run.Kind != "running"));
        var bad = failed.Entries.Last(e => e.Cmd == "definitely-not-a-command-here");
        Assert.NotEqual(0, bad.Run.Code);

        // The whole list comes back on a view, and Clear empties it.
        talk.Send(new TermCommand(s.Id));
        Assert.True((await talk.Until<TermEvent>(t => t.Id == s.Id && t.From == 0 && t.Entries.Count >= 2)).Ok);
        talk.Send(new TermCommand(s.Id, "clear"));
        Assert.Empty((await talk.Until<TermEvent>(t => t.Id == s.Id && t.Entries.Count == 0)).Entries);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task A_command_that_never_ends_is_interrupted()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        talk.Send(new TermCommand(s.Id, "run", "sleep 120"));
        await talk.Until<TermEvent>(t => t.Id == s.Id && t.Running);
        talk.Send(new TermCommand(s.Id, "interrupt"));
        var ended = await talk.Until<TermEvent>(t => t.Id == s.Id && !t.Running && t.Entries.Any(e => e.Cmd == "sleep 120"));
        Assert.NotEqual("running", ended.Entries.Last().Run.Kind);
        // And the shell takes the next command.
        talk.Send(new TermCommand(s.Id, "run", "echo after"));
        await talk.Until<TermEvent>(t => t.Id == s.Id && t.Entries.Any(e => e.Lines.Any(l => l.Text.Contains("after")) && e.Run.Kind != "running"));
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task The_shell_refuses_what_is_not_a_command_for_it()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        talk.Send(new TermCommand(s.Id, "run", "   "));
        Assert.False((await talk.Until<TermEvent>(t => t.Id == s.Id && !t.Ok)).Ok);
        talk.Send(new TermCommand(s.Id, "run", new string('x', 20_000)));
        Assert.False((await talk.Until<TermEvent>(t => t.Id == s.Id && !t.Ok)).Ok);
        talk.Send(new TermCommand(9999, "run", "echo no"));
        Assert.False((await talk.Until<TermEvent>(t => t.Id == 9999 && !t.Ok)).Ok);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    // MARK: Saving a file

    [Fact]
    public async Task A_file_is_saved_in_the_folder_and_a_path_outside_it_is_refused()
    {
        using var sandbox = new Sandbox();
        var notes = Path.Combine(sandbox.Project, "notes.txt");
        File.WriteAllText(notes, "one\n");
        var outside = Path.Combine(sandbox.Root, "outside.txt");
        File.WriteAllText(outside, "keep\n");
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);

        talk.Send(new WriteFileCommand(s.Id, "notes.txt", "one\ntwo\n"));
        var ok = await talk.Until<WriteFileEvent>(w => w.Id == s.Id);
        Assert.True(ok.Ok, ok.Error);
        Assert.Equal("one\ntwo\n", File.ReadAllText(notes).Replace("\r\n", "\n"));

        async Task Refused(string path, string text = "x")
        {
            talk.Send(new WriteFileCommand(s.Id, path, text));
            var w = await talk.Until<WriteFileEvent>(e => e.Id == s.Id);
            Assert.False(w.Ok, path);
            Assert.False(string.IsNullOrEmpty(w.Error));
        }
        await Refused("../outside.txt");
        await Refused(outside);
        await Refused("nope.txt");
        await Refused("notes.txt", new string('a', 1024 * 1024 + 1));
        Assert.Equal("keep\n", File.ReadAllText(outside).Replace("\r\n", "\n"));
        Assert.Equal("one\ntwo\n", File.ReadAllText(notes).Replace("\r\n", "\n"));

        // A file deleted while it is open: saving it says it is gone, and does not make it again.
        File.Delete(notes);
        await Refused("notes.txt");
        Assert.False(File.Exists(notes));
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task Open_in_lists_the_editors_and_refuses_what_it_does_not_know()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        talk.Send(new EditorsCommand());
        var editors = await talk.Until<EditorsEvent>();
        Assert.True(editors.Ok);
        output.WriteLine(string.Join(", ", editors.Choices.Select(c => c.Id)));
        talk.Send(new OpenInCommand(s.Id) { Editor = "notepad-that-is-not-listed" });
        var refused = await talk.Until<OpenInEvent>();
        Assert.False(refused.Ok);
        Assert.False(string.IsNullOrEmpty(refused.Error));
        talk.Send(new OpenInCommand(9999));
        Assert.False((await talk.Until<OpenInEvent>()).Ok);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    // MARK: Attachments on a reply

    [Fact]
    public async Task A_reply_carries_a_file_chip_and_a_picture_and_a_chip_that_cannot_be_sent_stops_it()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);

        // A chip for a file that is not there stops the reply and says why; nothing is sent.
        talk.Send(new ReplyCommand("Look at this", s.Id) { Chips = [Chip.FileLive("missing.txt").ToJson()] });
        var toast = await talk.Until<ToastEvent>();
        Assert.Contains("isn’t there any more", toast.Text);
        // A file outside the folder is refused the same way.
        talk.Send(new ReplyCommand("Look at this", s.Id) { Chips = [Chip.FileLive("../../etc/hosts").ToJson()] });
        Assert.Contains("isn’t there any more", (await talk.Until<ToastEvent>()).Text);

        // A good chip and a picture go with the reply and come back on the turn.
        talk.Send(new ReplyCommand("Look at app.js", s.Id) { Chips = [Chip.FileLive("app.js").ToJson()], Images = [OnePixel] });
        var asked = await talk.Until<StateEvent>(st => st.Sessions.Count > 0 && st.Sessions[0].Turns.Count >= 2);
        var turn = asked.Sessions[0].Turns[1];
        Assert.Equal("Look at app.js", turn.Prompt);
        Assert.Single(turn.Chips);
        Assert.Equal("app.js", new Jv(turn.Chips[0])["Source"].Str());
        Assert.Single(turn.Images);
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    [Fact]
    public async Task A_picture_over_eight_megabytes_is_not_kept_and_a_forty_megabyte_one_does_not_break_the_line()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        var s = await Finished(talk, sandbox);
        // Nine megabytes: the reply goes and the picture is left out.
        var big = "data:image/png;base64," + Convert.ToBase64String(new byte[9 * 1024 * 1024]);
        talk.Send(new ReplyCommand("Here is a big one", s.Id) { Images = [big] });
        var asked = await talk.Until<StateEvent>(st => st.Sessions.Count > 0 && st.Sessions[0].Turns.Count >= 2);
        Assert.Empty(asked.Sessions[0].Turns[1].Images);
        // Forty megabytes make a line the backend will not read: it says so and goes on.
        var huge = "data:image/png;base64," + Convert.ToBase64String(new byte[40 * 1024 * 1024]);
        talk.Send(new ReplyCommand("Here is a huge one", s.Id) { Images = [huge] });
        Assert.Contains("too long", (await talk.Until<ToastEvent>()).Text);
        // The backend is still there, and answers.
        talk.Send(new DeskCommand(s.Id, "probe"));
        await talk.Until<DeskEvent>(d => d.What == "probe");
        Assert.Equal(0, await talk.Client.StopAsync());
    }

    // MARK: Kiro Web and Kiro CLI

    [Fact]
    public async Task The_Kiro_lists_say_why_they_are_empty_when_Kiro_is_not_set_up()
    {
        using var sandbox = new Sandbox();
        var talk = await Conversation.Open(sandbox);
        await using var client = talk.Client;
        await talk.GetReady();
        talk.Send(new WebSessionsCommand());
        var web = await talk.Until<WebSessionsEvent>();
        // Where Kiro is not set up (the runner) the answer says so; where it is, it lists what it has.
        Assert.True(web.Ok || !string.IsNullOrEmpty(web.Error));
        talk.Send(new CliSessionsCommand());
        var cli = await talk.Until<CliSessionsEvent>();
        Assert.True(cli.Ok || !string.IsNullOrEmpty(cli.Error));
        // A session that was not listed cannot be adopted.
        talk.Send(new AdoptWebCommand("not-in-the-list"));
        var adopt = await talk.Until<AdoptWebEvent>();
        Assert.False(adopt.Ok);
        talk.Send(new AdoptCliCommand("not-in-the-list"));
        Assert.False((await talk.Until<AdoptCliEvent>()).Ok);
        Assert.Equal(0, await talk.Client.StopAsync());
    }
}
