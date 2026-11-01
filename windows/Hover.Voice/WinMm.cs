using System.Runtime.InteropServices;

namespace Hover.Voice;

// winmm's waveIn (the microphone) and waveOut (the speaker), declared by hand. Both are the
// Go app's choice too: no COM and no extra package. Nothing here is called off Windows.

[StructLayout(LayoutKind.Sequential, Pack = 1)]
struct WaveFormat
{
    public ushort Tag, Channels;
    public uint SamplesPerSec, BytesPerSec;
    public ushort BlockAlign, Bits, Size;

    public static WaveFormat Pcm(int rate, int channels)
    {
        var f = new WaveFormat { Tag = 1, Channels = (ushort)channels, SamplesPerSec = (uint)rate, Bits = 16 };
        f.BlockAlign = (ushort)(f.Channels * f.Bits / 8);
        f.BytesPerSec = f.SamplesPerSec * f.BlockAlign;
        return f;
    }
}

/// <summary>WAVEHDR. The driver keeps its address while a buffer is queued, so it lives in unmanaged memory.</summary>
[StructLayout(LayoutKind.Sequential)]
struct WaveHdr
{
    public nint Data;
    public uint BufferLength, BytesRecorded;
    public nint User;
    public uint Flags, Loops;
    public nint Next, Reserved;
}

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
struct WaveInCaps
{
    public ushort Mid, Pid;
    public uint DriverVersion;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Name;
    public uint Formats;
    public ushort Channels, Reserved;
}

static class WinMm
{
    const string Dll = "winmm.dll";

    public static readonly nuint Mapper = nuint.MaxValue;
    public const uint CallbackEvent = 0x00050000;
    public const uint HeaderDone = 0x1;
    public const uint NoError = 0;

    [DllImport(Dll)] public static extern uint waveInGetNumDevs();
    [DllImport(Dll, CharSet = CharSet.Unicode)] public static extern uint waveInGetDevCapsW(nuint id, ref WaveInCaps caps, uint size);
    [DllImport(Dll)] public static extern uint waveInOpen(out nint handle, nuint id, ref WaveFormat format, nint callback, nint instance, uint flags);
    [DllImport(Dll)] public static extern uint waveInClose(nint handle);
    [DllImport(Dll)] public static extern uint waveInReset(nint handle);
    [DllImport(Dll)] public static extern uint waveInStart(nint handle);
    [DllImport(Dll)] public static extern uint waveInPrepareHeader(nint handle, nint header, uint size);
    [DllImport(Dll)] public static extern uint waveInUnprepareHeader(nint handle, nint header, uint size);
    [DllImport(Dll)] public static extern uint waveInAddBuffer(nint handle, nint header, uint size);

    [DllImport(Dll)] public static extern uint waveOutOpen(out nint handle, nuint id, ref WaveFormat format, nint callback, nint instance, uint flags);
    [DllImport(Dll)] public static extern uint waveOutClose(nint handle);
    [DllImport(Dll)] public static extern uint waveOutReset(nint handle);
    [DllImport(Dll)] public static extern uint waveOutPrepareHeader(nint handle, nint header, uint size);
    [DllImport(Dll)] public static extern uint waveOutUnprepareHeader(nint handle, nint header, uint size);
    [DllImport(Dll)] public static extern uint waveOutWrite(nint handle, nint header, uint size);

    public static readonly uint HeaderSize = (uint)Marshal.SizeOf<WaveHdr>();
}
