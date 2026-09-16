using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Hover.Core;
using Hover.Windows;

namespace Hover.Images;

/// One picture in the tray: just the thumbnail, with a small ✕ in the corner to
/// delete it — no file name or wide buttons, so many pictures fit. Drag the
/// thumbnail out to a folder, a website or a chat box; click it to open full size;
/// right-click to copy.
public sealed class ShotRow : Border
{
    private readonly Shot _shot;
    private Button _delete = null!;
    private Point _pressAt;
    private bool _maybeDrag;
    private bool _dragging;

    public const double RowHeight = 96;

    public ShotRow(Shot shot, double width)
    {
        _shot = shot;
        Width = width;
        Height = RowHeight;
        Margin = new Thickness(0, 0, 0, 8);
        CornerRadius = new CornerRadius(8);
        ClipToBounds = true;
        Background = NoteColor.Tint(Colors.Black, 0.35);
        BorderBrush = NoteColor.Tint(Colors.White, 0.10);
        BorderThickness = new Thickness(1);
        Cursor = Cursors.Hand;
        ToolTip = $"{shot.Name}\nDrag out · click to open · right-click to copy";

        var grid = new Grid();

        // The picture fills the row.
        grid.Children.Add(new Image
        {
            Source = shot.Thumbnail((int)(width * 2)),
            Stretch = Stretch.UniformToFill,
            IsHitTestVisible = false,
        });

        // A small ✕ delete button in the top-right corner.
        var del = new Button
        {
            Content = "✕",
            FontSize = 11,
            FontWeight = FontWeights.Bold,
            Foreground = Brushes.White,
            Width = 20,
            Height = 20,
            Padding = new Thickness(0),
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(0, 5, 5, 0),
            Cursor = Cursors.Hand,
            Focusable = false,
            ToolTip = "Delete",
            Background = new SolidColorBrush(Color.FromArgb(0xC8, 0x20, 0x20, 0x24)),
            BorderThickness = new Thickness(0),
            Template = RoundButtonTemplate(),
        };
        del.Click += (_, e) => { e.Handled = true; ShotStore.Shared.Delete(_shot); };
        grid.Children.Add(del);
        _delete = del;

        Child = grid;

        ContextMenu = BuildMenu();

        PreviewMouseLeftButtonDown += OnDown;
        PreviewMouseMove += OnMove;
        PreviewMouseLeftButtonUp += OnUp;
        MouseEnter += (_, _) => BorderBrush = NoteColor.Tint(Colors.White, 0.5);
        MouseLeave += (_, _) => BorderBrush = NoteColor.Tint(Colors.White, 0.10);
    }

    private void OnDown(object sender, MouseButtonEventArgs e)
    {
        // A click on the delete button belongs to the button, not the row — don't
        // arm a drag or an open, so the button's own Click deletes the picture.
        if (IsOnDelete(e)) return;
        if (e.ClickCount == 2) { e.Handled = true; _maybeDrag = false; OpenFull(); return; }
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
    /// the row leaves it alone.
    private bool IsOnDelete(RoutedEventArgs e)
    {
        for (var d = e.OriginalSource as DependencyObject; d is not null;
             d = System.Windows.Media.VisualTreeHelper.GetParent(d))
        {
            if (ReferenceEquals(d, _delete)) return true;
        }
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
        menu.Items.Add(Item("Copy", () =>
        {
            var full = _shot.FullSize();
            if (full is not null) Clipboard.SetImage(full);
        }));
        menu.Items.Add(Item("Rename…", () =>
        {
            var current = System.IO.Path.GetFileNameWithoutExtension(_shot.Name);
            var input = RenameDialog.Ask(Window.GetWindow(this), current);
            if (!string.IsNullOrWhiteSpace(input)) ShotStore.Shared.Rename(_shot, input);
        }));
        menu.Items.Add(Item("Reveal in Explorer", () =>
        {
            try
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(
                    "explorer.exe", $"/select,\"{_shot.Path}\"") { UseShellExecute = true });
            }
            catch (Exception ex) { Log.Line($"reveal failed — {ex.Message}"); }
        }));
        menu.Items.Add(new Separator());
        menu.Items.Add(Item("Delete", () => ShotStore.Shared.Delete(_shot)));
        return menu;
    }

    private static MenuItem Item(string header, Action action)
    {
        var item = new MenuItem { Header = header };
        item.Click += (_, _) => action();
        return item;
    }

    /// A round flat button, so the corner ✕ reads as a chip rather than a boxy
    /// default WPF button.
    private static ControlTemplate RoundButtonTemplate()
    {
        var border = new FrameworkElementFactory(typeof(Border));
        border.SetValue(Border.CornerRadiusProperty, new CornerRadius(10));
        border.SetBinding(Border.BackgroundProperty,
            new System.Windows.Data.Binding("Background")
            { RelativeSource = System.Windows.Data.RelativeSource.TemplatedParent });
        var presenter = new FrameworkElementFactory(typeof(ContentPresenter));
        presenter.SetValue(HorizontalAlignmentProperty, HorizontalAlignment.Center);
        presenter.SetValue(VerticalAlignmentProperty, VerticalAlignment.Center);
        border.AppendChild(presenter);
        return new ControlTemplate(typeof(Button)) { VisualTree = border };
    }
}
