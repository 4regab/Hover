using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Hover.Host;

/// <summary>
/// One copy of Hover per user. The first copy holds the named mutex
/// <c>Local\HoverRunningInstance</c> and waits on the event <c>Local\HoverShowApp</c>. A
/// later launch sets that event and exits. The Go app uses the same two names, so a Go copy
/// and a C# copy keep out of each other's way and either one answers the other's launch.
/// </summary>
[SupportedOSPlatform("windows")]
public sealed class InstanceLock : IDisposable
{
    public const string MutexName = @"Local\HoverRunningInstance";
    public const string ShowEventName = @"Local\HoverShowApp";

    /// <summary>
    /// <c>hoverai.exe --quit</c> sets this event: the running copy ends in order (its agents are told, the history is written). The setup
    /// asks first this way, and only ends the program if it has not gone after a while.
    /// </summary>
    public const string QuitEventName = @"Local\HoverQuitApp";

    readonly Mutex mutex;
    readonly EventWaitHandle show;
    readonly EventWaitHandle? quit;
    readonly RegisteredWaitHandle registration;
    readonly RegisteredWaitHandle? quitRegistration;

    InstanceLock(Mutex mutex, EventWaitHandle show, Action onShow, EventWaitHandle? quit, Action? onQuit)
    {
        this.mutex = mutex;
        this.show = show;
        this.quit = quit;
        // Runs on a thread-pool thread each time a later launch sets the event.
        registration = ThreadPool.RegisterWaitForSingleObject(
            show, (_, _) => onShow(), null, Timeout.Infinite, executeOnlyOnce: false);
        if (quit is not null && onQuit is not null)
            quitRegistration = ThreadPool.RegisterWaitForSingleObject(quit, (_, _) => onQuit(), null, Timeout.Infinite, executeOnlyOnce: true);
    }

    /// <summary>
    /// Claims the instance and returns it, or returns null when another copy runs. In that
    /// case the running copy has been asked to open its dashboard. <paramref name="onShow"/>
    /// is called, on a pool thread, for every later launch.
    /// </summary>
    public static InstanceLock? Claim(Action onShow, string mutexName = MutexName, string showName = ShowEventName, Action? onQuit = null, string quitName = QuitEventName)
    {
        var mutex = new Mutex(initiallyOwned: true, mutexName, out var first);
        if (!first)
        {
            mutex.Dispose();
            // This process was just launched by the user, so it may pass on the right to take focus.
            if (EventWaitHandle.TryOpenExisting(showName, out var running))
            {
                AllowSetForegroundWindow(-1);
                running.Set();
                running.Dispose();
            }
            return null;
        }
        return new InstanceLock(mutex, new EventWaitHandle(false, EventResetMode.AutoReset, showName), onShow,
            onQuit is null ? null : new EventWaitHandle(false, EventResetMode.ManualReset, quitName), onQuit);
    }

    /// <summary>
    /// Asks the running copy to end, and waits for it to be gone (its mutex is released when its process ends). True when none runs now.
    /// False when there is one that did not end by the timeout.
    /// </summary>
    public static bool AskToQuit(TimeSpan timeout, string mutexName = MutexName, string quitName = QuitEventName)
    {
        if (!Mutex.TryOpenExisting(mutexName, out var running)) return true;
        running.Dispose();
        if (EventWaitHandle.TryOpenExisting(quitName, out var quit))
        {
            quit.Set();
            quit.Dispose();
        }
        for (var end = Environment.TickCount64 + (long)timeout.TotalMilliseconds; Environment.TickCount64 < end;)
        {
            if (!Mutex.TryOpenExisting(mutexName, out var still)) return true;
            still.Dispose();
            Thread.Sleep(200);
        }
        return false;
    }

    public void Dispose()
    {
        registration.Unregister(null);
        quitRegistration?.Unregister(null);
        show.Dispose();
        quit?.Dispose();
        try { mutex.ReleaseMutex(); } catch (ApplicationException) { /* released from another thread: the handle closing frees it */ }
        mutex.Dispose();
    }

    [DllImport("user32.dll")]
    static extern bool AllowSetForegroundWindow(int processId);
}
