namespace Hover.Chat.Desk;

/// <summary>
/// The line icons of the desk, on the 24-unit grid, as SVG bodies for <see cref="Ui.Pen.Icon"/>. The shapes are
/// Lucide's (ISC, the notice is in THIRD-PARTY-NOTICES.txt), the paths the Go desk draws.
/// </summary>
public static class DeskIcons
{
    static string P(string d) => $"<path d=\"{d}\"/>";

    public static readonly string Files = P("M 20 7 h -3 a 2 2 0 0 1 -2 -2 V 2 M 9 18 a 2 2 0 0 1 -2 -2 V 4 a 2 2 0 0 1 2 -2 h 7 l 4 4 v 10 a 2 2 0 0 1 -2 2 Z M 3 7.6 v 12.8 A 1.6 1.6 0 0 0 4.6 22 h 9.8");
    public static readonly string Diff = P("M 15 2 H 6 a 2 2 0 0 0 -2 2 v 16 a 2 2 0 0 0 2 2 h 12 a 2 2 0 0 0 2 -2 V 7 Z M 9 10 h 6 M 12 7 v 6 M 9 17 h 6");
    public static readonly string PullRequest = P("M 15 18 A 3 3 0 1 0 21 18 A 3 3 0 1 0 15 18 Z M 3 6 A 3 3 0 1 0 9 6 A 3 3 0 1 0 3 6 Z M 13 6 h 3 a 2 2 0 0 1 2 2 v 7 M 6 9 v 12");
    public static readonly string File = P("M 14 2 H 6 a 2 2 0 0 0 -2 2 v 16 a 2 2 0 0 0 2 2 h 12 a 2 2 0 0 0 2 -2 V 8 Z M 14 2 v 6 h 6");
    public static readonly string Back = P("M 12 19 l -7 -7 l 7 -7 M 19 12 H 5");
    public static readonly string Search = P("M 11 4 a 7 7 0 1 0 0 14 7 7 0 0 0 0 -14 Z M 20 20 l -3.5 -3.5");
    public static readonly string Chat = P("M 7.9 20 A 9 9 0 1 0 4 16.1 L 2 22 Z");
    public static readonly string Up = P("M 12 19 V 5 M 5 12 l 7 -7 l 7 7");
    public static readonly string Enter = P("M 20 4 v 7 a 4 4 0 0 1 -4 4 H 4 M 9 10 l -5 5 l 5 5");
    public static readonly string Expand = P("M15 3h6v6 M9 21H3v-6 M21 3l-7 7 M3 21l7-7");
    public static readonly string External = P("M 15 3 h 6 v 6 M 10 14 L 21 3 M 18 13 v 6 a 2 2 0 0 1 -2 2 H 5 a 2 2 0 0 1 -2 -2 V 8 a 2 2 0 0 1 2 -2 h 6");
    public static readonly string Github = P("M 15 22 v -4 a 4.8 4.8 0 0 0 -1 -3.5 c 3 0 6 -2 6 -5.5 c 0.08 -1.25 -0.27 -2.48 -1 -3.5 c 0.28 -1.15 0.28 -2.35 0 -3.5 c 0 0 -1 0 -3 1.5 c -2.64 -0.5 -5.36 -0.5 -8 0 C 6 2 5 2 5 2 c -0.3 1.15 -0.3 2.35 0 3.5 A 5.4 5.4 0 0 0 4 9 c 0 3.5 3 5.5 6 5.5 c -0.39 0.49 -0.68 1.05 -0.85 1.65 S 8.93 17.38 9 18 v 4 M 9 18 c -4.51 2 -5 -2 -7 -2");

    public static readonly string Check = P("M 20 6 L 9 17 l -5 -5");
    public static readonly string Add = P("M 5 12 h 14 M 12 5 v 14");
    public static readonly string Bell = P("M 10.268 21 a 2 2 0 0 0 3.464 0 M 3.262 15.326 A 1 1 0 0 0 4 17 h 16 a 1 1 0 0 0 0.74 -1.673 C 19.41 13.956 18 12.499 18 8 A 6 6 0 0 0 6 8 c 0 4.499 -1.411 5.956 -2.738 7.326");
    public static readonly string Copy = P("M 10 8 H 20 A 2 2 0 0 1 22 10 V 20 A 2 2 0 0 1 20 22 H 10 A 2 2 0 0 1 8 20 V 10 A 2 2 0 0 1 10 8 Z M 4 16 c -1.1 0 -2 -0.9 -2 -2 V 4 c 0 -1.1 0.9 -2 2 -2 h 10 c 1.1 0 2 0.9 2 2");
    public static readonly string Rename = P("M 21.174 6.812 a 1 1 0 0 0 -3.986 -3.987 L 3.842 16.174 a 2 2 0 0 0 -0.5 0.83 l -1.321 4.352 a 0.5 0.5 0 0 0 0.623 0.622 l 4.353 -1.32 a 2 2 0 0 0 0.83 -0.497 Z M 15 5 l 4 4");
    public static readonly string Close = P("M 18 6 L 6 18 M 6 6 l 12 12");
    public static readonly string ChevronDown = P("M 6 9 l 6 6 l 6 -6");
    public static readonly string ChevronRight = P("M 9 18 l 6 -6 l -6 -6");
    public static readonly string Folder = P("M 20 20 a 2 2 0 0 0 2 -2 V 8 a 2 2 0 0 0 -2 -2 h -7.9 a 2 2 0 0 1 -1.69 -0.9 L 9.6 3.9 A 2 2 0 0 0 7.93 3 H 4 a 2 2 0 0 0 -2 2 v 13 a 2 2 0 0 0 2 2 Z");
    public static readonly string Warning = P("M 21.73 18 l -8 -14 a 2 2 0 0 0 -3.48 0 l -8 14 A 2 2 0 0 0 4 21 h 16 a 2 2 0 0 0 1.73 -3 M 12 9 v 4 M 12 17 h 0.01");
    public static readonly string Terminal = P("M 7 11 l 2 -2 l -2 -2 M 11 13 h 4 M 5 3 H 19 A 2 2 0 0 1 21 5 V 19 A 2 2 0 0 1 19 21 H 5 A 2 2 0 0 1 3 19 V 5 A 2 2 0 0 1 5 3 Z");
    public static readonly string Bot = P("M 12 8 V 4 H 8 M 6 8 H 18 A 2 2 0 0 1 20 10 V 18 A 2 2 0 0 1 18 20 H 6 A 2 2 0 0 1 4 18 V 10 A 2 2 0 0 1 6 8 Z M 2 14 h 2 M 20 14 h 2 M 15 13 v 2 M 9 13 v 2");
    public static readonly string Code = P("M 16 18 l 6 -6 l -6 -6 M 8 6 l -6 6 l 6 6");
    public static readonly string Brain = P("M 12 18 V 5 M 15 13 a 4.17 4.17 0 0 1 -3 -4 a 4.17 4.17 0 0 1 -3 4 M 17.598 6.5 A 3 3 0 1 0 12 5 a 3 3 0 1 0 -5.598 1.5 M 17.997 5.125 a 4 4 0 0 1 2.526 5.77 M 18 18 a 4 4 0 0 0 2 -7.464 M 19.967 17.483 A 4 4 0 1 1 12 18 a 4 4 0 1 1 -7.967 -0.517 M 6 18 a 4 4 0 0 1 -2 -7.464 M 6.003 5.125 a 4 4 0 0 0 -2.526 5.77");
    public static readonly string Globe = P("M 2 12 A 10 10 0 1 0 22 12 A 10 10 0 1 0 2 12 Z M 12 2 a 14.5 14.5 0 0 0 0 20 a 14.5 14.5 0 0 0 0 -20 M 2 12 h 20");
    public static readonly string Sparkles = P("M 11.017 2.814 a 1 1 0 0 1 1.966 0 l 1.051 5.558 a 2 2 0 0 0 1.594 1.594 l 5.558 1.051 a 1 1 0 0 1 0 1.966 l -5.558 1.051 a 2 2 0 0 0 -1.594 1.594 l -1.051 5.558 a 1 1 0 0 1 -1.966 0 l -1.051 -5.558 a 2 2 0 0 0 -1.594 -1.594 l -5.558 -1.051 a 1 1 0 0 1 0 -1.966 l 5.558 -1.051 a 2 2 0 0 0 1.594 -1.594 Z M 20 2 v 4 M 22 4 h -4");
    public static readonly string Cloud = P("M 17.5 19 H 9 a 7 7 0 1 1 6.71 -9 h 1.79 a 4.5 4.5 0 1 1 0 9 Z");
    public static readonly string Shield = P("M 12 3 5 6 v 5 c 0 4.4 3 8.3 7 9.5 4 -1.2 7 -5.1 7 -9.5 V 6 Z");
    public static readonly string Trash = P("M 4 7 h 16 M 10 11 v 6 M 14 11 v 6 M 6 7 l 1 12 a 2 2 0 0 0 2 2 h 6 a 2 2 0 0 0 2 -2 l 1 -12 M 9 7 V 4 h 6 v 3");
    public static readonly string Image = P("M 5 3 h 14 a 2 2 0 0 1 2 2 v 14 a 2 2 0 0 1 -2 2 H 5 a 2 2 0 0 1 -2 -2 V 5 a 2 2 0 0 1 2 -2 Z M 9 7 a 2 2 0 1 0 0 4 a 2 2 0 1 0 0 -4 Z M 21 15 l -5 -5 L 5 21");
    public static readonly string Fork = P("M 15 18 A 3 3 0 1 0 9 18 A 3 3 0 1 0 15 18 Z M 9 6 A 3 3 0 1 0 3 6 A 3 3 0 1 0 9 6 Z M 21 6 A 3 3 0 1 0 15 6 A 3 3 0 1 0 21 6 Z M 18 9 v 2 c 0 .6 -.4 1 -1 1 H 7 c -.6 0 -1 -.4 -1 -1 V 9 M 12 12 v 3");

    /// <summary>A surface's icon by its id (desk.js ICONS).</summary>
    public static string Of(string id) => id switch
    {
        "terminal" => Terminal,
        "files" => Files,
        "diff" => Diff,
        "pr" => PullRequest,
        "agents" => Bot,
        "file" => File,
        "folder" => Folder,
        _ => Globe,
    };

    /// <summary>A step's icon by its kind.</summary>
    public static string Step(string kind) => kind switch
    {
        "edit" => Rename,
        "run" => Terminal,
        "search" => Search,
        "read" => File,
        "thought" or "think" => Brain,
        "agent" => Bot,
        "web" => Globe,
        _ => Sparkles,
    };
}
