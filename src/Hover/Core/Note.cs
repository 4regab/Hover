using System.Text.RegularExpressions;

namespace Hover.Core;

public sealed class Note
{
    public string Id { get; set; } = Guid.NewGuid().ToString();
    public string Title { get; set; } = "";
    public string Body { get; set; } = "";
    public int Color { get; set; }
    public DateTime Created { get; set; } = DateTime.Now;
    public DateTime Modified { get; set; } = DateTime.Now;
    public bool Archived { get; set; }
    public bool Pinned { get; set; }
    public double Order { get; set; }

    /// Set once the user names the note themselves. The title normally follows the
    /// first line of the body, and this is what stops an edit further down the note
    /// from throwing that name away.
    public bool TitleLocked { get; set; }

    public NoteColor Palette => NoteColor.At(Color);

    public string DisplayTitle => string.IsNullOrEmpty(Title) ? "New note" : Title;

    public Note Copy() => (Note)MemberwiseClone();

    private static readonly Regex Heading = new(@"^#{1,6}\s*", RegexOptions.Compiled);

    /// The longest title the deck can carry. A tab is as wide as the longest label
    /// on the deck, so an unbounded name would push the whole fan across the screen.
    public const int TitleLimit = 60;

    /// The title a body edit leaves behind: a name the user typed stays put, and
    /// everything else keeps following the first line.
    public static string TitleFor(string body, string current, bool locked) =>
        locked ? current : DerivedTitle(body);

    /// A typed title, tidied: one line, trimmed, and no longer than the deck can
    /// show. Empty means "go back to following the first line".
    public static string CleanTitle(string? raw)
    {
        if (raw is null) return "";
        var flat = raw.Replace('\r', ' ').Replace('\n', ' ').Replace('\t', ' ');
        // Collapse the runs the flattening leaves behind, so a pasted paragraph does
        // not become a title padded out with gaps.
        var clean = Whitespace.Replace(flat, " ").Trim();
        return clean.Length > TitleLimit ? clean[..TitleLimit].TrimEnd() + "…" : clean;
    }

    private static readonly Regex Whitespace = new(@"\s{2,}", RegexOptions.Compiled);

    /// Title shown in the fan / lists, derived from the first non-empty line.
    public static string DerivedTitle(string body)
    {
        var line = body.Split('\n', '\r').FirstOrDefault(l => true) ?? "";
        var clean = Heading.Replace(line.Trim(), "");
        clean = TaskSyntax.Stripped(clean).Trim();
        if (clean.Length == 0) return "";
        return clean.Length > 60 ? clean[..60] + "…" : clean;
    }

    /// Completed / total, or null when the note holds no tasks.
    public (int Done, int Total)? TaskProgress
    {
        get
        {
            int done = 0, total = 0;
            foreach (var line in Body.Split('\n'))
            {
                switch (TaskSyntax.Marker(line.TrimEnd('\r')))
                {
                    case TaskSyntax.Done: done++; total++; break;
                    case TaskSyntax.Open: total++; break;
                }
            }
            return total > 0 ? (done, total) : null;
        }
    }

    /// Second line onwards, collapsed — used as list subtitle.
    public string Preview
    {
        get
        {
            var lines = Body.Split('\n').Select(l => l.TrimEnd('\r')).ToList();
            var rest = string.Join(" ", lines.Skip(1)).Trim();
            return rest.Length > 120 ? rest[..120] + "…" : rest;
        }
    }
}
