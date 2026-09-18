using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Effects;
using System.Windows.Threading;
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

    /// Rows scale with the deck-size preference, the same one the note tabs use, so both
    /// screen edges change size together.
    public static double RowHeight => 80 * Deck.DeckGeom.Scale;

    /// A row never gets shorter than this share of its width, so a panorama is still a
    /// target you can hit, nor taller than this, so one long phone screenshot cannot
    /// take over the tray. Past the cap the picture is cropped, as every row used to be.
    private const double MinRatio = 0.3;
    private const double MaxRatio = 1.2;

    /// How tall the row for this picture will be, asked before the row is built: the
    /// tray needs it to size its own panel. Shot caches its thumbnail, so the second
    /// ask costs nothing.
    public static double HeightFor(Shot shot, double width)
    {
        var thumb = shot.Thumbnail((int)(width * 2));
        if (thumb is null || thumb.PixelWidth <= 0) return RowHeight;
        var ratio = (double)thumb.PixelHeight / thumb.PixelWidth;
        return Math.Round(width * Math.Clamp(ratio, MinRatio, MaxRatio));
    }

    public ShotRow(Shot shot, double width)
    {
        _shot = shot;
        Width = width;
        Height = HeightFor(shot, width);
        Margin = new Thickness(0, 0, 0, 8);
        CornerRadius = new CornerRadius(8);
        ClipToBounds = true;
        Background = NoteColor.Tint(Colors.Black, 0.35);
        BorderBrush = NoteColor.Tint(Colors.White, 0.10);
        BorderThickness = new Thickness(1);
        Cursor = Cursors.Hand;

        var grid = new Grid();

        // The picture fills the row, and the row was sized to the picture's own shape,
        // so nothing is cropped and no empty band is left around it. Only a picture
        // past the shape limits above loses anything.
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

        // Rows are small, so two screenshots of the same window still look alike. The
        // hover card shows enough to tell them apart. The notes deck solves this with
        // PreviewCard, which will not
        // work here: this window is about 200 px wide and would clip the card.
        //
        // A Popup, not a ToolTip. WPF shows a ToolTip only while its window is active,
        // and hovering never activates the tray, which is what keeps the tray from
        // taking focus. MouseEnter still fires on an inactive window, so opening a Popup
        // in code works.
        _card.PlacementTarget = this;
        _card.Placement = PlacementMode.Right;
        _card.HorizontalOffset = 10;
        _card.AllowsTransparency = true;
        _card.IsHitTestVisible = false;

        PreviewMouseLeftButtonDown += OnDown;
        PreviewMouseMove += OnMove;
        PreviewMouseLeftButtonUp += OnUp;
        MouseEnter += (_, _) =>
        {
            BorderBrush = NoteColor.Tint(Colors.White, 0.5);
            OpenCardAfterDelay();
        };
        MouseLeave += (_, _) =>
        {
            BorderBrush = NoteColor.Tint(Colors.White, 0.10);
            CloseCard();
        };
        // The tray discards its rows when it folds up, which can happen while the
        // pointer is still on one. MouseLeave never arrives in that case.
        Unloaded += (_, _) => CloseCard();
    }

    private const double PreviewWidth = 420;
    private const double PreviewHeight = 340;

    private readonly Popup _card = new();
    private DispatcherTimer? _cardDelay;

    /// Opens the card after a short wait, so cards do not flash past while the pointer
    /// travels down the tray.
    private void OpenCardAfterDelay()
    {
        _cardDelay?.Stop();
        _cardDelay = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(400) };
        _cardDelay.Tick += (_, _) =>
        {
            _cardDelay?.Stop();
            _cardDelay = null;
            if (!IsMouseOver) return;
            _card.Child = HoverCard(_shot);
            _card.IsOpen = true;
        };
        _cardDelay.Start();
    }

    private void CloseCard()
    {
        _cardDelay?.Stop();
        _cardDelay = null;
        _card.IsOpen = false;
        _card.Child = null;   // release the preview bitmap
    }

    /// Builds the card contents: the picture, its file name, and its age. Rebuilt on
    /// each open, because Shot.Preview does not cache.
    private static FrameworkElement HoverCard(Shot shot)
    {
        var stack = new StackPanel();

        var picture = shot.Preview((int)PreviewWidth);
        if (picture is not null)
        {
            stack.Children.Add(new Border
            {
                CornerRadius = new CornerRadius(6),
                ClipToBounds = true,
                Child = new Image
                {
                    Source = picture,
                    Stretch = Stretch.Uniform,
                    MaxWidth = PreviewWidth,
                    MaxHeight = PreviewHeight,
                },
            });
        }

        stack.Children.Add(new TextBlock
        {
            Text = shot.Name,
            FontFamily = Ink.SystemFace,
            FontSize = 11.5,
            FontWeight = FontWeights.SemiBold,
            Foreground = NoteColor.Tint(Colors.White, 0.9),
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxWidth = PreviewWidth,
            Margin = new Thickness(2, picture is null ? 0 : 8, 2, 0),
        });

        stack.Children.Add(new TextBlock
        {
            Text = $"{Fmt.Ago(shot.Taken)} · drag out · click to open · right-click to copy",
            FontFamily = Ink.SystemFace,
            FontSize = 10.5,
            Foreground = NoteColor.Tint(Colors.White, 0.5),
            TextWrapping = TextWrapping.Wrap,
            MaxWidth = PreviewWidth,
            Margin = new Thickness(2, 2, 2, 0),
        });

        return new Border
        {
            Background = new SolidColorBrush(Color.FromArgb(0xF7, 0x1C, 0x1C, 0x20)),
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(10),
            IsHitTestVisible = false,
            Effect = new DropShadowEffect
            {
                Color = Colors.Black,
                Opacity = 0.45,
                BlurRadius = 18,
                ShadowDepth = 4,
                Direction = 340,
            },
            Child = stack,
        };
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
    internal static ControlTemplate RoundButtonTemplate()
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
