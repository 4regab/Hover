using System.Diagnostics;
using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Text.RegularExpressions;
using Hover.Host;
using Xunit;
using Xunit.Abstractions;

namespace Hover.Tests;

/// <summary>A copy of part of the desktop as Windows shows it (DWM's composed picture, the notch's DirectComposition visuals included).</summary>
[SupportedOSPlatform("windows")]
sealed class DesktopPicture
{
    [StructLayout(LayoutKind.Sequential)]
    struct BITMAPINFO
    {
        public uint Size;
        public int Width, Height;
        public ushort Planes, BitCount;
        public uint Compression, SizeImage;
        public int XPels, YPels;
        public uint Used, Important, Colour;
    }

    [DllImport("user32.dll")] static extern nint GetDC(nint hwnd);
    [DllImport("user32.dll")] static extern int ReleaseDC(nint hwnd, nint dc);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("gdi32.dll")] static extern nint CreateCompatibleDC(nint dc);
    [DllImport("gdi32.dll")] static extern nint CreateCompatibleBitmap(nint dc, int w, int h);
    [DllImport("gdi32.dll")] static extern nint SelectObject(nint dc, nint obj);
    [DllImport("gdi32.dll")] static extern bool BitBlt(nint dest, int x, int y, int w, int h, nint source, int sx, int sy, uint rop);
    [DllImport("gdi32.dll")] static extern int GetDIBits(nint dc, nint bitmap, uint start, uint lines, byte[] bits, ref BITMAPINFO info, uint usage);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(nint obj);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(nint dc);

    readonly byte[] bgra;

    DesktopPicture(int left, int top, int width, int height, byte[] bgra)
    {
        Left = left;
        Top = top;
        Width = width;
        Height = height;
        this.bgra = bgra;
    }

    /// <summary>The place of the copy on the screen, in device pixels.</summary>
    public int Left { get; }
    public int Top { get; }
    public int Width { get; }
    public int Height { get; }

    /// <summary>Copies a rectangle of the main screen (cut to the screen).</summary>
    public static DesktopPicture Take(int left, int top, int width, int height)
    {
        int screenW = GetSystemMetrics(0), screenH = GetSystemMetrics(1);
        int x0 = Math.Max(left, 0), y0 = Math.Max(top, 0), x1 = Math.Min(left + width, screenW), y1 = Math.Min(top + height, screenH);
        int w = Math.Max(x1 - x0, 1), h = Math.Max(y1 - y0, 1);
        var screen = GetDC(0);
        var memory = CreateCompatibleDC(screen);
        var bitmap = CreateCompatibleBitmap(screen, w, h);
        var old = SelectObject(memory, bitmap);
        try
        {
            // 0x00CC0020 copies, 0x40000000 includes layered windows (the notch is one).
            if (!BitBlt(memory, 0, 0, w, h, screen, x0, y0, 0x00CC0020 | 0x40000000)) throw new InvalidOperationException("BitBlt of the screen failed");
            var info = new BITMAPINFO { Size = 40, Width = w, Height = -h, Planes = 1, BitCount = 32 };
            var bits = new byte[w * h * 4];
            SelectObject(memory, old);
            if (GetDIBits(memory, bitmap, 0, (uint)h, bits, ref info, 0) == 0) throw new InvalidOperationException("GetDIBits failed");
            return new DesktopPicture(x0, y0, w, h, bits);
        }
        finally
        {
            DeleteObject(bitmap);
            DeleteDC(memory);
            ReleaseDC(0, screen);
        }
    }

    /// <summary>The colour at a place of the screen (not of the copy).</summary>
    public (byte R, byte G, byte B) At(int x, int y)
    {
        int cx = Math.Clamp(x - Left, 0, Width - 1), cy = Math.Clamp(y - Top, 0, Height - 1);
        var i = (cy * Width + cx) * 4;
        return (bgra[i + 2], bgra[i + 1], bgra[i]);
    }

    /// <summary>How many pixels of a rectangle of the screen are this colour, each channel within a tolerance.</summary>
    public int CountNear(double left, double top, double right, double bottom, (byte R, byte G, byte B) colour, int tolerance)
    {
        var n = 0;
        for (var y = (int)Math.Floor(top); y < (int)Math.Ceiling(bottom); y++)
        for (var x = (int)Math.Floor(left); x < (int)Math.Ceiling(right); x++)
        {
            if (x < Left || y < Top || x >= Left + Width || y >= Top + Height) continue;
            var (r, g, b) = At(x, y);
            if (Math.Abs(r - colour.R) <= tolerance && Math.Abs(g - colour.G) <= tolerance && Math.Abs(b - colour.B) <= tolerance) n++;
        }
        return n;
    }

    /// <summary>The copy as a PNG file, for the report.</summary>
    public void SavePng(string file)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        var raw = new byte[(Width * 3 + 1) * Height];
        for (var y = 0; y < Height; y++)
        {
            var row = y * (Width * 3 + 1);
            for (var x = 0; x < Width; x++)
            {
                var i = (y * Width + x) * 4;
                raw[row + 1 + x * 3] = bgra[i + 2];
                raw[row + 2 + x * 3] = bgra[i + 1];
                raw[row + 3 + x * 3] = bgra[i];
            }
        }
        using var packed = new MemoryStream();
        using (var z = new ZLibStream(packed, CompressionLevel.Fastest, leaveOpen: true)) z.Write(raw);
        using var output = File.Create(file);
        output.Write([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        var header = new byte[13];
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(header.AsSpan(0), Width);
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(header.AsSpan(4), Height);
        header[8] = 8;
        header[9] = 2;
        Chunk(output, "IHDR", header);
        Chunk(output, "IDAT", packed.ToArray());
        Chunk(output, "IEND", []);
    }

    static void Chunk(Stream output, string kind, byte[] data)
    {
        var length = new byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteInt32BigEndian(length, data.Length);
        output.Write(length);
        var body = Encoding.ASCII.GetBytes(kind).Concat(data).ToArray();
        output.Write(body);
        var crc = 0xFFFFFFFFu;
        foreach (var b in body)
        {
            crc ^= b;
            for (var k = 0; k < 8; k++) crc = (crc & 1) != 0 ? (crc >> 1) ^ 0xEDB88320u : crc >> 1;
        }
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32BigEndian(length, ~crc);
        output.Write(length);
    }
}

/// <summary>
/// What the open notch really puts on the screen. The panels are a DirectComposition visual of their own, over the
/// office's; the checks that click them read where they are from the app's dump and never look at the pixels, and the
/// pictures paste the panels onto the office in software. This one copies the screen and looks for the panels' own colours.
/// </summary>
[SupportedOSPlatform("windows")]
public class NotchScreenTests(ITestOutputHelper output)
{
    // Pen.Lilac, the colour of the new-task button's dot and of the active icon of the office/chat switch.
    static readonly (byte R, byte G, byte B) Lilac = (0xC4, 0xA2, 0xFF);

    static string ReadLog(string dataDir)
    {
        var file = DataFolder.LogFile(dataDir);
        if (!File.Exists(file)) return "";
        using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        return new StreamReader(stream).ReadToEnd();
    }

    static int Count(string log, string text) => Regex.Matches(log, Regex.Escape(text)).Count;

    static async Task<bool> Until(Func<bool> condition, int milliseconds)
    {
        for (var end = Environment.TickCount64 + milliseconds; Environment.TickCount64 < end;)
        {
            if (condition()) return true;
            await Task.Delay(50);
        }
        return condition();
    }

    /// <summary>The folder the report's files go to (out\notch-live beside build.ps1, which CI uploads).</summary>
    static string OutFolder()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
            if (File.Exists(Path.Combine(d.FullName, "build.ps1"))) return Path.Combine(d.FullName, "out", "notch-live");
        return Path.Combine(Path.GetTempPath(), "hover-notch-live");
    }

    [AppFact]
    public async Task The_open_notch_shows_the_panels_over_the_office_on_the_screen_each_time_it_opens()
    {
        using var sandbox = new Sandbox();
        Directory.CreateDirectory(sandbox.Data);
        File.WriteAllText(Path.Combine(sandbox.Data, "settings.json"), """{"KiroNoticeSeen": true, "HoverOpensWorkspace": false, "NotchItems": ["claude"]}""");
        var dumpFile = Path.Combine(sandbox.Root, "ui-dump.json");
        var dump = new AppDump(dumpFile);
        var info = new ProcessStartInfo(TestEnvironment.AppExe!)
        {
            UseShellExecute = false, WorkingDirectory = Path.GetDirectoryName(TestEnvironment.AppExe!)!,
        };
        foreach (var (name, value) in sandbox.Environment)
            if (value is not null) info.Environment[name] = value;
        info.Environment["HOVER_DATA_DIR"] = sandbox.Data;
        info.Environment["HOVER_UI_DUMP"] = dumpFile;
        var report = OutFolder();
        Directory.CreateDirectory(report);
        Desktop.MovePointer(5, 5);
        using var app = Process.Start(info)!;
        var problems = new List<string>();

        string? SavedTime() => System.Text.Json.JsonSerializer.Deserialize<Dictionary<string, string>>(
            File.Exists(Path.Combine(sandbox.Data, "office.json")) ? File.ReadAllText(Path.Combine(sandbox.Data, "office.json")) : "{}")!.GetValueOrDefault("time");

        // The menu button opens the office menu: three time-of-day icons and the rows, and a pick of the time is kept in office.json.
        async Task MenuChecks(int round)
        {
            if (dump.Where("notch", "topMenu") is not { } button) { problems.Add($"round {round}: no menu button to click"); return; }
            Input.Click(button.X, button.Y);
            var opened = await Until(() => (string?)dump.Section("notch")?["menu"] == "top" && dump.Where("notch", "menuTime") is not null, 5000);
            await Task.Delay(800);
            var rect = Desktop.Rect(Desktop.Notch());
            DesktopPicture.Take(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top).SavePng(Path.Combine(report, "menu-open.png"));
            var targets = AppDump.Items(dump.Section("notch"), "targets").ToList();
            if (!opened) { problems.Add("the menu button did not open the menu"); return; }
            foreach (var row in new[] { "Chill beats", "Session history", "Settings" })
                if (!targets.Any(t => (string?)t["id"] == "menu" && (string?)t["s"] == row)) problems.Add($"the office menu has no '{row}' row");
            if (targets.Count(t => (string?)t["id"] == "menuTime") != 3) problems.Add("the office menu does not have three time-of-day icons");

            // Night, day, then the clock again: each is kept as the Go app keeps it ("night", "day", "").
            foreach (var (n, saved) in new[] { (1, "night"), (2, "day"), (0, "") })
            {
                if (dump.Where("notch", "menuTime", t => (int)t["n"]! == n) is not { } icon) { problems.Add($"no time-of-day icon {n}"); continue; }
                Input.Click(icon.X, icon.Y);
                if (!await Until(() => SavedTime() == saved, 5000)) problems.Add($"the time-of-day icon {n} did not save '{saved}' in office.json (it has '{SavedTime()}')");
            }

            // The button again puts the menu away.
            if (dump.Where("notch", "topMenu") is { } again) Input.Click(again.X, again.Y);
            if (!await Until(() => (string?)dump.Section("notch")?["menu"] is null, 5000)) problems.Add("the menu button did not close the menu");
            Desktop.MovePointer(5, 5);
            await Task.Delay(500);
        }

        try
        {
            await AppTests.WaitForLog(sandbox.Data, "hotkey Alt+N registered", TimeSpan.FromSeconds(60), app);
            await Task.Delay(1500);
            output.WriteLine($"the screen is {Desktop.WorkArea()} at scale {Desktop.Scale()}");
            var notch = Desktop.Notch();
            Assert.True(notch != 0, "no window of class HoverNotch");

            // Opened and folded three times: the office's view is taken away and put back each time.
            for (var round = 1; round <= 3; round++)
            {
                Desktop.AltN();
                var opened = await Until(() => Count(ReadLog(sandbox.Data), "panels: open in the notch") >= round, 15_000);
                // The panels' dump is written when they draw; the animation is over and the picture settled a moment later.
                await Task.Delay(2500);
                var log = ReadLog(sandbox.Data);
                var section = dump.Section("notch");
                var rect = Desktop.Rect(notch);
                var picture = DesktopPicture.Take(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top);
                picture.SavePng(Path.Combine(report, $"open-{round}.png"));
                File.WriteAllText(Path.Combine(report, "hover.log"), log);
                if (section is null) { problems.Add($"round {round}: the panels wrote no 'notch' part of the dump (opened: {opened})"); }
                else
                {
                    double left = (double)section["left"]!, top = (double)section["top"]!, scale = (double)section["scale"]!;
                    var targets = AppDump.Items(section, "targets").ToList();

                    // The new-task button's dot is a solid disc of one colour, 6 DIPs wide, that nothing in the office has.
                    var fab = targets.LastOrDefault(t => (string?)t["id"] == "fab");
                    if (fab is null) problems.Add($"round {round}: no new-task button in the dump");
                    else
                    {
                        double x = (double)fab["x"]!, y = (double)fab["y"]!, w = (double)fab["w"]!;
                        var dot = picture.CountNear(left + (x + w - 8) * scale, top + (y + 1) * scale, left + (x + w - 2) * scale, top + (y + 7) * scale, Lilac, 8);
                        output.WriteLine($"round {round}: {dot} pixels of the new-task button's dot are lilac (a full dot is about {Math.PI * 9 * scale * scale:0})");
                        if (dot < 12) problems.Add($"round {round}: the new-task button's dot is not on the screen ({dot} lilac pixels)");
                    }

                    // The menu button: dark glass with three light bars. The bars are the ink at 62 % over what the glass shows there.
                    var menuButton = targets.LastOrDefault(t => (string?)t["id"] == "topMenu");
                    if (menuButton is null) problems.Add($"round {round}: no menu button in the dump");
                    else
                    {
                        double x = (double)menuButton["x"]!, y = (double)menuButton["y"]!, w = (double)menuButton["w"]!, h = (double)menuButton["h"]!;
                        var back = picture.At((int)Math.Round(left + (x + 2) * scale), (int)Math.Round(top + (y + 2) * scale));
                        // The bars are about 1.5 pixels thick, so most of their pixels are part covered: any pixel clearly lighter than the glass counts.
                        var glass = back.R + back.G + back.B;
                        var bars = 0;
                        for (var py = (int)Math.Floor(top + y * scale); py < (int)Math.Ceiling(top + (y + h) * scale); py++)
                        for (var px = (int)Math.Floor(left + x * scale); px < (int)Math.Ceiling(left + (x + w) * scale); px++)
                        {
                            var (r, g, b) = picture.At(px, py);
                            if (r + g + b >= glass + 120) bars++;
                        }
                        output.WriteLine($"round {round}: the menu button's glass is {back}; {bars} pixels of its box are clearly lighter (its bars)");
                        if (bars < 15 || Math.Max(back.R, Math.Max(back.G, back.B)) > 100)
                            problems.Add($"round {round}: the menu button is not on the screen ({bars} pixels of its bars, glass {back})");
                    }

                    // The office/chat switch: its active icon is drawn in the same lilac.
                    var cell = targets.FirstOrDefault(t => (string?)t["id"] == "view" && (int)t["n"]! == 0);
                    if (cell is null) problems.Add($"round {round}: no office/chat switch in the dump");
                    else
                    {
                        double x = (double)cell["x"]!, y = (double)cell["y"]!, w = (double)cell["w"]!, h = (double)cell["h"]!;
                        var icon = picture.CountNear(left + x * scale, top + y * scale, left + (x + w) * scale, top + (y + h) * scale, Lilac, 8);
                        output.WriteLine($"round {round}: {icon} pixels of the office/chat switch are lilac");
                        if (icon < 6) problems.Add($"round {round}: the office/chat switch is not on the screen ({icon} lilac pixels)");
                    }
                }

                if (round == 1) await MenuChecks(round);

                // Folded again, and long enough for the animation to end.
                Desktop.AltN();
                await Until(() => Count(ReadLog(sandbox.Data), "notch: folding") >= round, 10_000);
                await Task.Delay(1500);
            }
        }
        finally
        {
            try { if (!app.HasExited) app.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) { }
            File.WriteAllText(Path.Combine(report, "hover.log"), ReadLog(sandbox.Data));
            output.WriteLine(string.Join("\n", ReadLog(sandbox.Data).Split('\n').Where(l => l.Contains("panels:") || l.Contains("office:") || l.Contains("notch:") || l.Contains("layer"))));
        }
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }
}
