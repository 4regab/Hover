# Installs the Go build's setup over a 5.x install, as a user would, and checks what the
# installer promises: one product (same AppId), the new exe in the same folder, the data
# folder (%APPDATA%\Hover) untouched, the Start Menu entry, and a clean uninstall.
# Old is the 5.x setup (Hover-Setup-5.0.2.exe from the GitHub release).
# Writes Out\report.json and exits 1 when a check fails.
param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][string]$Old, [Parameter(Mandatory)][string]$Built, [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
$report = [ordered]@{}
$app = Join-Path $env:LOCALAPPDATA 'Programs\Hover'
$exe = Join-Path $app 'hoverai.exe'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E2B4C1-6E3A-4E1F-9A2C-1D0F5A7C9E20}_is1'
function Run-Setup($path, $log) {
  $p = Start-Process $path -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/TASKS=startup', "/LOG=$log" -Wait -PassThru
  return $p.ExitCode
}

# 5.x first, with data of the user's already there.
$report.old_setup_exit = Run-Setup $Old (Join-Path $Out 'old-setup.log')
$report.old_installed = Test-Path $exe
$oldHash = if ($report.old_installed) { (Get-FileHash $exe).Hash } else { '' }
$data = Join-Path $env:APPDATA 'Hover'
New-Item -ItemType Directory -Force $data | Out-Null
$marker = Join-Path $data 'installer-check.txt'
Set-Content $marker 'kept' -Encoding ascii
$markerHash = (Get-FileHash $marker).Hash
$report.run_key_before = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).Hover

# The Go setup over it.
$report.new_setup_exit = Run-Setup $Setup (Join-Path $Out 'new-setup.log')
$report.exe_replaced = (Test-Path $exe) -and ((Get-FileHash $exe).Hash -ne $oldHash) -and ((Get-FileHash $exe).Hash -eq (Get-FileHash $Built).Hash)
$report.wgpu_dll_installed = Test-Path (Join-Path $app 'wgpu_native.dll')
$report.license_installed = Test-Path (Join-Path $app 'LICENSE')
$report.one_uninstall_entry = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Where-Object { $_.PSChildName -like '{B7E2B4C1*' }).Count -eq 1
$report.uninstall_version = (Get-ItemProperty $uninstallKey -ErrorAction SilentlyContinue).DisplayVersion
$report.start_menu_entry = Test-Path (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Hover\Hover.lnk')
$report.data_kept = (Test-Path $marker) -and ((Get-FileHash $marker).Hash -eq $markerHash)
$report.run_key_after = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).Hover
$report.run_key_kept = $report.run_key_after -eq $report.run_key_before
$v = & $exe --version 2>&1 | Out-String
$report.version_output = $v.Trim()

# And out again.
$unins = Join-Path $app 'unins000.exe'
$p = Start-Process $unins -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
$report.uninstall_exit = $p.ExitCode
Start-Sleep -Seconds 2
$report.uninstalled = -not (Test-Path $exe)
$report.data_kept_after_uninstall = Test-Path $marker
Remove-Item $marker -ErrorAction SilentlyContinue

$report | ConvertTo-Json | Set-Content (Join-Path $Out 'report.json')
$report.GetEnumerator() | ForEach-Object { Write-Host "$($_.Key) = $($_.Value)" }
$must = 'old_installed', 'exe_replaced', 'wgpu_dll_installed', 'license_installed', 'one_uninstall_entry', 'start_menu_entry', 'data_kept', 'run_key_kept', 'uninstalled', 'data_kept_after_uninstall'
$bad = $must | Where-Object { -not $report[$_] }
if ($report.old_setup_exit -ne 0 -or $report.new_setup_exit -ne 0 -or $report.uninstall_exit -ne 0) { $bad += 'setup_exit' }
if ($bad) { Write-Host "FAILED: $($bad -join ', ')"; exit 1 }
