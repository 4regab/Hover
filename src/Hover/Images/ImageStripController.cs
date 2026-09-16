using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Effects;
using System.Windows.Threading;
using Hover.Core;
using Hover.Deck;
using Hover.Interop;

namespace Hover.Images;

/// The image tray for one display, on the LEFT edge. At rest it is a slim pill;
/// hovering slides out a scrollable list of pictures, each row carrying the
/// thumbnail plus its own Copy and Delete buttons. Drag a thumbnail out to a
/// folder, a website or a chat box; click it to open full size.
public sealed class ImageStripController : IDisposable
{
    public string Device { get; }
    public ImageStripManager? Manager { get; set; }

    private readonly DeckWindow _window = new();
    private readonly DispatcherTimer _idle;
    private ScreenInfo _screen;
    private double _panelHeight;
    private bool _open;
    private DateTime _lastActivity = DateTime.Now;
    private Win32.POINT _lastPointer;

    private const double PanelWidth = 300;
    private static readonly TimeSpan OpenIdle = TimeSpan.FromSeconds(6);

    public ImageStripController(ScreenInfo screen)
    {
        Device = screen.Device;
        _screen = screen;

        _window.Show();
        Layout();
        Render();

        ShotStore.Shared.Changed += OnShotsChanged;

        _idle = new DispatcherTimer(DispatcherPriority.Background)
        {
            Interval = TimeSpan.FromMilliseconds(150),
        };
        _idle.Tick += (_, _) => IdleTick();
    }

    private void OnShotsChanged(object? sender, EventArgs e)
    {
        if (!_open) Layout();
        Render();
    }

    public void UpdateScreen(ScreenInfo screen)
    {
        _screen = screen;
        Layout();
        Render();
    }

    public void Layout()
    {
        var s = _screen;
        // A window wide enough for the open panel, docked to the LEFT edge.
        var w = (int)Math.Round((PanelWidth + 20) * s.Scale);
        var h = s.Work.Height;
        _window.PlaceDevice(s.Bounds.Left, s.Work.Top, w, h);
        _panelHeight = h / s.Scale;
        _window.Root.Width = w / s.Scale;
        _window.Root.Height = _panelHeight;
    }

    public void RefreshLevel()
    {
        _window.Topmost = true;
        _window.Raise();
    }

    // MARK: Wake zone

    public Win32.RECT EdgeStrip
    {
        get
        {
            var s = _screen;
            // A thin vertical band down the left edge — the whole middle stretch, so
            // resting the pointer anywhere along the edge wakes the tray. Nothing is
            // drawn here at rest; the band is only a wake zone.
            var w = (int)Math.Round(Math.Max(3, Settings.EdgeWidth) * s.Scale);
            var margin = (int)Math.Round(s.Work.Height * 0.15);
            return new Win32.RECT
            {
                Left = s.Bounds.Left,
                Right = s.Bounds.Left + w,
                Top = s.Work.Top + margin,
                Bottom = s.Work.Bottom - margin,
            };
        }
    }

    private Win32.RECT HotZone
    {
        get
        {
            if (!_open) return EdgeStrip;
            var s = _screen;
            var strip = (int)Math.Round((PanelWidth + 24) * s.Scale);
            return new Win32.RECT
            {
                Left = s.Bounds.Left,
                Right = s.Bounds.Left + strip,
                Top = s.Work.Top,
                Bottom = s.Work.Bottom,
            };
        }
    }

    public void PointerEntered()
    {
        _lastActivity = DateTime.Now;
        if (_open) return;
        Manager?.StripDidActivate(this);
        _open = true;
        _lastPointer = Screens.Cursor;
        // The panel has buttons, a scrollbar and drag — it needs to take clicks and
        // be a normal window, so it stops being the no-activate pill while open.
        _window.SetAcceptsKeys(true);
        if (!_idle.IsEnabled) _idle.Start();
        Render();
    }

    public void Collapse()
    {
        if (!_open) return;
        _open = false;
        _idle.Stop();
        _window.SetAcceptsKeys(false);
        Layout();
        Render();
    }

    private void IdleTick()
    {
        if (!_open) { _idle.Stop(); return; }
        var now = Screens.Cursor;
        if (!HotZone.Contains(now)) { Collapse(); return; }
        if (Math.Abs(now.X - _lastPointer.X) > 2 || Math.Abs(now.Y - _lastPointer.Y) > 2)
        {
            _lastPointer = now;
            _lastActivity = DateTime.Now;
        }
        if (DateTime.Now - _lastActivity > OpenIdle) Collapse();
    }

    // MARK: Rendering

    private void Render()
    {
        var root = _window.Root;
        root.Children.Clear();
        var h = Math.Max(1, _panelHeight);

        // Hidden when not in use: nothing is drawn at rest, so the edge stays clean
        // and the tray is unnoticeable until the pointer rests against the edge.
        if (!_open) return;

        root.Children.Add(BuildPanel(h));
    }

    private FrameworkElement BuildPanel(double h)
    {
        var shots = ShotStore.Shared.Shots;

        var card = new Border
        {
            Width = PanelWidth,
            Height = Math.Min(h - 16, Math.Max(160, shots.Count * (ShotRow.RowHeight + 8) + 52)),
            Background = new SolidColorBrush(Color.FromArgb(0xF2, 0x1C, 0x1C, 0x20)),
            CornerRadius = new CornerRadius(0, 12, 12, 0),
            Effect = new DropShadowEffect
                { Color = Colors.Black, Opacity = 0.4, BlurRadius = 24, ShadowDepth = 6, Direction = 340 },
        };
        Canvas.SetLeft(card, 0);
        Canvas.SetTop(card, Math.Max(8, (h - card.Height) / 2));

        var outer = new Grid { Margin = new Thickness(12, 10, 12, 12) };
        outer.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        outer.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var header = new TextBlock
        {
            Text = shots.Count == 0 ? "Screenshots" : $"Screenshots · {shots.Count}",
            FontFamily = Ink.SystemFace,
            FontSize = 12.5,
            FontWeight = FontWeights.SemiBold,
            Foreground = NoteColor.Tint(Colors.White, 0.9),
            Margin = new Thickness(2, 0, 0, 8),
        };
        outer.Children.Add(header);

        if (shots.Count == 0)
        {
            var empty = new TextBlock
            {
                Text = "No pictures yet.\nSnip or copy an image and it lands here.",
                FontFamily = Ink.SystemFace,
                FontSize = 11.5,
                Foreground = NoteColor.Tint(Colors.White, 0.5),
                TextWrapping = TextWrapping.Wrap,
                Margin = new Thickness(2, 8, 0, 0),
            };
            Grid.SetRow(empty, 1);
            outer.Children.Add(empty);
        }
        else
        {
            var list = new StackPanel();
            var rowWidth = PanelWidth - 24 - 12;   // card padding + scrollbar room
            foreach (var shot in shots) list.Children.Add(new ShotRow(shot, rowWidth));

            var scroller = new ScrollViewer
            {
                Content = list,
                VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
                HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            };
            Grid.SetRow(scroller, 1);
            outer.Children.Add(scroller);
        }

        card.Child = outer;
        return card;
    }

    private int ShotCount() => ShotStore.Shared.Shots.Count;

    public void Dispose()
    {
        _idle.Stop();
        ShotStore.Shared.Changed -= OnShotsChanged;
        _window.Close();
    }
}
