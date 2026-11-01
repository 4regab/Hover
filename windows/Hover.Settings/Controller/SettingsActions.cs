using System.Text.Json.Nodes;
using Hover.Host.Protocol;

namespace Hover.Settings;

public sealed partial class SettingsController
{
    // MARK: Helpers

    void Voice(Func<VoiceData, VoiceData> change)
    {
        var next = change(data.Voice);
        // The page shows the pick at once; the backend's answer is the model.
        Hold("voice", d => d with { Voice = next }, d => d.Voice == next);
        env.Send(new SetSettingCommand("Voice", next.ToJson()));
        Dirty();
    }

    void Save(SaveSettingsCommand command)
    {
        env.Send(command);
        // The answer to a save is the preferences; the model that pages read comes from `settings`.
        env.Send(new SettingsCommand());
    }

    void Say(string id, string text)
    {
        Note = (id, text);
        Dirty();
    }

    void EditProject(string controlId, Func<ProjectData, ProjectData> change)
    {
        if (Project == "" || data.Project(Project) is not { } p) return;
        lastProjectControl = controlId;
        var next = change(p);
        // Held at once, so a second edit before the answer builds on the first.
        Hold("project:" + p.Id, d => d with { Projects = d.Projects.Select(x => x.Id == p.Id ? next : x).ToList() }, d => next.SameAs(d.Project(p.Id)));
        env.Send(new ProjectUpdateCommand(next.ToJson()));
    }

    static ToolDef? ToolOfPrefix(string id, string suffix)
        => id.EndsWith(suffix, StringComparison.Ordinal) ? Tools.All.FirstOrDefault(t => t.Name == id[..^suffix.Length]) : null;

    // MARK: A switch

    /// <summary>A switch's click.</summary>
    public void Toggled(string id, bool on)
    {
        Note = null;
        switch (id)
        {
            case "LaunchAtLogin":
                env.SetLaunchAtLogin(on);
                break;
            case "HoverOpens":
                Save(new SaveSettingsCommand { Hover = on });
                break;
            case "VoiceEnabled":
                Voice(v => v with { Enabled = on });
                return;
            case "VoiceCleanup":
                Voice(v => v with { Cleanup = on });
                return;
            case "ProjectVoice":
                EditProject(id, p => p with { Voice = on });
                break;
            case "DiscordPresence":
                Save(new SaveSettingsCommand { DiscordPresence = on });
                break;
            case "KiroAutoCompact":
                Save(new SaveSettingsCommand { KiroAutoCompact = on });
                break;
            case "KiroRetryBusy":
                env.Send(new SetSettingCommand("KiroRetryBusy", JsonValue.Create(on)));
                break;
            // Computer use, the sandbox, the agent browser and agent desktops are not on Windows: their switches are off.
            case "ComputerUse" or "Sandbox" or "AgentBrowser" or "AgentSpaces":
                break;
            default:
                if (id.StartsWith("NotchItem", StringComparison.Ordinal))
                {
                    var item = id["NotchItem".Length..];
                    var set = data.NotchItems.ToList();
                    if (on && !set.Contains(item) && Array.IndexOf(PageBuilder.QuotaIds, item) >= 0) set.Add(item);
                    else if (!on) set.RemoveAll(x => x == item);
                    Hold("notch-items", d => d with { NotchItems = set }, d => d.NotchItems.SequenceEqual(set));
                    Save(new SaveSettingsCommand { QuotaItems = [.. set] });
                }
                else if (ToolOfPrefix(id, "ShowSteps") is { } tool)
                    Save(new SaveSettingsCommand { Tools = [new ToolSetting(tool.Id, HideSteps: !on)] });
                else if (id.StartsWith("Mcp:", StringComparison.Ordinal))
                {
                    Live.Mcp.Notice = null;
                    env.Send(new McpCommand("enable") { Name = id["Mcp:".Length..], Enabled = on });
                }
                break;
        }
        Dirty();
    }

    // MARK: A segment picked, or a slider's number

    /// <summary>A segment picked (its index), or a slider's new number (the percent).</summary>
    public void Picked(string id, int index)
    {
        Note = null;
        switch (id)
        {
            case "WorkspaceSize":
                if (index >= 0 && index < PageBuilder.Sizes.Length) env.Send(new SetSettingCommand("WorkspaceSize", PageBuilder.Sizes[index].Name));
                break;
            case "Appearance":
                if (index >= 0 && index < PageBuilder.Appearances.Length)
                {
                    // Picking an appearance puts Hover's own colours back: the theme goes first.
                    env.Send(new SetSettingCommand("Theme", null));
                    env.Send(new SetSettingCommand("Appearance", PageBuilder.Appearances[index].Name));
                    var appearance = PageBuilder.Appearances[index].Name;
                    Hold("appearance", d => d with { Theme = null, Appearance = appearance }, d => d.Theme is null && d.Appearance == appearance);
                    Apply();
                }
                break;
            case "VoiceSpeech":
                if (index is 0 or 1) Voice(v => v with { Speech = PageBuilder.SpeechModes[index] });
                return;
            case "VoiceMode":
                Voice(v => v with { Hold = index == 1 });
                return;
            case "VoiceCleanupProvider":
                if (index >= 0 && index < PageBuilder.CleanupProviders.Length) Voice(v => v with { CleanupProvider = PageBuilder.CleanupProviders[index] });
                return;
            case "VoiceCountdown":
                if (index >= 0 && index < PageBuilder.VoiceCountdowns.Length) Voice(v => v with { Countdown = PageBuilder.VoiceCountdowns[index] });
                return;
            case "DefaultAccess":
                if (index >= 0 && index < Access.Ids.Length)
                {
                    lastDefaultControl = id;
                    env.Send(new DefaultWorkspaceCommand(null, Access.Ids[index]));
                }
                break;
            case "ProjectAccess":
                if (index >= 0 && index < Access.Ids.Length) EditProject(id, p => p with { Access = Access.Ids[index] });
                break;
            case "KiroCompactAt":
                Save(new SaveSettingsCommand { KiroCompactAt = Math.Clamp(index, SettingsData.CompactMin, 100) });
                break;
            case "KiroCreditsRange":
                Live.CreditsRange = index;
                break;
            default:
                PickedTool(id, index);
                break;
        }
        Dirty();
    }

    void PickedTool(string id, int index)
    {
        if (ToolOfPrefix(id, "Effort") is { } e)
        {
            var o = data.OptionsOf(e);
            var levels = PageBuilder.EffortLevels(e, o, data.OffersOf(e));
            if (index >= 0 && index < levels.Count) SetModel(e, o.Model, levels[index]);
        }
        else if (ToolOfPrefix(id, "Tools") is { } t)
        {
            // Read only keeps the asking it had; the rest are full access.
            var access = index switch { 1 => "risky", 2 => "always", 3 => "read", _ => "full" };
            Save(new SaveSettingsCommand { Tools = [new ToolSetting(t.Id, Access: access)] });
        }
        else if (ToolOfPrefix(id, "Idle") is { } m && index >= 0 && index < PageBuilder.IdleChoices.Length)
            Save(new SaveSettingsCommand { Tools = [new ToolSetting(m.Id, Idle: PageBuilder.IdleChoices[index])] });
    }

    // MARK: A picker's menu

    /// <summary>The picker for a row was clicked: its menu opens at (x, y), the button's bottom left.</summary>
    public void OpenPicker(string id, float x, float y)
    {
        foreach (var b in Blocks)
        {
            if (b.Kind != BlockKind.Group) continue;
            foreach (var r in b.Rows)
                if (r.Control is { Kind: ControlKind.Picker } c && c.Id == id)
                {
                    Menu = new OpenMenu(id, c.Options, x, y);
                    Changed?.Invoke();
                    return;
                }
        }
    }

    public void CloseMenu()
    {
        if (Menu is null) return;
        Menu = null;
        Changed?.Invoke();
    }

    /// <summary>An option picked in a picker's menu.</summary>
    public void MenuPick(string id, int index)
    {
        Menu = null;
        switch (id)
        {
            // 0 is the system default; past the devices is a saved one not plugged in now.
            case "VoiceMicrophone":
                if (index == 0) Voice(v => v with { Microphone = null });
                else if (index - 1 < Live.Mics.Count) Voice(v => v with { Microphone = Live.Mics[index - 1] });
                return;
            case "VoiceModel":
                Voice(v => v with { Model = PageBuilder.TranscribeModels[Math.Clamp(index, 0, PageBuilder.TranscribeModels.Length - 1)].Id });
                return;
            case "VoiceAuraColor":
                if (index >= 0 && index < PageBuilder.AuraColors.Length) Voice(v => v with { AuraColor = PageBuilder.AuraColors[index].Hex });
                return;
            // Voice's own default agent from then on; the new-task box keeps its own.
            case "VoiceAgentTool":
                if (index >= 0 && index < Tools.All.Count) Voice(v => v with { Agent = Tools.All[index].Id });
                return;
            case "OpenCodeAgent":
                {
                    var modes = PageBuilder.OpenCodeAgents(data.OffersOf(Tools.OpenCode));
                    SetToolAgent(Tools.OpenCode, index == 0 || index - 1 >= modes.Count ? null : modes[index - 1].Value);
                    break;
                }
            case "KiroAgent":
                {
                    var modes = PageBuilder.KiroModes(data.OffersOf(Tools.Kiro), kiroAgents).Where(m => m.Value != "vibe").ToList();
                    SetToolAgent(Tools.Kiro, index == 0 || index - 1 >= modes.Count ? null : modes[index - 1].Value);
                    break;
                }
            default:
                if (ToolOfPrefix(id, "Model") is { } t)
                {
                    var models = PageBuilder.Models(t, data.OffersOf(t));
                    var value = models[Math.Min(Math.Max(index, 0), models.Count - 1)].Value;
                    // The first is the tool's own choice: none is saved, as the Go pick does.
                    var o = data.OptionsOf(t);
                    SetModel(t, value == models[0].Value || value == "" ? null : value, o.Effort);
                }
                break;
        }
        Dirty();
    }

    /// <summary>A tool's model and effort. The backend answers with the office's state, so the settings are read again for the page.</summary>
    void SetModel(ToolDef tool, string? model, string? effort)
    {
        env.Send(new SetModelCommand(tool.Id, model, effort));
        env.Send(new SettingsCommand());
    }

    void SetToolAgent(ToolDef tool, string? agent)
        => env.Send(new SetSettingCommand("ToolAgent", new JsonObject { ["tool"] = tool.Id, ["agent"] = agent }));

    // MARK: A theme tile

    public void PickTile(string id)
    {
        if (id == "ThemeHover")
        {
            ApplyTheme(null);
            return;
        }
        foreach (var p in installed)
            if ("Theme" + p.Label == id)
            {
                ApplyTheme(p.Theme);
                return;
            }
    }

    // MARK: A button

    /// <summary>A button's click. A text box's commit is <see cref="Edited"/>.</summary>
    public void Pressed(string id)
    {
        Note = null;
        switch (id)
        {
            case "Quit":
                env.Quit();
                return;
            case "RefreshQuotas":
                env.Send(new RefreshCommand());
                break;
            case "ImportTheme":
                env.ChooseThemeFile(file =>
                {
                    if (file is null) return;
                    importPath = file;
                    env.Send(new ThemeReadCommand(file));
                });
                break;
            case "SettingsKiroFolder":
                env.ChooseFolder(f =>
                {
                    if (f is not null) env.Send(new SetSettingCommand("KiroFolder", f));
                });
                break;
            case "KiroNoticeAgain":
                // saveSettings only ever sets it, so the way back is setSetting.
                env.Send(new SetSettingCommand("KiroNoticeSeen", JsonValue.Create(false)));
                break;
            case "ProjectAdd":
                env.ChooseFolder(f =>
                {
                    if (f is not null) env.Send(new ProjectsCommand("add") { Folder = f });
                });
                break;
            case "ProjectBack":
                Project = "";
                break;
            case "ProjectFolder":
                env.ChooseFolder(f =>
                {
                    if (f is not null) EditProject(id, p => p with { Folder = f });
                });
                break;
            // Only the entry goes: the folder, its sessions and any run are left alone.
            case "ProjectRemove":
                if (Project != "") env.Send(new ProjectsCommand("remove") { Id = Project });
                Project = "";
                break;
            case "DefaultFolder":
                env.ChooseFolder(f =>
                {
                    if (f is null) return;
                    lastDefaultControl = id;
                    env.Send(new DefaultWorkspaceCommand(f));
                });
                break;
            case "VoiceShortcut":
                RecordAs(true);
                return;
            case "VoiceAgent":
                Section = Sections.Of(Tools.ById(data.Voice.Agent) ?? Tools.ByName(data.AgentTool));
                Project = "";
                OnOpened();
                break;
            case "VoiceWorkspace":
                Section = Section.Projects;
                Project = "";
                break;
            case "groq.check":
                Live.GroqCheck = "Checking…";
                env.Send(VoiceCommandsFor.GroqCheck);
                break;
            case "voice.try.press":
                env.TrialPress();
                break;
            case "voice.try.release":
                env.TrialRelease();
                break;
            case "phonon.download" or "phonon.retry":
                phononError = null;
                env.Send(VoiceCommandsFor.PhononOp("download"));
                break;
            case "phonon.cancel":
                env.Send(VoiceCommandsFor.PhononOp("cancel"));
                break;
            case "phonon.repair":
                phononError = null;
                env.Send(VoiceCommandsFor.PhononOp("repair"));
                break;
            case "phonon.remove":
                phononError = null;
                env.Send(VoiceCommandsFor.PhononOp("remove"));
                break;
            default:
                if (id.StartsWith("Project.", StringComparison.Ordinal))
                    Project = id["Project.".Length..];
                else if (id.EndsWith("Recheck", StringComparison.Ordinal))
                    env.Send(new RefreshCommand());
                // integ.* are Cua Driver's and the tools' one-click setup: not on Windows.
                break;
        }
        Dirty();
    }

    // MARK: A text box

    /// <summary>A text box's new value (Enter or focus out). A key's box sends an empty value only from Remove key.</summary>
    public void Edited(string id, string value)
    {
        Note = null;
        string? Text() => value.Trim() is { Length: > 0 } t ? t : null;
        switch (id)
        {
            case "ProjectName":
                EditProject(id, p => p with { Name = value });
                break;
            case "ProjectAliases":
                EditProject(id, p => p with { Aliases = value.Split(',').Select(a => a.Trim()).Where(a => a.Length > 0).ToList() });
                break;
            case "VoiceCleanupModel":
                Voice(v => v with { CleanupModel = Text() });
                return;
            case "VoiceCleanupBase":
                {
                    var b = Text();
                    if (b is not null && !(b.StartsWith("https://", StringComparison.Ordinal) || b.StartsWith("http://", StringComparison.Ordinal)))
                    {
                        Say(id, "Use the full address, starting with https://.");
                        return;
                    }
                    b = b?.TrimEnd('/');
                    Voice(v => v with { CleanupBase = b });
                    return;
                }
            // Empty goes back to the default colour.
            case "VoiceAuraHex":
                {
                    var t = Text();
                    if (t is null)
                    {
                        Voice(v => v with { AuraColor = null });
                        return;
                    }
                    var hex = Hex(t);
                    if (hex is null)
                    {
                        Say(id, "Use a hex colour, like #1FD5F9.");
                        return;
                    }
                    Voice(v => v with { AuraColor = hex });
                    return;
                }
            case "VoiceGroqKey" or "VoiceCleanupKey":
                {
                    var name = id == "VoiceGroqKey" ? PageBuilder.GroqSecret : PageBuilder.CleanupSecret(data.Voice.CleanupProvider);
                    // The value goes to the backend and nowhere else: not into a note, a log or a message of ours.
                    env.Send(new SecretCommand("set", name) { Value = value });
                    // The last check's answer was about the old key.
                    if (id == "VoiceGroqKey") Live.GroqCheck = null;
                    break;
                }
        }
        Dirty();
    }

    /// <summary>#RRGGBB from "#rgb", "rrggbb" and the like (upper case), or null.</summary>
    public static string? Hex(string s)
    {
        var h = s.Trim().TrimStart('#');
        if (!h.All(Uri.IsHexDigit)) return null;
        if (h.Length == 3) h = string.Concat(h.Select(c => new string(c, 2)));
        return h.Length == 6 ? "#" + h.ToUpperInvariant() : null;
    }

    string? phononError;
}
