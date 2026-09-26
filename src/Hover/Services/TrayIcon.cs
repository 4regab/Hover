using System.Drawing;
using System.Windows;
using System.Windows.Controls.Primitives;
using System.Windows.Forms;
using System.Windows.Interop;
using Hover.Core;
using Hover.Interop;

namespace Hover.Services;

/// Windows has no accessory-app dock trick to opt out of, but it does expect a way
/// back into an app with no window — so the pill's menu is also a tray icon.
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
            Icon = AppIcon(),
            Text = "Hover",
            Visible = true,
        };
        _icon.MouseUp += (_, e) =>
        {
            if (e.Button == MouseButtons.Left)
            {
                Hover.Owl.OwlApp.OpenDashboard?.Invoke();
                return;
            }
            ShowMenu();
        };
    }

    /// A Windows notification from the tray icon. The time's-up chime is played here
    /// too, since Windows silences notifications during a Focus session — exactly
    /// when a focus timer ends.
    public void Notify(string title, string text)
    {
        if (title == "Time's up") System.Media.SystemSounds.Asterisk.Play();
        _icon.ShowBalloonTip(6000, title, text, ToolTipIcon.None);
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

    /// The app's own icon at the size the tray draws it. hover.ico carries a frame
    /// drawn for each of 16, 20, 24 and 32 px, so Windows shows one as it is instead
    /// of shrinking a larger one into a blur.
    private static Icon AppIcon()
    {
        var uri = new Uri("pack://application:,,,/Hover;component/Assets/hover.ico");
        using var stream = System.Windows.Application.GetResourceStream(uri)!.Stream;
        return new Icon(stream, SystemInformation.SmallIconSize);
    }

    public void Dispose()
    {
        if (_menu is not null) _menu.IsOpen = false;
        _menuOwner.Close();
        _icon.Visible = false;
        _icon.Dispose();
    }
}
