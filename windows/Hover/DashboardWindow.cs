using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Hover.Chatting;
using Hover.Configuring;
using Hover.Host.Protocol;
using Hover.Settings;
using Hover.Windowing;
using Microsoft.UI.Windowing;
using Windows.Graphics;

namespace Hover;

/// <summary>
/// The dashboard, as small as it can be: the name, how the backend is doing and where the
/// data lives, and the place the office goes. Built in code; there are no .xaml files.
/// </summary>
public sealed class DashboardWindow : Window
{
    readonly TextBlock status;

    /// <summary>The element the pictures draw.</summary>
    public FrameworkElement Root { get; }

    /// <summary>
    /// Where the office is drawn in this window (the Go app shows it here as well as in the notch).
    /// It is the Direct3D 11 view's place; until that exists the frame says so.
    /// </summary>
    public Border OfficeArea { get; }

    /// <summary>True while this window has the keyboard.</summary>
    public bool IsActive { get; private set; }

    /// <summary>The office's panels, drawn over the office area (null when there is no graphics device).</summary>
    internal UiOverlay? Panels { get; }

    /// <summary>Settings, drawn over the office when it is open (null when there is no graphics device).</summary>
    internal SettingsOverlay? SettingsView { get; }

    internal DashboardWindow(string dataDir, BackendSession backend, OfficeHost? office = null, UiCore? panels = null, SettingsHost? settings = null)
    {
        Title = "Hover";
        AppWindow.Resize(new SizeInt32(960, 600));
        Activated += (_, e) => IsActive = e.WindowActivationState != WindowActivationState.Deactivated;

        status = new TextBlock { Text = backend.Status, TextWrapping = TextWrapping.Wrap };
        // A slim bar above the office: the name and how the backend is doing. The office fills the rest.
        status.FontSize = 12;
        status.Opacity = 0.7;
        status.VerticalAlignment = VerticalAlignment.Center;
        status.TextTrimming = TextTrimming.CharacterEllipsis;
        ToolTipService.SetToolTip(status, $"Data folder: {dataDir}");
        var header = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12, Padding = new Thickness(16, 0, 16, 0), Height = 36 };
        header.Children.Add(new TextBlock { Text = "Hover", FontSize = 14, FontWeight = FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center });
        // File, Settings and Help, as the Go app's title bar has them.
        var menuBar = AppMenu.Build(new AppMenu.Actions(
            NewChat: () =>
            {
                settings?.CloseDash();
                if (panels is not null) { panels.Ui.ChatView = true; panels.Ui.CloseChat(); panels.Ui.Invalidate(); }
            },
            OpenFolder: () =>
            {
                // A folder for the next task: it is the one the new-task box starts with.
                if (Dialogs.Folder("Open a folder") is not { } folder) return;
                backend.Send(new SetSettingCommand("KiroFolder", folder));
                settings?.CloseDash();
                panels?.Ui.OpenNewTask();
            },
            CloseWindow: Close,
            OpenSettings: section => settings?.ShowInDashboard(section),
            DataFolder: dataDir));
        header.Children.Add(menuBar);
        AppMenu.Track(menuBar, WinRT.Interop.WindowNative.GetWindowHandle(this));
        header.Children.Add(status);

        OfficeArea = new Border
        {
            Margin = new Thickness(0),
            Name = "OfficeArea",
            Child = new StackPanel
            {
                VerticalAlignment = VerticalAlignment.Center,
                HorizontalAlignment = HorizontalAlignment.Center,
                Spacing = 4,
                Children =
                {
                    new TextBlock { Text = "The office draws here", FontSize = 15, FontWeight = FontWeights.Medium, Opacity = 0.65, HorizontalAlignment = HorizontalAlignment.Center },
                    new TextBlock { Text = "Direct3D 11 view", FontSize = 12, Opacity = 0.45, HorizontalAlignment = HorizontalAlignment.Center },
                },
            },
        };

        // The window's own background is not part of the element tree, so the pictures draw this.
        var page = new Grid { Background = (Brush)Application.Current.Resources["ApplicationPageBackgroundThemeBrush"] };
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.Children.Add(header);
        Grid.SetRow(OfficeArea, 1);
        page.Children.Add(OfficeArea);
        if (panels is not null)
        {
            Panels = new UiOverlay(panels, page, DispatcherQueue) { WindowHandle = WinRT.Interop.WindowNative.GetWindowHandle(this) };
            Grid.SetRow(Panels, 1);
            page.Children.Add(Panels);
        }
        if (settings is not null && panels is not null)
        {
            SettingsView = new SettingsOverlay(settings, DispatcherQueue) { WindowHandle = WinRT.Interop.WindowNative.GetWindowHandle(this) };
            Grid.SetRow(SettingsView, 1);
            page.Children.Add(SettingsView);
            // A theme colours Settings, its menus and the app window; Hover's own colours follow the appearance.
            ApplyPalette(page, settings.Controller.Palette);
            settings.PaletteChanged += palette => DispatcherQueue.TryEnqueue(() => ApplyPalette(page, palette));
        }
        Root = page;
        Content = page;

        backend.Changed += () => DispatcherQueue.TryEnqueue(() => status.Text = backend.Status);
        office?.AttachDashboard(OfficeArea, WinRT.Interop.WindowNative.GetWindowHandle(this));
    }

    /// <summary>The window wears the palette: its page, its title bar and the light or dark of its controls.</summary>
    void ApplyPalette(Grid page, AppPalette p)
    {
        static Windows.UI.Color Of(uint argb) => Color((byte)(argb >> 24), (byte)(argb >> 16), (byte)(argb >> 8), (byte)argb);
        page.Background = new SolidColorBrush(Of(p.Panel | 0xFF000000));
        page.RequestedTheme = p.Dark ? ElementTheme.Dark : ElementTheme.Light;
        if (AppWindowTitleBar.IsCustomizationSupported())
        {
            var bar = AppWindow.TitleBar;
            bar.BackgroundColor = bar.InactiveBackgroundColor = bar.ButtonBackgroundColor = bar.ButtonInactiveBackgroundColor = Of(p.Panel | 0xFF000000);
            bar.ForegroundColor = bar.ButtonForegroundColor = Of(p.Ink | 0xFF000000);
            bar.InactiveForegroundColor = bar.ButtonInactiveForegroundColor = Of(p.InkDim | 0xFF000000);
        }
    }

    static Windows.UI.Color Color(byte a, byte r, byte g, byte b) => Windows.UI.Color.FromArgb(a, r, g, b);
}
