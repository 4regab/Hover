using Vortice.Direct2D1;
using Vortice.Direct3D;
using Vortice.Direct3D11;
using Vortice.DirectWrite;
using Vortice.DXGI;
using Vortice.WIC;
using Xunit;
using Xunit.Abstractions;
using FeatureLevel = Vortice.Direct3D.FeatureLevel;
using PixelFormat = Vortice.WIC.PixelFormat;

namespace Hover.Voice.Tests;

/// <summary>A software Direct3D 11 device with Direct2D and DirectWrite on it, as the app has, and the card's painter.</summary>
public sealed class CardGfx : IDisposable
{
    public ID3D11Device Device { get; }
    public ID2D1Factory1 Factory { get; }
    public ID2D1DeviceContext Context { get; }
    public IDWriteFactory Write { get; }
    public VoiceCardPainter Painter { get; }
    public string Adapter { get; }

    public CardGfx()
    {
        if (!OperatingSystem.IsWindows())
        {
            Device = null!;
            Factory = null!;
            Context = null!;
            Write = null!;
            Painter = null!;
            Adapter = "none";
            return;
        }
        var levels = new[] { FeatureLevel.Level_11_1, FeatureLevel.Level_11_0, FeatureLevel.Level_10_1, FeatureLevel.Level_10_0 };
        var hr = D3D11.D3D11CreateDevice((IDXGIAdapter?)null, DriverType.Hardware, DeviceCreationFlags.BgraSupport, levels, out var device);
        Adapter = "hardware";
        if (hr.Failure || device is null)
        {
            Adapter = "WARP";
            D3D11.D3D11CreateDevice((IDXGIAdapter?)null, DriverType.Warp, DeviceCreationFlags.BgraSupport, levels, out device).CheckError();
        }
        Device = device!;
        using var dxgi = Device.QueryInterface<IDXGIDevice>();
        Factory = D2D1.D2D1CreateFactory<ID2D1Factory1>();
        using var d2dDevice = Factory.CreateDevice(dxgi);
        Context = d2dDevice.CreateDeviceContext(DeviceContextOptions.None);
        Write = DWrite.DWriteCreateFactory<IDWriteFactory>();
        var fonts = Path.Combine(AppContext.BaseDirectory, "Assets");
        Painter = new VoiceCardPainter(Write, VoiceCardPainter.LoadInter(Write, Path.Combine(fonts, "Fonts")), Factory, Path.Combine(fonts, "Office"));
    }

    public void Dispose()
    {
        Painter?.Dispose();
        Context?.Dispose();
        Factory?.Dispose();
        Write?.Dispose();
        Device?.Dispose();
    }
}

/// <summary>Writes opaque BGRA pixels as a JPEG file with Windows' imaging component.</summary>
static class JpegFile
{
    public static void Write(string file, byte[] bgra, int width, int height)
    {
        var bgr = new byte[width * height * 3];
        for (var i = 0; i < width * height; i++)
        {
            bgr[i * 3] = bgra[i * 4];
            bgr[i * 3 + 1] = bgra[i * 4 + 1];
            bgr[i * 3 + 2] = bgra[i * 4 + 2];
        }
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        using var factory = new IWICImagingFactory();
        using var stream = File.Create(file);
        using var encoder = factory.CreateEncoder(ContainerFormat.Jpeg);
        encoder.Initialize(stream, BitmapEncoderCacheOption.NoCache);
        using var frame = encoder.CreateNewFrame(out var bag);
        frame.Initialize(bag);
        frame.SetSize((uint)width, (uint)height);
        var format = PixelFormat.Format24bppBGR;
        frame.SetPixelFormat(ref format);
        unsafe
        {
            fixed (byte* p = bgr) frame.WritePixels((uint)height, (uint)(width * 3), (uint)bgr.Length, p);
        }
        frame.Commit();
        encoder.Commit();
    }

    /// <summary>The size a JPEG says it has (its start-of-frame marker).</summary>
    public static (int Width, int Height) Size(string file)
    {
        var j = File.ReadAllBytes(file);
        if (j.Length < 4 || j[0] != 0xFF || j[1] != 0xD8) throw new InvalidDataException(file + " is not a JPEG");
        for (var i = 2; i + 9 < j.Length;)
        {
            if (j[i] != 0xFF) { i++; continue; }
            var m = j[i + 1];
            if (m is >= 0xC0 and <= 0xCF and not 0xC4 and not 0xC8 and not 0xCC) return (j[i + 7] << 8 | j[i + 8], j[i + 5] << 8 | j[i + 6]);
            i += 2 + (j[i + 2] << 8 | j[i + 3]);
        }
        throw new InvalidDataException("no frame in " + file);
    }
}

/// <summary>The card drawn: every picture of the Go set, its sizes, where its buttons are, and odd inputs.</summary>
public sealed class RenderTests(CardGfx gfx, ITestOutputHelper output) : IClassFixture<CardGfx>
{
    static string Data() => Path.Combine(Path.GetTempPath(), "hover-voice-shots-1234");

    static string Out()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d is not null; d = d.Parent)
        {
            if (!File.Exists(Path.Combine(d.FullName, "go.mod"))) continue;
            var dir = Path.Combine(d.FullName, "out", "voice");
            Directory.CreateDirectory(dir);
            return dir;
        }
        var fallback = Path.Combine(Path.GetTempPath(), "hover-voice-pictures");
        Directory.CreateDirectory(fallback);
        return fallback;
    }

    [Fact]
    public void The_pictures_have_the_names_of_the_go_pictures()
    {
        var go = File.ReadAllLines(Path.Combine(AppContext.BaseDirectory, "GoVoicePictures.txt")).Where(l => l.Length > 0).ToHashSet();
        var mine = CardFixtures.All(Data()).Select(s => s.Name).ToList();
        Assert.Equal(mine.Count, mine.Distinct().Count());
        Assert.Equal(82, go.Count);
        Assert.Empty(go.Except(mine));
        Assert.Empty(mine.Except(go));
    }

    [WindowsFact]
    public void Every_picture_is_drawn_and_kept_as_a_jpeg_no_wider_than_1400()
    {
        var dir = Out();
        var count = 0;
        var problems = new List<string>();
        foreach (var spec in CardFixtures.All(Data()))
        {
            try
            {
                var ui = new CardUi { Now = 0.35 };
                spec.Setup?.Invoke(ui);
                var openH = spec.Height > 200 ? spec.Height - 40 : 440;
                var (bgra, w, h) = CardShot.Render(gfx.Factory, gfx.Context, gfx.Painter, spec.View, ui, spec.Width, spec.Height, spec.Scale, openH);
                if (w > CardShot.MaxWidth) problems.Add($"{spec.Name} is {w} wide");
                // Something was drawn: more than the desktop colour.
                int b0 = (int)(CardShot.Desk & 0xff), g0 = (int)((CardShot.Desk >> 8) & 0xff), r0 = (int)((CardShot.Desk >> 16) & 0xff);
                var other = 0;
                for (var i = 0; i < w * h; i++)
                    if (bgra[i * 4] != b0 || bgra[i * 4 + 1] != g0 || bgra[i * 4 + 2] != r0) other++;
                if (other <= 2000) problems.Add($"{spec.Name} drew {other} pixels");
                var file = Path.Combine(dir, spec.Name + ".jpg");
                JpegFile.Write(file, bgra, w, h);
                if (JpegFile.Size(file) != (w, h)) problems.Add($"{spec.Name}: the file is not {w} x {h}");
                count++;
            }
            catch (Exception e)
            {
                var text = e.ToString();
                problems.Add($"{spec.Name}: {text[..Math.Min(text.Length, 900)]}");
            }
        }
        output.WriteLine($"{count} pictures in {dir} ({gfx.Adapter})");
        Assert.True(problems.Count == 0, string.Join("\n", problems.Take(6)));
        Assert.Equal(82, count);
    }

    (double W, double H) Measure(VoiceView view, CardUi? ui = null, double openH = 440) => gfx.Painter.Measure(gfx.Context, view, ui ?? new CardUi(), openH);

    static VoiceView Of(string name) => CardFixtures.All(Data()).First(s => s.Name == name).View;

    [WindowsFact]
    public void The_cards_have_the_sizes_the_go_card_has()
    {
        // Listening and working: only the aura, 24 + 66 + 24 wide and 74 high.
        Assert.Equal((114.0, 74.0), Measure(Of("voice-default-listening")));
        Assert.Equal((114.0, 74.0), Measure(Of("voice-default-transcribing")));
        // With the screenshot note it grows by the icon, a gap and the words.
        var note = Measure(Of("voice-default-screenshot-note-2x"));
        Assert.True(note.W > 114 + 10 + 13 + 6 + 40 && note.W <= 440, $"{note.W}");
        Assert.Equal(74, note.H);
        // Started and cancelled: 34 high, 330 wide at the most.
        foreach (var name in new[] { "voice-default-started", "voice-default-cancelled" })
        {
            var (w, h) = Measure(Of(name));
            Assert.Equal(34, h);
            Assert.InRange(w, 100, 330);
        }
        // An error is 420 wide.
        var error = Measure(Of("voice-default-error"));
        Assert.Equal(420, error.W);
        Assert.True(error.H >= 58, $"{error.H}");
        // The preview and the choice of an agent are 460 wide.
        foreach (var name in new[] { "voice-default-preview", "voice-default-editing", "voice-default-choose-agent", "voice-default-preview-kiro-cloud" })
            Assert.Equal(460, Measure(Of(name)).W);
    }

    [WindowsFact]
    public void A_preview_never_grows_past_the_window_whatever_the_menu_and_the_task_hold()
    {
        foreach (var (size, open) in new[] { ("small", 340.0), ("default", 440.0), ("large", 520.0), ("extra-large", 600.0) })
        {
            var (w, h) = Measure(Of($"voice-{size}-menu-long"), openH: open);
            Assert.Equal(460, w);
            Assert.True(h <= open, $"{size}: the card is {h} high in a window {open} high");
        }
    }

    [WindowsFact]
    public void The_buttons_of_a_drawn_card_answer_the_actions_the_card_asks_for()
    {
        var ui = new CardUi { Now = 0.35 };
        gfx.Painter.Draw(gfx.Context, Of("voice-default-preview"), ui, 0, 0, 440);
        var acts = gfx.Painter.Targets.Select(t => t.Act.Name + (t.Act.Name == "openMenu" ? t.Act.N : "")).ToList();
        output.WriteLine(string.Join(" ", acts));
        foreach (var expected in new[] { "start", "cancel", "openMenu3", "openMenu1", "openMenu2" }) Assert.Contains(expected, acts);
        Assert.DoesNotContain("toggleCloud", acts);
        Assert.Contains(gfx.Painter.Targets, t => t.Kind == "task");
        // Start is the wide button, and Cancel sits to its right.
        var start = gfx.Painter.Targets.First(t => t.Act.Name == "start");
        var cancel = gfx.Painter.Targets.First(t => t.Act.Name == "cancel");
        Assert.True(start.W > cancel.W && cancel.X > start.X + start.W);
        Assert.Equal(28, start.H);

        // In Kiro Web: the cloud switch and the repository pick, no folder pick.
        gfx.Painter.Draw(gfx.Context, Of("voice-default-preview-kiro-cloud"), ui, 0, 0, 440);
        acts = gfx.Painter.Targets.Select(t => t.Act.Name + (t.Act.Name == "openMenu" ? t.Act.N : "")).ToList();
        Assert.Contains("toggleCloud", acts);
        Assert.Contains("openMenu4", acts);
        Assert.DoesNotContain("openMenu3", acts);

        // An error with Retry has Retry and Esc; the setup error has Settings.
        gfx.Painter.Draw(gfx.Context, Of("voice-default-error"), ui, 0, 0, 440);
        Assert.Contains(gfx.Painter.Targets, t => t.Act.Name == "retry");
        Assert.Contains(gfx.Painter.Targets, t => t.Act.Name == "cancel");
        gfx.Painter.Draw(gfx.Context, Of("voice-default-error-setup"), ui, 0, 0, 440);
        Assert.Contains(gfx.Painter.Targets, t => t.Act.Name == "settings");

        // The choice of an agent: a pick for each, and Settings when there is none.
        gfx.Painter.Draw(gfx.Context, Of("voice-default-choose-agent"), ui, 0, 0, 440);
        Assert.Equal(["codex", "opencode"], gfx.Painter.Targets.Where(t => t.Act.Name == "pick").Select(t => t.Act.S));

        // The open agent menu: a row for each ready agent.
        gfx.Painter.Draw(gfx.Context, Of("voice-default-menu-long"), ui, 0, 0, 440);
        Assert.Equal(5, gfx.Painter.Targets.Count(t => t.Act.Name == "pick"));
        Assert.Contains(gfx.Painter.Targets, t => t.Kind == "menu");
    }

    [WindowsFact]
    public void Odd_inputs_are_drawn_without_a_crash_and_inside_the_window()
    {
        var card = Of("voice-default-preview").Card;
        var odd = new[]
        {
            card with { Task = new string('x', 5000), Heard = new string('h', 5000), Note = new string('n', 3000) },
            card with { Task = "", Heard = "", Target = "", Folder = "", Agent = "", Model = "", Letter = "", Start = "" },
            card with { Target = new string('T', 300), Folder = new string('F', 300), Model = new string('M', 300), Agent = new string('A', 100), Repo = new string('R', 300), Cloud = true, CloudShown = true },
            card with { Task = "line\nbreaks\n\nand\ttabs\n" + string.Concat(Enumerable.Repeat("ü ", 300)) },
            card with { Tool = "no-such-tool", Letter = "\U0001F600", Tint = 0xffffff },
        };
        foreach (var c in odd)
        {
            var view = Of("voice-default-preview") with { Card = c, Shots = Enumerable.Range(0, 12).Select(i => "s" + i).ToList() };
            var ui = new CardUi { Now = 1 };
            var (w, h) = Measure(view, ui);
            Assert.Equal(460, w);
            Assert.True(h <= 440 && h > 0, $"{h}");
            gfx.Painter.Draw(gfx.Context, view, ui, 20, 0, 440);
        }
        var many = Of("voice-default-menu-long") with
        {
            Tools = Enumerable.Range(0, 300).Select(i => new CardTool("kiro", "Agent " + i)).ToList(),
        };
        var (_, manyH) = Measure(many);
        Assert.True(manyH <= 440, $"{manyH}");
        var error = Of("voice-default-error");
        var longError = error with { Card = error.Card with { Words = new string('w', 4000), Sub = new string('s', 4000) } };
        var (ew, eh) = Measure(longError);
        Assert.Equal(420, ew);
        Assert.True(eh > 0);
    }

    [Fact]
    public void The_shrink_rules_are_slints_the_row_gives_its_cut_items_and_the_stack_gives_equally()
    {
        // A row of 600 in 500: only the pick that can be cut (index 0 and 2) give, the same each.
        double[] widths = [300, 100, 200];
        VoiceCardPainter.ShrinkRow(widths, [100, 100, 100], [true, false, true], 500);
        Assert.Equal([250, 100, 150], widths);
        // When the cut items run out, the rest share what is left.
        widths = [200, 100, 200];
        VoiceCardPainter.ShrinkRow(widths, [190, 20, 190], [true, false, true], 400);
        Assert.Equal([190, 20, 190], widths);
        // Never below the least, even when the room is too small for all of them.
        widths = [200, 100, 200];
        VoiceCardPainter.ShrinkRow(widths, [190, 90, 190], [true, false, true], 100);
        Assert.Equal([190, 90, 190], widths);
        // A row that fits is left alone.
        widths = [100, 100];
        VoiceCardPainter.ShrinkRow(widths, [10, 10], [true, true], 300);
        Assert.Equal([100, 100], widths);

        // Two stacked boxes give the same amount until one is at its least, then the other goes on alone.
        double a = 168, b = 98;
        var took = VoiceCardPainter.ShrinkEqually(100, ref a, 64, true, ref b, 52);
        Assert.Equal(100, took);
        Assert.Equal(114, a);
        Assert.Equal(52, b);
        a = 70; b = 98;
        took = VoiceCardPainter.ShrinkEqually(100, ref a, 64, true, ref b, 52);
        Assert.Equal(6 + 46, took);
        Assert.Equal(64, a);
        Assert.Equal(52, b);
        // Without the second box only the first gives.
        a = 168; b = 98;
        took = VoiceCardPainter.ShrinkEqually(30, ref a, 64, false, ref b, 52);
        Assert.Equal(30, took);
        Assert.Equal(138, a);
        Assert.Equal(98, b);
    }
}
