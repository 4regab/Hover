# Drives the real C# app on a Windows runner (or a desktop): starts it with an empty data
# folder, waits for the notch, the tray icon, the shortcut and the backend, opens the notch with
# its shortcut (Alt+N), folds it with Esc, asks for the app window with a second launch, and
# quits from the tray menu. Then it starts a second copy and kills it, to see the backend go
# with it. It writes what it saw (hover.log lines, memory, the panels' own dump of where their
# buttons are) to Out\report.json and exits 1 when a step didn't happen.
#
# No screen capture: the pictures come from `hoverai.exe --shots`, and what the panels drew
# is read from the file HOVER_UI_DUMP names. Memory is the private bytes and working set of
# hoverai.exe, read the way the Go app's were, and of the hover-backend.exe it started.
#
#   -Exe            hoverai.exe (hover-backend.exe sits beside it)
#   -Out            where the report, the data folders and the dump files go
#   -ExpectVersion  when given, `hoverai.exe --version` must say "Hover <this>"
param([Parameter(Mandatory)][string]$Exe, [Parameter(Mandatory)][string]$Out, [string]$ExpectVersion = '')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$Exe = (Resolve-Path $Exe).Path
# C# 5 only: Windows PowerShell compiles this with the old compiler.
if (-not ('HoverSmoke.Win' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
namespace HoverSmoke {
public static class Win {
  delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr data);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder text, int max);
  public static void Down(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); }
  public static void Up(byte vk) { keybd_event(vk, 0, 2, UIntPtr.Zero); }
  public static void Press(byte vk) { Down(vk); Up(vk); }
  public static uint ForegroundPid() { uint pid; GetWindowThreadProcessId(GetForegroundWindow(), out pid); return pid; }
  // The notch window, by its class; the pid it belongs to.
  public static long NotchWindow() { return (long)FindWindowW("HoverNotch", null); }
  public static uint PidOf(long h) { uint pid; GetWindowThreadProcessId((IntPtr)h, out pid); return pid; }
  // The notch's tray message (WM_APP + 3) carrying a right-button release: the tray menu opens.
  public static bool TrayMenu(long notch) { return PostMessageW((IntPtr)notch, 0x8003, (IntPtr)1, (IntPtr)0x205); }
  // A visible top-level window of this process with this title.
  public static bool HasWindow(uint pid, string title) {
    bool found = false;
    EnumWindows(delegate(IntPtr h, IntPtr p) {
      uint owner; GetWindowThreadProcessId(h, out owner);
      if (owner != pid || !IsWindowVisible(h)) return true;
      StringBuilder text = new StringBuilder(128);
      GetWindowTextW(h, text, 128);
      if (text.ToString() == title) { found = true; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
}
'@
}

$report = [ordered]@{}
$log = $null
$dump = Join-Path $Out 'ui-dump.json'

function Start-Fresh($name) {
  $data = Join-Path $Out $name
  New-Item -ItemType Directory -Force $data | Out-Null
  $script:log = Join-Path $data 'hover.log'
  # The app and its backend write only here, and into the dump file: never into the real %APPDATA%\Hover.
  $env:HOVER_DATA_DIR = (Resolve-Path $data).Path
  $env:HOVER_UI_DUMP = $dump
  Remove-Item $dump -ErrorAction SilentlyContinue
  return Start-Process $Exe -PassThru
}
function LogText { if (Test-Path $log) { Get-Content $log -Raw } else { '' } }
function Count($pattern) { ([regex]::Matches((LogText), $pattern)).Count }
function Wait-Count($pattern, $n, $secs) {
  $end = (Get-Date).AddSeconds($secs)
  while ((Get-Date) -lt $end) { if ((Count $pattern) -ge $n) { return $true }; Start-Sleep -Milliseconds 200 }
  return $false
}
function Wait-For($pattern, $secs) { Wait-Count $pattern 1 $secs }
function Wait-Until([scriptblock]$test, $secs) {
  $end = (Get-Date).AddSeconds($secs)
  while ((Get-Date) -lt $end) { if (& $test) { return $true }; Start-Sleep -Milliseconds 200 }
  return $false
}
function Backend-Of($hostPid) {
  Get-CimInstance Win32_Process -Filter "Name = 'hover-backend.exe' AND ParentProcessId = $hostPid" -ErrorAction SilentlyContinue | Select-Object -First 1
}
function Alive($id) { $null -ne (Get-Process -Id $id -ErrorAction SilentlyContinue) }
function Mb($bytes) { [math]::Round($bytes / 1MB, 1) }
function Memory($prefix, $proc, $backendPid) {
  $proc.Refresh()
  $report["${prefix}private_mb"] = Mb $proc.PrivateMemorySize64
  $report["${prefix}working_set_mb"] = Mb $proc.WorkingSet64
  $b = if ($backendPid) { Get-Process -Id $backendPid -ErrorAction SilentlyContinue } else { $null }
  if ($b) {
    $report["${prefix}backend_private_mb"] = Mb $b.PrivateMemorySize64
    $report["${prefix}backend_working_set_mb"] = Mb $b.WorkingSet64
    $report["${prefix}total_private_mb"] = Mb ($proc.PrivateMemorySize64 + $b.PrivateMemorySize64)
    $report["${prefix}total_working_set_mb"] = Mb ($proc.WorkingSet64 + $b.WorkingSet64)
  }
}
function Keep-Dump($name) {
  if (Test-Path $dump) {
    Copy-Item $dump (Join-Path $Out "ui-dump-$name.json") -Force
    try {
      $d = Get-Content $dump -Raw | ConvertFrom-Json
      $report["dump_${name}_parts"] = (($d.PSObject.Properties | ForEach-Object Name) -join ',')
    } catch { $report["dump_${name}_parts"] = "unreadable: $($_.Exception.Message)" }
  } else { $report["dump_${name}_parts"] = 'none' }
}

try {
  # --version answers before anything else starts, as the Go app's did. Piping makes
  # PowerShell wait for a program that has no console of its own.
  $ver = (& $Exe --version 2>&1 | Out-String).Trim()
  $report.version_output = $ver
  $report.version_ok = if ($ExpectVersion) { $ver -eq "Hover $ExpectVersion" } else { $ver -match '^Hover \d+\.\d+\.\d+$' }

  # ---- Run 1: start, shortcut, second launch, quit from the tray menu.
  $p = Start-Fresh 'data'
  $report.started = Wait-For '(?m)hover: started\s*$' 40
  $report.backend_started = Wait-For 'backend started' 40
  $report.notch_ready = Wait-For 'notch ready' 40
  $report.tray_icon = Wait-For 'tray icon added' 10
  $report.shortcut_registered = Wait-For 'hotkey \S+ registered' 10
  Start-Sleep -Seconds 2
  $report.alive_after_start = -not $p.HasExited
  $backend = Backend-Of $p.Id
  $report.backend_is_child = $null -ne $backend
  $backendPid = if ($backend) { [int]$backend.ProcessId } else { 0 }
  $report.backend_pid = $backendPid
  # The notch at rest: the number the memory rule in docs/development/go-port.md compares
  # (the Go app was one process; here the app and its backend are two, and both are written).
  Memory 'rest_' $p $backendPid
  Start-Sleep -Seconds 8
  Memory 'settled_' $p $backendPid
  Keep-Dump 'rest'

  # The shortcut opens the notch.
  [HoverSmoke.Win]::Down(0x12); [HoverSmoke.Win]::Press(0x4E); [HoverSmoke.Win]::Up(0x12)
  $report.opened = Wait-For 'notch: opening' 5
  Start-Sleep -Milliseconds 1500
  Keep-Dump 'notch-open'
  $report.alive_after_open = -not $p.HasExited
  $report.foreground_is_hover_after_open = ([HoverSmoke.Win]::ForegroundPid() -eq $p.Id)

  # Esc folds it (and if it didn't, the shortcut again; the report says which it took).
  [HoverSmoke.Win]::Press(0x1B)
  $report.folded = Wait-For 'notch: folding' 4
  $report.folded_by = 'Esc'
  if (-not $report.folded) {
    [HoverSmoke.Win]::Down(0x12); [HoverSmoke.Win]::Press(0x4E); [HoverSmoke.Win]::Up(0x12)
    $report.folded = Wait-For 'notch: folding' 5
    $report.folded_by = 'Alt+N'
  }
  Start-Sleep -Milliseconds 1000

  # A second launch opens the first one's app window and ends.
  $q = Start-Process $Exe -PassThru
  $report.second_launch_ended = $q.WaitForExit(10000)
  $report.second_launch_exit_code = if ($q.HasExited) { $q.ExitCode } else { -1 }
  $report.window_opened = Wait-For 'opening the dashboard' 8
  $report.window_visible = Wait-Until { [HoverSmoke.Win]::HasWindow([uint32]$p.Id, 'Hover') } 8
  Start-Sleep -Milliseconds 1500
  Keep-Dump 'window'
  Memory '' $p $backendPid
  $report.alive_with_window = -not $p.HasExited

  # Quit in the tray menu: the menu is opened with the tray message and picked with the keyboard.
  $notch = [HoverSmoke.Win]::NotchWindow()
  $report.notch_window_is_ours = ($notch -ne 0) -and ([HoverSmoke.Win]::PidOf($notch) -eq $p.Id)
  if ($report.notch_window_is_ours) {
    [void][HoverSmoke.Win]::TrayMenu($notch)
    Start-Sleep -Milliseconds 800
    [HoverSmoke.Win]::Press(0x26)   # Up: the last item, Quit
    Start-Sleep -Milliseconds 200
    [HoverSmoke.Win]::Press(0x0D)   # Enter
  }
  $report.quit_from_tray = $p.WaitForExit(30000)
  $report.quit_exit_code = if ($p.HasExited) { $p.ExitCode } else { -1 }
  $report.quit_logged = Wait-For 'quitting' 3
  $report.backend_stop_logged = Wait-For 'backend stopped \(exit code 0\)' 5
  $report.tray_removed_logged = Wait-For 'tray icon removed' 3
  $report.backend_gone_after_quit = (-not $backendPid) -or (Wait-Until { -not (Alive $backendPid) } 10)
  if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
  $report.log = @((LogText) -split "`r?`n" | Where-Object { $_ })

  # ---- Run 2: the app is killed, and its backend goes with it (the job object).
  $k = Start-Fresh 'data-killed'
  $okStart = (Wait-For 'backend started' 40)
  $kb = Backend-Of $k.Id
  $kbPid = if ($kb) { [int]$kb.ProcessId } else { 0 }
  $report.kill_run_backend_started = $okStart -and ($kbPid -ne 0)
  Stop-Process -Id $k.Id -Force
  $report.killed_host_backend_gone = ($kbPid -ne 0) -and (Wait-Until { -not (Alive $kbPid) } 10)
} finally {
  Get-Process hoverai, hover-backend -ErrorAction SilentlyContinue | Stop-Process -Force
}

$report | ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Out 'report.json')
$report.GetEnumerator() | Where-Object { $_.Key -ne 'log' } | ForEach-Object { Write-Host "$($_.Key) = $($_.Value)" }
$must = @('version_ok', 'started', 'backend_started', 'notch_ready', 'tray_icon', 'shortcut_registered', 'alive_after_start', 'backend_is_child',
  'opened', 'alive_after_open', 'folded', 'second_launch_ended', 'window_opened', 'window_visible', 'alive_with_window',
  'notch_window_is_ours', 'quit_from_tray', 'quit_logged', 'backend_stop_logged', 'tray_removed_logged', 'backend_gone_after_quit',
  'kill_run_backend_started', 'killed_host_backend_gone')
$bad = @($must | Where-Object { -not $report[$_] })
if ($report.second_launch_exit_code -ne 0) { $bad += 'second_launch_exit_code' }
if ($report.quit_exit_code -ne 0) { $bad += 'quit_exit_code' }
if ($bad.Count -gt 0) {
  Write-Host "FAILED: $($bad -join ', ')"
  if ($report.log) { $report.log | ForEach-Object { Write-Host "  log: $_" } }
  exit 1
}
