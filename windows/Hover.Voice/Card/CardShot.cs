using System.Numerics;
using System.Runtime.InteropServices;
using Hover.Notch;
using Vortice;
using Vortice.Direct2D1;
using Vortice.DXGI;
using Vortice.Mathematics;
using AlphaMode = Vortice.DCommon.AlphaMode;
using FigureBegin = Vortice.Direct2D1.FigureBegin;
using FigureEnd = Vortice.Direct2D1.FigureEnd;
using PixelFormat = Vortice.DCommon.PixelFormat;
using Size = Hover.Notch.Size;

namespace Hover.Voice;

/// <summary>
/// The card in a picture: the resting notch's shape drawn black with its shadow (or the amber glow
/// of a turned-away press) over the desktop colour the Go pictures use, and the card on it, drawn
/// offscreen and read back. No screen capture. The window is the notch's whole window, as the Go
/// pictures are; a picture wider than 1400 pixels is cut to the middle 1400.
/// </summary>
public static class CardShot
{
    /// <summary>The desktop colour the Go pictures lay the notch over.</summary>
    public const uint Desk = 0x3a4a5e;

    public const int MaxWidth = 1400;

    /// <summary>Draws <paramref name="view"/> in a window of <paramref name="windowW"/> by <paramref name="windowH"/> DIPs at <paramref name="scale"/>.</summary>
    public static (byte[] Bgra, int Width, int Height) Render(ID2D1Factory1 factory, ID2D1DeviceContext dc, VoiceCardPainter painter, VoiceView view, CardUi ui,
        double windowW, double windowH, double scale, double openH)
    {
        var (cw, ch) = painter.Measure(dc, view, ui, openH);
        var rest = Geometry.RestSize(RestKind.Card, cw, ch);
        var (r, ear) = Geometry.RestCorners(rest.H);
        var x0 = (windowW - rest.W) / 2;
        int pw = (int)Math.Round(windowW * scale), ph = (int)Math.Round(windowH * scale);
        var format = new PixelFormat(Format.B8G8R8A8_UNorm, AlphaMode.Premultiplied);
        using var target = dc.CreateBitmap(new SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.Target));
        dc.Target = target;
        dc.SetDpi((float)(96 * scale), (float)(96 * scale));
        dc.BeginDraw();
        try
        {
            dc.AntialiasMode = AntialiasMode.PerPrimitive;
            dc.TextAntialiasMode = TextAntialiasMode.Grayscale;
            dc.Clear(new Color4(0, 0, 0, 0));
            using var outline = factory.CreatePathGeometry();
            using (var sink = outline.Open())
            {
                var cmds = SvgPath.Parse(Geometry.Outline(rest.W, rest.H, r, ear, x0)) ?? [];
                var open = false;
                foreach (var c in cmds)
                {
                    switch (c.Op)
                    {
                        case PathOp.Move:
                            if (open) sink.EndFigure(FigureEnd.Open);
                            sink.BeginFigure(new Vector2((float)c.X, (float)c.Y), FigureBegin.Filled);
                            open = true;
                            break;
                        case PathOp.Line: sink.AddLine(new Vector2((float)c.X, (float)c.Y)); break;
                        case PathOp.Cubic:
                            sink.AddBezier(new BezierSegment(new Vector2((float)c.X1, (float)c.Y1), new Vector2((float)c.X2, (float)c.Y2), new Vector2((float)c.X, (float)c.Y)));
                            break;
                        case PathOp.Arc:
                            sink.AddArc(new ArcSegment(new Vector2((float)c.X, (float)c.Y), new Vortice.Mathematics.Size((float)c.Rx, (float)c.Ry), (float)c.Rotation,
                                c.Sweep ? SweepDirection.Clockwise : SweepDirection.CounterClockwise, c.Large ? ArcSize.Large : ArcSize.Small));
                            break;
                        case PathOp.Close:
                            if (open) sink.EndFigure(FigureEnd.Closed);
                            open = false;
                            break;
                    }
                }
                if (open) sink.EndFigure(FigureEnd.Open);
                sink.Close();
            }
            Shadow(dc, outline, view.Busy ? 0xffb340u : 0);
            using (var black = dc.CreateSolidColorBrush(new Color4(0, 0, 0, 1))) dc.FillGeometry(outline, black);
            // The card is laid out at the shape's corner, clipped to the shape.
            painter.Draw(dc, view, ui, x0, 0, openH);
        }
        finally
        {
            dc.EndDraw().CheckError();
            dc.Target = null;
        }

        using var cpu = dc.CreateBitmap(new SizeI(pw, ph), IntPtr.Zero, 0, new BitmapProperties1(format, 96, 96, BitmapOptions.CpuRead | BitmapOptions.CannotDraw));
        cpu.CopyFromBitmap(target).CheckError();
        var mapped = cpu.Map(MapOptions.Read);
        var src = new byte[mapped.Pitch * ph];
        try { Marshal.Copy(mapped.Bits, src, 0, src.Length); }
        finally { cpu.Unmap(); }

        // Premultiplied colour over an opaque back is colour + back * (1 - alpha).
        var cwOut = Math.Min(pw, MaxWidth);
        var left = (pw - cwOut) / 2;
        byte b0 = (byte)(Desk & 0xff), g0 = (byte)((Desk >> 8) & 0xff), r0 = (byte)((Desk >> 16) & 0xff);
        var output = new byte[cwOut * ph * 4];
        for (var y = 0; y < ph; y++)
        {
            for (var x = 0; x < cwOut; x++)
            {
                var i = y * mapped.Pitch + (left + x) * 4;
                var o = (y * cwOut + x) * 4;
                int a = src[i + 3];
                output[o] = (byte)Math.Min(src[i] + b0 * (255 - a) / 255, 255);
                output[o + 1] = (byte)Math.Min(src[i + 1] + g0 * (255 - a) / 255, 255);
                output[o + 2] = (byte)Math.Min(src[i + 2] + r0 * (255 - a) / 255, 255);
                output[o + 3] = 255;
            }
        }
        return (output, cwOut, ph);
    }

    /// <summary>The soft shadow under the shape, or the glow round it (24 blur, 4 down, black at one half; the glow 26 blur, in its colour).</summary>
    static void Shadow(ID2D1DeviceContext dc, ID2D1PathGeometry outline, uint glow)
    {
        try
        {
            double blur = 24, depth = 4;
            var color = new Vector4(0, 0, 0, 0.5f);
            if (glow != 0)
            {
                blur = 26;
                depth = 0;
                color = new Vector4(((glow >> 16) & 0xff) / 255f, ((glow >> 8) & 0xff) / 255f, (glow & 0xff) / 255f, 0.62f);
            }
            using var list = dc.CreateCommandList();
            var target = dc.Target;
            dc.Target = list;
            using (var black = dc.CreateSolidColorBrush(new Color4(0, 0, 0, 1))) dc.FillGeometry(outline, black);
            dc.Target = target;
            list.Close();
            using var effect = new ID2D1Effect(dc.CreateEffect(EffectGuids.Shadow));
            effect.SetInput(0, list, true);
            effect.SetValue((uint)ShadowProperties.BlurStandardDeviation, (float)(blur / 2));
            effect.SetValue((uint)ShadowProperties.Color, color);
            using var image = effect.Output;
            dc.DrawImage(image, new Vector2(0, (float)depth), null, InterpolationMode.Linear, CompositeMode.SourceOver);
        }
        catch (Exception e)
        {
            Hover.Host.HostLog.Line($"voice shot: the shadow was left out ({e.Message})");
        }
    }
}
