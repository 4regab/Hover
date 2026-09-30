<#
  Compares two `hover --shots` folders pixel by pixel: for each PNG in -A, the number
  of pixels that differ in -B and the largest channel difference. A size change or a
  missing file is reported as such. Uses System.Drawing (Windows PowerShell 5.1); the
  inner loop is C#, since a PowerShell loop over every byte takes minutes per shot.

    .\native\tools\hover-measure\compare-shots.ps1 -A shots-before -B shots-after

  The office's 3D scene isn't the same twice (the camera may be mid-flight when the shot
  is taken, and the wall clock is the real time), so -Region x,y,w,h compares only a part,
  e.g. -Region 792,8,360,424 -Filter office-* for the side panel of the 1200x480 shots.
#>
param(
    [Parameter(Mandatory = $true)][string]$A,
    [Parameter(Mandatory = $true)][string]$B,
    [string]$Csv = "",
    [string]$Region = "",
    [string]$Filter = "*.png"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
public static class ShotDiff {
    // Returns { differing pixels, largest channel difference }, or null on a size change.
    public static int[] Compare(string a, string b, int rx, int ry, int rw, int rh) {
        using (var x = new Bitmap(a)) using (var y = new Bitmap(b)) {
            if (x.Width != y.Width || x.Height != y.Height) return null;
            var r = rw > 0 ? Rectangle.Intersect(new Rectangle(rx, ry, rw, rh), new Rectangle(0, 0, x.Width, x.Height))
                           : new Rectangle(0, 0, x.Width, x.Height);
            var dx = x.LockBits(r, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            var dy = y.LockBits(r, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            // Row by row: a locked part's rows are the whole bitmap's stride apart, so one
            // copy of stride * height would run past the end of the bitmap.
            int row = r.Width * 4, n = row * r.Height;
            var bx = new byte[n]; var by = new byte[n];
            for (int j = 0; j < r.Height; j++) {
                Marshal.Copy(IntPtr.Add(dx.Scan0, j * dx.Stride), bx, j * row, row);
                Marshal.Copy(IntPtr.Add(dy.Scan0, j * dy.Stride), by, j * row, row);
            }
            x.UnlockBits(dx); y.UnlockBits(dy);
            int diff = 0, max = 0;
            for (int i = 0; i < n; i += 4) {
                int d = 0;
                for (int c = 0; c < 4; c++) { int e = Math.Abs(bx[i + c] - by[i + c]); if (e > d) d = e; }
                if (d > 0) { diff++; if (d > max) max = d; }
            }
            return new int[] { diff, max };
        }
    }
}
"@
$reg = @(0, 0, 0, 0)
if ($Region) { $reg = @($Region.Split(",") | ForEach-Object { [int]$_ }) }
$rows = @()
foreach ($f in Get-ChildItem -Path $A -Filter $Filter | Sort-Object Name) {
    $other = Join-Path $B $f.Name
    if (-not (Test-Path $other)) { $rows += [pscustomobject]@{ Shot = $f.Name; Differing = "missing"; MaxDelta = "" }; continue }
    $r = [ShotDiff]::Compare($f.FullName, (Resolve-Path $other).Path, $reg[0], $reg[1], $reg[2], $reg[3])
    if ($null -eq $r) { $rows += [pscustomobject]@{ Shot = $f.Name; Differing = "size"; MaxDelta = "" } }
    else { $rows += [pscustomobject]@{ Shot = $f.Name; Differing = $r[0]; MaxDelta = $r[1] } }
}
foreach ($g in Get-ChildItem -Path $B -Filter $Filter) {
    if (-not (Test-Path (Join-Path $A $g.Name))) { $rows += [pscustomobject]@{ Shot = $g.Name; Differing = "new"; MaxDelta = "" } }
}
if ($Csv) { $rows | Export-Csv -NoTypeInformation -Path $Csv }
$rows | Format-Table -AutoSize
