using System.Globalization;
using System.Text;
using Hover.Host.Protocol;

namespace Hover.Chat.Desk;

// The desk card's and panel's lists laid out row by row (internal/app/desk.go, web/office's desk.js),
// over the JSON the backend's `desk` answers carry. Each row has its kind, height and words; the
// panel draws them, and only the rows in view.

/// <summary>One row of a tab's list, before its y is known (desk.slint's DRow).</summary>
public sealed record DRow(int Kind, float H)
{
    public int Depth { get; init; }
    public string Text { get; init; } = "";
    public string Sub { get; init; } = "";
    public string Right { get; init; } = "";
    public string Num { get; init; } = "";
    public string Num2 { get; init; } = "";
    public string Add { get; init; } = "";
    public string Del { get; init; } = "";
    public string Badge { get; init; } = "";
    public int Tone { get; init; }
    public int Flag { get; init; }
    public string Act { get; init; } = "";
    public string Tag1 { get; init; } = "";
    public string Tag2 { get; init; } = "";
    /// <summary>A row that is a document (kind 25): the height of its Markdown, laid out by the panel.</summary>
    public string? Doc { get; init; }
}

/// <summary>What a tab shows when it has no rows: an icon's id, a title, a line.</summary>
public sealed record DeskEmpty(string Icon, string Title, string Text);

/// <summary>A tab laid out: its rows, each one's top, and the whole height.</summary>
public sealed class Laid
{
    public List<DRow> Rows { get; }
    public float[] Ys { get; }
    public float Total { get; }
    public DeskEmpty? Empty { get; init; }
    public bool Loading { get; init; }

    public Laid(List<DRow> rows, DeskEmpty? empty = null)
    {
        Rows = rows;
        Empty = empty;
        Ys = new float[rows.Count];
        float y = 0;
        for (var i = 0; i < rows.Count; i++)
        {
            Ys[i] = y;
            y += rows[i].H;
        }
        Total = y;
    }

    public static Laid Wait => new([]) { Loading = true };

    public static Laid Nothing(string icon, string title, string text) => new([], new DeskEmpty(icon, title, text));

    /// <summary>The rows in view at top for a list height tall, with some either side to scroll into.</summary>
    public (int From, int To) Window(float top, float height)
    {
        float lo = top - 240, hi = top + height + 240;
        var n = Ys.Length;
        // The first row whose top is at or past lo, less one.
        int l = 0, r = n;
        while (l < r)
        {
            var m = (l + r) / 2;
            if (Ys[m] >= lo) r = m; else l = m + 1;
        }
        var a = Math.Max(l - 1, 0);
        l = 0;
        r = n;
        while (l < r)
        {
            var m = (l + r) / 2;
            if (Ys[m] > hi) r = m; else l = m + 1;
        }
        return (Math.Min(a, n), Math.Min(l, n));
    }
}

public static class DeskRows
{
    // Row heights (px) and the width of a character in the fonts the rows use.
    public const float GapH = 8, HeadH = 32, FileH = 30, TreeH = 30, LineH = 18, DiffFileH = 34, HunkH = 22, PreH = 17,
        TermH = 20, AgentH = 46, SectionH = 24, CheckH = 26, SummaryH = 30, FaintH = 24;
    public const float CharMono = 6.95f, CharProse = 6.5f, CharTerm = 7.55f;

    /// <summary>The most lines of one command's output that are shown (its end).</summary>
    const int OutLines = 80;

    public const string CloudNote = "This session runs in Kiro’s cloud, not on this computer.";

    static DRow R(int kind, float h) => new(kind, h);

    static DRow Gap() => R(0, GapH);

    static DRow Faint(string t) => R(3, FaintH) with { Text = t };

    // MARK: Words and numbers

    /// <summary>A thousands-separated number (toLocaleString).</summary>
    public static string Num(long n)
    {
        var t = Math.Abs(n).ToString(CultureInfo.InvariantCulture);
        var b = new StringBuilder();
        for (var i = 0; i < t.Length; i++)
        {
            if (i > 0 && (t.Length - i) % 3 == 0) b.Append(',');
            b.Append(t[i]);
        }
        return n < 0 ? "-" + b : b.ToString();
    }

    /// <summary>desk.js dur(): "640 ms", "3.2 s", "41 s", "2m 05s".</summary>
    public static string Dur(double ms)
    {
        var inv = CultureInfo.InvariantCulture;
        if (ms < 1000) return Math.Round(ms, MidpointRounding.AwayFromZero).ToString(inv) + " ms";
        if (ms < 60e3)
            return ms < 10e3 ? (ms / 1000).ToString("0.0", inv) + " s" : Math.Round(ms / 1000, MidpointRounding.AwayFromZero).ToString(inv) + " s";
        var m = Math.Floor(ms / 60e3);
        var s = (long)Math.Round(ms % 60e3 / 1000, MidpointRounding.AwayFromZero) % 60;
        return m.ToString(inv) + "m " + s.ToString("00", inv) + "s";
    }

    /// <summary>The characters that fit a list width wide, less pad.</summary>
    public static int Cols(float width, float pad, float charW) => (int)Math.Min(Math.Max(Math.Floor((width - pad) / charW), 20), 400);

    /// <summary>A text cut into lines of at most n characters (tabs as four spaces).</summary>
    public static List<string> WrapChars(string text, int n)
    {
        n = Math.Max(n, 1);
        var output = new List<string>();
        foreach (var line in text.Replace("\r", "").Split('\n'))
        {
            var cs = Runes(line.Replace("\t", "    "));
            if (cs.Count == 0)
            {
                output.Add("");
                continue;
            }
            for (var i = 0; i < cs.Count; i += n) output.Add(string.Concat(cs.Skip(i).Take(n)));
        }
        return output;
    }

    /// <summary>A text cut into lines at words, at most n characters each.</summary>
    public static List<string> WrapWords(string text, int n)
    {
        n = Math.Max(n, 1);
        var output = new List<string>();
        foreach (var para in text.Replace("\r", "").Split('\n'))
        {
            var current = "";
            foreach (var word in para.Split(' '))
            {
                if (current.Length > 0 && Runes(current).Count + 1 + Runes(word).Count > n)
                {
                    output.Add(current);
                    current = "";
                }
                if (current.Length > 0) current += " ";
                current += word;
                var rs = Runes(current);
                while (rs.Count > n)
                {
                    output.Add(string.Concat(rs.Take(n)));
                    current = string.Concat(rs.Skip(n));
                    rs = Runes(current);
                }
            }
            output.Add(current);
        }
        return output;
    }

    static List<string> Runes(string s)
    {
        var l = new List<string>(s.Length);
        foreach (var r in s.EnumerateRunes()) l.Add(r.ToString());
        return l;
    }

    static int Count(string s) => s.EnumerateRunes().Count();

    public static string Base(string path)
    {
        var i = path.LastIndexOf('/');
        return i >= 0 ? path[(i + 1)..] : path;
    }

    public static string DirOf(string path)
    {
        var i = path.LastIndexOf('/');
        return i >= 0 ? path[..i] : "";
    }

    public static bool IsMd(string path)
    {
        var l = path.ToLowerInvariant();
        return l.EndsWith(".md", StringComparison.Ordinal) || l.EndsWith(".markdown", StringComparison.Ordinal);
    }

    /// <summary>desk.js STATUS: the one letter a file's state shows as.</summary>
    public static string BadgeOf(string status) => status switch
    {
        "A" => "A",
        "D" => "D",
        "R" => "R",
        "?" => "U",
        _ => "M",
    };

    /// <summary>A path cut to about max characters by taking out middle folders: C:\Users\…\Hover.</summary>
    public static string Shorten(string path, int max)
    {
        if (Count(path) <= max) return path;
        var sep = path.Contains('\\') ? "\\" : "/";
        var parts = path.Split(sep, StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length < 3) return path;
        var lead = path.StartsWith(sep, StringComparison.Ordinal) ? sep : "";
        return lead + parts[0] + sep + "…" + sep + parts[^1];
    }

    /// <summary>The prompt the shell shows: <c>PS C:\…\project&gt; </c> on Windows.</summary>
    public static string Prompt(string cwd) => OperatingSystem.IsWindows() ? "PS " + Shorten(cwd, 44) + "> " : Shorten(cwd, 40) + "$ ";

    /// <summary>The dim line at the top of the user's terminal: the shell, and that it runs as the user.</summary>
    public static string Banner() => OperatingSystem.IsWindows()
        ? "PowerShell · runs as you (Windows has no sandbox)"
        : "bash · runs as you (your own shell, outside the agents’ sandbox)";

    // MARK: The terminal

    /// <summary>The agent's tab: what it ran, as a terminal shows it. Read only.</summary>
    public static Laid TerminalRows(Jv terminal, int cols, string bot)
    {
        var v = new List<DRow> { R(0, 10), R(28, TermH) with { Text = $"What {bot} ran in this folder. Read only.", Tone = 7 }, R(0, 6) };
        var commands = terminal["commands"].Items().ToList();
        if (commands.Count == 0) v.Add(R(28, TermH) with { Text = "Nothing yet.", Tone = 7 });
        foreach (var c in commands)
        {
            var status0 = c["status"].Str();
            var run = status0 == "in_progress";
            var exit = c["exit"];
            var bad = status0 == "failed" || (exit.Has && exit.Long() != 0);
            var how = exit.Has ? $"exit {exit.Long()}" : bad ? "failed" : "done";
            var status = "running";
            if (!run)
            {
                status = how;
                if (c["ms"].Has && c["ms"].Dbl() > 0) status += " · " + Dur(c["ms"].Dbl());
            }
            var cmd = c["cmd"].Str();
            var nl = cmd.IndexOf('\n');
            if (nl >= 0) cmd = cmd[..nl];
            cmd = cmd.Trim();
            var parts = WrapChars(cmd, Math.Max(cols - 2, 0));
            v.Add(R(27, TermH) with { Text = parts[0], Sub = "$ ", Right = status, Tone = bad ? 2 : 7, Flag = run ? 1 : 0 });
            foreach (var more in parts.Skip(1)) v.Add(R(28, TermH) with { Text = "  " + more });
            var output = c["out"].Str();
            var lines = output.Length > 0 ? WrapChars(output.TrimEnd('\n'), cols) : [];
            var skip = Math.Max(lines.Count - OutLines, 0);
            if (skip > 0) v.Add(R(28, TermH) with { Text = $"… {Num(skip)} earlier lines", Tone = 7 });
            foreach (var l in lines.Skip(skip)) v.Add(R(28, TermH) with { Text = l, Tone = bad ? 2 : 0 });
        }
        v.Add(R(0, 10));
        return new Laid(v);
    }

    /// <summary>My commands: the banner, then each command as typed at its prompt with what it printed.</summary>
    public static Laid MineRows(IReadOnlyList<TermEntryInfo> entries, int cols)
    {
        var v = new List<DRow> { R(0, 10), R(28, TermH) with { Text = Banner(), Tone = 7 }, R(0, 6) };
        foreach (var e in entries)
        {
            var prompt = Prompt(e.Cwd);
            var parts = WrapChars(e.Cmd, Math.Max(cols - Count(prompt), 8));
            v.Add(R(27, TermH) with { Text = parts[0], Sub = prompt });
            foreach (var more in parts.Skip(1)) v.Add(R(28, TermH) with { Text = more });
            if (e.Cut > 0) v.Add(R(28, TermH) with { Text = $"… {Num(e.Cut)} earlier lines", Tone = 7 });
            foreach (var l in e.Lines)
            {
                var tone = l.Text == "^C" && !l.Err ? 7 : l.Err ? 2 : 0;
                foreach (var t in WrapChars(l.Text, cols)) v.Add(R(28, TermH) with { Text = t, Tone = tone });
            }
        }
        return new Laid(v);
    }

    // MARK: Files

    /// <summary>Files, searched: the paths holding q, 200 at most.</summary>
    public static Laid FindRows(IReadOnlyList<string> tree, string q)
    {
        q = q.ToLowerInvariant();
        var v = new List<DRow>();
        foreach (var p in tree)
        {
            if (v.Count == 200) break;
            if (p.Contains(q, StringComparison.OrdinalIgnoreCase)) v.Add(R(4, FileH) with { Text = Base(p), Sub = DirOf(p), Act = "file:" + p });
        }
        if (v.Count == 0) v.Add(Faint("Nothing matches."));
        return new Laid(v);
    }

    /// <summary>Files: the folder as a tree, and nothing else (what changed is the Diff tab's).</summary>
    public static Laid FilesRows(Jv files, ISet<string> open, ISet<string> saved)
    {
        var tree = files["tree"].Strings();
        var v = TreeRows(tree, open, ChangedMap(files, saved));
        if (v.Count == 0) v.Add(Faint("The folder is empty."));
        if (files["more"].Bool()) v.Add(Faint("Showing the first 5,000 files. Find one by name above."));
        return new Laid(v);
    }

    /// <summary>Each changed file's letter: git's, and M for one saved here that git does not list.</summary>
    public static Dictionary<string, string> ChangedMap(Jv files, ISet<string> saved)
    {
        var m = new Dictionary<string, string>();
        foreach (var c in files["changed"].Items()) m[c["path"].Str()] = BadgeOf(c["status"].Str());
        foreach (var p in saved) m.TryAdd(p, "M");
        return m;
    }

    sealed class TreeNode
    {
        public readonly Dictionary<string, TreeNode> Dirs = [];
        public readonly List<string> Names = [];
        public readonly List<string> Files = [];
    }

    /// <summary>The folder as a tree: folders first (by name), each folded until opened.</summary>
    public static List<DRow> TreeRows(IReadOnlyList<string> paths, ISet<string> open, IReadOnlyDictionary<string, string> changed)
    {
        var root = new TreeNode();
        foreach (var p in paths)
        {
            var parts = p.Split('/');
            var n = root;
            foreach (var part in parts.Take(parts.Length - 1))
            {
                if (!n.Dirs.TryGetValue(part, out var c))
                {
                    c = new TreeNode();
                    n.Dirs[part] = c;
                    n.Names.Add(part);
                }
                n = c;
            }
            n.Files.Add(p);
        }
        var hot = new HashSet<string>();
        foreach (var c in changed.Keys)
        {
            var parts = c.Split('/');
            for (var i = 1; i < parts.Length; i++) hot.Add(string.Join('/', parts.Take(i)));
        }
        var output = new List<DRow>();
        void Walk(TreeNode n, string prefix, int depth)
        {
            var names = n.Names.ToList();
            names.Sort(StringComparer.Ordinal);
            foreach (var name in names)
            {
                var path = prefix + name;
                var isOpen = open.Contains(path);
                output.Add(R(5, TreeH) with { Text = name, Act = "dir:" + path, Depth = depth, Flag = (isOpen ? 1 : 0) | (hot.Contains(path) ? 2 : 0) });
                if (isOpen) Walk(n.Dirs[name], path + "/", depth + 1);
            }
            foreach (var f in n.Files)
            {
                var r = R(5, TreeH) with { Text = Base(f), Act = "file:" + f, Depth = depth };
                if (changed.TryGetValue(f, out var letter)) r = r with { Flag = 2, Badge = letter };
                output.Add(r);
            }
        }
        Walk(root, "", 0);
        return output;
    }

    /// <summary>A file of the Files tab: its lines with their numbers, or its Markdown as a document. The bar above is the panel's.</summary>
    public static Laid FileRows(Jv file, string? markdown, float markdownH)
    {
        var v = new List<DRow>();
        var path = file["path"].Str();
        if (file["error"].Has) v.Add(Faint("Couldn’t open it. " + file["error"].Str()));
        else if (file["binary"].Bool()) v.Add(Faint("Not a text file. Binary files aren’t shown here."));
        else if (markdown is not null)
        {
            v.Add(R(0, 10));
            v.Add(R(25, (float)Math.Ceiling(markdownH) + 8) with { Doc = markdown });
            v.Add(R(0, 16));
        }
        else
        {
            var text = file["text"].Str().Replace("\r\n", "\n");
            if (text.EndsWith('\n')) text = text[..^1];
            var lines = text.Split('\n');
            v.Add(R(0, 8));
            for (var i = 0; i < lines.Length; i++)
            {
                if (i == 6000) break;
                v.Add(R(7, LineH) with { Text = lines[i].Replace("\t", "    "), Num = (i + 1).ToString(CultureInfo.InvariantCulture), Act = $"open:{path}:{i + 1}" });
            }
            if (file["truncated"].Bool() || lines.Length > 6000) v.Add(Faint("The rest of this file isn’t shown."));
        }
        return new Laid(v);
    }

    // MARK: The diff

    /// <summary>Reads "@@ -o[,n] +n[,m] @@": the old and the new first line.</summary>
    static bool HunkHeader(string l, out long o, out long n)
    {
        o = n = 0;
        if (!l.StartsWith("@@ -", StringComparison.Ordinal)) return false;
        var rest = l[4..];
        bool Number(ref string t, out long v)
        {
            var k = 0;
            while (k < t.Length && t[k] is >= '0' and <= '9') k++;
            var ok = long.TryParse(t.AsSpan(0, k), NumberStyles.None, CultureInfo.InvariantCulture, out v);
            t = t[k..];
            return ok && k > 0;
        }
        if (!Number(ref rest, out o)) return false;
        if (rest.StartsWith(',')) { rest = rest[1..]; if (!Number(ref rest, out _)) return false; }
        if (!rest.StartsWith(" +", StringComparison.Ordinal)) return false;
        rest = rest[2..];
        return Number(ref rest, out n);
    }

    /// <summary>One file's hunks, with the old and the new line numbers, up to a budget of lines.</summary>
    public static (List<DRow> Rows, int Used) Hunks(string patch, int budget)
    {
        long o = 0, n = 0;
        var rows = 0;
        var v = new List<DRow>();
        var patchLines = patch.Split('\n').AsEnumerable();
        // The patch ends with a line break: that is not a line.
        if (patch.EndsWith('\n')) patchLines = patchLines.SkipLast(1);
        foreach (var raw in patchLines)
        {
            var l = raw.TrimEnd('\r');
            if (rows >= budget)
            {
                v.Add(R(11, LineH) with { Text = "More lines aren’t shown." });
                break;
            }
            if (HunkHeader(l, out var os, out var ns))
            {
                o = os;
                n = ns;
                v.Add(R(9, HunkH) with { Text = l });
                rows++;
                continue;
            }
            if (l.StartsWith("@@", StringComparison.Ordinal))
            {
                v.Add(R(9, HunkH) with { Text = l });
                continue;
            }
            if (l.StartsWith('\\'))
            {
                v.Add(R(9, HunkH) with { Text = l[1..].TrimStart() });
                continue;
            }
            var text = l.Length > 1 ? l[1..].Replace("\t", "    ") : " ";
            var r = R(10, LineH) with { Text = text };
            if (l.StartsWith('+'))
            {
                r = r with { Tone = 1, Num2 = n > 0 ? n.ToString(CultureInfo.InvariantCulture) : "" };
                n++;
            }
            else if (l.StartsWith('-'))
            {
                r = r with { Tone = 2, Num = o > 0 ? o.ToString(CultureInfo.InvariantCulture) : "" };
                o++;
            }
            else
            {
                r = r with { Num = o > 0 ? o.ToString(CultureInfo.InvariantCulture) : "", Num2 = n > 0 ? n.ToString(CultureInfo.InvariantCulture) : "" };
                o++;
                n++;
            }
            v.Add(r);
            rows++;
        }
        return (v, rows);
    }

    /// <summary>The diff: a file per block, folded or open, with both sides' line numbers. <paramref name="pr"/>: a pull request can be opened from here.</summary>
    public static Laid DiffRows(Jv diff, IReadOnlyDictionary<string, bool> open, bool pr)
    {
        var files = diff["files"].Items().ToList();
        long a = 0, del = 0;
        foreach (var f in files)
        {
            a += f["add"].Long();
            del += f["del"].Long();
        }
        var sum = R(24, SummaryH) with { Text = $"{files.Count} file{(files.Count == 1 ? "" : "s")} changed" };
        if (a > 0) sum = sum with { Add = "+" + Num(a) };
        if (del > 0) sum = sum with { Del = "−" + Num(del) };
        if (diff["branch"].StrOrNull() is { } branch) sum = sum with { Sub = branch };
        if (pr) sum = sum with { Act = "gopr" };
        var v = new List<DRow> { sum };
        var git = diff["git"].Bool();
        if (!git) v.Add(R(2, 34) with { Text = "Not a Git repository: these are the parts of each edit the session kept." });
        if (diff["truncated"].Bool()) v.Add(R(2, 34) with { Text = "The diff is long; the end isn’t shown." });
        var budget = 4000;
        for (var i = 0; i < files.Count; i++)
        {
            var f = files[i];
            var path = f["path"].Str();
            var isOpen = i < 12;
            if (open.TryGetValue(path, out var t)) isOpen = t;
            var name = f["old"].StrOrNull() is { } old ? old + " → " + path : path;
            var binary = f["binary"].Bool();
            var r = R(8, DiffFileH) with { Text = name, Act = "df:" + path, Flag = isOpen ? 1 : 0, Badge = BadgeOf(f["status"].Str()) };
            if (!binary) r = r with { Tag2 = "chip-diff:" + path };
            if (f["add"].Long() > 0) r = r with { Add = "+" + Num(f["add"].Long()) };
            if (f["del"].Long() > 0) r = r with { Del = "−" + Num(f["del"].Long()) };
            v.Add(r);
            if (isOpen)
            {
                if (binary) v.Add(R(11, LineH + 6) with { Text = "Binary file" });
                else
                {
                    var (rows, used) = Hunks(f["patch"].Str(), budget);
                    budget = Math.Max(budget - used, 0);
                    if (rows.Count == 0) v.Add(R(11, LineH + 6) with { Text = "No text changes" });
                    // A line that is in the file now can be opened in the editor at its place.
                    foreach (var row in rows)
                        v.Add(row.Kind == 10 && row.Num2.Length > 0 ? row with { Act = $"open:{path}:{row.Num2}" } : row);
                }
            }
            v.Add(Gap());
        }
        return new Laid(v);
    }

    // MARK: Subagents

    /// <summary>Subagents: each with its task and what came back.</summary>
    public static Laid AgentRows(Jv agents, IReadOnlySet<string> open, int cols)
    {
        var list = agents["agents"].Items().ToList();
        var running = agents["running"].Int();
        var sum = R(24, SummaryH) with { Text = $"{list.Count} subagent{(list.Count == 1 ? "" : "s")}" };
        if (running > 0) sum = sum with { Sub = $"{running} running" };
        var v = new List<DRow> { sum };
        for (var i = list.Count - 1; i >= 0; i--)
        {
            var a = list[i];
            var id = a["id"].Str();
            var status = a["status"].Str();
            var isOpen = open.Contains(id);
            var (tone, word) = status switch
            {
                "in_progress" => (4, "Running"),
                "failed" => (2, "Failed"),
                "completed" => (1, "Done"),
                _ => (1, status),
            };
            var right = a["ms"].Has && a["ms"].Dbl() > 0 ? word + " · " + Dur(a["ms"].Dbl()) : word;
            v.Add(R(14, AgentH) with { Text = a["name"].Str(), Sub = a["task"].Str(), Right = right, Tone = tone, Flag = isOpen ? 1 : 0, Act = "sa:" + id });
            if (isOpen)
            {
                var prompt = a["prompt"].StrOrNull();
                if (!string.IsNullOrEmpty(prompt))
                {
                    v.Add(R(15, SectionH) with { Text = "TASK" });
                    var k = 0;
                    foreach (var l in WrapChars(prompt.TrimEnd(), cols))
                    {
                        if (k++ == 60) break;
                        v.Add(R(13, PreH) with { Text = l });
                    }
                }
                v.Add(R(15, SectionH) with { Text = "RESULT" });
                var outText = a["out"].StrOrNull();
                if (!string.IsNullOrEmpty(outText))
                {
                    var k = 0;
                    foreach (var l in WrapChars(outText.TrimEnd(), cols))
                    {
                        if (k++ == 120) break;
                        v.Add(R(13, PreH) with { Text = l });
                    }
                }
                else v.Add(Faint(status == "in_progress" ? "Still working…" : "Nothing came back."));
            }
            v.Add(Gap());
        }
        return new Laid(v);
    }

    // MARK: Pull requests

    /// <summary>A pull request's state as a pill: its word and tone (green open, grey draft, purple merged, red closed).</summary>
    public static (string Word, int Tone) PrState(string state, bool draft) => state switch
    {
        "open" when draft => ("Draft", 0),
        "merged" => ("Merged", 5),
        "closed" => ("Closed", 2),
        _ => ("Open", 1),
    };

    static string Plural(long n, string one) => n == 1 ? one : one + "s";

    /// <summary>Pull request: the branch's own, with its checks and description. <paramref name="mdH"/> is the description's height as a document, when it is one.</summary>
    public static Laid PrRows(Jv p, float width, float? mdH)
    {
        var (word, tone) = PrState(p["state"].Str(), p["isDraft"].Bool());
        var v = new List<DRow> { R(16, HeadH) with { Text = word, Tone = tone, Sub = "#" + p["number"].Long() } };
        var title = p["title"].Str();
        var titleLines = Math.Max(Math.Ceiling(Count(title) * 8.6f / Math.Max(width - 24, 1)), 1);
        v.Add(R(17, 10 + (float)titleLines * 21) with { Text = title });
        var facts = new List<string> { p["head"].Str() + " → " + p["base"].Str() };
        if (p["author"].StrOrNull() is { } author) facts.Add("by " + author);
        facts.Add($"+{Num(p["additions"].Long())} −{Num(p["deletions"].Long())}");
        facts.Add($"{Num(p["changedFiles"].Long())} {Plural(p["changedFiles"].Long(), "file")}");
        var comments = p["comments"].Long();
        if (comments > 0) facts.Add($"{comments} {Plural(comments, "comment")}");
        switch (p["review"].StrOrNull())
        {
            case "APPROVED": facts.Add("Approved"); break;
            case "CHANGES_REQUESTED": facts.Add("Changes requested"); break;
            case "REVIEW_REQUIRED": facts.Add("Review required"); break;
        }
        var line = string.Join(" · ", facts);
        var factLines = Math.Max(Math.Ceiling(Count(line) * 6.2f / Math.Max(width - 24, 1)), 1);
        v.Add(R(18, 6 + (float)factLines * 17) with { Text = line });
        v.Add(R(19, 44) with { Text = "Open on GitHub", Act = "ext:" + p["url"].Str() });
        long pass = p["pass"].Long(), fail = p["fail"].Long(), pending = p["pending"].Long(), skip = p["skip"].Long();
        if (pass + fail + pending + skip > 0)
        {
            var t = R(20, SummaryH);
            if (fail > 0) t = t with { Del = $"✗ {fail} failing" };
            if (pending > 0) t = t with { Sub = $"◌ {pending} pending" };
            if (pass > 0) t = t with { Add = $"✓ {pass} passed" };
            if (skip > 0) t = t with { Tag1 = $"{skip} skipped" };
            v.Add(Gap());
            v.Add(t);
            foreach (var c in p["checks"].Items())
            {
                var ctone = c["state"].Str() switch { "pass" => 1, "fail" => 2, "skip" => 7, _ => 3 };
                var act = c["url"].StrOrNull() is { } url ? "ext:" + url : "";
                v.Add(R(21, CheckH) with { Text = c["name"].Str(), Tone = ctone, Act = act });
            }
        }
        var body = p["body"].Str().Trim();
        if (body.Length > 0)
        {
            v.Add(R(1, HeadH) with { Text = "DESCRIPTION" });
            if (mdH is { } h) v.Add(R(25, (float)Math.Ceiling(h) + 8) with { Doc = body });
            else
            {
                var n = Cols(width, 24, CharProse * 1.08f);
                var k = 0;
                foreach (var l in WrapWords(body, n))
                {
                    if (k++ == 400) break;
                    v.Add(R(22, 20) with { Text = l });
                }
            }
        }
        return new Laid(v);
    }

    // MARK: The browser

    /// <summary>What an address box takes: http(s) URLs, and "localhost:3000" and the like. False when it is not an address.</summary>
    public static bool NormalizeUrl(string text, out string url)
    {
        url = "";
        var t = text.Trim();
        if (t.Length == 0) return false;
        var hasScheme = false;
        var at = t.IndexOf("://", StringComparison.Ordinal);
        if (at > 0)
        {
            var s = t[..at];
            hasScheme = char.IsAsciiLetter(s[0]) && s.All(c => c < 128 && (char.IsAsciiLetterOrDigit(c) || c is '+' or '.' or '-'));
        }
        var full = t;
        if (!hasScheme)
        {
            var lower = t.ToLowerInvariant();
            var local = new[] { "localhost", "127.", "0.0.0.0", "[::1]" }.Any(p => lower.StartsWith(p, StringComparison.Ordinal));
            full = (local ? "http" : "https") + "://" + t;
        }
        var cut = full.IndexOf("://", StringComparison.Ordinal);
        if (cut < 0) return false;
        var scheme = full[..cut].ToLowerInvariant();
        if (scheme is not ("http" or "https")) return false;
        var rest = full[(cut + 3)..];
        var end = rest.IndexOfAny(['/', '?', '#']);
        var authority = end >= 0 ? rest[..end] : rest;
        var hostport = authority[(authority.LastIndexOf('@') + 1)..];
        string host;
        string? port = null;
        if (hostport.StartsWith('['))
        {
            var close = hostport.IndexOf(']');
            if (close < 0) return false;
            host = hostport[1..close];
            var after = hostport[(close + 1)..];
            if (after.Length > 0)
            {
                if (!after.StartsWith(':')) return false;
                port = after[1..];
            }
        }
        else if (hostport.LastIndexOf(':') is var colon && colon >= 0)
        {
            host = hostport[..colon];
            port = hostport[(colon + 1)..];
        }
        else host = hostport;
        var portOk = port is null || (port.Length is > 0 and <= 5 && port.All(char.IsAsciiDigit));
        if (host.Length == 0 || !portOk || full.Any(c => char.IsWhiteSpace(c) || char.IsControl(c))) return false;
        url = full;
        return true;
    }
}
