using System.Threading;
using System.Windows;
using Hover.Core;
using Hover.Deck;
using Hover.Images;
using Hover.Interop;
using Hover.Services;
using Hover.Windows;

namespace Hover;

public partial class App : Application
{
    private static Mutex? _single;

    private DeckManager? _decks;
    private ImageStripManager? _imageStrips;
    private HotKeys? _hotKeys;
    private TrayIcon? _tray;
    private string? _reportedHotKeyFailures;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        // One deck per display is the point; two copies of the app is not.
        _single = new Mutex(true, "Local\\HoverRunningInstance", out var fresh);
        if (!fresh)
        {
            Shutdown();
            return;
        }

        DispatcherUnhandledException += (_, args) =>
        {
            Log.Line($"unhandled — {args.Exception}");
            args.Handled = true;
        };

        _decks = new DeckManager();
        Actions.Decks = _decks;
        Actions.OnShortcutsChanged = RegisterHotKeys;

        // The image tray watches for screenshots and copied pictures, and shows them
        // on the opposite screen edge from the note deck.
        ShotStore.Shared.Start();
        _imageStrips = new ImageStripManager();
        Actions.ImageStrips = _imageStrips;

        _hotKeys = new HotKeys();
        RegisterHotKeys();

        _tray = new TrayIcon();
        UndoToast.Shared.Start();

        // Nothing is on screen at this point — the panels are hidden and there is no
        // main window — so a first run needs telling where the app went.
        WelcomeWindow.ShowOnce();

        Log.Line("started");
    }

    /// Rebinding in Settings tears the whole set down and puts it back, which is the
    /// only way RegisterHotKey lets a binding change.
    private void RegisterHotKeys()
    {
        if (_hotKeys is null) return;
        _hotKeys.Clear();

        var failed = new List<(string Command, Shortcut Shortcut)>();
        TryRegister("New note", Settings.ScNewNote, Actions.NewNote);
        TryRegister("All Notes", Settings.ScAllNotes, Actions.OpenAllNotes);
        TryRegister("Archive", Settings.ScArchive, Actions.OpenArchive);
        TryRegister("New screenshot", Settings.ScSnip, Actions.Snip);

        if (failed.Count == 0)
        {
            _reportedHotKeyFailures = null;
            return;
        }

        var signature = string.Join("\n", failed.Select(f => $"{f.Command}:{f.Shortcut}"));
        if (signature == _reportedHotKeyFailures) return;
        _reportedHotKeyFailures = signature;

        // Run after the current key event (and, at startup, after the tray icon has
        // been created) so a failed registration cannot disappear into the log.
        Dispatcher.BeginInvoke(() =>
        {
            if (_reportedHotKeyFailures != signature) return;
            ShowHotKeyWarning(failed);
        });

        void TryRegister(string command, Shortcut shortcut, Action action)
        {
            if (!_hotKeys.Register(shortcut, action)) failed.Add((command, shortcut));
        }
    }

    private static void ShowHotKeyWarning(IReadOnlyList<(string Command, Shortcut Shortcut)> failed)
    {
        var bindings = string.Join("\n", failed.Select(f => $"• {f.Command} — {f.Shortcut}"));
        var subject = failed.Count == 1 ? "this global shortcut" : "these global shortcuts";
        var message = $"Hover couldn't register {subject}:\n\n{bindings}\n\n" +
                      "Windows has reserved the shortcut or another app is already using it. " +
                      "Choose a different shortcut in Settings.";

        var owner = Current.Windows.OfType<Window>().FirstOrDefault(w => w.IsActive);
        if (owner is not null)
            MessageBox.Show(owner, message, "Global shortcut unavailable",
                MessageBoxButton.OK, MessageBoxImage.Warning);
        else
            MessageBox.Show(message, "Global shortcut unavailable",
                MessageBoxButton.OK, MessageBoxImage.Warning);
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _tray?.Dispose();
        _hotKeys?.Dispose();
        _decks?.Dispose();
        _imageStrips?.Dispose();
        ShotStore.Shared.Dispose();
        Settings.Flush();
        base.OnExit(e);
    }
}
