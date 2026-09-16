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
/// The icon is drawn at startup rather than shipped as a file: a single sticky note
/// on its edge, which is the whole app.
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

    /// The Hover mark, drawn to match assets/hover.svg: a white pointer floating
    /// above a soft shadow on a blue→purple tile. Drawn in code so no icon file has
    /// to ship, and it stays crisp at tray size.
    public static Icon Draw()
    {
        const int S = 32;
        using var bmp = new Bitmap(S, S);
        using (var g = Graphics.FromImage(bmp))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.Clear(Color.Transparent);

            // Rounded tile with the app's blue→purple gradient.
            using var tilePath = RoundedRect(1, 1, S - 2, S - 2, 7);
            using var tile = new LinearGradientBrush(
                new Rectangle(0, 0, S, S),
                Color.FromArgb(0x3A, 0x86, 0xE0), Color.FromArgb(0x5A, 0x4B, 0xD6), 45f);
            g.FillPath(tile, tilePath);

            // The pointer's shadow, then the pointer — 256-space coords scaled to 32.
            float k = S / 256f;
            using var shadow = new SolidBrush(Color.FromArgb(46, 0, 0, 0));
            g.FillEllipse(shadow, (128 - 46) * k, (188 - 12) * k, 92 * k, 24 * k);

            var pts = new[]
            {
                new PointF(104, 60), new PointF(104, 168), new PointF(131, 141),
                new PointF(150, 182), new PointF(168, 174), new PointF(149, 133),
                new PointF(186, 133),
            };
            for (var i = 0; i < pts.Length; i++) pts[i] = new PointF(pts[i].X * k, pts[i].Y * k);
            using var white = new SolidBrush(Color.White);
            g.FillPolygon(white, pts);
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
