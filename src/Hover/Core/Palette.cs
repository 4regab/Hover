using System.IO;
using System.Text.Json;

namespace Hover.Core;

/// A theme picked from a VS Code colour theme: its name, whether it is dark, and the
/// colours of it Hover reads (Palette.Keys), as the theme file wrote them. Kept in
/// the settings, so the theme stays even if the editor it came from is removed.
public sealed record SavedTheme(string Name, bool Dark, Dictionary<string, string> Colors);

/// A colour theme another editor has installed on this PC.
public sealed record InstalledTheme(string Label, string Path, bool Dark, string From);

/// Every colour the workspace draws with, as 0xAARRGGBB. Hover's own light and dark
/// are Apple's system colours; any other theme is worked out from a VS Code colour
/// theme: its editor background for the cards, its side bar (or a darker shade) for
/// the panel they sit on, its text colour, its button colour as the accent and its
/// terminal colours for the rest. The shades in between are the text colour at set
/// strengths, so they suit any background. No WPF here, so it can be tested anywhere.
public sealed record Palette
{
    public required string Name { get; init; }
    public required bool Dark { get; init; }
    public required uint Ink { get; init; }
    public required uint InkDim { get; init; }
    public required uint InkFaint { get; init; }
    public required uint Fill { get; init; }
    public required uint Wash { get; init; }
    public required uint WashStrong { get; init; }
    public required uint Separator { get; init; }
    public required uint Surface { get; init; }
    public required uint Sheet { get; init; }
    public required uint SheetEdge { get; init; }
    public required uint Panel { get; init; }
    public required uint PanelEdge { get; init; }
    public required uint Thumb { get; init; }
    public required uint RowHover { get; init; }
    public required uint SwitchOff { get; init; }
    public required uint Handle { get; init; }
    public required uint Blue { get; init; }
    public required uint Green { get; init; }
    public required uint Purple { get; init; }
    public required uint Yellow { get; init; }
    public required uint Teal { get; init; }
    public required uint Orange { get; init; }
    public required uint Red { get; init; }

    // Apple's dark and light system colour tables: label, secondaryLabel,
    // tertiaryLabel; secondarySystemFill, tertiarySystemFill, systemFill; separator;
    // secondarySystemGroupedBackground (cards) and systemGroupedBackground (panel).
    public static readonly Palette HoverDark = new()
    {
        Name = "Hover", Dark = true,
        Ink = 0xFFFFFFFF, InkDim = 0x99EBEBF5, InkFaint = 0x4DEBEBF5,
        Fill = 0x52787880, Wash = 0x3D767680, WashStrong = 0x5C787880, Separator = 0x99545458,
        Surface = 0xFF1C1C1E, Sheet = 0xFF2C2C2E, SheetEdge = 0x1FFFFFFF, Panel = 0xFF000000, PanelEdge = 0x1AFFFFFF,
        Thumb = 0xFF636366, RowHover = 0x14FFFFFF, SwitchOff = 0xFF39393D, Handle = 0x66FFFFFF,
        Blue = 0xFF0A84FF, Green = 0xFF30D158, Purple = 0xFFBF5AF2, Yellow = 0xFFFFD60A,
        Teal = 0xFF64D2FF, Orange = 0xFFFF9F0A, Red = 0xFFFF453A,
    };

    public static readonly Palette HoverLight = new()
    {
        Name = "Hover", Dark = false,
        Ink = 0xFF000000, InkDim = 0x993C3C43, InkFaint = 0x4D3C3C43,
        Fill = 0x29787880, Wash = 0x1F767680, WashStrong = 0x33787880, Separator = 0x4A3C3C43,
        Surface = 0xFFFFFFFF, Sheet = 0xFFFFFFFF, SheetEdge = 0x1A000000, Panel = 0xFFF2F2F7, PanelEdge = 0x1A000000,
        Thumb = 0xFFFFFFFF, RowHover = 0x0D000000, SwitchOff = 0xFFE9E9EB, Handle = 0x40000000,
        Blue = 0xFF007AFF, Green = 0xFF34C759, Purple = 0xFFAF52DE, Yellow = 0xFFFFCC00,
        Teal = 0xFF32ADE6, Orange = 0xFFFF9500, Red = 0xFFFF3B30,
    };

    /// The VS Code colour ids From reads, most wanted first where several can serve.
    public static IReadOnlySet<string> Keys => KeySet;

    private static readonly HashSet<string> KeySet = new(StringComparer.OrdinalIgnoreCase)
    {
        "editor.background", "editor.foreground", "foreground", "descriptionForeground",
        "sideBar.background", "activityBar.background", "menu.background", "editorWidget.background",
        "button.background", "focusBorder", "textLink.foreground", "errorForeground",
        "terminal.ansiBlue", "terminal.ansiGreen", "terminal.ansiMagenta", "terminal.ansiYellow",
        "terminal.ansiCyan", "terminal.ansiRed",
    };

    public static Palette From(SavedTheme theme)
    {
        var dark = theme.Dark;
        var apple = dark ? HoverDark : HoverLight;
        uint? Get(params string[] keys)
        {
            foreach (var k in keys)
                if (theme.Colors.TryGetValue(k, out var v) && ParseColor(v) is { } c) return c;
            return null;
        }

        var surface = Over(Get("editor.background") ?? (dark ? 0xFF1E1E1Eu : 0xFFFFFFFFu), apple.Panel);
        // Apple's order: the panel a step darker than the cards on it, in light and dark.
        var side = Get("sideBar.background", "activityBar.background") is { } s ? Over(s, surface) : (uint?)null;
        var panel = side is { } p && Luma(p) < Luma(surface) - 0.01 ? p : Mix(surface, 0xFF000000, dark ? 0.35 : 0.05);
        var ink = Over(Get("foreground", "editor.foreground") ?? apple.Ink, surface);
        // A theme whose text barely stands off its background would be unreadable here.
        if (Math.Abs(Luma(ink) - Luma(surface)) < 0.3) ink = dark ? 0xFFF2F2F2 : 0xFF1A1A1A;
        uint Accent(uint fallback, params string[] keys) => Over(Get(keys) ?? fallback, surface);
        var red = Accent(apple.Red, "terminal.ansiRed", "errorForeground");
        var yellow = Accent(apple.Yellow, "terminal.ansiYellow");
        // The accent colours links, the picked tab and the main buttons, so it has to
        // read as a colour: a theme whose buttons are grey gives its blue instead.
        var blue = new[] { "button.background", "focusBorder", "textLink.foreground", "terminal.ansiBlue" }
            .Select(k => Get(k) is { } c ? Over(c, surface) : (uint?)null)
            .FirstOrDefault(c => c is { } x && Saturation(x) >= 0.3) ?? apple.Blue;

        return new Palette
        {
            Name = theme.Name, Dark = dark,
            Ink = ink,
            InkDim = Get("descriptionForeground") is { } d ? Over(d, surface) : Alpha(ink, 0x99),
            InkFaint = Alpha(ink, 0x4D),
            Fill = Alpha(ink, dark ? (byte)0x29 : (byte)0x1A),
            Wash = Alpha(ink, dark ? (byte)0x1C : (byte)0x12),
            WashStrong = Alpha(ink, dark ? (byte)0x2E : (byte)0x1F),
            Separator = Alpha(ink, dark ? (byte)0x26 : (byte)0x1F),
            Surface = surface,
            Sheet = Get("menu.background", "editorWidget.background") is { } m ? Over(m, surface) : dark ? Mix(surface, ink, 0.06) : surface,
            SheetEdge = Alpha(ink, 0x1F),
            Panel = panel,
            PanelEdge = Alpha(ink, 0x1A),
            Thumb = dark ? Mix(surface, ink, 0.22) : Mix(surface, 0xFFFFFFFF, 0.8),
            RowHover = Alpha(ink, dark ? (byte)0x14 : (byte)0x0D),
            SwitchOff = Mix(surface, ink, dark ? 0.16 : 0.1),
            Handle = Alpha(ink, dark ? (byte)0x66 : (byte)0x40),
            Blue = blue,
            Green = Accent(apple.Green, "terminal.ansiGreen"),
            Purple = Accent(apple.Purple, "terminal.ansiMagenta"),
            Yellow = yellow,
            Teal = Accent(apple.Teal, "terminal.ansiCyan"),
            // Few themes name an orange; halfway between their red and yellow is one.
            Orange = Mix(red, yellow, 0.5),
            Red = red,
        };
    }

    // MARK: Reading VS Code theme files

    private static readonly JsonDocumentOptions Jsonc = new()
    {
        CommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    /// A VS Code colour theme file, following its "include" chain (the file's own
    /// colours win). Label and dark come from the extension that lists the theme,
    /// when there is one; otherwise from the file. Null if it cannot be read.
    public static SavedTheme? Read(string path, string? label = null, bool? dark = null)
    {
        var colors = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        string? name = null, type = null;
        try { Load(path, 0); }
        catch (Exception e)
        {
            Log.Line($"theme {path} unreadable — {e.Message}");
            return null;
        }
        if (colors.Count == 0) return null;
        var isDark = dark ?? type switch
        {
            "light" or "hcLight" => false,
            "dark" or "hc" or "hcDark" or "hc-black" => true,
            _ => !(colors.TryGetValue("editor.background", out var bg) && ParseColor(bg) is { } c && Luma(c) > 0.5),
        };
        return new SavedTheme(label ?? name ?? System.IO.Path.GetFileNameWithoutExtension(path), isDark, colors);

        void Load(string file, int depth)
        {
            if (depth > 4) return;
            using var doc = JsonDocument.Parse(File.ReadAllText(file), Jsonc);
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Object) return;
            if (root.TryGetProperty("include", out var inc) && inc.ValueKind == JsonValueKind.String)
            {
                var parent = System.IO.Path.Combine(System.IO.Path.GetDirectoryName(file) ?? "", inc.GetString()!);
                if (File.Exists(parent)) Load(parent, depth + 1);
            }
            if (root.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String) name = n.GetString();
            if (root.TryGetProperty("type", out var t) && t.ValueKind == JsonValueKind.String) type = t.GetString();
            if (root.TryGetProperty("colors", out var cs) && cs.ValueKind == JsonValueKind.Object)
                foreach (var prop in cs.EnumerateObject())
                    // Kept under the spelling above, so a lookup after a reload finds it.
                    if (KeySet.TryGetValue(prop.Name, out var key) && prop.Value.ValueKind == JsonValueKind.String)
                        colors[key] = prop.Value.GetString()!;
        }
    }

    /// The colour themes VS Code, Cursor, Kiro and Windsurf have installed: the
    /// extensions the user added first, then each editor's own. One per name.
    public static List<InstalledTheme> Installed()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var programs = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        var roots = new (string From, string Dir)[]
        {
            ("VS Code", System.IO.Path.Combine(home, ".vscode", "extensions")),
            ("Cursor", System.IO.Path.Combine(home, ".cursor", "extensions")),
            ("Kiro", System.IO.Path.Combine(home, ".kiro", "extensions")),
            ("Windsurf", System.IO.Path.Combine(home, ".windsurf", "extensions")),
            ("VS Code", System.IO.Path.Combine(local, "Programs", "Microsoft VS Code", "resources", "app", "extensions")),
            ("VS Code", System.IO.Path.Combine(programs, "Microsoft VS Code", "resources", "app", "extensions")),
            ("Cursor", System.IO.Path.Combine(local, "Programs", "cursor", "resources", "app", "extensions")),
            ("Kiro", System.IO.Path.Combine(local, "Programs", "Kiro", "resources", "app", "extensions")),
            ("Windsurf", System.IO.Path.Combine(local, "Programs", "Windsurf", "resources", "app", "extensions")),
        };
        var found = new List<InstalledTheme>();
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (from, dir) in roots)
        {
            if (!Directory.Exists(dir)) continue;
            // Newest version first when an update left the old folder behind.
            foreach (var ext in Directory.GetDirectories(dir).OrderByDescending(d => d, StringComparer.OrdinalIgnoreCase))
            {
                if (System.IO.Path.GetFileName(ext).StartsWith('.')) continue;
                try { ReadExtension(ext, from, found, names); }
                catch (Exception e) { Log.Line($"theme extension {ext} skipped — {e.Message}"); }
            }
        }
        return found.OrderBy(t => t.Label, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    private static void ReadExtension(string ext, string from, List<InstalledTheme> found, HashSet<string> names)
    {
        var manifest = System.IO.Path.Combine(ext, "package.json");
        if (!File.Exists(manifest)) return;
        var text = File.ReadAllText(manifest);
        if (!text.Contains("\"themes\"", StringComparison.Ordinal)) return;
        using var doc = JsonDocument.Parse(text, Jsonc);
        if (!doc.RootElement.TryGetProperty("contributes", out var contributes) ||
            !contributes.TryGetProperty("themes", out var themes) || themes.ValueKind != JsonValueKind.Array) return;
        JsonDocument? nls = null;
        try
        {
            foreach (var t in themes.EnumerateArray())
            {
                if (!t.TryGetProperty("path", out var p) || p.GetString() is not { } rel) continue;
                var label = t.TryGetProperty("label", out var l) ? l.GetString() : null;
                // "%themeLabel%" is a key into the extension's package.nls.json.
                if (label is ['%', .., '%'])
                {
                    var nlsPath = System.IO.Path.Combine(ext, "package.nls.json");
                    nls ??= File.Exists(nlsPath) ? JsonDocument.Parse(File.ReadAllText(nlsPath), Jsonc) : null;
                    label = nls?.RootElement.TryGetProperty(label[1..^1], out var v) == true
                        ? v.ValueKind == JsonValueKind.String ? v.GetString() : v.TryGetProperty("message", out var msg) ? msg.GetString() : null
                        : null;
                }
                var file = System.IO.Path.GetFullPath(System.IO.Path.Combine(ext, rel));
                label ??= System.IO.Path.GetFileNameWithoutExtension(file);
                // Some labels pad with runs of spaces, or of invisible Hangul and
                // Braille fillers, to line up in VS Code's picker.
                foreach (var filler in "\u115F\u1160\u3164\uFFA0\u2800") label = label.Replace(filler, ' ');
                label = string.Join(' ', label.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
                if (!File.Exists(file) || !names.Add(label)) continue;
                var ui = t.TryGetProperty("uiTheme", out var u) ? u.GetString() : null;
                found.Add(new InstalledTheme(label, file, ui is "vs-dark" or "hc-black", from));
            }
        }
        finally { nls?.Dispose(); }
    }

    // MARK: Colour arithmetic

    /// "#rgb", "#rgba", "#rrggbb" or "#rrggbbaa", as VS Code writes them.
    public static uint? ParseColor(string? s)
    {
        if (s is not ['#', .. var hex]) return null;
        if (hex.Length is 3 or 4) hex = string.Concat(hex.Select(ch => $"{ch}{ch}"));
        if (hex.Length == 6) hex += "FF";
        if (hex.Length != 8 || !uint.TryParse(hex, System.Globalization.NumberStyles.HexNumber, null, out var rgba)) return null;
        return (rgba >> 8) | (rgba << 24);
    }

    public static uint Alpha(uint argb, byte a) => (argb & 0x00FFFFFF) | ((uint)a << 24);

    public static uint Mix(uint a, uint b, double k)
    {
        byte Ch(int shift) => (byte)Math.Round(((a >> shift) & 0xFF) + ((((b >> shift) & 0xFF) - (double)((a >> shift) & 0xFF)) * k));
        return (uint)(Ch(24) << 24 | Ch(16) << 16 | Ch(8) << 8 | Ch(0));
    }

    /// A see-through colour laid over an opaque one, so what is drawn is predictable.
    public static uint Over(uint top, uint under) => Alpha(Mix(under, top | 0xFF000000, (top >> 24) / 255.0), 0xFF);

    /// Relative brightness, 0 to 1, weighted as the eye sees it.
    public static double Luma(uint argb) =>
        (0.2126 * ((argb >> 16) & 0xFF) + 0.7152 * ((argb >> 8) & 0xFF) + 0.0722 * (argb & 0xFF)) / 255;

    /// How far from grey, 0 to 1 (HSV saturation).
    public static double Saturation(uint argb)
    {
        int r = (int)((argb >> 16) & 0xFF), g = (int)((argb >> 8) & 0xFF), b = (int)(argb & 0xFF);
        var max = Math.Max(r, Math.Max(g, b));
        return max == 0 ? 0 : (max - Math.Min(r, Math.Min(g, b))) / (double)max;
    }
}
