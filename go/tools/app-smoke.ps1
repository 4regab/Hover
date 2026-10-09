# Drives the real Go app on a Windows runner: starts it, opens the notch with its shortcut
# (Alt+N), folds it with Esc, asks for the app window with a second launch, and writes
# what it saw (hover.log lines, pictures of the screen, memory) to Out\report.json.
# Exits 1 when a step didn't happen.
param([Parameter(Mandatory)][string]$Exe, [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Keys {
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  public static void Down(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); }
  public static void Up(byte vk) { keybd_event(vk, 0, 2, UIntPtr.Zero); }
  public static void Press(byte vk) { Down(vk); Up(vk); }
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static uint ForegroundPid() { uint pid; GetWindowThreadProcessId(GetForegroundWindow(), out pid); return pid; }
}
'@

$data = Join-Path $Out 'data'
New-Item -ItemType Directory -Force $data | Out-Null
$env:HOVER_DATA_DIR = (Resolve-Path $data).Path
$env:HOVER_TRACE = '1'
$log = Join-Path $data 'hover.log'
$report = [ordered]@{}

function Shot($name) {
  try {
    $b = [System.Windows.Forms.SystemInformation]::PrimaryMonitorSize
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    # CAPTUREBLT: without it a see-through window (the notch) is left out.
    $op = [System.Drawing.CopyPixelOperation]::SourceCopy -bor [System.Drawing.CopyPixelOperation]::CaptureBlt
    $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size, $op)
    $bmp.Save((Join-Path $Out "$name.png"))
    $g.Dispose(); $bmp.Dispose()
  } catch { $report["shot_$name"] = "failed: $($_.Exception.Message)" }
}
function Has($text) { (Test-Path $log) -and ((Get-Content $log -Raw) -match [regex]::Escape($text)) }
function Wait-For($text, $secs) {
  $end = (Get-Date).AddSeconds($secs)
  while ((Get-Date) -lt $end) { if (Has $text) { return $true }; Start-Sleep -Milliseconds 200 }
  return $false
}

$p = Start-Process $Exe -PassThru
$report.started = Wait-For 'started' 20
Start-Sleep -Seconds 2
$report.alive_after_start = -not $p.HasExited
Shot '1-rest'

# The shortcut opens the notch.
[Keys]::Down(0x12); [Keys]::Press(0x4E); [Keys]::Up(0x12)
$report.opened = Wait-For 'notch: opening' 5
Start-Sleep -Milliseconds 1500
Shot '2-open'
$report.alive_after_open = -not $p.HasExited
$report.foreground_is_hover_after_open = ([Keys]::ForegroundPid() -eq $p.Id)

# Esc folds it.
[Keys]::Press(0x1B)
$report.folded = Wait-For 'notch: folding' 5
Start-Sleep -Milliseconds 1000
Shot '3-folded'

# A second launch opens the first one's app window and ends.
$q = Start-Process $Exe -PassThru
$report.second_launch_ended = $q.WaitForExit(10000)
$report.window_opened = Wait-For 'app window opened' 5
Start-Sleep -Milliseconds 1500
Shot '4-app-window'

$p.Refresh()
$report.private_mb = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
$report.working_set_mb = [math]::Round($p.WorkingSet64 / 1MB, 1)
$report.alive_at_end = -not $p.HasExited
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
if (Test-Path $log) { $report.log = @(Get-Content $log) }
$report | ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Out 'report.json')
$report.GetEnumerator() | Where-Object { $_.Key -ne 'log' } | ForEach-Object { Write-Host "$($_.Key) = $($_.Value)" }
$bad = @('started', 'alive_after_start', 'opened', 'alive_after_open', 'folded', 'second_launch_ended', 'window_opened', 'alive_at_end') | Where-Object { -not $report[$_] }
if ($bad) { Write-Host "FAILED: $($bad -join ', ')"; if ($report.log) { $report.log | ForEach-Object { Write-Host "  log: $_" } }; exit 1 }
