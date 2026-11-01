using System.Text.Json;
using System.Text.Json.Nodes;
using Hover.Host.Protocol;

namespace Hover.Settings;

public sealed partial class SettingsController
{
    int mcpSerial = 1;

    /// <summary>The MCP section's form and list, for the tests and the painter.</summary>
    public McpView Mcp => Live.Mcp;

    void OnMcp(McpEvent m)
    {
        var view = Live.Mcp;
        switch (m.Op)
        {
            case "get" when m.Server is { } one && m.Ok:
                // Opening a server to edit: its values come with it.
                OpenForm(FormOf(one));
                break;
            case "get" or "enable" or "remove":
                if (m.Ok)
                {
                    view.Servers = m.Servers;
                    view.Error = null;
                    view.File = m.File ?? view.File;
                    view.HasFile = m.File is not null;
                    if (m.Op == "remove") view.Confirm = null;
                }
                else if (m.Op == "get")
                {
                    // The file doesn't parse: the list is empty and nothing is written.
                    view.Servers = [];
                    view.Error = m.Error;
                }
                else view.Notice = m.Error;
                break;
            case "save":
                if (m.Ok)
                {
                    view.Form = null;
                    view.Notice = null;
                    view.Servers = m.Servers;
                    view.Error = null;
                }
                else if (m.Fields is { } f && view.Form is { } form)
                {
                    form.NameProblem = f.Name;
                    form.UrlProblem = f.Url;
                    form.CommandProblem = f.Command;
                    form.PairsProblem = f.Pairs;
                }
                else view.Notice = m.Error;
                break;
        }
        Dirty();
    }

    McpForm FormOf(McpServer s) => new()
    {
        Editing = s.Name,
        Name = s.Name,
        Remote = s.Remote,
        Url = s.Remote ? s.Url : "",
        Command = s.Remote ? "" : s.Command,
        Args = s.Remote ? "" : string.Join("\n", s.Args),
        Pairs = s.Pairs.Count == 0 ? [("", "")] : s.Pairs.Select(p => (p.Name, p.Value ?? "")).ToList(),
        Serial = mcpSerial++,
    };

    void OpenForm(McpForm form)
    {
        Live.Mcp.Form = form;
        Live.Mcp.Confirm = null;
        Live.Mcp.Notice = null;
    }

    /// <summary>A click or a key in the MCP section: the action and what it is about. A key typed in a field is <c>McpType</c>.</summary>
    public void McpAct(string action, string what)
    {
        var m = Live.Mcp;
        switch (action)
        {
            case "McpAdd":
                m.Form = new McpForm { Serial = mcpSerial++ };
                m.Confirm = null;
                m.Notice = null;
                break;
            case "McpEdit":
                env.Send(new McpCommand("get") { Name = what });
                return;
            case "McpCancel":
                m.Form = null;
                break;
            case "McpKind":
                if (m.Form is { } k)
                {
                    k.Remote = what == "1";
                    k.NameProblem = k.UrlProblem = k.CommandProblem = k.PairsProblem = null;
                }
                break;
            case "McpPairAdd":
                m.Form?.Pairs.Add(("", ""));
                break;
            case "McpPairDel":
                if (m.Form is { } d && int.TryParse(what, out var i) && i >= 0 && i < d.Pairs.Count)
                {
                    d.Pairs.RemoveAt(i);
                    if (d.Pairs.Count == 0) d.Pairs.Add(("", ""));
                }
                break;
            // "{field}\x1f{text}": field is name, url, command, args, or k{row} / v{row} of a pair.
            case "McpType":
                {
                    var cut = what.IndexOf('\u001f');
                    if (m.Form is not { } f || cut < 0) return;
                    var field = what[..cut];
                    var text = what[(cut + 1)..];
                    switch (field)
                    {
                        case "name": f.Name = text; break;
                        case "url": f.Url = text; break;
                        case "command": f.Command = text; break;
                        case "args": f.Args = text; break;
                        default:
                            if (field.Length > 1 && int.TryParse(field.AsSpan(1), out var row) && row >= 0 && row < f.Pairs.Count)
                                f.Pairs[row] = field[0] == 'k' ? (text, f.Pairs[row].Value) : (f.Pairs[row].Key, text);
                            break;
                    }
                    // A key typed in a field needn't draw the page again: the box shows it.
                    return;
                }
            case "McpSave":
                if (m.Form is { } form)
                {
                    var pairs = new JsonArray();
                    foreach (var (key, value) in form.Pairs) pairs.Add(new JsonArray(key, value));
                    var draft = new JsonObject
                    {
                        ["Name"] = form.Name, ["Remote"] = form.Remote, ["URL"] = form.Url,
                        ["Command"] = form.Command, ["Args"] = form.Args, ["Pairs"] = pairs,
                    };
                    env.Send(new McpCommand("save") { Draft = draft, Editing = form.Editing });
                }
                return;
            // The editor is found on the disk, so this waits a moment.
            case "McpOpen":
                m.Notice = null;
                if (m.File != "") env.OpenInEditor(m.File);
                break;
            case "McpRemove":
                m.Confirm = what;
                m.Form = null;
                m.Notice = null;
                break;
            case "McpRemoveNo":
                m.Confirm = null;
                break;
            case "McpRemoveYes":
                m.Notice = null;
                env.Send(new McpCommand("remove") { Name = what });
                return;
        }
        Dirty();
    }

    // MARK: Phonon and Try it

    void OnPhonon(JsonElement raw)
    {
        Live.Phonon = VoicePageCards.Phonon(raw, phononError);
        Dirty();
    }

    /// <summary>Try it's card for a stage of a trial (null: nothing to show).</summary>
    public void SetTry(TryCard? card)
    {
        Live.VoiceTry = card;
        Dirty();
    }

    /// <summary>Why the voice shortcut could not be taken, or null.</summary>
    public void SetShortcutError(string? why)
    {
        if (Live.ShortcutError == why) return;
        Live.ShortcutError = why;
        Dirty();
    }
}
