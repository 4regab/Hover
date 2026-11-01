using System.Runtime.InteropServices;
using Hover.Host;

namespace Hover.Voice;

/// <summary>A microphone that records until it is stopped. Blocks go to the callback it was opened with.</summary>
public interface IMicrophone : IDisposable
{
    /// <summary>Lets the device go at once and hands over the last block. No block comes after it returns.</summary>
    void Stop();
}

/// <summary>An opened microphone, or the words that say why there is none (the Go app's own).</summary>
public sealed record MicOpen(IMicrophone? Mic, string? Error);

/// <summary>Opens the microphone named in the settings ("" or null is the system's default).</summary>
public delegate MicOpen MicOpener(string? device, Action<short[]> block, Action<string> failed);

/// <summary>
/// The input devices of winmm, and the choice of one by the name in <c>settings.json</c>. winmm
/// names a device in 31 characters, so a longer stored name is matched by its start, as the Go
/// app does (internal/voice/audio_windows.go).
/// </summary>
public static class MicDevices
{
    /// <summary>The most characters winmm puts in a device name.</summary>
    public const int NameLimit = 31;

    /// <summary>The Go app's words for each way a microphone can be missing.</summary>
    public const string NoneConnected = "No microphone is connected.";

    public static string NotConnected(string device) => $"The microphone “{device}” isn’t connected. Pick another in Settings → Voice.";

    /// <summary>The device id that <paramref name="wanted"/> names, or -1: the same name, or a name cut at 31 that the stored one starts with.</summary>
    public static int Pick(IReadOnlyList<string> names, string wanted)
    {
        for (var i = 0; i < names.Count; i++)
        {
            var name = names[i];
            if (name.Length == 0) continue;
            if (name == wanted || (name.Length >= NameLimit && wanted.StartsWith(name, StringComparison.Ordinal))) return i;
        }
        return -1;
    }

    /// <summary>The names by device id (an empty text where a device would not answer). Nothing off Windows.</summary>
    public static IReadOnlyList<string> RawNames()
    {
        if (!OperatingSystem.IsWindows()) return [];
        var count = (int)WinMm.waveInGetNumDevs();
        var names = new List<string>(count);
        for (var i = 0; i < count; i++)
        {
            var caps = new WaveInCaps();
            names.Add(WinMm.waveInGetDevCapsW((nuint)i, ref caps, (uint)Marshal.SizeOf<WaveInCaps>()) == WinMm.NoError ? caps.Name ?? "" : "");
        }
        return names;
    }

    /// <summary>The names Settings lists: each once, in order.</summary>
    public static IReadOnlyList<string> Names() => RawNames().Where(n => n.Length > 0).Distinct().ToList();

    /// <summary>The words for a winmm error from opening the device.</summary>
    public static string OpenError(uint code) => code switch
    {
        4 => "Another app is using the microphone.", // MMSYSERR_ALLOCATED
        2 or 6 or 7 => "The microphone isn’t available any more.", // BADDEVICEID, NODRIVER, NOMEM
        _ => $"The microphone failed: error {code}",
    };

    /// <summary>The real microphone through winmm. With no device, or a device that is not there, the result says why.</summary>
    public static MicOpen Open(string? device, Action<short[]> block, Action<string> failed)
    {
        if (!OperatingSystem.IsWindows()) return new MicOpen(null, NoneConnected);
        var names = RawNames();
        nuint id = WinMm.Mapper;
        if (!string.IsNullOrEmpty(device))
        {
            // The one picked, or an error: recording another microphone without saying so is not on.
            var at = Pick(names, device);
            if (at < 0) return new MicOpen(null, NotConnected(device));
            id = (nuint)at;
        }
        else if (names.Count == 0) return new MicOpen(null, NoneConnected);

        try
        {
            return WinMmMicrophone.Open(id, block, failed);
        }
        catch (Exception e)
        {
            HostLog.Line($"voice: the microphone could not be opened — {e.Message}");
            return new MicOpen(null, "The microphone failed: " + e.Message);
        }
    }
}

/// <summary>waveIn at 16 kHz mono 16-bit, in four buffers of 50 ms taken as the device fills each (a wait on an event).</summary>
sealed unsafe class WinMmMicrophone : IMicrophone
{
    const int Buffers = 4;
    const int BufferSamples = 16_000 / 20;

    nint handle;
    readonly AutoResetEvent ready = new(false);
    readonly nint[] headers = new nint[Buffers];
    readonly nint[] data = new nint[Buffers];
    readonly Action<short[]> block;
    readonly Action<string> failed;
    readonly Thread thread;
    volatile bool stopping;
    bool released;
    int next;

    WinMmMicrophone(Action<short[]> block, Action<string> failed)
    {
        this.block = block;
        this.failed = failed;
        for (var i = 0; i < Buffers; i++)
        {
            data[i] = (nint)NativeMemory.AllocZeroed((nuint)(BufferSamples * 2));
            headers[i] = (nint)NativeMemory.AllocZeroed((nuint)WinMm.HeaderSize);
            var h = (WaveHdr*)headers[i];
            h->Data = data[i];
            h->BufferLength = BufferSamples * 2;
        }
        thread = new Thread(Run) { IsBackground = true, Name = "Hover microphone" };
    }

    /// <summary>Opens the device, queues the buffers and starts it. The driver signals the instance's own event.</summary>
    public static MicOpen Open(nuint id, Action<short[]> block, Action<string> failed)
    {
        var m = new WinMmMicrophone(block, failed);
        var format = WaveFormat.Pcm(16_000, 1);
        var code = WinMm.waveInOpen(out m.handle, id, ref format, m.ready.SafeWaitHandle.DangerousGetHandle(), 0, WinMm.CallbackEvent);
        if (code != WinMm.NoError)
        {
            m.FreeMemory();
            return new MicOpen(null, MicDevices.OpenError(code));
        }
        for (var i = 0; i < Buffers; i++)
        {
            code = WinMm.waveInPrepareHeader(m.handle, m.headers[i], WinMm.HeaderSize);
            if (code == WinMm.NoError) code = WinMm.waveInAddBuffer(m.handle, m.headers[i], WinMm.HeaderSize);
            if (code != WinMm.NoError)
            {
                m.Stop();
                return new MicOpen(null, MicDevices.OpenError(code));
            }
        }
        code = WinMm.waveInStart(m.handle);
        if (code != WinMm.NoError)
        {
            m.Stop();
            return new MicOpen(null, MicDevices.OpenError(code));
        }
        m.thread.Start();
        return new MicOpen(m, null);
    }

    public void Stop()
    {
        if (released) return;
        released = true;
        stopping = true;
        ready.Set();
        if (thread.IsAlive && Environment.CurrentManagedThreadId != thread.ManagedThreadId) thread.Join();
        // Reset hands every queued buffer back with what it holds, so the tail of the recording is kept.
        WinMm.waveInReset(handle);
        Drain(requeue: false);
        for (var i = 0; i < Buffers; i++) WinMm.waveInUnprepareHeader(handle, headers[i], WinMm.HeaderSize);
        WinMm.waveInClose(handle);
        FreeMemory();
    }

    void FreeMemory()
    {
        for (var i = 0; i < Buffers; i++)
        {
            NativeMemory.Free((void*)headers[i]);
            NativeMemory.Free((void*)data[i]);
        }
        ready.Dispose();
    }

    public void Dispose() => Stop();

    void Run()
    {
        while (!stopping)
        {
            ready.WaitOne(200);
            if (stopping) break;
            if (!Drain(requeue: true)) break;
        }
    }

    /// <summary>Takes the buffers the device has filled, in the order they were queued. False once the device fails.</summary>
    bool Drain(bool requeue)
    {
        while (true)
        {
            var h = (WaveHdr*)headers[next];
            if ((h->Flags & WinMm.HeaderDone) == 0) return true;
            var samples = (int)(h->BytesRecorded / 2);
            if (samples > 0)
            {
                var copy = new short[samples];
                new ReadOnlySpan<short>((void*)data[next], samples).CopyTo(copy);
                try { block(copy); }
                catch (Exception e) { HostLog.Line($"voice: a block of audio was not taken — {e.Message}"); }
            }
            h->Flags &= ~WinMm.HeaderDone;
            h->BytesRecorded = 0;
            var current = headers[next];
            next = (next + 1) % Buffers;
            if (!requeue) continue;
            var code = WinMm.waveInAddBuffer(handle, current, WinMm.HeaderSize);
            if (code != WinMm.NoError)
            {
                failed(MicDevices.OpenError(code));
                return false;
            }
        }
    }
}
