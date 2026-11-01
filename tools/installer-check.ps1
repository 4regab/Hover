# Installs the C# build's setup as a user would, and checks what the installer promises: one
# product (same AppId), the whole publish folder in the install folder, the data folder
# (%APPDATA%\Hover) untouched, the Start Menu entry, the Run value, a running Hover stopped
# first, and an uninstall that leaves nothing of it behind. Four cases, each from a clean state:
#
#   A  over a 5.x (Go) install while that app runs; the new app starts; uninstall while it runs
#   B  into nothing, the default folder: the installed app is driven by app-smoke.ps1; uninstall
#   C  into a folder with spaces in its name, twice (the same version again, while the app runs); uninstall
#   D  over a 2.x install: its Hover.exe and its Run value; uninstall removes the Run value too
#
# Old is the 5.x setup (the oldest Go release's Hover-Setup-*.exe that CI finds).
# Writes Out\report.json and exits 1 when a check fails.
param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][string]$Old, [Parameter(Mandatory)][string]$Built,
  [Parameter(Mandatory)][string]$Out, [string]$ExpectVersion = '')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$Setup = (Resolve-Path $Setup).Path
$Old = (Resolve-Path $Old).Path
$Built = (Resolve-Path $Built).Path
$Out = (Resolve-Path $Out).Path
$published = Split-Path $Built -Parent
$report = [ordered]@{}
$bad = [System.Collections.Generic.List[string]]::new()
$app = Join-Path $env:LOCALAPPDATA 'Programs\Hover'
$uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hover'
$data = Join-Path $env:APPDATA 'Hover'
$smoke = Join-Path $PSScriptRoot 'app-smoke.ps1'
$self = (Get-Process -Id $PID).Path

function Check($key, $ok) {
  $report[$key] = [bool]$ok
  if (-not $ok) { $bad.Add($key) }
}
function Run-Setup($path, $log, $dir = '', $tasks = 'startup') {
  $setupArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/TASKS=$tasks", "/LOG=$log")
  # Inno wants the quotes in /DIR="..." itself when the folder has spaces in its name.
  if ($dir) { $setupArgs += "/DIR=`"$dir`"" }
  return (Start-Process $path -ArgumentList $setupArgs -Wait -PassThru).ExitCode
}
function Run-Uninstall($dir) {
  $unins = Join-Path $dir 'unins000.exe'
  $p = Start-Process $unins -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
  # The uninstaller copies itself to a temporary folder and finishes from there, so it is waited for.
  $end = (Get-Date).AddSeconds(60)
  while ((Test-Path $dir) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 500 }
  return $p.ExitCode
}
function Uninstall-Entries { @(Get-ChildItem $uninstallRoot -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like '{B7E2B4C1*' }) }
function Run-Value { (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).Hover }
function Hash($path) { if (Test-Path $path) { (Get-FileHash $path).Hash } else { '' } }
function Version-Of($exe) { (& $exe --version 2>&1 | Out-String).Trim() }
function Left-Behind($dir) {
  if (-not (Test-Path $dir)) { return @() }
  return @(Get-ChildItem $dir -Recurse -Force | ForEach-Object { $_.FullName.Substring($dir.Length).TrimStart('\') })
}
# Nothing of the install is left, the user's data folder is. Run values that point elsewhere are none of ours.
function Check-Gone($case, $dir) {
  $left = Left-Behind $dir
  if ($left.Count -gt 0) { $report["${case}_left_behind"] = ($left | Select-Object -First 20) -join '; ' }
  Check "${case}_install_folder_gone" ((-not (Test-Path $dir)) -or $left.Count -eq 0)
  Check "${case}_run_value_gone" (-not (Run-Value))
  Check "${case}_start_menu_gone" (-not (Test-Path $startMenu))
  Check "${case}_uninstall_entry_gone" ((Uninstall-Entries).Count -eq 0)
  Check "${case}_desktop_icon_gone" (-not (Test-Path (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Hover.lnk')))
  Check "${case}_data_kept" ((Test-Path $marker) -and ((Hash $marker) -eq $markerHash))
}
function Hover-Running { @(Get-Process hoverai, hover-backend -ErrorAction SilentlyContinue).Count -gt 0 }
function Wait-Until([scriptblock]$test, $secs) {
  $end = (Get-Date).AddSeconds($secs)
  while ((Get-Date) -lt $end) { if (& $test) { return $true }; Start-Sleep -Milliseconds 300 }
  return $false
}
# Starts an app with its own empty data folder and waits until its log has every line in Ready
# (the C# app's backend and notch; the Go app logs only that it started).
function Start-App($exe, $name, [string[]]$ready = @('backend started', 'notch ready')) {
  $dir = Join-Path $Out $name
  New-Item -ItemType Directory -Force $dir | Out-Null
  $env:HOVER_DATA_DIR = $dir
  $p = Start-Process $exe -PassThru
  $log = Join-Path $dir 'hover.log'
  $up = Wait-Until {
    if (-not (Test-Path $log)) { return $false }
    $text = Get-Content $log -Raw
    foreach ($line in $ready) { if ($text -notmatch [regex]::Escape($line)) { return $false } }
    return $true
  } 40
  Remove-Item Env:\HOVER_DATA_DIR -ErrorAction SilentlyContinue
  return @{ Process = $p; Up = $up }
}
function Stop-All { Get-Process hoverai, hover-backend, Hover -ErrorAction SilentlyContinue | Stop-Process -Force }

# The user's data, which no install or uninstall may touch.
New-Item -ItemType Directory -Force $data | Out-Null
$marker = Join-Path $data 'installer-check.txt'
Set-Content $marker 'kept' -Encoding ascii
$markerHash = Hash $marker

try {
  # ---- A: over 5.x, which runs.
  Stop-All
  $report.a_old_setup_exit = Run-Setup $Old (Join-Path $Out 'a-old-setup.log')
  Check 'a_old_installed' (Test-Path (Join-Path $app 'hoverai.exe'))
  $oldHash = Hash (Join-Path $app 'hoverai.exe')
  $report.a_old_had_wgpu_dll = Test-Path (Join-Path $app 'wgpu_native.dll')
  $report.a_run_value_before = Run-Value
  $oldRun = Start-App (Join-Path $app 'hoverai.exe') 'a-old-app-data' @('started')
  Check 'a_old_app_runs' ($oldRun.Up -and -not $oldRun.Process.HasExited)

  $report.a_new_setup_exit = Run-Setup $Setup (Join-Path $Out 'a-new-setup.log')
  Check 'a_old_app_stopped' ($oldRun.Process.HasExited)
  $exe = Join-Path $app 'hoverai.exe'
  Check 'a_exe_replaced' ((Test-Path $exe) -and ((Hash $exe) -ne $oldHash) -and ((Hash $exe) -eq (Hash $Built)))
  Check 'a_backend_installed' (Test-Path (Join-Path $app 'hover-backend.exe'))
  Check 'a_wgpu_dll_removed' (-not (Test-Path (Join-Path $app 'wgpu_native.dll')))
  Check 'a_license_installed' (Test-Path (Join-Path $app 'LICENSE'))
  Check 'a_notices_installed' (Test-Path (Join-Path $app 'THIRD-PARTY-NOTICES.txt'))
  Check 'a_no_2x_exe' (-not (Test-Path (Join-Path $app 'Hover.exe')))
  # Every file of the publish folder (but the debug symbols) is in the install folder.
  $missing = @(Get-ChildItem $published -Recurse -File | Where-Object { $_.Extension -ne '.pdb' } | ForEach-Object {
      $rel = $_.FullName.Substring($published.Length).TrimStart('\')
      if (-not (Test-Path -LiteralPath (Join-Path $app $rel))) { $rel } })
  if ($missing.Count -gt 0) { $report.a_missing_files = ($missing | Select-Object -First 20) -join '; ' }
  Check 'a_all_published_files_installed' ($missing.Count -eq 0)
  Check 'a_one_uninstall_entry' ((Uninstall-Entries).Count -eq 1)
  if ((Uninstall-Entries).Count -gt 0) { $report.a_uninstall_version = (Get-ItemProperty @(Uninstall-Entries)[0].PSPath).DisplayVersion }
  Check 'a_start_menu_entry' (Test-Path (Join-Path $startMenu 'Hover.lnk'))
  Check 'a_data_kept' ((Test-Path $marker) -and ((Hash $marker) -eq $markerHash))
  $report.a_run_value_after = Run-Value
  Check 'a_run_value_kept' ($report.a_run_value_after -eq $report.a_run_value_before)
  $report.a_version_output = Version-Of $exe
  Check 'a_version_ok' ($(if ($ExpectVersion) { $report.a_version_output -eq "Hover $ExpectVersion" } else { $report.a_version_output -match '^Hover \d+\.\d+\.\d+$' }))

  # The installed app starts, and the uninstaller stops it before it removes its files.
  $run = Start-App $exe 'a-new-app-data'
  Check 'a_installed_app_starts' ($run.Up -and -not $run.Process.HasExited)
  $report.a_uninstall_exit = Run-Uninstall $app
  Check 'a_running_app_stopped_by_uninstall' ($run.Process.HasExited -and -not (Hover-Running))
  Check-Gone 'a' $app

  # ---- B: into nothing, the default folder. The installed app is driven end to end.
  Stop-All
  $report.b_setup_exit = Run-Setup $Setup (Join-Path $Out 'b-setup.log')
  Check 'b_installed' (Test-Path (Join-Path $app 'hoverai.exe'))
  Check 'b_run_value_set' ((Run-Value) -eq "`"$app\hoverai.exe`"")
  $smokeArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $smoke, '-Exe', (Join-Path $app 'hoverai.exe'), '-Out', (Join-Path $Out 'b-smoke'))
  if ($ExpectVersion) { $smokeArgs += @('-ExpectVersion', $ExpectVersion) }
  & $self @smokeArgs
  $report.b_smoke_exit = $LASTEXITCODE
  Check 'b_smoke_passed' ($report.b_smoke_exit -eq 0)
  $report.b_uninstall_exit = Run-Uninstall $app
  Check-Gone 'b' $app

  # ---- C: a folder with spaces in its name, installed twice.
  Stop-All
  $spaced = Join-Path ([IO.Path]::GetTempPath()) 'Hover test folder\with spaces'
  $report.c_first_setup_exit = Run-Setup $Setup (Join-Path $Out 'c-first-setup.log') $spaced 'desktopicon'
  Check 'c_installed' (Test-Path (Join-Path $spaced 'hoverai.exe'))
  $firstHash = Hash (Join-Path $spaced 'hoverai.exe')
  $run = Start-App (Join-Path $spaced 'hoverai.exe') 'c-app-data'
  Check 'c_installed_app_starts' ($run.Up -and -not $run.Process.HasExited)
  # The same version again, while the app runs: it is stopped, and the files are the same.
  $report.c_second_setup_exit = Run-Setup $Setup (Join-Path $Out 'c-second-setup.log') $spaced 'desktopicon'
  Check 'c_running_app_stopped_by_install' ($run.Process.HasExited -and -not (Hover-Running))
  Check 'c_second_install_same_files' ((Hash (Join-Path $spaced 'hoverai.exe')) -eq $firstHash)
  Check 'c_one_uninstall_entry' ((Uninstall-Entries).Count -eq 1)
  $again = Start-App (Join-Path $spaced 'hoverai.exe') 'c-app-data-2'
  Check 'c_app_starts_after_second_install' ($again.Up -and -not $again.Process.HasExited)
  Stop-All
  $report.c_uninstall_exit = Run-Uninstall $spaced
  Check-Gone 'c' $spaced
  $parent = Join-Path ([IO.Path]::GetTempPath()) 'Hover test folder'
  if (Test-Path $parent) { Remove-Item $parent -Recurse -Force }

  # ---- D: over 2.x, which left Hover.exe and a Run value that starts it.
  Stop-All
  New-Item -ItemType Directory -Force $app | Out-Null
  Set-Content (Join-Path $app 'Hover.exe') 'a 2.x program' -Encoding ascii
  New-Item -Path $runKey -Force | Out-Null
  Set-ItemProperty -Path $runKey -Name 'Hover' -Value "`"$app\Hover.exe`""
  # Not the startup task: the Run value must be moved by the setup's rule for a 2.x value.
  $report.d_setup_exit = Run-Setup $Setup (Join-Path $Out 'd-setup.log') '' 'desktopicon'
  Check 'd_2x_exe_removed' (-not (Test-Path (Join-Path $app 'Hover.exe')))
  Check 'd_run_value_moved' ((Run-Value) -eq "`"$app\hoverai.exe`"")
  $report.d_uninstall_exit = Run-Uninstall $app
  Check-Gone 'd' $app
} catch {
  $report.error = $_.ToString()
  $report.error_at = $_.InvocationInfo.PositionMessage
  $bad.Add('exception')
} finally {
  Stop-All
  Remove-Item Env:\HOVER_DATA_DIR -ErrorAction SilentlyContinue
  Remove-Item $marker -ErrorAction SilentlyContinue
  # A failed run must not leave a Run value or an install for the next steps.
  if (Test-Path (Join-Path $app 'unins000.exe')) { Start-Process (Join-Path $app 'unins000.exe') -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait }
}

foreach ($name in 'a_old_setup_exit', 'a_new_setup_exit', 'a_uninstall_exit', 'b_setup_exit', 'b_uninstall_exit', 'c_first_setup_exit', 'c_second_setup_exit', 'c_uninstall_exit', 'd_setup_exit', 'd_uninstall_exit') {
  if ($null -ne $report[$name] -and $report[$name] -ne 0) { $bad.Add($name) }
}
$report | ConvertTo-Json | Set-Content (Join-Path $Out 'report.json')
$report.GetEnumerator() | ForEach-Object { Write-Host "$($_.Key) = $($_.Value)" }
if ($bad.Count -gt 0) { Write-Host "FAILED: $($bad -join ', ')"; exit 1 }
