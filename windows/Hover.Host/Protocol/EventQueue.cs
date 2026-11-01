using System.Threading.Channels;

namespace Hover.Host.Protocol;

/// <summary>The backend failed to start or went away while the caller waited for a message.</summary>
public sealed class BackendException(string message) : Exception(message);

/// <summary>
/// The messages that arrive after it is made, in order, for code that waits for one: a test,
/// or a start-up that waits for <c>initialized</c>. Messages passed over while waiting are
/// dropped, as a wait for a later one does not want the earlier ones.
/// </summary>
public sealed class EventQueue
{
    readonly Channel<BackendEvent> channel = Channel.CreateUnbounded<BackendEvent>(
        new UnboundedChannelOptions { SingleReader = true });

    internal void Add(BackendEvent e) => channel.Writer.TryWrite(e);

    internal void Complete(string reason) => channel.Writer.TryComplete(new BackendException(reason));

    /// <summary>
    /// The next message of type T that satisfies the predicate. A <see cref="BackendFailureEvent"/>
    /// or the backend ending stops the wait with a <see cref="BackendException"/>, unless that is
    /// what is waited for.
    /// </summary>
    public async Task<T> Until<T>(Func<T, bool>? predicate = null, TimeSpan? timeout = null) where T : BackendEvent
    {
        using var cancel = new CancellationTokenSource(timeout ?? TimeSpan.FromSeconds(25));
        try
        {
            while (await channel.Reader.WaitToReadAsync(cancel.Token).ConfigureAwait(false))
            {
                while (channel.Reader.TryRead(out var e))
                {
                    if (e is T match && (predicate is null || predicate(match))) return match;
                    if (e is BackendFailureEvent failure)
                        throw new BackendException(failure.Text);
                }
            }
        }
        catch (OperationCanceledException)
        {
            throw new BackendException($"Timed out waiting for {typeof(T).Name}.");
        }
        throw new BackendException($"The backend ended before sending {typeof(T).Name}.");
    }
}
