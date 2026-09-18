using Avalonia;
using Avalonia.Headless.NUnit;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Hover.Images;
using NUnit.Framework;

namespace Hover.Tests;

/// What gets drawn on a snip, and what comes out when it is saved.
public sealed class MarkupTests
{
    private static readonly PixelSize Picture = new(400, 300);

    [Test]
    public void A_new_markup_has_nothing_on_it()
    {
        var m = new Markup();
        Assert.Multiple(() =>
        {
            Assert.That(m.IsEmpty, Is.True);
            Assert.That(m.CanUndo, Is.False);
            Assert.That(m.Marks, Is.Empty);
            Assert.That(m.Crop, Is.Null);
            Assert.That(m.SizeFor(Picture), Is.EqualTo(Picture));
        });
    }

    [Test]
    public void Undo_walks_back_through_what_was_done_in_order()
    {
        var m = new Markup();
        m.Add(new BoxMark(new Rect(10, 10, 50, 50), Markup.Red));
        m.SetCrop(new Rect(0, 0, 200, 150));
        m.Add(new HighlightMark(new Rect(5, 5, 20, 20), Markup.Yellow));

        // Newest first: the highlight, then the crop, then the box.
        m.Undo();
        Assert.That(m.Marks, Has.Count.EqualTo(1));
        Assert.That(m.Crop, Is.Not.Null);

        m.Undo();
        Assert.That(m.Crop, Is.Null, "undoing should take the crop off again");
        Assert.That(m.Marks, Has.Count.EqualTo(1));

        m.Undo();
        Assert.Multiple(() =>
        {
            Assert.That(m.Marks, Is.Empty);
            Assert.That(m.CanUndo, Is.False);
        });
    }

    [Test]
    public void Undo_on_an_empty_markup_does_nothing_rather_than_failing()
    {
        var m = new Markup();
        Assert.DoesNotThrow(() => m.Undo());
        Assert.That(m.IsEmpty, Is.True);
    }

    [Test]
    public void A_second_crop_replaces_the_first_instead_of_stacking()
    {
        var m = new Markup();
        m.SetCrop(new Rect(10, 10, 100, 100));
        m.SetCrop(new Rect(0, 0, 50, 50));

        Assert.That(m.Crop, Is.EqualTo(new Rect(0, 0, 50, 50)));
        Assert.That(m.SizeFor(Picture), Is.EqualTo(new PixelSize(50, 50)));
    }

    [Test]
    public void A_crop_dragged_off_the_edge_is_pulled_back_inside_the_picture()
    {
        var clamped = Markup.Clamp(new Rect(-40, -20, 800, 700), Picture);
        Assert.That(clamped, Is.EqualTo(new Rect(0, 0, 400, 300)));
    }

    [Test]
    public void A_crop_entirely_outside_the_picture_has_no_area()
    {
        var clamped = Markup.Clamp(new Rect(900, 900, 100, 100), Picture);
        Assert.Multiple(() =>
        {
            Assert.That(clamped.Width, Is.Zero);
            Assert.That(clamped.Height, Is.Zero);
        });
    }

    [Test]
    public void Mark_thickness_follows_the_size_of_the_picture()
    {
        Assert.Multiple(() =>
        {
            Assert.That(Mark.ThicknessFor(new PixelSize(40, 30)), Is.EqualTo(2),
                "a tiny snip should not get a fat pen");
            Assert.That(Mark.ThicknessFor(new PixelSize(4000, 3000)), Is.EqualTo(6),
                "a huge snip should not get an enormous pen");
            Assert.That(Mark.ThicknessFor(new PixelSize(800, 600)), Is.EqualTo(3));
        });
    }

    [Test]
    public void Changes_are_announced_so_the_editor_can_redraw()
    {
        var m = new Markup();
        var told = 0;
        m.Changed += (_, _) => told++;

        m.Add(new BoxMark(new Rect(0, 0, 10, 10), Markup.Red));
        m.SetCrop(new Rect(0, 0, 20, 20));
        m.Undo();
        m.Clear();
        m.Clear();   // already empty, so nothing to announce

        Assert.That(told, Is.EqualTo(4));
    }

    // MARK: Saving

    private static Bitmap Plain(int width, int height, Color fill)
    {
        var target = new RenderTargetBitmap(new PixelSize(width, height));
        using (var ctx = target.CreateDrawingContext())
            ctx.FillRectangle(new SolidColorBrush(fill), new Rect(0, 0, width, height));
        return target;
    }

    [AvaloniaTest]
    public void Saving_an_untouched_picture_keeps_its_size()
    {
        using var picture = Plain(120, 80, Colors.White);
        var png = new Markup().Export(picture);

        Assert.That(png, Is.Not.Null);
        using var stream = new MemoryStream(png!);
        using var saved = new Bitmap(stream);
        Assert.That(saved.PixelSize, Is.EqualTo(new PixelSize(120, 80)));
    }

    [AvaloniaTest]
    public void Saving_a_cropped_picture_comes_out_the_size_of_the_crop()
    {
        using var picture = Plain(200, 160, Colors.White);
        var m = new Markup();
        m.SetCrop(new Rect(20, 30, 100, 50));

        var png = m.Export(picture)!;
        using var stream = new MemoryStream(png);
        using var saved = new Bitmap(stream);
        Assert.That(saved.PixelSize, Is.EqualTo(new PixelSize(100, 50)));
    }

    /// The point of a mark is that it ends up in the file. A red box on a white picture
    /// must leave red pixels behind.
    [AvaloniaTest]
    public void A_mark_really_is_in_the_saved_file()
    {
        using var picture = Plain(100, 100, Colors.White);
        var m = new Markup();
        m.Add(new BoxMark(new Rect(20, 20, 60, 60), Markup.Red));

        var png = m.Export(picture)!;
        using var stream = new MemoryStream(png);
        using var saved = new Bitmap(stream);

        Assert.That(CountReddish(saved), Is.GreaterThan(0), "the box was not drawn");
    }

    /// A mark drawn outside the crop must not appear in the result.
    [AvaloniaTest]
    public void A_mark_outside_the_crop_is_left_out()
    {
        using var picture = Plain(200, 200, Colors.White);

        var kept = new Markup();
        kept.Add(new BoxMark(new Rect(10, 10, 40, 40), Markup.Red));
        kept.SetCrop(new Rect(0, 0, 100, 100));

        var dropped = new Markup();
        dropped.Add(new BoxMark(new Rect(140, 140, 40, 40), Markup.Red));
        dropped.SetCrop(new Rect(0, 0, 100, 100));

        Assert.Multiple(() =>
        {
            Assert.That(CountReddish(Decode(kept.Export(picture)!)), Is.GreaterThan(0),
                "a mark inside the crop should survive");
            Assert.That(CountReddish(Decode(dropped.Export(picture)!)), Is.Zero,
                "a mark outside the crop should not appear");
        });
    }

    [AvaloniaTest]
    public void A_crop_with_no_area_refuses_to_save_rather_than_writing_nothing()
    {
        using var picture = Plain(100, 100, Colors.White);
        var m = new Markup();
        m.SetCrop(new Rect(500, 500, 10, 10));   // entirely off the picture

        Assert.That(m.Export(picture), Is.Null);
    }

    private static Bitmap Decode(byte[] png)
    {
        using var stream = new MemoryStream(png);
        return new Bitmap(stream);
    }

    /// Counts pixels that are clearly more red than anything else.
    private static int CountReddish(Bitmap bitmap)
    {
        using (bitmap)
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
            {
                var b = buffer[i];
                var g = buffer[i + 1];
                var r = buffer[i + 2];
                if (r > 140 && g < 110 && b < 110) count++;
            }
            return count;
        }
    }
}
