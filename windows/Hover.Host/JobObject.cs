using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Hover.Host;

/// <summary>
/// A Windows job object that kills every process in it when its last handle closes. The
/// host holds the only handle, so the backend and everything it started die with the host,
/// however the host ends (crash, task manager, power-off of the session).
/// </summary>
[SupportedOSPlatform("windows")]
public sealed class JobObject : IDisposable
{
    const int JobObjectExtendedLimitInformation = 9;
    const uint KillOnJobClose = 0x2000;

    nint handle;

    JobObject(nint handle) => this.handle = handle;

    /// <summary>A new job whose processes are killed when the job is closed.</summary>
    public static JobObject CreateKillOnClose()
    {
        var job = CreateJobObjectW(0, null);
        if (job == 0) throw new InvalidOperationException("CreateJobObject failed.", new System.ComponentModel.Win32Exception());

        var info = new ExtendedLimitInformation();
        info.Basic.LimitFlags = KillOnJobClose;
        if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, ref info, (uint)Marshal.SizeOf<ExtendedLimitInformation>()))
        {
            var error = new System.ComponentModel.Win32Exception();
            CloseHandle(job);
            throw new InvalidOperationException("SetInformationJobObject failed.", error);
        }
        return new JobObject(job);
    }

    /// <summary>Puts a running process in the job. Its later children join it.</summary>
    public void Assign(Process process)
    {
        if (!AssignProcessToJobObject(handle, process.SafeHandle.DangerousGetHandle()))
            throw new InvalidOperationException("AssignProcessToJobObject failed.", new System.ComponentModel.Win32Exception());
    }

    /// <summary>Closing the handle is what kills the processes.</summary>
    public void Dispose()
    {
        var h = Interlocked.Exchange(ref handle, 0);
        if (h != 0) CloseHandle(h);
    }

    [StructLayout(LayoutKind.Sequential)]
    struct BasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public nuint MinimumWorkingSetSize;
        public nuint MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public nuint Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct IoCounters
    {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
        public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct ExtendedLimitInformation
    {
        public BasicLimitInformation Basic;
        public IoCounters Io;
        public nuint ProcessMemoryLimit;
        public nuint JobMemoryLimit;
        public nuint PeakProcessMemoryUsed;
        public nuint PeakJobMemoryUsed;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern nint CreateJobObjectW(nint attributes, string? name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool SetInformationJobObject(nint job, int infoClass, ref ExtendedLimitInformation info, uint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool AssignProcessToJobObject(nint job, nint process);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CloseHandle(nint handle);
}
