using System.Diagnostics;
using System.Text.Json;
using Hover.Host.Protocol;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>
/// The controller on a real hover-backend in a data folder of its own: each kind of control is changed, the backend is stopped
/// and started again on the same folder, and the setting is still there. settings.json is written by the backend alone.
/// </summary>
public class BackendTests
{
    static string SettingsText(string dir) => File.ReadAllText(Path.Combine(dir, "settings.json"));

    /// <summary>Go's own tool reads the folder back and would save the same bytes (a Windows check: note.key is wrapped for the user).</summary>
    static void CheckWithGo(string dir)
    {
        if (!OperatingSystem.IsWindows() || TestEnvironment.DataTool is not { } tool) return;
        var p = Process.Start(new ProcessStartInfo(tool, ["check", dir]) { RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false })!;
        var text = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
        p.WaitForExit();
        Assert.True(p.ExitCode == 0, "hover-data check said: " + text);
    }

    [Fact]
    public async Task Every_kind_of_control_changes_a_setting_that_a_restart_keeps()
    {
        using var temp = new TempDir();
        var dir = Path.Combine(temp.Path, "data");
        Directory.CreateDirectory(dir);
        var key = System.Security.Cryptography.RandomNumberGenerator.GetBytes(32);

        await using (var rig = await Rig.StartAsync(dir, key))
        {
            rig.Do(c => c.Open(Section.General));
            // A switch.
            rig.Do(c => c.Toggled("HoverOpens", false));
            // Segments.
            rig.Do(c => c.Picked("WorkspaceSize", 2));
            rig.Do(c => c.Picked("Appearance", 2));
            // A shortcut.
            rig.Do(c => { c.RecordShortcut(); c.Chord(0x4D, Hover.Notch.Shortcut.Control | Hover.Notch.Shortcut.Alt); });
            // The voice page: a switch, segments, a picker, a text field.
            rig.Do(c => c.Open(Section.Voice));
            rig.Do(c => c.Toggled("VoiceEnabled", true));
            rig.Do(c => c.Picked("VoiceMode", 1));
            rig.Do(c => c.Picked("VoiceCountdown", 3));
            rig.Do(c => c.MenuPick("VoiceMicrophone", 2));
            rig.Do(c => c.MenuPick("VoiceAuraColor", 3));
            rig.Do(c => c.Edited("VoiceCleanupModel", "my-model"));
            rig.Do(c => c.Edited("VoiceAuraHex", "#abc"));
            // Kiro: a switch that opens a slider, and the slider.
            rig.Do(c => c.Open(Section.Kiro));
            rig.Do(c => c.Toggled("KiroAutoCompact", true));
            rig.Do(c => c.Picked("KiroCompactAt", 55));
            rig.Do(c => c.Toggled("KiroRetryBusy", true));
            // A tool's access, how long it stays open, and whether the chat lists its steps.
            rig.Do(c => c.Open(Section.Codex));
            rig.Do(c => c.Picked("CodexTools", 2));
            rig.Do(c => c.Picked("CodexIdle", 1));
            rig.Do(c => c.Toggled("CodexShowSteps", false));
            // A quota in the notch, and the Discord status.
            rig.Do(c => c.Open(Section.Integrations));
            rig.Do(c => c.Toggled("NotchItemkiro", true));
            rig.Do(c => c.Toggled("DiscordPresence", true));

            await rig.Until(c => c.Data is { HoverOpens: false, WorkspaceSize: "Large", Appearance: "Dark", KiroRetryBusy: true, DiscordPresence: true }
                && c.Data.Voice is { Enabled: true, Hold: true, Countdown: 10, CleanupModel: "my-model", AuraColor: "#AABBCC" }
                && c.Data.NotchItems.Contains("kiro") && c.Data.KiroAutoCompact && c.Data.KiroCompactAt == 55
                && c.Data.OptionsOf(Tools.Codex) is { IdleMinutes: 15, HideSteps: true, Approval: "Always" }
                && c.Data.Notch.Key == 56, "every setting");
            Assert.Equal("Line In", rig.Read(c => c.Data.Voice.Microphone));
            // Amber is the fourth aura colour.
            Assert.Equal("#AABBCC", rig.Read(c => c.Data.Voice.AuraColor));
        }

        var text = SettingsText(dir);
        Assert.Contains("\"HoverOpensWorkspace\": false", text);
        Assert.Contains("\"WorkspaceSize\": \"Large\"", text);
        Assert.Contains("\"KiroCompactAt\": 55", text);
        CheckWithGo(dir);

        // A new run on the same folder: the pages show what was set.
        await using (var rig = await Rig.StartAsync(dir, key))
        {
            Assert.False(rig.Read(c => c.Data.HoverOpens));
            Assert.Equal("Large", rig.Read(c => c.Data.WorkspaceSize));
            Assert.Equal("Dark", rig.Read(c => c.Data.Appearance));
            Assert.Equal(56, rig.Read(c => c.Data.Notch.Key));
            Assert.Equal("Alt, Control", Hover.Settings.VoiceData.ModifierText(rig.Read(c => c.Data.Notch.Modifiers)));
            Assert.Equal(55, rig.Read(c => c.Data.KiroCompactAt));
            Assert.True(rig.Read(c => c.Data.Voice.Hold));
            Assert.Equal("Line In", rig.Read(c => c.Data.Voice.Microphone));
            rig.Do(c => c.Open(Section.General));
            var blocks = rig.Read(c => c.Blocks);
            var rows = blocks.Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).ToList();
            Assert.False(rows.First(r => r.Control.Id == "HoverOpens").Control.On);
            Assert.Equal(2, rows.First(r => r.Control.Id == "WorkspaceSize").Control.Picked);
            Assert.Equal(2, rows.First(r => r.Control.Id == "Appearance").Control.Picked);
            Assert.Equal("Ctrl+Alt+M", rows.First(r => r.Control.Id == "WorkspaceShortcut").Control.Text);
            rig.Do(c => c.Open(Section.Codex));
            var codex = rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).ToList();
            Assert.Equal(2, codex.First(r => r.Control.Id == "CodexTools").Control.Picked);
            Assert.Equal(1, codex.First(r => r.Control.Id == "CodexIdle").Control.Picked);
            Assert.False(codex.First(r => r.Control.Id == "CodexShowSteps").Control.On);
        }
    }

    [Fact]
    public async Task Two_settings_changed_within_400_ms_are_both_kept()
    {
        using var temp = new TempDir();
        var key = System.Security.Cryptography.RandomNumberGenerator.GetBytes(32);
        await using (var rig = await Rig.StartAsync(temp.Path, key))
        {
            rig.Do(c => c.Picked("WorkspaceSize", 0));
            await Task.Delay(100);
            rig.Do(c => c.Toggled("HoverOpens", false));
            await Task.Delay(100);
            rig.Do(c => c.Picked("Appearance", 1));
            await rig.Until(c => c.Data is { WorkspaceSize: "Small", HoverOpens: false, Appearance: "Light" }, "all three");
        }
        var text = SettingsText(temp.Path);
        Assert.Contains("\"WorkspaceSize\": \"Small\"", text);
        Assert.Contains("\"HoverOpensWorkspace\": false", text);
        Assert.Contains("\"Appearance\": \"Light\"", text);
    }

    [Fact]
    public async Task A_key_saved_from_the_voice_page_is_sealed_in_secrets_dat_and_never_in_a_message_or_the_log()
    {
        using var temp = new TempDir();
        var key = System.Security.Cryptography.RandomNumberGenerator.GetBytes(32);
        var groq = "gsk_" + new string('Z', 1996);
        Assert.Equal(2000, groq.Length);
        await using (var rig = await Rig.StartAsync(temp.Path, key))
        {
            rig.Do(c => c.Open(Section.Voice));
            Assert.False(rig.Read(c => c.HasSecret("voice.groq")));
            rig.Do(c => c.Edited("VoiceGroqKey", groq));
            await rig.Until(c => c.HasSecret("voice.groq"), "the key kept");
            // The page now says a key is kept, and still holds no value.
            var field = rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).First(r => r.Control.Id == "VoiceGroqKey").Control;
            Assert.True(field.On);
            Assert.Equal("", field.Text);
            Assert.Equal("Saved", field.Placeholder);
            // A cleanup key is a different one.
            Assert.False(rig.Read(c => c.HasSecret("cleanup.gemini")));
            // Nothing the host was told, said or logged holds the key.
            foreach (var e in rig.Seen) Assert.DoesNotContain("ZZZZZZZZ", e.ToString());
            foreach (var line in rig.Env.Lines().Where(l => !l.Contains("\"secret\"") || !l.Contains("\"set\""))) Assert.DoesNotContain("ZZZZZZZZ", line);
            Assert.DoesNotContain("ZZZZZZZZ", rig.Read(c => c.Note?.Text ?? ""));
        }
        var sealedFile = Path.Combine(temp.Path, "secrets.dat");
        Assert.True(File.Exists(sealedFile));
        Assert.DoesNotContain("ZZZZZZZZ", File.ReadAllText(sealedFile, System.Text.Encoding.Latin1));
        foreach (var f in Directory.EnumerateFiles(temp.Path, "*", SearchOption.AllDirectories).Where(f => !f.EndsWith("secrets.dat")))
            Assert.DoesNotContain("ZZZZZZZZ", File.ReadAllText(f, System.Text.Encoding.Latin1));

        // The Go tool agrees the key is kept, and is that one (Windows: note.key is wrapped for the user).
        if (OperatingSystem.IsWindows() && TestEnvironment.DataTool is { } tool)
        {
            var p = Process.Start(new ProcessStartInfo(tool, ["secret", "check", temp.Path, "voice.groq"]) { RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false })!;
            p.StandardInput.WriteLine(groq);
            p.StandardInput.Close();
            var said = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
            p.WaitForExit();
            Assert.True(p.ExitCode == 0, "hover-data secret check said: " + said);
            Assert.DoesNotContain("ZZZZZZZZ", said);
        }

        // A new run still has it; Remove key takes it away.
        await using (var rig = await Rig.StartAsync(temp.Path, key))
        {
            await rig.Until(c => c.HasSecret("voice.groq"), "the key kept after a restart");
            rig.Do(c => c.Edited("VoiceGroqKey", ""));
            await rig.Until(c => !c.HasSecret("voice.groq"), "the key removed");
        }
    }

    [Fact]
    public async Task A_key_with_a_control_character_is_refused_and_says_so_under_the_field()
    {
        using var temp = new TempDir();
        await using var rig = await Rig.StartAsync(temp.Path);
        rig.Do(c => c.Open(Section.Voice));
        rig.Do(c => c.Edited("VoiceGroqKey", "line one\nline two"));
        await rig.Until(c => c.Note is { Id: "VoiceGroqKey" }, "the note");
        Assert.DoesNotContain("line one", rig.Read(c => c.Note!.Value.Text));
        Assert.False(rig.Read(c => c.HasSecret("voice.groq")));
    }

    [Fact]
    public async Task A_project_is_added_edited_made_the_place_tasks_go_and_removed()
    {
        using var temp = new TempDir();
        var project = Directory.CreateDirectory(Path.Combine(temp.Path, "site")).FullName;
        var workspace = Directory.CreateDirectory(Path.Combine(temp.Path, "work")).FullName;
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"));
        rig.Do(c => c.Open(Section.Projects));

        rig.Env.NextFolder = project;
        rig.Do(c => c.Pressed("ProjectAdd"));
        await rig.Until(c => c.Data.Projects.Count == 1, "the project");
        var added = rig.Read(c => c.Data.Projects[0]);
        Assert.Equal("site", added.Name);
        Assert.Equal("risky", added.Access);
        Assert.True(added.Voice);

        // Its own page: a name to say, other names, voice off, full access.
        rig.Do(c => c.Pressed("Project." + added.Id));
        Assert.Equal(added.Id, rig.Read(c => c.Project));
        rig.Do(c => c.Edited("ProjectName", "The site"));
        rig.Do(c => c.Edited("ProjectAliases", "website, the web site , "));
        rig.Do(c => c.Toggled("ProjectVoice", false));
        rig.Do(c => c.Picked("ProjectAccess", 3));
        await rig.Until(c => c.Data.Projects is [{ Name: "The site", Voice: false, Access: "read" }] && c.Data.Projects[0].Aliases.SequenceEqual(new[] { "website", "the web site" }), "the edits");

        // The same folder cannot be added twice: the backend's words go under the link.
        rig.Env.NextFolder = project;
        rig.Do(c => c.Pressed("ProjectBack"));
        rig.Do(c => c.Pressed("ProjectAdd"));
        await rig.Until(c => c.Note is { Id: "ProjectAdd" }, "the refusal");
        Assert.Single(rig.Read(c => c.Data.Projects));

        // The default workspace: a folder, and the access it gives.
        rig.Env.NextFolder = workspace;
        rig.Do(c => c.Pressed("DefaultFolder"));
        rig.Do(c => c.Picked("DefaultAccess", 2));
        await rig.Until(c => c.Data.Workspace is { Access: "always" } w && w.Folder != null && Path.GetFullPath(w.Folder) == Path.GetFullPath(workspace), "the default workspace");

        // Remove takes it off the list and leaves the folder.
        rig.Do(c => c.Pressed("Project." + added.Id));
        rig.Do(c => c.Pressed("ProjectRemove"));
        await rig.Until(c => c.Data.Projects.Count == 0, "the project removed");
        Assert.True(Directory.Exists(project));
        Assert.Equal("", rig.Read(c => c.Project));
    }

    [Fact]
    public async Task A_project_whose_folder_has_gone_says_so_and_a_missing_default_workspace_is_made_when_needed()
    {
        using var temp = new TempDir();
        var gone = Directory.CreateDirectory(Path.Combine(temp.Path, "gone")).FullName;
        await using (var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data")))
        {
            rig.Env.NextFolder = gone;
            rig.Do(c => c.Open(Section.Projects));
            rig.Do(c => c.Pressed("ProjectAdd"));
            await rig.Until(c => c.Data.Projects.Count == 1, "the project");
            Directory.Delete(gone);
            rig.Do(c => c.Open(Section.Projects));
            var row = rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).First(r => r.Control.Open.StartsWith("Project."));
            Assert.Contains("isn’t there any more", row.Sub);
            // The page still opens, with the problem where the folder would be.
            rig.Do(c => c.Pressed(row.Control.Open));
            var folder = rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).First(r => r.Label == "Folder");
            Assert.Contains("isn’t there any more", folder.Sub);
        }
    }

    static string Extension(string home, string name, string themeJson)
    {
        var ext = Directory.CreateDirectory(Path.Combine(home, ".vscode", "extensions", name + "-1.0.0")).FullName;
        Directory.CreateDirectory(Path.Combine(ext, "themes"));
        File.WriteAllText(Path.Combine(ext, "package.json"), $$"""{ "name": "{{name}}", "contributes": { "themes": [ { "label": "{{name}} theme", "uiTheme": "vs-dark", "path": "./themes/t.json" } ] } }""");
        File.WriteAllText(Path.Combine(ext, "themes", "t.json"), themeJson);
        return ext;
    }

    [Fact]
    public async Task Installed_themes_are_listed_and_one_is_applied_as_Hover_colours_and_Appearance_puts_them_back()
    {
        using var temp = new TempDir();
        var home = Directory.CreateDirectory(Path.Combine(temp.Path, "home")).FullName;
        Extension(home, "acme-night", """{ "name": "Acme Night", "type": "dark", "colors": { "editor.background": "#102030", "editor.foreground": "#E0E0E0", "sideBar.background": "#0A1520", "button.background": "#2B7BFF" } }""");
        // A theme file that cannot be read is left out of the list, not an error.
        Extension(home, "broken", "{ this is not json");
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"), home: home);
        rig.Do(c => c.Open(Section.General));
        await rig.Until(c => c.Installed.Any(t => t.Label == "acme-night theme"), "the theme listed");
        Assert.DoesNotContain(rig.Read(c => c.Installed), t => t.Label.StartsWith("broken"));
        var tile = rig.Read(c => c.Blocks).First(b => b.Kind == BlockKind.Tiles).Tiles.First(t => t.From == "VS Code" && t.Name.Contains("cme"));
        Assert.False(tile.Picked);
        Assert.Equal(0xFF102030u, tile.Palette.Surface);

        rig.Do(c => c.PickTile(tile.Id));
        await rig.Until(c => c.Data.Theme is { Name: "acme-night theme", Dark: true }, "the theme set");
        await rig.Until(c => c.Palette.Surface == 0xFF102030u, "the palette");
        Assert.True(rig.Read(c => c.Palette.Dark));
        Assert.Equal(0xFF0A1520u, rig.Read(c => c.Palette.Panel));
        Assert.Equal(0xFF2B7BFFu, rig.Read(c => c.Palette.Blue));
        Assert.True(rig.Read(c => c.Blocks).First(b => b.Kind == BlockKind.Tiles).Tiles.Any(t => t.Picked && t.Name.Contains("cme")));
        // An appearance puts Hover's own colours back (the theme goes first).
        rig.Do(c => c.Picked("Appearance", 1));
        await rig.Until(c => c.Data.Theme is null && c.Data.Appearance == "Light", "Hover's colours");
        Assert.Equal(Palettes.HoverLight.Surface, rig.Read(c => c.Palette.Surface));
    }

    [Fact]
    public async Task A_vscode_theme_file_with_comments_is_imported_and_a_broken_one_says_so()
    {
        using var temp = new TempDir();
        var good = Path.Combine(temp.Path, "good.jsonc");
        File.WriteAllText(good, """
        {
          // A comment, as VS Code themes have.
          "name": "Imported Day",
          "type": "light",
          "colors": { "editor.background": "#FAFAFA", "editor.foreground": "#222222", /* trailing */ },
        }
        """);
        var broken = Path.Combine(temp.Path, "broken.json");
        File.WriteAllText(broken, "this is { not a theme");
        var empty = Path.Combine(temp.Path, "empty.json");
        File.WriteAllText(empty, """{ "name": "No colours" }""");
        var missing = Path.Combine(temp.Path, "nothing.json");

        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"));
        rig.Do(c => c.Open(Section.General));
        foreach (var bad in new[] { broken, empty, missing })
        {
            rig.Env.NextThemeFile = bad;
            rig.Do(c => c.Pressed("ImportTheme"));
            await rig.Until(c => c.ImportStatus != "", "the status for " + Path.GetFileName(bad));
            Assert.Equal("That file has no VS Code theme colours in it.", rig.Read(c => c.ImportStatus));
            Assert.Null(rig.Read(c => c.Data.Theme));
            // The status sits beside the link.
            Assert.Contains(rig.Read(c => c.Blocks), b => b is { Kind: BlockKind.Link, Id: "ImportTheme" } && b.Status != "");
            rig.Do(c => c.Open(Section.General));
        }
        rig.Env.NextThemeFile = good;
        rig.Do(c => c.Pressed("ImportTheme"));
        await rig.Until(c => c.Data.Theme is { Name: "Imported Day", Dark: false }, "the theme");
        Assert.Equal("", rig.Read(c => c.ImportStatus));
        await rig.Until(c => c.Palette.Surface == 0xFFFAFAFAu, "the palette");
        // It is not in the list of installed themes, but shows as imported while in use.
        Assert.Contains(rig.Read(c => c.Blocks).First(b => b.Kind == BlockKind.Tiles).Tiles, t => t.Name == "Imported Day" && t.From == "Imported" && t.Picked);
    }

    [Fact]
    public async Task Show_the_note_again_puts_the_note_about_tool_access_back()
    {
        using var temp = new TempDir();
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"));
        rig.Env.Send(new SaveSettingsCommand { NoticeSeen = true });
        rig.Env.Send(new SettingsCommand());
        await rig.Until(c => c.Data.KiroNoticeSeen, "the note seen");
        rig.Do(c => c.Open(Section.Kiro));
        Assert.True(rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows).First(r => r.Control.Id == "KiroNoticeAgain").Control.Enabled);
        rig.Do(c => c.Pressed("KiroNoticeAgain"));
        await rig.Until(c => !c.Data.KiroNoticeSeen, "the note back");
    }

    [Fact]
    public async Task A_read_only_settings_file_does_not_stop_the_pages()
    {
        using var temp = new TempDir();
        var dir = Path.Combine(temp.Path, "data");
        Directory.CreateDirectory(dir);
        var file = Path.Combine(dir, "settings.json");
        await using (var first = await Rig.StartAsync(dir))
        {
            first.Do(c => c.Picked("WorkspaceSize", 2));
            await first.Until(c => c.Data.WorkspaceSize == "Large", "large");
        }
        File.SetAttributes(file, FileAttributes.ReadOnly);
        try
        {
            await using var rig = await Rig.StartAsync(dir, new byte[32]);
            // The page reads what is there; a change is held in the backend's model, and the app goes on.
            Assert.Equal("Large", rig.Read(c => c.Data.WorkspaceSize));
            rig.Do(c => c.Toggled("HoverOpens", false));
            await rig.Until(c => !c.Data.HoverOpens, "the change shown");
            rig.Do(c => c.Open(Section.Voice));
            Assert.NotEmpty(rig.Read(c => c.Blocks));
        }
        finally { File.SetAttributes(file, FileAttributes.Normal); }
    }

    [Fact]
    public async Task The_credits_and_the_mcp_list_and_the_kiro_agents_arrive_for_the_Kiro_page()
    {
        using var temp = new TempDir();
        var home = Directory.CreateDirectory(Path.Combine(temp.Path, "home")).FullName;
        var kiro = Directory.CreateDirectory(Path.Combine(home, ".kiro", "settings")).FullName;
        File.WriteAllText(Path.Combine(kiro, "mcp.json"), """{ "mcpServers": { "files": { "command": "npx", "args": ["-y", "files-mcp"], "env": { "TOKEN": "abc" } }, "web": { "url": "https://web.example/mcp", "disabled": true } } }""");
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"), home: home);
        rig.Do(c => c.Open(Section.Kiro));
        await rig.Until(c => c.Mcp.Servers.Count == 2, "the servers");
        Assert.Equal("MCP servers · 1 of 2 on", rig.Read(c => c.Mcp.Title));
        // The list holds names of variables, never their values.
        var files = rig.Read(c => c.Mcp.Servers.First(s => s.Name == "files"));
        Assert.Equal("TOKEN", files.Pairs[0].Name);
        Assert.Null(files.Pairs[0].Value);
        Assert.True(rig.Read(c => c.Mcp.HasFile));

        // Switching one off writes the file; editing opens the form with the values.
        rig.Do(c => c.Toggled("Mcp:files", false));
        await rig.Until(c => c.Mcp.Servers.First(s => s.Name == "files").Disabled, "the server off");
        Assert.Contains("\"disabled\": true", File.ReadAllText(Path.Combine(kiro, "mcp.json")));
        rig.Do(c => c.McpAct("McpEdit", "files"));
        await rig.Until(c => c.Mcp.Form is { Editing: "files" }, "the form");
        Assert.Equal("TOKEN", rig.Read(c => c.Mcp.Form!.Pairs[0].Key));
        Assert.Equal("abc", rig.Read(c => c.Mcp.Form!.Pairs[0].Value));
        Assert.Equal("-y\nfiles-mcp", rig.Read(c => c.Mcp.Form!.Args));

        // A form the backend refuses keeps its place and says what is wrong.
        rig.Do(c => { c.McpAct("McpType", "command\u001f"); c.McpAct("McpSave", ""); });
        await rig.Until(c => c.Mcp.Form is { CommandProblem: not null }, "the problem");
        rig.Do(c => c.McpAct("McpCancel", ""));
        rig.Do(c => c.McpAct("McpRemoveYes", "web"));
        await rig.Until(c => c.Mcp.Servers.Count == 1, "the server removed");

        await rig.Until(c => rig.Seen.OfType<CreditsEvent>().Any(), "the credits");
    }

    [Fact]
    public async Task Phonon_and_the_Groq_check_answer_for_the_voice_page()
    {
        using var temp = new TempDir();
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"));
        rig.Do(c => c.Open(Section.Voice));
        rig.Do(c => c.Picked("VoiceSpeech", 0));
        await rig.Until(c => c.Live.Phonon is not null, "the Phonon card");
        Assert.Equal("Not installed", rig.Read(c => c.Live.Phonon!.State));
        Assert.Equal([PhononAction.Download], rig.Read(c => c.Live.Phonon!.Actions.ToArray()));
        Assert.Contains(rig.Read(c => c.Blocks).Where(b => b.Kind == BlockKind.Group).SelectMany(b => b.Rows), r => r.Label == "Phonon-2" || r.Label.StartsWith("Phonon"));

        // With no key, the check says so and nothing is sent anywhere.
        rig.Do(c => c.Picked("VoiceSpeech", 1));
        rig.Do(c => c.Pressed("groq.check"));
        Assert.Equal("Checking…", rig.Read(c => c.Live.GroqCheck));
        await rig.Until(c => c.Live.GroqCheck != "Checking…", "the answer");
        Assert.Equal("Add your Groq key first.", rig.Read(c => c.Live.GroqCheck));
    }

    [Fact]
    public async Task A_launch_at_login_switch_is_the_apps_not_the_backends()
    {
        using var temp = new TempDir();
        await using var rig = await Rig.StartAsync(Path.Combine(temp.Path, "data"));
        rig.Do(c => c.Toggled("LaunchAtLogin", true));
        Assert.True(rig.Env.Login);
        rig.Do(c => c.Toggled("LaunchAtLogin", false));
        Assert.False(rig.Env.Login);
        Assert.DoesNotContain(rig.Env.Lines(), l => l.Contains("LaunchAtLogin", StringComparison.OrdinalIgnoreCase));
        Assert.DoesNotContain("LaunchAtLogin", SettingsOrEmpty(temp.Path));
    }

    static string SettingsOrEmpty(string root)
    {
        var f = Path.Combine(root, "data", "settings.json");
        return File.Exists(f) ? File.ReadAllText(f) : "";
    }

    [Fact]
    public void The_voice_settings_go_to_the_backend_as_a_whole_object_it_checks()
    {
        var voice = new VoiceData { Enabled = true, Speech = "Local", Microphone = null, CleanupProvider = "Custom", CleanupBase = "https://x.example/v1", Agent = "codex", Countdown = 3 };
        var json = voice.ToJson();
        Assert.Equal("Local", json["Speech"]!.GetValue<string>());
        Assert.Equal("codex", json["Agent"]!.GetValue<string>());
        Assert.Equal(18, json["Shortcut"]!["Key"]!.GetValue<int>());
        Assert.Equal("Control, Alt".Replace("Control, Alt", "Alt, Control"), json["Shortcut"]!["Modifiers"]!.GetValue<string>());
        Assert.Null(json["Microphone"]);
        using var doc = JsonDocument.Parse(json.ToJsonString());
        Assert.Equal(JsonValueKind.Null, doc.RootElement.GetProperty("Microphone").ValueKind);
    }
}
