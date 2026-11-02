using System.Globalization;
using System.Text;
using Xunit;

namespace Hover.Settings.Tests;

/// <summary>
/// Every page of Settings, in four states, against the Go page (<c>app.Build</c>, <c>internal/app/pages.go</c>) as text: each row's words, its
/// sub line, its lead, its control with its id, labels, options and what is on. The Go side is <c>golden/pages-go.txt</c>, made by
/// <c>golden/pages-dump.go.txt</c> (its first lines say how). Sweep 2 found the two identical in all 40 pages but the lines named below,
/// which differ because the Go dump was made on Linux or because Windows says something else on purpose.
/// </summary>
public class PagesAgainstGoTests
{
    static string Q(string s)
    {
        var b = new StringBuilder("\"");
        foreach (var c in s)
        {
            if (c == '\\') b.Append("\\\\");
            else if (c == '"') b.Append("\\\"");
            else if (c == '\n') b.Append("\\n");
            else if (c == '\t') b.Append("\\t");
            else b.Append(c);
        }
        return b.Append('"').ToString();
    }

    static string Bool(bool v) => v ? "true" : "false";

    static string Opts(IEnumerable<Opt> o) => string.Join(", ", o.Select(x => $"{Q(x.Label)}/{Bool(x.On)}"));

    static string Ctl(Control c)
    {
        var p = new List<string> { c.Kind.ToString() };
        if (c.Id != "") p.Add("id=" + c.Id);
        if (c.Name != "") p.Add("name=" + Q(c.Name));
        if (c.Text != "") p.Add("text=" + Q(c.Text));
        switch (c.Kind)
        {
            case ControlKind.Switch: p.Add("on=" + Bool(c.On)); break;
            case ControlKind.Button: p.Add("enabled=" + Bool(c.Enabled)); break;
            case ControlKind.Segments: p.Add($"labels=[{string.Join(" ", c.Labels.Select(Q))}] picked={c.Picked}"); break;
            case ControlKind.Slider: p.Add($"value={c.Value} min={c.Min} max={c.Max}"); break;
            case ControlKind.Picker: p.Add($"options=[{Opts(c.Options)}]"); break;
            case ControlKind.Field: p.Add($"placeholder={Q(c.Placeholder)} secret={Bool(c.Secret)} on={Bool(c.On)}"); break;
            case ControlKind.Chips:
                p.Add($"badges=[{Opts(c.Badges)}] buttons=[{string.Join(", ", c.Buttons.Select(x => $"{x.Id}:{Q(x.Text)}:{Bool(x.Red)}"))}] open={c.Open}");
                break;
        }
        return string.Join(" ", p);
    }

    static string Dump(IReadOnlyList<Block> blocks)
    {
        var o = new StringBuilder();
        foreach (var x in blocks)
        {
            switch (x.Kind)
            {
                case BlockKind.Title or BlockKind.Lead or BlockKind.Footnote: o.Append($"[{x.Kind}] {Q(x.Text)}\n"); break;
                case BlockKind.Heading: o.Append($"[Heading] {Q(x.Text)} first={Bool(x.First)}\n"); break;
                case BlockKind.Link: o.Append($"[Link] id={x.Id} name={Q(x.Name)} icon={x.Icon} text={Q(x.Text)} dim={Bool(x.Dim)} status={Q(x.Status)}\n"); break;
                case BlockKind.Group:
                    o.Append("[Group]\n");
                    foreach (var r in x.Rows)
                        o.Append($"  row {Q(r.Label)} | sub={Q(r.Sub)} | enabled={Bool(r.Enabled)} | lead={r.Lead.Kind}/{r.Lead.Icon}/{(int)r.Lead.Tint}/{r.Lead.Letter} | subid={r.SubId} | progress={r.Progress.ToString("0.00", CultureInfo.InvariantCulture)} | ctl: {Ctl(r.Control)}\n");
                    break;
                case BlockKind.Tiles:
                    foreach (var t in x.Tiles) o.Append($"[Tile] id={t.Id} name={Q(t.Name)} from={Q(t.From)} picked={Bool(t.Picked)}\n");
                    break;
                case BlockKind.Credits: o.Append("[Credits]\n"); break;
                case BlockKind.Mcp: o.Append("[Mcp]\n"); break;
            }
        }
        return o.ToString();
    }

    static readonly string[] States = ["default-ready", "default-notready", "voice-local", "rich"];

    // Git may write the file with Windows line ends.
    static string Golden(string name) => File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "golden", name)).Replace("\r\n", "\n");

    static string Mine(string state, Section section)
    {
        var settings = state switch
        {
            "voice-local" => SettingsData.From(Json.Parse("""{"Voice":{"Enabled":true,"Speech":"Local"}}""")),
            "rich" => SettingsData.From(Json.Parse(Golden("rich-settings.json"))),
            _ => SettingsData.Default,
        };
        Func<ToolDef, ToolReady?> ready = _ => new ToolReady(true, true, "");
        if (state == "default-notready")
            ready = t => t.Id switch
            {
                "codex" => new ToolReady(true, false, "Codex isn't installed."),
                "cursor" => new ToolReady(true, false, "Sign in to Cursor."),
                _ => new ToolReady(true, true, ""),
            };
        var input = new PageInput { Settings = settings, Shortcut = "Alt+N", VoiceShortcut = settings.Voice.Shortcut.Label, Ready = ready, SystemDark = true };
        return Dump(PageBuilder.Build(section, input));
    }

    /// <summary>
    /// What is left out of the comparison, and why:
    /// the folder lines (the Go dump names the Linux home); the line that names the program an agent runs (Windows runs <c>codex-acp</c> and
    /// <c>cursor-agent</c>); and Codex's Read only, which the Go page on Windows leaves out with the note the C# page gives (the dump was made on Linux,
    /// where the sandbox works).
    /// </summary>
    static IEnumerable<string> Normal(string text)
    {
        foreach (var raw in text.Split('\n'))
        {
            var line = raw;
            if (line.Contains("row \"Location\"") || line.Contains("row \"Default workspace\"")) continue;
            if (line.StartsWith("[Footnote] \"", StringComparison.Ordinal) && line.Contains(" runs in the background as an ACP server (")) continue;
            if (line.Contains("id=CodexTools"))
            {
                line = line.Replace(" Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows.", "")
                    .Replace(" \"Read only\"]", "]");
            }
            yield return line;
        }
    }

    [Theory]
    [MemberData(nameof(Pages))]
    public void A_page_is_the_Go_page_row_for_row(string state, Section section)
    {
        var all = Golden("pages-go.txt");
        var head = $"=== {state} / {section.Title()}\n";
        var start = all.IndexOf(head, StringComparison.Ordinal);
        Assert.True(start >= 0, $"The Go dump has no '{head.Trim()}'");
        start += head.Length;
        var end = all.IndexOf("\n=== ", start, StringComparison.Ordinal);
        var go = Normal(all[start..(end < 0 ? all.Length : end + 1)]).Where(l => l.Length > 0).ToList();
        var cs = Normal(Mine(state, section)).Where(l => l.Length > 0).ToList();
        var diffs = new List<string>();
        for (var i = 0; i < Math.Max(go.Count, cs.Count); i++)
        {
            var a = i < go.Count ? go[i] : "(nothing)";
            var b = i < cs.Count ? cs[i] : "(nothing)";
            if (a != b) diffs.Add($"line {i + 1}\n  Go: {a}\n  C#: {b}");
        }
        Assert.True(diffs.Count == 0, $"{state} / {section.Title()}: {diffs.Count} lines differ\n" + string.Join("\n", diffs.Take(8)));
    }

    public static IEnumerable<object[]> Pages()
    {
        foreach (var state in States)
            foreach (var section in Sections.All)
                yield return [state, section];
    }
}
