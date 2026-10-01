<#
  run-memory.ps1 in the console session (session 1), as SYSTEM, through Sysinternals
  PsExec: for a Windows machine reached only over SSM or SSH, where the shell runs in
  session 0. Session 0 has no DWM, so the real renderer (femtovg-wgpu, DirectComposition)
  can't make its surface there, the shortcut can't be registered (error 1459), and a
  non-interactive logon may have no DPAPI key. The console session has DWM.

    .\native\tools\hover-measure\run-console.ps1 -PsExec C:\tools\PsExec64.exe -Exe native\target\release\hoverai.exe -Out evidence\memory\x -Script memory.hms -Runs 3

  Measurements from here are labelled "console session, SYSTEM": no user is logged on,
  so the input desktop is the logon screen and real pointer input doesn't reach Hover.
  A machine whose only adapter is WARP also needs -Env SLINT_WGPU_CPU=1.
#>
param(
    [Parameter(Mandatory = $true)][string]$PsExec,
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Runs = 3,
    [string]$Script = "memory.hms",
    [int]$IntervalMs = 250,
    [string[]]$Env = @(),
    [string]$Tools = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path $here "..\..\..")).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$log = Join-Path $Out "console-run.log"
$args2 = @("-accepteula", "-nobanner", "-i", "1", "-s", "-w", $root, "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $here "run-memory.ps1"), "-Exe", (Resolve-Path $Exe).Path, "-Out", $Out, "-Runs", $Runs, "-Script", $Script, "-IntervalMs", $IntervalMs)
if ($Tools) { $args2 += @("-Tools", (Resolve-Path $Tools).Path) }
if ($Env.Count -gt 0) { $args2 += @("-Env", ($Env -join ",")) }
# PsExec writes its banner to stderr, which Windows PowerShell 5.1 turns into an error.
$ErrorActionPreference = "Continue"
& $PsExec @args2 *> $log
$ErrorActionPreference = "Stop"
Write-Host (Join-Path $Out "summary.md")
