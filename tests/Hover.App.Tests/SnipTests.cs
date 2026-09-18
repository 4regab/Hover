using Avalonia;
using Avalonia.Headless.NUnit;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Hover.Images;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

/// The box a snip drag makes, and the cut that comes out of it. Checked without a mouse,
/// because driving a real drag across the screen proves nothing repeatable.
public sealed class SnipTests
{
    private static Win32.POINT P(int x, int y) => new() { X = x, Y = y };

    [Test]
    public void A_box_is_the_same_whichever_corner_it_was_dragged_from()
    {
        var forwards = SnipOverlay.Box(P(100, 80), P(300, 200), 0, 0, 1920, 1080);
        var backwards = SnipOverlay.Box(P(300, 200), P(100, 80), 0, 0, 1920, 1080);

        Assert.Multiple(() =>
        {
            Assert.That(forwards.Left, Is.EqualTo(100));
            Assert.That(forwards.Top, Is.EqualTo(80));
            Assert.That(forwards.Width, Is.EqualTo(200));
            Assert.That(forwards.Height, Is.EqualTo(120));
            Assert.That(backwards, Is.EqualTo(forwards));
        });
    }

    [Test]
    public void A_box_dragged_off_the_desktop_stops_at_its_edge()
    {
        var box = SnipOverlay.Box(P(-500, -500), P(5000, 5000), 0, 0, 1920, 1080);
        Assert.Multiple(() =>
        {
            Assert.That(box.Left, Is.Zero);
            Assert.That(box.Top, Is.Zero);
            Assert.That(box.Right, Is.EqualTo(1920));
            Assert.That(box.Bottom, Is.EqualTo(1080));
        });
    }

    /// A second display can start at a negative coordinate, so the desktop does not
    /// begin at zero.
    [Test]
    public void A_desktop_that_starts_left_of_zero_is_handled()
    {
        var box = SnipOverlay.Box(P(-3000, -200), P(-1900, 100), -1920, -200, 1920, 1080);
        Assert.Multiple(() =>
        {
            Assert.That(box.Left, Is.EqualTo(-1920));
            Assert.That(box.Top, Is.EqualTo(-200));
            Assert.That(box.Right, Is.EqualTo(-1900));
        });
    }

    [Test]
    public void A_click_with_no_drag_makes_an_empty_box()
    {
        var box = SnipOverlay.Box(P(400, 400), P(400, 400), 0, 0, 1920, 1080);
        Assert.Multiple(() =>
        {
            Assert.That(box.Width, Is.Zero);
            Assert.That(box.Height, Is.Zero);
        });
    }

    [AvaloniaTest]
    public void A_cut_is_the_size_of_the_box_and_holds_the_right_pixels()
    {
        // A picture with a red square at 40,20 so the cut can be checked by colour.
        var source = new RenderTargetBitmap(new PixelSize(200, 100));
        using (var ctx = source.CreateDrawingContext())
        {
            ctx.FillRectangle(Brushes.White, new Rect(0, 0, 200, 100));
            ctx.FillRectangle(new SolidColorBrush(Markup.Red), new Rect(40, 20, 30, 30));
        }

        using (source)
        {
            using var onTheRed = SnipOverlay.Crop(source, 40, 20, 30, 30);
            using var offTheRed = SnipOverlay.Crop(source, 120, 20, 30, 30);

            Assert.Multiple(() =>
            {
                Assert.That(onTheRed.PixelSize, Is.EqualTo(new PixelSize(30, 30)));
                Assert.That(Reddish(onTheRed), Is.GreaterThan(500),
                    "cutting over the red square should be mostly red");
                Assert.That(Reddish(offTheRed), Is.Zero,
                    "cutting away from the red square should have no red in it");
            });
        }
    }

    [AvaloniaTest]
    public void A_cut_of_nothing_still_produces_a_picture_rather_than_failing()
    {
        var source = new RenderTargetBitmap(new PixelSize(50, 50));
        using (source)
        {
            using var cut = SnipOverlay.Crop(source, 0, 0, 0, 0);
            Assert.That(cut.PixelSize, Is.EqualTo(new PixelSize(1, 1)));
        }
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
