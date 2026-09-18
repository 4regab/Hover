using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.NUnit;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Hover.Images;
using NUnit.Framework;

namespace Hover.Tests;

/// The mark-up window. Enter saves, Escape throws the picture away, and nothing is
/// written unless the user asks for it.
public sealed class AnnotateWindowTests
{
    private static Bitmap Picture(int width = 320, int height = 200)
    {
        var target = new RenderTargetBitmap(new PixelSize(width, height));
        using (var ctx = target.CreateDrawingContext())
        {
            ctx.FillRectangle(Brushes.White, new Rect(0, 0, width, height));
            ctx.FillRectangle(new SolidColorBrush(Color.Parse("#DDEBF1")),
                new Rect(20, 20, width - 40, 40));
        }
        return target;
    }

    [AvaloniaTest]
    public void Saving_hands_back_a_picture()
    {
        var window = new AnnotateWindow(Picture());
        byte[]? saved = null;
        window.Saved += (_, png) => saved = png;

        window.Show();
        Dispatcher.UIThread.RunJobs();
        window.Save();
        Dispatcher.UIThread.RunJobs();

        Assert.That(saved, Is.Not.Null, "saving produced nothing");
        using var stream = new MemoryStream(saved!);
        using var decoded = new Bitmap(stream);
        Assert.That(decoded.PixelSize, Is.EqualTo(new PixelSize(320, 200)));
    }

    /// The whole reason the editor opens before the file is written: escaping must leave
    /// nothing behind.
    [AvaloniaTest]
    public void Discarding_hands_back_nothing()
    {
        var window = new AnnotateWindow(Picture());
        var told = 0;
        window.Saved += (_, _) => told++;

        window.Show();
        Dispatcher.UIThread.RunJobs();
        window.Discard();
        Dispatcher.UIThread.RunJobs();

        Assert.That(told, Is.Zero, "discarding should not save anything");
    }

    [AvaloniaTest]
    public void Escape_discards_and_Enter_saves()
    {
        var escaped = new AnnotateWindow(Picture());
        var escapedSaves = 0;
        escaped.Saved += (_, _) => escapedSaves++;
        escaped.Show();
        Dispatcher.UIThread.RunJobs();
        escaped.KeyPressQwerty(PhysicalKey.Escape, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();

        var entered = new AnnotateWindow(Picture());
        var enteredSaves = 0;
        entered.Saved += (_, _) => enteredSaves++;
        entered.Show();
        Dispatcher.UIThread.RunJobs();
        entered.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();

        Assert.Multiple(() =>
        {
            Assert.That(escapedSaves, Is.Zero, "Escape should throw the picture away");
            Assert.That(enteredSaves, Is.EqualTo(1), "Enter should save");
        });
    }

    /// Drawn on and saved, the marks must be in the file. This is the end-to-end proof
    /// that the editor is wired to the exporter.
    [AvaloniaTest]
    public void What_was_drawn_ends_up_in_the_saved_file()
    {
        var window = new AnnotateWindow(Picture(200, 200));
        byte[]? saved = null;
        window.Saved += (_, png) => saved = png;
        window.Show();
        Dispatcher.UIThread.RunJobs();

        var canvas = FindCanvas(window);
        Assert.That(canvas, Is.Not.Null, "the editor has no drawing surface");

        // Drag a box across the middle of the picture.
        canvas!.Tool = Tool.Box;
        canvas.Colour = Markup.Red;
        Drag(window, canvas, new Vector(-60, -60), new Vector(60, 60));
        Dispatcher.UIThread.RunJobs();

        window.Save();
        Dispatcher.UIThread.RunJobs();

        Assert.That(saved, Is.Not.Null);
        using var stream = new MemoryStream(saved!);
        using var decoded = new Bitmap(stream);
        Assert.That(Reddish(decoded), Is.GreaterThan(0), "the box did not reach the file");
    }

    [AvaloniaTest]
    public void The_editor_draws_and_is_saved_for_looking_at()
    {
        var window = new AnnotateWindow(Picture(420, 260));
        window.Show();
        Dispatcher.UIThread.RunJobs();

        var canvas = FindCanvas(window)!;
        canvas.Tool = Tool.Box;
        Drag(window, canvas, new Vector(-180, -100), new Vector(-20, -20));
        canvas.Tool = Tool.Arrow;
        canvas.Colour = Markup.Blue;
        Drag(window, canvas, new Vector(20, -100), new Vector(140, 10));
        canvas.Tool = Tool.Highlight;
        canvas.Colour = Markup.Yellow;
        Drag(window, canvas, new Vector(-150, 40), new Vector(150, 75));
        Dispatcher.UIThread.RunJobs();

        var frame = window.GetLastRenderedFrame();
        Assert.That(frame, Is.Not.Null, "the editor rendered nothing");

        var output = Path.Combine(AppContext.BaseDirectory, "annotate-window.png");
        frame!.Save(output, new PngBitmapEncoderOptions());
        TestContext.Out.WriteLine($"rendered: {output}");
        Assert.That(File.Exists(output), Is.True);
    }

    private static MarkupCanvas? FindCanvas(Visual root)
    {
        if (root is MarkupCanvas found) return found;
        foreach (var child in root.GetVisualChildren())
            if (FindCanvas(child) is { } hit) return hit;
        return null;
    }

    /// A press, a move and a release across the middle of the drawing surface.
    ///
    /// Measured out from the centre on purpose. The picture is centred inside the canvas
    /// and anything smaller than the canvas has empty margins either side, so coordinates
    /// taken from the canvas corner can land beside the picture rather than on it.
    private static void Drag(Window window, MarkupCanvas canvas, Vector from, Vector to)
    {
        var middle = canvas.TranslatePoint(
            new Point(canvas.Bounds.Width / 2, canvas.Bounds.Height / 2), window)
            ?? new Point(0, 0);
        window.MouseDown(middle + from, MouseButton.Left);
        window.MouseMove(middle + to);
        window.MouseUp(middle + to, MouseButton.Left);
    }

    private static int Reddish(Bitmap bitmap)
    {
        var size = bitmap.PixelSize;
        var stride = size.Width * 4;
        var buffer = new byte[stride * size.Height];
        var handle = System.Runtime.InteropServices.GCHandle.Alloc(
            buffer, System.Runtime.InteropServices.GCHandleType.Pinned);
        try
        {
            bitmap.CopyPixels(new PixelRect(0, 0, size.Width, size.Height),
                handle.AddrOfPinnedObject(), buffer.Length, stride);
        }
        finally
        {
            handle.Free();
        }

        var count = 0;
        for (var i = 0; i + 3 < buffer.Length; i += 4)
            if (buffer[i + 2] > 140 && buffer[i + 1] < 110 && buffer[i] < 110) count++;
        return count;
    }
}
