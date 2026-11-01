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
public sealed class SingleInstance : IDisposable
{
    public const string MutexName = @"Local\HoverRunningInstance";
    public const string ShowEventName = @"Local\HoverShowApp";

    readonly Mutex mutex;
    readonly EventWaitHandle show;
    readonly RegisteredWaitHandle registration;

    SingleInstance(Mutex mutex, EventWaitHandle show, Action onShow)
    {
        this.mutex = mutex;
        this.show = show;
        // Runs on a thread-pool thread each time a later launch sets the event.
        registration = ThreadPool.RegisterWaitForSingleObject(
            show, (_, _) => onShow(), null, Timeout.Infinite, executeOnlyOnce: false);
    }

    /// <summary>
    /// Claims the instance and returns it, or returns null when another copy runs. In that
    /// case the running copy has been asked to open its dashboard. <paramref name="onShow"/>
    /// is called, on a pool thread, for every later launch.
    /// </summary>
    public static SingleInstance? Claim(Action onShow, string mutexName = MutexName, string showName = ShowEventName)
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
        return new SingleInstance(mutex, new EventWaitHandle(false, EventResetMode.AutoReset, showName), onShow);
    }

    public void Dispose()
    {
        registration.Unregister(null);
        show.Dispose();
        try { mutex.ReleaseMutex(); } catch (ApplicationException) { /* released from another thread: the handle closing frees it */ }
        mutex.Dispose();
    }

    [DllImport("user32.dll")]
    static extern bool AllowSetForegroundWindow(int processId);
}
