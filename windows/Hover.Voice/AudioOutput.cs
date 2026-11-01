using System.Runtime.InteropServices;
using Hover.Host;

namespace Hover.Voice;

/// <summary>An open sound stream. Closing it lets the device go, so a quiet Hover holds no stream.</summary>
public interface ISoundSink : IDisposable { }

/// <summary>An opened stream, or the reason there is none.</summary>
public sealed record AudioOpen(ISoundSink? Output, string? Error);

/// <summary>
/// Starts a stream. <c>fill</c> is called on a thread of the stream's own to fill each buffer with
/// interleaved stereo float frames at 44.1 kHz, and must not block.
/// </summary>
public delegate AudioOpen AudioOpener(Action<float[]> fill);

/// <summary>The system's sound output through winmm's waveOut: 16-bit stereo at 44.1 kHz in four buffers of 100 ms, refilled as the device finishes each.</summary>
public static class AudioOutput
{
    /// <summary>Frames a second and channels in a frame: what <c>fill</c> writes.</summary>
    public const int Rate = 44_100, Channels = 2;

    public static AudioOpen Open(Action<float[]> fill)
    {
        if (!OperatingSystem.IsWindows()) return new AudioOpen(null, "There is no sound output on this system.");
        try
        {
            return WinMmOutput.Open(fill);
        }
        catch (Exception e)
        {
            HostLog.Line($"sound: the output could not be opened — {e.Message}");
            return new AudioOpen(null, e.Message);
        }
    }
}

sealed unsafe class WinMmOutput : ISoundSink
{
    const int Buffers = 4;
    const int BufferFrames = AudioOutput.Rate / 10;

    nint handle;
    readonly AutoResetEvent ready = new(false);
    readonly nint[] headers = new nint[Buffers];
    readonly nint[] data = new nint[Buffers];
    readonly Action<float[]> fill;
    readonly float[] scratch = new float[BufferFrames * AudioOutput.Channels];
    readonly Thread thread;
    volatile bool stopping;
    bool released;
    int next;

    WinMmOutput(Action<float[]> fill)
    {
        this.fill = fill;
        for (var i = 0; i < Buffers; i++)
        {
            data[i] = (nint)NativeMemory.AllocZeroed((nuint)(scratch.Length * 2));
            headers[i] = (nint)NativeMemory.AllocZeroed((nuint)WinMm.HeaderSize);
            var h = (WaveHdr*)headers[i];
            h->Data = data[i];
            h->BufferLength = (uint)(scratch.Length * 2);
        }
        thread = new Thread(Run) { IsBackground = true, Name = "Hover sound" };
    }

    public static AudioOpen Open(Action<float[]> fill)
    {
        var o = new WinMmOutput(fill);
        var format = WaveFormat.Pcm(AudioOutput.Rate, AudioOutput.Channels);
        var code = WinMm.waveOutOpen(out o.handle, WinMm.Mapper, ref format, o.ready.SafeWaitHandle.DangerousGetHandle(), 0, WinMm.CallbackEvent);
        if (code != WinMm.NoError)
        {
            o.FreeMemory();
            return new AudioOpen(null, code == 4 ? "The sound output is in use." : code is 2 or 6 ? "There is no sound output." : $"waveOutOpen: error {code}");
        }
        for (var i = 0; i < Buffers; i++)
        {
            code = WinMm.waveOutPrepareHeader(o.handle, o.headers[i], WinMm.HeaderSize);
            if (code != WinMm.NoError)
            {
                o.Dispose();
                return new AudioOpen(null, "waveOutPrepareHeader failed");
            }
            o.Queue(i);
        }
        o.thread.Start();
        return new AudioOpen(o, null);
    }

    /// <summary>Fills buffer i and hands it to the device.</summary>
    void Queue(int i)
    {
        Array.Clear(scratch);
        try { fill(scratch); }
        catch (Exception e) { HostLog.Line($"sound: filling a buffer failed — {e.Message}"); Array.Clear(scratch); }
        var target = (short*)data[i];
        for (var k = 0; k < scratch.Length; k++)
        {
            var v = scratch[k];
            target[k] = (short)(Math.Clamp(float.IsFinite(v) ? v : 0f, -1f, 1f) * 32767f);
        }
        ((WaveHdr*)headers[i])->Flags &= ~WinMm.HeaderDone;
        WinMm.waveOutWrite(handle, headers[i], WinMm.HeaderSize);
    }

    void Run()
    {
        while (!stopping)
        {
            ready.WaitOne(200);
            // The device finishes the buffers in the order they were queued.
            while (!stopping && (((WaveHdr*)headers[next])->Flags & WinMm.HeaderDone) != 0)
            {
                Queue(next);
                next = (next + 1) % Buffers;
            }
        }
    }

    public void Dispose()
    {
        if (released) return;
        released = true;
        stopping = true;
        ready.Set();
        if (thread.IsAlive && Environment.CurrentManagedThreadId != thread.ManagedThreadId) thread.Join();
        WinMm.waveOutReset(handle);
        for (var i = 0; i < Buffers; i++) WinMm.waveOutUnprepareHeader(handle, headers[i], WinMm.HeaderSize);
        WinMm.waveOutClose(handle);
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
}
