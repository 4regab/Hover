using System.Threading;
using System.Windows;
using Hover.Core;
using Hover.Images;
using Hover.Interop;
using Hover.Owl;
using Hover.Services;

namespace Hover;

public partial class App : Application
{
    private static Mutex? _single;

    private NotchManager? _notch;
    private HotKeys? _hotKeys;
    private TrayIcon? _tray;
    private string? _reportedHotKeyFailures;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        // One notch per display is the point; two copies of the app is not.
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

        Actions.OnShortcutsChanged = RegisterHotKeys;

        // Screenshots and copied pictures land in the workspace's Screenshots card.
        ShotStore.Shared.Start();

        // The workspace: tasks, focus timer, notepad, events and screenshots at the
        // top centre.
        OwlApp.Start();
        _notch = new NotchManager();

        _hotKeys = new HotKeys();
        RegisterHotKeys();

        _tray = new TrayIcon();
        OwlApp.Notify = (title, text) =>
        {
            _notch?.Alert(title, text);
            _tray?.Notify(title, text);
        };

        Log.Line("started");
    }

    /// Rebinding in Settings tears the set down and puts it back, which is the only
    /// way RegisterHotKey lets a binding change.
    private void RegisterHotKeys()
    {
        if (_hotKeys is null) return;
        _hotKeys.Clear();

        var shortcut = Settings.ScWorkspace;
        if (_hotKeys.Register(shortcut, () => _notch?.Toggle()))
        {
            _reportedHotKeyFailures = null;
            return;
        }

        var signature = shortcut.ToString();
        if (signature == _reportedHotKeyFailures) return;
        _reportedHotKeyFailures = signature;

        // Run after the current key event (and, at startup, after the tray icon has
        // been created) so a failed registration cannot disappear into the log.
        Dispatcher.BeginInvoke(() =>
        {
            if (_reportedHotKeyFailures != signature) return;
            ShowHotKeyWarning(shortcut);
        });
    }

    private static void ShowHotKeyWarning(Shortcut shortcut)
    {
        var message = $"Hover couldn't register the workspace shortcut, {shortcut}.\n\n" +
                      "Windows has reserved it or another app is already using it. " +
                      "Choose a different shortcut in Settings → General.";

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
        // Bank a running session's focus time before anything is torn down.
        OwlApp.Shutdown();
        _notch?.Dispose();
        _tray?.Dispose();
        _hotKeys?.Dispose();
        ShotStore.Shared.Dispose();
        Settings.Flush();
        base.OnExit(e);
    }
}
