using System.Text.RegularExpressions;

namespace Hover.Owl;

/// Kiro answers in Markdown; the office and the notifications show plain words.
internal static partial class KiroText
{
    /// Markdown as plain prose: no heading marks, emphasis, code fences, link targets
    /// or table rules; list items become bullets.
    internal static string Plain(string md)
    {
        var lines = new List<string>();
        var code = false;
        foreach (var raw in md.Replace("\r\n", "\n").Split('\n'))
        {
            var line = raw.TrimEnd();
            if (line.TrimStart().StartsWith("```")) { code = !code; continue; }
            if (code) { lines.Add(line); continue; }
            if (Rule().IsMatch(line)) continue;
            line = Heading().Replace(line, "");
            line = Quote().Replace(line, "");
            line = Bullet().Replace(line, "$1• ");
            if (line.TrimStart().StartsWith('|'))
                line = string.Join(" · ", line.Split('|', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries));
            line = Image().Replace(line, "$1");
            line = Link().Replace(line, "$1");
            line = Strong().Replace(line, "$2");
            line = Em().Replace(line, "$2");
            line = Strike().Replace(line, "$1");
            line = Tick().Replace(line, "$1");
            lines.Add(line);
        }
        return Blanks().Replace(string.Join("\n", lines), "\n\n").Trim();
    }

    [GeneratedRegex(@"^\s*(\|?\s*:?-{3,}:?\s*)+\|?\s*$|^\s*([-*_])(\s*\2){2,}\s*$")] private static partial Regex Rule();
    [GeneratedRegex(@"^\s{0,3}#{1,6}\s+")] private static partial Regex Heading();
    [GeneratedRegex(@"^\s*>\s?")] private static partial Regex Quote();
    [GeneratedRegex(@"^(\s*)[-*+]\s+(\[[ xX]\]\s+)?")] private static partial Regex Bullet();
    [GeneratedRegex(@"!\[([^\]]*)\]\([^)]*\)")] private static partial Regex Image();
    [GeneratedRegex(@"\[([^\]]+)\]\([^)]*\)")] private static partial Regex Link();
    [GeneratedRegex(@"(\*\*|__)(?=\S)(.+?)(?<=\S)\1")] private static partial Regex Strong();
    [GeneratedRegex(@"(?<![\w*])([*_])(?=\S)(.+?)(?<=\S)\1(?![\w*])")] private static partial Regex Em();
    [GeneratedRegex(@"~~(.+?)~~")] private static partial Regex Strike();
    [GeneratedRegex(@"`([^`]+)`")] private static partial Regex Tick();
    [GeneratedRegex(@"\n{3,}")] private static partial Regex Blanks();
}
