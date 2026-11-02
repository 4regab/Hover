# Sets the display scale of the main display (100, 125, 150, 175 ...) the way Settings > Display does, without signing out,
# or says what the display allows. Windows has no documented call for it: this uses DisplayConfigSetDeviceInfo with the two
# request types (-3 get, -4 set) that the Settings app itself uses. The numbers are steps from the display's recommended scale,
# so the script reads the minimum, the current and the maximum first.
#   ./tools/set-scale.ps1            prints what the display allows and the scale it has
#   ./tools/set-scale.ps1 -Percent 150
param([int]$Percent = 0)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DisplayScale
{
    [StructLayout(LayoutKind.Sequential)] public struct LUID { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] public struct Header { public int Type; public uint Size; public LUID Adapter; public uint Id; }
    [StructLayout(LayoutKind.Sequential)] public struct Get { public Header H; public int Min, Cur, Max; }
    [StructLayout(LayoutKind.Sequential)] public struct Set { public Header H; public int Rel; }
    [StructLayout(LayoutKind.Sequential)] public struct Rational { public uint N, D; }
    [StructLayout(LayoutKind.Sequential)] public struct PathSource { public LUID Adapter; public uint Id, ModeIdx, Flags; }
    [StructLayout(LayoutKind.Sequential)] public struct PathTarget { public LUID Adapter; public uint Id, ModeIdx, Technology, Rotation, Scaling; public Rational Refresh; public uint Order, Available, Flags; }
    [StructLayout(LayoutKind.Sequential)] public struct Path { public PathSource Source; public PathTarget Target; public uint Flags; }
    [StructLayout(LayoutKind.Sequential, Size = 64)] public struct Mode { public uint Type, Id; public LUID Adapter; }
    [DllImport("user32.dll")] public static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")] public static extern int QueryDisplayConfig(uint flags, ref uint paths, [Out] Path[] pathArray, ref uint modes, [Out] Mode[] modeArray, IntPtr topology);
    [DllImport("user32.dll", EntryPoint = "DisplayConfigGetDeviceInfo")] public static extern int GetInfo(ref Get info);
    [DllImport("user32.dll", EntryPoint = "DisplayConfigSetDeviceInfo")] public static extern int SetInfo(ref Set info);

    static readonly int[] Steps = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };

    public static string Run(int percent)
    {
        uint np, nm;
        int r = GetDisplayConfigBufferSizes(2, out np, out nm); // QDC_ONLY_ACTIVE_PATHS
        if (r != 0) return "GetDisplayConfigBufferSizes failed: " + r;
        var paths = new Path[np];
        var modes = new Mode[nm];
        r = QueryDisplayConfig(2, ref np, paths, ref nm, modes, IntPtr.Zero);
        if (r != 0) return "QueryDisplayConfig failed: " + r;
        if (np == 0) return "no active display path";
        var src = paths[0].Source;
        var g = new Get { H = new Header { Type = -3, Size = (uint)Marshal.SizeOf(typeof(Get)), Adapter = src.Adapter, Id = src.Id } };
        r = GetInfo(ref g);
        if (r != 0) return "DisplayConfigGetDeviceInfo failed: " + r;
        // Cur and Min are steps from the recommended scale (Min is negative or zero); the scale of step 0 is not given, so the
        // recommended one is found from the current one: recommended = Steps[index(current scale) - Cur], which needs the real scale.
        var dpi = GetDpiForSystem();
        int current = (int)Math.Round(dpi / 96.0 * 100);
        int at = Array.IndexOf(Steps, current);
        string text = "paths " + np + "; scale now " + current + "%; steps min " + g.Min + ", cur " + g.Cur + ", max " + g.Max;
        if (at < 0) return text + "; the current scale is not one of the standard steps";
        int recommended = at - g.Cur;
        text += "; recommended " + Steps[Math.Max(recommended, 0)] + "%; allowed " + Steps[Math.Max(recommended + g.Min, 0)] + "% to " + Steps[Math.Min(recommended + g.Max, Steps.Length - 1)] + "%";
        if (percent == 0) return text;
        int want = Array.IndexOf(Steps, percent);
        if (want < 0) return text + "; " + percent + "% is not a standard step";
        int rel = want - recommended;
        if (rel < g.Min || rel > g.Max) return text + "; " + percent + "% is not allowed on this display";
        var s = new Set { H = new Header { Type = -4, Size = (uint)Marshal.SizeOf(typeof(Set)), Adapter = src.Adapter, Id = src.Id }, Rel = rel };
        r = SetInfo(ref s);
        return text + "; asked for " + percent + "%: " + (r == 0 ? "done" : "failed " + r);
    }

    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
}
'@
[DisplayScale]::Run($Percent)
