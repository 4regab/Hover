using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows;
using System.Windows.Controls.Primitives;
using System.Windows.Forms;
using System.Windows.Interop;
using Hover.Core;
using Hover.Interop;

namespace Hover.Services;

/// Windows has no accessory-app dock trick to opt out of, but it does expect a way
/// back into an app with no window — so the pill's menu is also a tray icon.
/// The icon is drawn at startup rather than shipped as a file: a small fan of sticky
/// notes, which is the whole app.
public sealed class TrayIcon : IDisposable
{
    private readonly NotifyIcon _icon;
    private readonly Window _menuOwner;
    private System.Windows.Controls.ContextMenu? _menu;

    public TrayIcon()
    {
        _menuOwner = new Window
        {
            WindowStyle = WindowStyle.None,
            ShowInTaskbar = false,
            ShowActivated = false,
            AllowsTransparency = true,
            Background = System.Windows.Media.Brushes.Transparent,
            Opacity = 0,
            Left = -32000,
            Top = -32000,
            Width = 1,
            Height = 1,
        };

        _icon = new NotifyIcon
        {
            Icon = Draw(),
            Text = "Hover",
            Visible = true,
        };
        _icon.MouseUp += (_, e) =>
        {
            if (e.Button == MouseButtons.Left)
            {
                Actions.OpenAllNotes();
                return;
            }
            ShowMenu();
        };
    }

    private void ShowMenu()
    {
        if (_menu is not null) _menu.IsOpen = false;

        _menu = Actions.BuildMainMenu();
        _menu.Placement = PlacementMode.MousePoint;
        _menu.PlacementTarget = _menuOwner;
        _menu.StaysOpen = false;
        _menu.Closed += (_, _) => _menuOwner.Hide();

        _menuOwner.Show();
        var hwnd = new WindowInteropHelper(_menuOwner).Handle;
        Win32.SetForegroundWindow(hwnd);
        _menuOwner.Activate();
        _menu.IsOpen = true;
    }

    /// The Hover mark, drawn to match assets/hover.svg: a fan of three sticky notes
    /// floating above a soft shadow on an indigo tile. Drawn in code so no icon file
    /// has to ship, and it stays crisp at tray size.
    ///
    /// The writing and the tick on the front note are left out here. At 32 px a
    /// 7-unit stroke from the 256-space artwork is under one pixel, so they only turn
    /// the note grey; the three fanned colours are what has to survive.
    public static Icon Draw()
    {
        const int S = 32;
        using var bmp = new Bitmap(S, S);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);

            // Rounded tile with the app's indigo gradient, top to bottom.
            using var tilePath = RoundedRect(1, 1, S - 2, S - 2, 7);
            using var tile = new LinearGradientBrush(
                new Rectangle(0, 0, S, S),
                Color.FromArgb(0x53, 0x43, 0xBE), Color.FromArgb(0x1F, 0x19, 0x48), 90f);
            g.FillPath(tile, tilePath);

            // Everything below is in the artwork's own 256-space, put through the
            // same transform the SVG applies to the fan: rotate about the hinge the
            // notes splay from, lift the whole group, then scale down to the tray.
            const float Hinge = 208f, LiftAbout = 130f, Lift = 1.07f;
            var k = S / 256f;

            Matrix Fan(float rot)
            {
                var m = new Matrix();
                // Added last applies first, so this reads bottom-up.
                m.Scale(k, k);
                m.Translate(128, LiftAbout);
                m.Scale(Lift, Lift);
                m.Translate(-128, -LiftAbout);
                m.RotateAt(rot, new PointF(128, Hinge));
                return m;
            }

            // The shadow the fan floats above: 256-space centre 128,200 radius 86x22,
            // carried through the group lift and down to tray pixels.
            var shadowCy = LiftAbout + (200 - LiftAbout) * Lift;
            using var shadow = new SolidBrush(Color.FromArgb(70, 0x12, 0x08, 0x2A));
            g.FillEllipse(shadow,
                (128 - 86 * Lift) * k, (shadowCy - 22 * Lift) * k,
                2 * 86 * Lift * k, 2 * 22 * Lift * k);

            // Mint and Rose sit behind, so they carry the same shading the SVG gives
            // them rather than their raw palette colour.
            Note(-16f, 72, 60, 112, 126, Color.FromArgb(0x9E, 0xD3, 0xBB), null);
            Note(16f, 72, 60, 112, 126, Color.FromArgb(0xE9, 0xB8, 0xC5), null);
            Note(-3f, 70, 54, 116, 132, Color.FromArgb(0xFC, 0xE7, 0x95),
                 Color.FromArgb(0xE0, 0xAD, 0x08));

            void Note(float rot, float x, float y, float w, float h, Color paper, Color? bar)
            {
                using var m = Fan(rot);
                using var path = RoundedRect(x, y, w, h, 15);
                path.Transform(m);
                using var fill = new SolidBrush(paper);
                g.FillPath(fill, path);

                if (bar is not { } barColor) return;
                // The saturated strip down the edge the note hangs from, kept inside
                // the note's own rounded outline.
                var clip = g.Clip;
                g.SetClip(path);
                using var barBrush = new SolidBrush(barColor);
                var strip = new[]
                {
                    new PointF(x, y), new PointF(x + 15, y),
                    new PointF(x + 15, y + h), new PointF(x, y + h),
                };
                m.TransformPoints(strip);
                g.FillPolygon(barBrush, strip);
                g.Clip = clip;
            }
        }
        var handle = bmp.GetHicon();
        try
        {
            // FromHandle borrows the native HICON. Clone it before releasing the
            // original so NotifyIcon owns an independent managed icon.
            return (Icon)Icon.FromHandle(handle).Clone();
        }
        finally
        {
            Win32.DestroyIcon(handle);
        }
    }

    private static GraphicsPath RoundedRect(float x, float y, float w, float h, float r)
    {
        var path = new GraphicsPath();
        var d = r * 2;
        path.AddArc(x, y, d, d, 180, 90);
        path.AddArc(x + w - d, y, d, d, 270, 90);
        path.AddArc(x + w - d, y + h - d, d, d, 0, 90);
        path.AddArc(x, y + h - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }

    public void Dispose()
    {
        if (_menu is not null) _menu.IsOpen = false;
        _menuOwner.Close();
        _icon.Visible = false;
        _icon.Dispose();
    }
}
