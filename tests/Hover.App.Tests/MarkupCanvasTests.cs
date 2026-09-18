using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.NUnit;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Hover.Images;
using NUnit.Framework;

namespace Hover.Tests;

/// Turning a drag into a mark. Checked on the canvas alone, so a failure here is about
/// the drawing surface and not about the window around it.
public sealed class MarkupCanvasTests
{
    private static Bitmap Plain(int width, int height)
    {
        var target = new RenderTargetBitmap(new PixelSize(width, height));
        using (var ctx = target.CreateDrawingContext())
            ctx.FillRectangle(Brushes.White, new Rect(0, 0, width, height));
        return target;
    }

    /// The canvas is given exactly the picture's size, so control coordinates and
    /// picture coordinates are the same and the sums are easy to check.
    private static (Window Window, MarkupCanvas Canvas, Markup Markup) Open(int size = 200)
    {
        var markup = new Markup();
        var canvas = new MarkupCanvas(Plain(size, size), markup);
        var window = new Window
        {
            Width = size,
            Height = size,
            WindowDecorations = Avalonia.Controls.WindowDecorations.None,
            Content = canvas,
        };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, canvas, markup);
    }

    private static void Drag(Window window, Point from, Point to)
    {
        window.MouseDown(from, MouseButton.Left);
        window.MouseMove(to);
        window.MouseUp(to, MouseButton.Left);
        Dispatcher.UIThread.RunJobs();
    }

    [AvaloniaTest]
    public void A_drag_with_the_box_tool_leaves_a_box()
    {
        var (window, canvas, markup) = Open();
        canvas.Tool = Tool.Box;
        canvas.Colour = Markup.Red;

        Drag(window, new Point(30, 40), new Point(130, 140));

        Assert.That(markup.Marks, Has.Count.EqualTo(1), "the drag did not reach the canvas");
        Assert.That(markup.Marks[0], Is.TypeOf<BoxMark>());
        var box = (BoxMark)markup.Marks[0];
        Assert.Multiple(() =>
        {
            Assert.That(box.Area.X, Is.EqualTo(30).Within(1));
            Assert.That(box.Area.Y, Is.EqualTo(40).Within(1));
            Assert.That(box.Area.Width, Is.EqualTo(100).Within(1));
            Assert.That(box.Area.Height, Is.EqualTo(100).Within(1));
            Assert.That(box.Colour, Is.EqualTo(Markup.Red));
        });
        window.Close();
    }

    [AvaloniaTest]
    public void Each_tool_leaves_its_own_kind_of_mark()
    {
        var (window, canvas, markup) = Open();

        canvas.Tool = Tool.Arrow;
        Drag(window, new Point(10, 10), new Point(90, 90));
        canvas.Tool = Tool.Highlight;
        Drag(window, new Point(20, 120), new Point(160, 150));
        canvas.Tool = Tool.Crop;
        Drag(window, new Point(5, 5), new Point(180, 180));

        Assert.Multiple(() =>
        {
            Assert.That(markup.Marks.Select(m => m.GetType()),
                Is.EqualTo(new[] { typeof(ArrowMark), typeof(HighlightMark) }));
            Assert.That(markup.Crop, Is.Not.Null, "the crop tool should set a crop");
        });
        window.Close();
    }

    /// A stray tap should not leave a dot behind.
    [AvaloniaTest]
    public void A_click_that_barely_moves_leaves_nothing()
    {
        var (window, canvas, markup) = Open();
        canvas.Tool = Tool.Box;

        Drag(window, new Point(50, 50), new Point(52, 51));

        Assert.That(markup.IsEmpty, Is.True);
        window.Close();
    }

    [AvaloniaTest]
    public void A_drag_that_runs_off_the_picture_is_pulled_back_inside_it()
    {
        var (window, canvas, markup) = Open(100);
        canvas.Tool = Tool.Box;

        Drag(window, new Point(50, 50), new Point(400, 400));

        Assert.That(markup.Marks, Has.Count.EqualTo(1));
        var box = (BoxMark)markup.Marks[0];
        Assert.Multiple(() =>
        {
            Assert.That(box.Area.Right, Is.LessThanOrEqualTo(100));
            Assert.That(box.Area.Bottom, Is.LessThanOrEqualTo(100));
        });
        window.Close();
    }
}
