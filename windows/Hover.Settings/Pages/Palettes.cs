using System.Globalization;
using System.Text.Json;

namespace Hover.Settings;

/// <summary>Every colour Settings and the app window draw with, as 0xAARRGGBB (<c>core.Palette</c>).</summary>
public sealed record AppPalette
{
    public string Name { get; init; } = "Hover";
    public bool Dark { get; init; }
    public uint Ink, InkDim, InkFaint, Fill, Wash, WashStrong, Separator, Surface, Sheet, SheetEdge, Panel, PanelEdge;
    public uint Thumb, RowHover, SwitchOff, Handle, Blue, Green, Purple, Yellow, Teal, Orange, Red;
}

public static class Palettes
{
    /// <summary>Apple's dark system colours: label, secondaryLabel, tertiaryLabel; fills; separator; grouped backgrounds.</summary>
    public static readonly AppPalette HoverDark = new()
    {
        Name = "Hover", Dark = true,
        Ink = 0xFFFFFFFF, InkDim = 0x99EBEBF5, InkFaint = 0x4DEBEBF5,
        Fill = 0x52787880, Wash = 0x3D767680, WashStrong = 0x5C787880, Separator = 0x99545458,
        Surface = 0xFF1C1C1E, Sheet = 0xFF2C2C2E, SheetEdge = 0x1FFFFFFF, Panel = 0xFF000000, PanelEdge = 0x1AFFFFFF,
        Thumb = 0xFF636366, RowHover = 0x14FFFFFF, SwitchOff = 0xFF39393D, Handle = 0x66FFFFFF,
        Blue = 0xFF0A84FF, Green = 0xFF30D158, Purple = 0xFFBF5AF2, Yellow = 0xFFFFD60A, Teal = 0xFF64D2FF, Orange = 0xFFFF9F0A, Red = 0xFFFF453A,
    };

    public static readonly AppPalette HoverLight = new()
    {
        Name = "Hover", Dark = false,
        Ink = 0xFF000000, InkDim = 0x993C3C43, InkFaint = 0x4D3C3C43,
        Fill = 0x29787880, Wash = 0x1F767680, WashStrong = 0x33787880, Separator = 0x4A3C3C43,
        Surface = 0xFFFFFFFF, Sheet = 0xFFFFFFFF, SheetEdge = 0x1A000000, Panel = 0xFFF2F2F7, PanelEdge = 0x1A000000,
        Thumb = 0xFFFFFFFF, RowHover = 0x0D000000, SwitchOff = 0xFFE9E9EB, Handle = 0x40000000,
        Blue = 0xFF007AFF, Green = 0xFF34C759, Purple = 0xFFAF52DE, Yellow = 0xFFFFCC00, Teal = 0xFF32ADE6, Orange = 0xFFFF9500, Red = 0xFFFF3B30,
    };

    public static AppPalette Hover(bool dark) => dark ? HoverDark : HoverLight;

    public static TilePalette Tile(AppPalette p) => new(p.Panel, p.Surface, p.Blue, p.Ink, p.InkDim, p.Green, p.Orange, p.Red, p.Purple, p.Teal);

    /// <summary>A palette the backend wrote (<c>palette</c> and <c>themeRead</c>): colours as "#AARRGGBB". Null when a colour is missing or malformed.</summary>
    public static AppPalette? Parse(JsonElement p)
    {
        if (p.ValueKind != JsonValueKind.Object) return null;
        bool ok = true;
        uint C(string name)
        {
            if (p.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String && v.GetString() is { Length: 9 } s && s[0] == '#'
                && uint.TryParse(s.AsSpan(1), NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var n)) return n;
            ok = false;
            return 0;
        }
        var r = new AppPalette
        {
            Name = p.TryGetProperty("name", out var nm) && nm.ValueKind == JsonValueKind.String ? nm.GetString()! : "",
            Dark = p.TryGetProperty("dark", out var d) && d.ValueKind == JsonValueKind.True,
            Ink = C("ink"), InkDim = C("inkDim"), InkFaint = C("inkFaint"), Fill = C("fill"), Wash = C("wash"), WashStrong = C("washStrong"),
            Separator = C("separator"), Surface = C("surface"), Sheet = C("sheet"), SheetEdge = C("sheetEdge"), Panel = C("panel"), PanelEdge = C("panelEdge"),
            Thumb = C("thumb"), RowHover = C("rowHover"), SwitchOff = C("switchOff"), Handle = C("handle"),
            Blue = C("blue"), Green = C("green"), Purple = C("purple"), Yellow = C("yellow"), Teal = C("teal"), Orange = C("orange"), Red = C("red"),
        };
        return ok ? r : null;
    }
}
