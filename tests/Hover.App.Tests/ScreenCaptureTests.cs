using Avalonia.Headless.NUnit;
using Avalonia.Media.Imaging;
using Hover.Interop;
using NUnit.Framework;

namespace Hover.Tests;

/// Photographing the screen is the first half of snipping. If the bytes it hands back
/// are not a picture a decoder accepts, every snip is broken, so that is what is
/// checked rather than the call merely returning something.
public sealed class ScreenCaptureTests
{
    [Test]
    public void The_desktop_has_a_real_size()
    {
        var (_, _, width, height) = ScreenCapture.VirtualScreen;
        Assert.Multiple(() =>
        {
            Assert.That(width, Is.GreaterThan(0));
            Assert.That(height, Is.GreaterThan(0));
        });
    }

    [AvaloniaTest]
    public void A_capture_decodes_at_the_size_that_was_asked_for()
    {
        var bmp = ScreenCapture.Capture(0, 0, 64, 40);
        Assert.That(bmp, Is.Not.Null, "Windows refused the capture");

        using var stream = new MemoryStream(bmp!);
        Bitmap decoded = null!;
        Assert.DoesNotThrow(() => decoded = new Bitmap(stream),
            "the captured bytes are not a picture a decoder reads");

        using (decoded)
        {
            Assert.Multiple(() =>
            {
                Assert.That(decoded.PixelSize.Width, Is.EqualTo(64));
                Assert.That(decoded.PixelSize.Height, Is.EqualTo(40));
            });
        }
    }

    /// BitBlt leaves the fourth byte of every pixel at zero, which a decoder reads as
    /// fully see-through. Without fixing that up, a snip saves as an invisible picture.
    [AvaloniaTest]
    public void A_capture_is_opaque_rather_than_invisible()
    {
        var bmp = ScreenCapture.Capture(0, 0, 8, 8)!;
        var transparent = 0;
        for (var i = 54 + 3; i < bmp.Length; i += 4)
            if (bmp[i] != 0xFF) transparent++;

        Assert.That(transparent, Is.Zero, $"{transparent} pixels would save as see-through");
    }

    [AvaloniaTest]
    public void A_silly_size_is_survived_rather_than_throwing()
    {
        Assert.DoesNotThrow(() => ScreenCapture.Capture(0, 0, 0, 0));
        Assert.DoesNotThrow(() => ScreenCapture.Capture(-5000, -5000, 4, 4));
    }
}
