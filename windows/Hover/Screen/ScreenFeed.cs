using Hover.Chat;
using Hover.Chat.Desk;
using Hover.Chatting;
using Hover.Host;
using Microsoft.UI.Dispatching;

namespace Hover.Screening;

/// <summary>
/// The panels' Screen tab on this app: a picture is taken off the UI thread (<see cref="ScreenSource"/>), kept in the panels' picture
/// store under an address of its own, and handed over on the UI thread. A picture the panels have done with is let go.
/// </summary>
sealed class ScreenFeed(ScreenSource source, ImageStore images, DispatcherQueue queue) : IScreenFeed
{
    int counter;

    public bool Supported => ScreenSource.Supported;

    /// <summary>For the pictures of <c>--shots</c>: a made-up screen taken at once in place of the real one (the argument says whether it is live).</summary>
    public Func<bool, Hover.Settings.ScreenFrame>? Fixture { get; set; }

    public void Take(IReadOnlyList<uint> pids, bool live, Action<string?, string?> done)
    {
        var asked = pids.ToArray();
        if (Fixture is { } fixture)
        {
            var frame = fixture(live);
            var at = "hover://screen/" + Interlocked.Increment(ref counter);
            images.Put(at, frame.Width, frame.Height, frame.Bgra);
            done(at, null);
            return;
        }
        _ = Run();

        async Task Run()
        {
            string? src = null, why = null;
            try
            {
                var frame = await source.CaptureAsync(asked, live);
                src = "hover://screen/" + Interlocked.Increment(ref counter);
                images.Put(src, frame.Width, frame.Height, frame.Bgra);
            }
            catch (Exception e)
            {
                HostLog.Line($"screen: no picture — {e.Message}");
                why = "Windows didn’t give a picture of the screen.";
            }
            if (queue.TryEnqueue(() => done(src, why))) return;
            // The window is gone: nobody will show it.
            if (src is not null) images.Forget(src);
        }
    }

    public void Release(string src) => images.Forget(src);
}
