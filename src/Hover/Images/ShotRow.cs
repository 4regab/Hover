using System.Windows;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;
using Hover.Owl;
using Hover.Windows;

namespace Hover.Images;

/// One picture in the Screenshots card: just the thumbnail, with a small ✕ that
/// appears on hover to delete it. Drag the thumbnail out to a folder, a website or a
/// chat box; click it to open full size; right-click to copy, rename or reveal.
public sealed class ShotRow : Border
{
    private readonly Shot _shot;
    private readonly Button _delete;
    private Point _pressAt;
    private bool _maybeDrag;
    private bool _dragging;

    private static readonly Brush Rest = Ui.Edge;
    private static readonly Brush Hot = Ui.Frozen(Color.FromArgb(0x73, 0xFF, 0xFF, 0xFF));

    /// Decoded once at this width whatever the tile's size, so resizing the card
    /// never goes back to the disk.
    private const int ThumbPixels = 480;

    public ShotRow(Shot shot)
    {
        _shot = shot;
        CornerRadius = new CornerRadius(9);
        ClipToBounds = true;
        Background = Ui.Wash;
        BorderBrush = Rest;
        BorderThickness = new Thickness(1);
        Cursor = Cursors.Hand;
        ToolTip = $"{shot.Name}\nDrag out · click to open · right-click for more";
        System.Windows.Automation.AutomationProperties.SetName(this, shot.Name);
        System.Windows.Automation.AutomationProperties.SetAutomationId(this, "Shot");

        var grid = new Grid();
        var image = new Image
        {
            Source = shot.Thumbnail(ThumbPixels),
            Stretch = Stretch.UniformToFill,
            IsHitTestVisible = false,
        };
        RenderOptions.SetBitmapScalingMode(image, BitmapScalingMode.HighQuality);
        grid.Children.Add(image);

        _delete = Ui.Button("OwlBase", Ui.Icon(Ui.IcClose, 8, Ui.White), "DeleteShot", "Delete", () => ShotStore.Shared.Delete(_shot));
        _delete.Width = _delete.Height = 20;
        _delete.Padding = new Thickness(0);
        _delete.Tag = new CornerRadius(10);
        _delete.Background = Ui.Frozen(Color.FromArgb(0xCC, 0x1C, 0x1C, 0x1E));
        _delete.HorizontalAlignment = HorizontalAlignment.Right;
        _delete.VerticalAlignment = VerticalAlignment.Top;
        _delete.Margin = new Thickness(0, 5, 5, 0);
        _delete.Focusable = false;
        _delete.Visibility = Visibility.Hidden;
        grid.Children.Add(_delete);

        Child = grid;
        ContextMenu = BuildMenu();
        Popover.Track(ContextMenu);

        PreviewMouseLeftButtonDown += OnDown;
        PreviewMouseMove += OnMove;
        PreviewMouseLeftButtonUp += OnUp;
        MouseEnter += (_, _) => { BorderBrush = Hot; _delete.Visibility = Visibility.Visible; };
        MouseLeave += (_, _) => { BorderBrush = Rest; _delete.Visibility = Visibility.Hidden; };
    }

    /// A Border has no automation peer of its own; the tile needs one to be found.
    protected override AutomationPeer OnCreateAutomationPeer() => new FrameworkElementAutomationPeer(this);

    private void OnDown(object sender, MouseButtonEventArgs e)
    {
        // A click on the delete button belongs to the button, not the tile — don't
        // arm a drag or an open, so the button's own Click deletes the picture.
        if (IsOnDelete(e)) return;
        _pressAt = e.GetPosition(this);
        _maybeDrag = true;
        _dragging = false;
    }

    private void OnMove(object sender, MouseEventArgs e)
    {
        if (!_maybeDrag || e.LeftButton != MouseButtonState.Pressed) return;
        var now = e.GetPosition(this);
        if (Math.Abs(now.X - _pressAt.X) < SystemParameters.MinimumHorizontalDragDistance &&
            Math.Abs(now.Y - _pressAt.Y) < SystemParameters.MinimumVerticalDragDistance) return;
        _maybeDrag = false;
        _dragging = true;
        ShotDrag.Start(this, _shot);
    }

    private void OnUp(object sender, MouseButtonEventArgs e)
    {
        if (IsOnDelete(e)) { _maybeDrag = false; return; }
        if (_maybeDrag && !_dragging) OpenFull();
        _maybeDrag = false;
    }

    /// True when the event started on the delete button (or its inner content), so
    /// the tile leaves it alone.
    private bool IsOnDelete(RoutedEventArgs e)
    {
        for (var d = e.OriginalSource as DependencyObject; d is not null; d = VisualTreeHelper.GetParent(d))
            if (ReferenceEquals(d, _delete)) return true;
        return false;
    }

    private void OpenFull()
    {
        var full = _shot.FullSize();
        if (full is not null) ImagePreviewWindow.Show(full, null);
    }

    private ContextMenu BuildMenu()
    {
        var menu = new ContextMenu();
        menu.Items.Add(Ui.MenuItem(Ui.IcCopy, "Copy", () =>
        {
            var full = _shot.FullSize();
            if (full is not null) Clipboard.SetImage(full);
        }));
        menu.Items.Add(Ui.MenuItem(Ui.IcRename, "Rename…", () =>
        {
            var current = System.IO.Path.GetFileNameWithoutExtension(_shot.Name);
            var input = RenameDialog.Ask(Window.GetWindow(this), current);
            if (!string.IsNullOrWhiteSpace(input)) ShotStore.Shared.Rename(_shot, input);
        }));
        menu.Items.Add(Ui.MenuItem(Ui.IcFolder, "Show in Explorer", () =>
        {
            try
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(
                    "explorer.exe", $"/select,\"{_shot.Path}\"") { UseShellExecute = true });
            }
            catch (Exception ex) { Log.Line($"reveal failed — {ex.Message}"); }
        }));
        menu.Items.Add(new Separator());
        menu.Items.Add(Ui.MenuItem(Ui.IcDelete, "Delete", () => ShotStore.Shared.Delete(_shot)));
        return menu;
    }
}
