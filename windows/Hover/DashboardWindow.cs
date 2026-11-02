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
/// The dashboard, as small as it can be: Hover's one title bar (File, Settings and Help, and the caption
/// buttons) and the place the office goes. Built in code; there are no .xaml files.
/// </summary>
public sealed class DashboardWindow : Window
{
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

    internal DashboardWindow(string dataDir, BackendSession backend, OfficeHost? office = null, UiCore? panels = null, SettingsPane? settings = null)
    {
        Title = "Hover";
        WindowMinimum.Apply(AppWindow, WinRT.Interop.WindowNative.GetWindowHandle(this));
        Activated += (_, e) => IsActive = e.WindowActivationState != WindowActivationState.Deactivated;

        // One title bar, as the Go app has (internal/ui/dashboard.go): Hover's dark bar holds File, Settings and Help on the left, and
        // the system's own minimize, maximize and close buttons are on its right, so the snap layouts and the system menu stay.
        // The bar says nothing else, except one line when the backend has a problem.
        ExtendsContentIntoTitleBar = true;
        StyleCaptionButtons();
        problem = new TextBlock
        {
            FontSize = 12, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis,
            Margin = new Thickness(12, 0, 12, 0), Foreground = new SolidColorBrush(Rgb(0xff6b62)), Visibility = Visibility.Collapsed,
        };
        ShowProblem(backend.Problem);
        var header = new Grid { Height = BarHeight - 1, RequestedTheme = ElementTheme.Dark };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
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
        StyleMenuBar(menuBar);
        header.Children.Add(menuBar);
        AppMenu.Track(menuBar, WinRT.Interop.WindowNative.GetWindowHandle(this));
        Grid.SetColumn(problem, 1);
        header.Children.Add(problem);
        // The space the system's caption buttons take on the right.
        captions = new Border();
        Grid.SetColumn(captions, 2);
        header.Children.Add(captions);
        var bar = new Border
        {
            Background = new SolidColorBrush(Rgb(BarRgb)), BorderBrush = new SolidColorBrush(Rgb(0xffffff, 0x0f)),
            BorderThickness = new Thickness(0, 0, 0, 1), Child = header,
        };

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
        var page = new CursorGrid { Background = (Brush)Application.Current.Resources["ApplicationPageBackgroundThemeBrush"] };
        page.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        page.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        page.Children.Add(bar);
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
            SettingsView = new SettingsOverlay(settings, DispatcherQueue) { WindowHandle = WinRT.Interop.WindowNative.GetWindowHandle(this), Page = page };
            Grid.SetRow(SettingsView, 1);
            page.Children.Add(SettingsView);
            // While Settings shows, the office's panels under it step aside: a press on a Settings control must not also press the panel beneath.
            settings.DashOpenChanged += () => DispatcherQueue.TryEnqueue(() =>
            {
                if (Panels is not null) Panels.Visibility = settings.DashOpen ? Visibility.Collapsed : Visibility.Visible;
                if (!settings.DashOpen) panels?.Ui.Invalidate();
                if (Hover.Chatting.UiDump.On) Hover.Chatting.UiDump.Set("panels-away", new System.Text.Json.Nodes.JsonObject { ["away"] = settings.DashOpen });
            });
            // A theme colours Settings, its menus and the app window; Hover's own colours follow the appearance.
            ApplyPalette(page, settings.Controller.Palette);
            settings.PaletteChanged += palette => DispatcherQueue.TryEnqueue(() => ApplyPalette(page, palette));
        }
        Root = page;
        Content = page;
        // The bar is the window's caption: dragged to move, double-clicked to maximize, its right-click menu is the system's. The menus
        // in it take the pointer, and the caption buttons' space is kept free.
        SetTitleBar(bar);
        page.Loaded += (_, _) => FitTitleBar(menuBar);
        page.SizeChanged += (_, _) => FitTitleBar(menuBar);
        menuBar.SizeChanged += (_, _) => FitTitleBar(menuBar);

        backend.Changed += () => DispatcherQueue.TryEnqueue(() => ShowProblem(backend.Problem));
        office?.AttachDashboard(OfficeArea, WinRT.Interop.WindowNative.GetWindowHandle(this));
    }

    /// <summary>The window wears the palette: its page, its title bar and the light or dark of its controls.</summary>
    void ApplyPalette(Grid page, AppPalette p)
    {
        static Windows.UI.Color Of(uint argb) => Color((byte)(argb >> 24), (byte)(argb >> 16), (byte)(argb >> 8), (byte)argb);
        page.Background = new SolidColorBrush(Of(p.Panel | 0xFF000000));
        page.RequestedTheme = p.Dark ? ElementTheme.Dark : ElementTheme.Light;
    }

    static Windows.UI.Color Color(byte a, byte r, byte g, byte b) => Windows.UI.Color.FromArgb(a, r, g, b);

    // MARK: The title bar

    /// <summary>Hover's own title bar colour (internal/ui/dashboard.go): the same dark in every theme.</summary>
    const uint BarRgb = 0x0b0810;

    /// <summary>The bar's height in DIPs: the system's standard caption height (the Go app's bar is 36; the system offers 32 or 48).</summary>
    const double BarHeight = 32;

    readonly TextBlock problem;
    readonly Border captions;

    static Windows.UI.Color Rgb(uint rgb, byte alpha = 0xff) => Color(alpha, (byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);

    /// <summary>A colour laid over another, as the bar shows it (the caption buttons take no see-through colours).</summary>
    static Windows.UI.Color Blend(uint top, double alpha, uint under)
    {
        byte Mix(int shift) => (byte)Math.Round(((top >> shift) & 0xff) * alpha + ((under >> shift) & 0xff) * (1 - alpha));
        return Color(0xff, Mix(16), Mix(8), Mix(0));
    }

    /// <summary>The system's caption buttons in the Go bar's colours: no fill, a light one on hover (the close button's red is the system's).</summary>
    void StyleCaptionButtons()
    {
        if (!AppWindowTitleBar.IsCustomizationSupported()) return;
        var bar = AppWindow.TitleBar;
        var ink = Blend(0xffffff, 0xd9 / 255.0, BarRgb);
        bar.BackgroundColor = bar.InactiveBackgroundColor = Rgb(BarRgb);
        bar.ButtonBackgroundColor = bar.ButtonInactiveBackgroundColor = Colors.Transparent;
        bar.ButtonHoverBackgroundColor = Blend(0xffffff, 0x1a / 255.0, BarRgb);
        bar.ButtonPressedBackgroundColor = Blend(0xffffff, 0x10 / 255.0, BarRgb);
        bar.ForegroundColor = bar.InactiveForegroundColor = ink;
        bar.ButtonForegroundColor = bar.ButtonHoverForegroundColor = bar.ButtonPressedForegroundColor = bar.ButtonInactiveForegroundColor = ink;
    }

    /// <summary>File, Settings and Help as the Go bar draws its words: 12.5 pixels, quiet until the pointer is on one or its menu is out.</summary>
    static void StyleMenuBar(MenuBar menuBar)
    {
        var none = new SolidColorBrush(Colors.Transparent);
        var lit = new SolidColorBrush(Rgb(0xffffff, 0x0f));
        var quiet = new SolidColorBrush(Rgb(0xf6f2ff, 0xb0));
        var ink = new SolidColorBrush(Rgb(0xf6f2ff));
        menuBar.Resources["MenuBarItemBackground"] = none;
        menuBar.Resources["MenuBarItemBackgroundPointerOver"] = lit;
        menuBar.Resources["MenuBarItemBackgroundPressed"] = lit;
        menuBar.Resources["MenuBarItemBackgroundSelected"] = lit;
        menuBar.Resources["MenuBarItemForeground"] = quiet;
        menuBar.Resources["MenuBarItemForegroundPointerOver"] = ink;
        menuBar.Resources["MenuBarItemForegroundPressed"] = ink;
        menuBar.Resources["MenuBarItemForegroundSelected"] = ink;
        menuBar.VerticalAlignment = VerticalAlignment.Center;
        // The words start 6 pixels in, and sit 1 pixel apart.
        menuBar.Margin = new Thickness(6, 0, 0, 0);
        foreach (var item in menuBar.Items)
        {
            item.Height = 26;
            item.Margin = new Thickness(0, 0, 1, 0);
            item.CornerRadius = new CornerRadius(6);
            item.VerticalAlignment = VerticalAlignment.Center;
            // A word is a button inside the item, with its own size and padding (10 and 4 pixels): the word is 12.5 pixels, 9 pixels from each side.
            item.Loaded += (_, _) =>
            {
                if (FindButton(item) is not { } button) return;
                button.FontSize = 12.5;
                button.Padding = new Thickness(9, 0, 9, 0);
            };
            // The Go bar has no focus ring; the word lights while its menu is out. (The system draws a white ring on the first word when
            // the window opens, as the first thing that can have the keyboard.)
            item.FocusVisualPrimaryThickness = item.FocusVisualSecondaryThickness = new Thickness(0);
        }
    }

    static Button? FindButton(DependencyObject parent)
    {
        for (var i = 0; i < Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(parent); i++)
        {
            var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(parent, i);
            if (child is Button button) return button;
            if (FindButton(child) is { } found) return found;
        }
        return null;
    }

    /// <summary>The backend's problem, if it has one, in the bar; nothing while it is starting or well.</summary>
    void ShowProblem(string? text)
    {
        problem.Text = text ?? "";
        problem.Visibility = text is null ? Visibility.Collapsed : Visibility.Visible;
        ToolTipService.SetToolTip(problem, text);
        // For the checks of the real app: what the bar says.
        if (Hover.Chatting.UiDump.On) Hover.Chatting.UiDump.Set("bar-problem", new System.Text.Json.Nodes.JsonObject { ["text"] = text });
    }

    /// <summary>
    /// Keeps the caption buttons' space free on the right, and tells the system that the menus are not caption: a press on them is
    /// theirs, a press on the rest of the bar moves the window.
    /// </summary>
    void FitTitleBar(MenuBar menuBar)
    {
        if (menuBar.XamlRoot is not { } root || !AppWindowTitleBar.IsCustomizationSupported()) return;
        var scale = root.RasterizationScale;
        captions.Width = AppWindow.TitleBar.RightInset / scale;
        var at = menuBar.TransformToVisual(null).TransformPoint(new Windows.Foundation.Point(0, 0));
        var area = new RectInt32((int)Math.Floor(at.X * scale), (int)Math.Floor(at.Y * scale), (int)Math.Ceiling(menuBar.ActualWidth * scale), (int)Math.Ceiling(menuBar.ActualHeight * scale));
        Microsoft.UI.Input.InputNonClientPointerSource.GetForWindowId(AppWindow.Id).SetRegionRects(Microsoft.UI.Input.NonClientRegionKind.Passthrough, [area]);
        if (Hover.Chatting.UiDump.On)
            Hover.Host.HostLog.Line($"title bar: menus {area.X},{area.Y} {area.Width}x{area.Height}, caption buttons {AppWindow.TitleBar.RightInset}px wide, scale {scale}");
    }
}
