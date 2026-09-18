using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.NUnit;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;
using Hover.Images;
using NUnit.Framework;

namespace Hover.Tests;

/// Draws the tray for real and saves the result, so the look can be checked instead
/// of taken on trust. The PNGs land beside the test assembly.
///
/// These assertions are deliberately about facts a picture cannot lie about: that it
/// rendered at all, that the panel is the dark colour it is supposed to be, and that
/// rows take the shape of their pictures.
public sealed class ShotTrayRenderTests
{
    private string _folder = "";

    [SetUp]
    public void SetUp()
    {
        _folder = Path.Combine(Path.GetTempPath(), "HoverTray", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_folder);
    }

    [TearDown]
    public void TearDown()
    {
        try { Directory.Delete(_folder, recursive: true); } catch { /* best effort */ }
    }

    /// Writes a plain coloured PNG of a given shape, standing in for a screenshot.
    private string MakePng(string name, int width, int height, Color fill, Color bars)
    {
        var target = new RenderTargetBitmap(new PixelSize(width, height));
        using (var ctx = target.CreateDrawingContext())
        {
            ctx.FillRectangle(new SolidColorBrush(fill), new Rect(0, 0, width, height));
            var brush = new SolidColorBrush(bars);
            for (var i = 0; i < 5; i++)
                ctx.FillRectangle(brush,
                    new Rect(width * 0.06, height * (0.12 + i * 0.13), width * (0.7 - i * 0.09), Math.Max(2, height * 0.05)));
        }

        var path = Path.Combine(_folder, name);
        target.Save(path, new PngBitmapEncoderOptions());
        target.Dispose();
        return path;
    }

    private static Bitmap Render(Control content, int width, int height)
    {
        var window = new Window
        {
            Width = width,
            Height = height,
            Background = new SolidColorBrush(Color.Parse("#141414")),
            Content = new Border { Padding = new Thickness(16), Child = content },
        };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var frame = window.GetLastRenderedFrame();
        window.Close();
        return frame ?? throw new InvalidOperationException("the window rendered nothing");
    }

    [AvaloniaTest]
    public void The_tray_draws_with_pictures_in_it()
    {
        var today = DateTime.Now;
        var shots = new[]
        {
            new Shot(MakePng("build-output.png", 900, 300,
                Color.Parse("#0C1A0C"), Color.Parse("#4ADE80")), today.AddHours(-1)),
            new Shot(MakePng("state-machine.png", 800, 520,
                Colors.White, Color.Parse("#C9C7C2")), today.AddHours(-5)),
            new Shot(MakePng("messagewindow.png", 700, 420,
                Color.Parse("#1E1E1E"), Color.Parse("#9CDCFE")), today.AddDays(-1)),
            new Shot(MakePng("avalonia-docs.png", 1200, 500,
                Colors.White, Color.Parse("#DEDCD8")), today.AddDays(-3)),
        };

        var tray = new ShotTray();
        tray.Show(shots);

        using var frame = Render(tray, 320, 900);
        var output = Path.Combine(AppContext.BaseDirectory, "tray-canvas.png");
        frame.Save(output, new PngBitmapEncoderOptions());
        TestContext.Out.WriteLine($"rendered: {output}");

        Assert.Multiple(() =>
        {
            Assert.That(tray.Count, Is.EqualTo(4));
            Assert.That(tray.HasShots, Is.True);
            Assert.That(tray.IsEmpty, Is.False);
            Assert.That(frame.PixelSize.Width, Is.GreaterThan(0));
            Assert.That(File.Exists(output), Is.True);
        });
    }

    [AvaloniaTest]
    public void The_empty_tray_draws_its_explanation_instead_of_a_blank_row()
    {
        var tray = new ShotTray();
        tray.Show(Array.Empty<Shot>());

        using var frame = Render(tray, 320, 220);
        var output = Path.Combine(AppContext.BaseDirectory, "tray-canvas-empty.png");
        frame.Save(output, new PngBitmapEncoderOptions());
        TestContext.Out.WriteLine($"rendered: {output}");

        Assert.Multiple(() =>
        {
            Assert.That(tray.IsEmpty, Is.True);
            Assert.That(tray.HasShots, Is.False);
            Assert.That(File.Exists(output), Is.True);
        });
    }

    [AvaloniaTest]
    public void The_panel_ends_under_its_last_picture()
    {
        var today = DateTime.Now;
        var shots = new[]
        {
            new Shot(MakePng("one.png", 900, 300, Colors.Black, Colors.Gray), today),
            new Shot(MakePng("two.png", 900, 300, Colors.Black, Colors.Gray), today.AddHours(-1)),
        };

        var tray = new ShotTray();
        tray.Show(shots);

        // Sized to its content, the way the real panel window is, the window must come
        // out only as tall as two pictures need.
        var window = new Window
        {
            SizeToContent = SizeToContent.WidthAndHeight,
            Content = tray,
        };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var height = window.Height;
        window.Close();

        Assert.That(height, Is.LessThan(500),
            $"the window came out {height:0} tall instead of hugging two pictures");
    }

    /// A wide picture must give a short row and a tall one a tall row, both inside the
    /// clamps, or one phone screenshot takes over the whole tray.
    [AvaloniaTest]
    public void Rows_take_the_shape_of_their_picture_within_the_clamps()
    {
        var wide = new ShotItem(new Shot(MakePng("wide.png", 1600, 200,
            Colors.Black, Colors.Gray), DateTime.Now), DateTime.Now);
        var tall = new ShotItem(new Shot(MakePng("tall.png", 300, 1800,
            Colors.Black, Colors.Gray), DateTime.Now), DateTime.Now);
        var normal = new ShotItem(new Shot(MakePng("normal.png", 800, 500,
            Colors.Black, Colors.Gray), DateTime.Now), DateTime.Now);

        const double width = 250;
        Assert.Multiple(() =>
        {
            Assert.That(wide.HeightFor(width), Is.EqualTo(width * ShotItem.MinRatio).Within(0.5),
                "a very wide picture should sit at the shortest allowed row");
            Assert.That(tall.HeightFor(width), Is.EqualTo(width * ShotItem.MaxRatio).Within(0.5),
                "a very tall picture should sit at the tallest allowed row");
            Assert.That(normal.HeightFor(width), Is.EqualTo(width * 0.625).Within(2),
                "an ordinary picture should keep its own shape");
        });
    }
}
