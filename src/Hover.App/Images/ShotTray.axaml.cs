using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;

namespace Hover.Images;

/// The screenshot tray, in the dark "Canvas" look.
///
/// It only shows what it is given and says what was asked of it. The pictures live in
/// ShotStore and the controller owns that, which keeps this a plain view that can be
/// rendered and checked on its own.
public partial class ShotTray : UserControl, INotifyPropertyChanged
{
    public ObservableCollection<ShotGroup> Groups { get; } = new();

    private int _count;

    public int Count
    {
        get => _count;
        private set
        {
            if (_count == value) return;
            _count = value;
            Raise();
            Raise(nameof(HasShots));
            Raise(nameof(IsEmpty));
        }
    }

    public bool HasShots => Count > 0;
    public bool IsEmpty => Count == 0;

    /// The Snip button was pressed.
    public event EventHandler? SnipRequested;

    /// A picture's delete button was pressed.
    public event EventHandler<Shot>? DeleteRequested;

    /// A picture is being dragged out of the tray.
    public event EventHandler<Shot>? DragRequested;

    public ShotTray()
    {
        InitializeComponent();
        DataContext = this;
        AddHandler(ShotRowView.DeleteRequestedEvent, OnRowDelete);
        AddHandler(ShotRowView.DragRequestedEvent, OnRowDrag);
    }

    /// Replaces everything on show. Newest day first.
    public void Show(IReadOnlyList<Shot> shots)
    {
        Groups.Clear();
        foreach (var group in ShotGroups.Build(shots, DateTime.Now)) Groups.Add(group);
        Count = shots.Count;
    }

    private void OnSnip(object? sender, RoutedEventArgs e) =>
        SnipRequested?.Invoke(this, EventArgs.Empty);

    private void OnRowDelete(object? sender, RoutedEventArgs e)
    {
        if (e.Source is ShotRowView { Item: not null } row)
            DeleteRequested?.Invoke(this, row.Item.Shot);
    }

    private void OnRowDrag(object? sender, RoutedEventArgs e)
    {
        if (e.Source is ShotRowView { Item: not null } row)
            DragRequested?.Invoke(this, row.Item.Shot);
    }

    // MARK: Change notification

    /// Hides the control's own event of the same name; the bindings here need the
    /// plain interface version.
    public new event PropertyChangedEventHandler? PropertyChanged;

    private void Raise([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
