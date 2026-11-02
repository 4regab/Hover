using System.Diagnostics;
using System.Text;
using System.Threading.Channels;
using Hover.Host.Protocol;

namespace Hover.Host;

/// <summary>How to start the backend.</summary>
public sealed record BackendOptions
{
    /// <summary>Path of hover-backend(.exe).</summary>
    public required string ExecutablePath { get; init; }

    /// <summary>Becomes HOVER_DATA_DIR. Never the command line: nothing secret goes there.</summary>
    public required string DataDir { get; init; }

    /// <summary>Command-line arguments. The real backend takes none; this is for stand-ins in tests.</summary>
    public IReadOnlyList<string> Arguments { get; init; } = [];

    /// <summary>Environment variables to add to the host's. A null value removes the variable.</summary>
    public IReadOnlyDictionary<string, string?> Environment { get; init; } = new Dictionary<string, string?>();

    /// <summary>
    /// The longest line read from the backend. A longer one is dropped and the next line is
    /// read as usual (64 MiB, as the macOS host does).
    /// </summary>
    public int MaxLineBytes { get; init; } = 64 * 1024 * 1024;

    /// <summary>
    /// Whether this host runs the agent browser. When it does not, the backend is told not to
    /// offer one to agents (HOVER_NO_BROWSER): a call nobody answers would only hang.
    /// </summary>
    public bool HostRunsBrowser { get; init; }

    /// <summary>
    /// Whether to put the backend in a job object that kills it, and everything it started,
    /// when the host ends. Windows only; on by default.
    /// </summary>
    public bool UseJobObject { get; init; } = true;

    /// <summary>hover-backend.exe in the folder of the running app.</summary>
    public static string BesideApp() => Path.Combine(
        AppContext.BaseDirectory, OperatingSystem.IsWindows() ? "hover-backend.exe" : "hover-backend");
}

/// <summary>
/// The host's side of the backend: starts hover-backend, keeps it in a job object, sends
/// commands and raises its messages. One writer, one reader, and nothing from the backend is
/// trusted: an over-long line, bad JSON or an unknown type is dropped and the next line is read.
/// </summary>
public sealed class BackendClient : IAsyncDisposable
{
    readonly Process process;
    readonly IDisposable? job;
    readonly Channel<byte[]> outgoing = Channel.CreateUnbounded<byte[]>(new UnboundedChannelOptions { SingleReader = true });
    readonly List<EventQueue> queues = [];
    readonly TaskCompletionSource<int> exited = new(TaskCreationOptions.RunContinuationsAsynchronously);
    readonly Task reading;
    readonly Task writing;
    readonly Task watching;
    volatile bool stopping;
    volatile bool failureSeen;

    /// <summary>A message arrived. Raised on a thread of the client's, one message at a time.</summary>
    public event Action<BackendEvent>? Event;

    /// <summary>A line the backend wrote to stderr.</summary>
    public event Action<string>? StderrLine;

    /// <summary>The backend process ended, with its exit code.</summary>
    public event Action<int>? Exited;

    /// <summary>Lines dropped so far: too long, not JSON, or invalid for their type.</summary>
    public int DroppedLines => dropped;
    int dropped;

    public int ProcessId { get; }

    BackendClient(Process process, IDisposable? job, BackendOptions options)
    {
        this.process = process;
        this.job = job;
        ProcessId = process.Id;
        reading = Task.Run(() => ReadAsync(options.MaxLineBytes));
        writing = Task.Run(WriteAsync);
        var errors = Task.Run(ReadErrorsAsync);
        watching = Task.Run(() => WatchAsync(errors));
    }

    /// <summary>
    /// Starts the backend, already inside its job object. The backend does nothing until it
    /// reads <c>initialize</c>, which the caller sends after this returns, so nothing it starts
    /// can be outside the job.
    /// </summary>
    public static BackendClient Start(BackendOptions options)
    {
        var info = new ProcessStartInfo(options.ExecutablePath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardInputEncoding = new UTF8Encoding(false),
            StandardOutputEncoding = new UTF8Encoding(false),
            StandardErrorEncoding = new UTF8Encoding(false),
        };
        foreach (var argument in options.Arguments) info.ArgumentList.Add(argument);
        info.Environment[DataFolder.OverrideVariable] = options.DataDir;
        if (!options.HostRunsBrowser) info.Environment["HOVER_NO_BROWSER"] = "1";
        foreach (var (name, value) in options.Environment)
        {
            if (value is null) info.Environment.Remove(name);
            else info.Environment[name] = value;
        }

        IDisposable? job = null;
        if (options.UseJobObject && OperatingSystem.IsWindows()) job = JobObject.CreateKillOnClose();
        Process process;
        try
        {
            process = Process.Start(info) ?? throw new InvalidOperationException("The backend did not start.");
        }
        catch
        {
            job?.Dispose();
            throw;
        }
        try
        {
            if (job is not null && OperatingSystem.IsWindows()) ((JobObject)job).Assign(process);
        }
        catch
        {
            // Without the job the backend could outlive the host: do not run it.
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            job?.Dispose();
            process.Dispose();
            throw;
        }
        return new BackendClient(process, job, options);
    }

    /// <summary>Messages from now on, for code that waits for one. Make it before the command it waits on.</summary>
    public EventQueue Subscribe()
    {
        var queue = new EventQueue();
        lock (queues) queues.Add(queue);
        if (exited.Task.IsCompleted) queue.Complete(ExitReason(exited.Task.Result));
        return queue;
    }

    /// <summary>Queues a command. Never blocks; a backend that is gone drops it.</summary>
    public void Send(BackendCommand command) => outgoing.Writer.TryWrite(ProtocolJson.Serialize(command));

    /// <summary>
    /// Sends <c>initialize</c> with the history key (none: the history stays off) and waits for
    /// the backend to say it is ready. Throws <see cref="BackendException"/> with the backend's
    /// own reason when it refuses the key (a history it cannot open, say).
    /// </summary>
    public async Task InitializeAsync(byte[]? historyKey, TimeSpan? timeout = null, bool hostDiscord = false, bool credits = false)
    {
        var queue = Subscribe();
        Send(new InitializeCommand(historyKey is null ? null : Convert.ToBase64String(historyKey))
        {
            HostDiscord = hostDiscord ? true : null,
            Credits = credits ? true : null,
        });
        await queue.Until<InitializedEvent>(timeout: timeout ?? TimeSpan.FromSeconds(30)).ConfigureAwait(false);
    }

    /// <summary>
    /// The orderly end: <c>shutdown</c>, stdin closed, then wait for the backend to write the
    /// history and exit. If it has not by the timeout it is killed with everything it started.
    /// Returns its exit code.
    /// </summary>
    public async Task<int> StopAsync(TimeSpan? timeout = null)
    {
        stopping = true;
        if (!exited.Task.IsCompleted)
        {
            Send(new ShutdownCommand());
            outgoing.Writer.TryComplete();
            await Task.WhenAny(writing, Task.Delay(2000)).ConfigureAwait(false);
            CloseInput();
        }
        return await WaitForExitAsync(timeout ?? TimeSpan.FromSeconds(15)).ConfigureAwait(false);
    }

    /// <summary>
    /// Closes the backend's stdin without a word, as a host that died would. The backend must
    /// stop its tools, write the history and exit.
    /// </summary>
    public void CloseInput()
    {
        try { process.StandardInput.Close(); } catch (IOException) { } catch (ObjectDisposedException) { }
    }

    /// <summary>Waits for the backend to end; kills it and its tree when it has not by the timeout.</summary>
    public async Task<int> WaitForExitAsync(TimeSpan timeout)
    {
        if (await Task.WhenAny(exited.Task, Task.Delay(timeout)).ConfigureAwait(false) != exited.Task)
        {
            HostLog.Line($"backend pid {ProcessId} did not exit in {timeout.TotalSeconds:0}s; killing it");
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
        }
        return await exited.Task.ConfigureAwait(false);
    }

    /// <summary>Ends the backend if it still runs, without the orderly shutdown, then lets go of everything.</summary>
    public async ValueTask DisposeAsync()
    {
        if (!exited.Task.IsCompleted)
        {
            stopping = true;
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
            await Task.WhenAny(exited.Task, Task.Delay(5000)).ConfigureAwait(false);
        }
        outgoing.Writer.TryComplete();
        job?.Dispose();
        process.Dispose();
    }

    async Task ReadAsync(int maxLineBytes)
    {
        try
        {
            await LineReader.ReadAsync(
                process.StandardOutput.BaseStream,
                maxLineBytes,
                line =>
                {
                    var message = ProtocolJson.Parse(line, out var problem);
                    if (message is null)
                    {
                        Interlocked.Increment(ref dropped);
                        HostLog.Line($"backend line dropped: {problem}");
                        return;
                    }
                    Deliver(message);
                },
                length =>
                {
                    Interlocked.Increment(ref dropped);
                    HostLog.Line($"backend line dropped: {length} bytes is over the limit");
                }).ConfigureAwait(false);
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException)
        {
            // The pipe broke: the process is ending, and WatchAsync reports that.
        }
    }

    void Deliver(BackendEvent message)
    {
        if (message is BackendFailureEvent)
        {
            // After a failure the backend does nothing more but wait; ending its input lets it go.
            failureSeen = true;
            CloseInput();
        }
        EventQueue[] snapshot;
        lock (queues) snapshot = [.. queues];
        foreach (var queue in snapshot) queue.Add(message);
        try
        {
            Event?.Invoke(message);
        }
        catch (Exception e)
        {
            // A handler's bug must not stop the messages that follow.
            HostLog.Line($"backend message handler failed: {e}");
        }
    }

    async Task WriteAsync()
    {
        var input = process.StandardInput.BaseStream;
        try
        {
            await foreach (var line in outgoing.Reader.ReadAllAsync().ConfigureAwait(false))
            {
                await input.WriteAsync(line).ConfigureAwait(false);
                await input.WriteAsync("\n"u8.ToArray()).ConfigureAwait(false);
                await input.FlushAsync().ConfigureAwait(false);
            }
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException)
        {
            // The backend is gone. What was queued is dropped; say so once.
            outgoing.Writer.TryComplete();
            if (!stopping) Deliver(new ToastEvent { Text = "The backend connection closed." });
        }
    }

    async Task ReadErrorsAsync()
    {
        try
        {
            await LineReader.ReadAsync(
                process.StandardError.BaseStream,
                1024 * 1024,
                line =>
                {
                    try { StderrLine?.Invoke(Encoding.UTF8.GetString(line)); }
                    catch (Exception e) { HostLog.Line($"backend stderr handler failed: {e}"); }
                },
                _ => { }).ConfigureAwait(false);
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException)
        {
        }
    }

    async Task WatchAsync(Task errors)
    {
        await process.WaitForExitAsync().ConfigureAwait(false);
        // Let what the backend wrote last be read first. A grandchild that still holds the
        // pipe must not keep this waiting.
        await Task.WhenAny(Task.WhenAll(reading, errors), Task.Delay(2000)).ConfigureAwait(false);

        var code = process.ExitCode;
        if (!stopping && !failureSeen)
            Deliver(new BackendFailureEvent { Text = $"The agent backend stopped ({code}). Quit Hover from its tray menu and open it again." });
        exited.TrySetResult(code);
        EventQueue[] snapshot;
        lock (queues) snapshot = [.. queues];
        foreach (var queue in snapshot) queue.Complete(ExitReason(code));
        try { Exited?.Invoke(code); } catch (Exception e) { HostLog.Line($"backend exit handler failed: {e}"); }
    }

    static string ExitReason(int code) => $"The agent backend stopped ({code}).";
}
