<#
  The memory scenarios on Windows, N times, against one Hover build, with the fake
  agent standing in for Kiro and Codex. Each run gets a fresh data folder and project
  folder under -Out; nothing of the user's own Hover data is read or written.

    .\native\tools\hover-measure\run-memory.ps1 -Exe native\target\release\hover.exe -Out evidence\memory\after -Runs 3
    .\native\tools\hover-measure\run-memory.ps1 -Exe ... -Script startup.hms -Runs 1

  Needs a release build of hover-measure (cargo build --release -p hover-measure).
  A Hover already running takes the single-instance lock: quit it first.
#>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Runs = 3,
    [string]$Script = "memory.hms",
    [int]$IntervalMs = 250
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = Resolve-Path (Join-Path $here "..\..\..")
$bin = Join-Path $root "native\target\release"
$measure = Join-Path $bin "hover-measure.exe"
$fake = Join-Path $bin "fake-agent.exe"
if (-not (Test-Path $measure) -or -not (Test-Path $fake)) { throw "Build the tools first: cargo build --manifest-path native/Cargo.toml --release -p hover-measure" }
if (Get-Process Hover -ErrorAction SilentlyContinue) { throw "Hover is running; quit it first (it holds the single-instance lock)." }
$Exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

# The stand-in tools, first on PATH for Hover only.
$fakebin = Join-Path $Out "fakebin"
New-Item -ItemType Directory -Force -Path $fakebin | Out-Null
foreach ($n in "kiro-cli", "codex-acp", "codex") { Copy-Item $fake (Join-Path $fakebin "$n.exe") -Force }

$scriptPath = if (Test-Path $Script) { (Resolve-Path $Script).Path } else { Join-Path $here "scenarios\$Script" }
$dirs = @()
for ($i = 1; $i -le $Runs; $i++) {
    $run = Join-Path $Out ("run{0}" -f $i)
    if (Test-Path $run) { Remove-Item -Recurse -Force $run }
    $data = Join-Path $run "data"
    $proj = Join-Path $run "project dir ü"
    New-Item -ItemType Directory -Force -Path $data, $proj | Out-Null
    # The first-use note is taken as read, so the office itself is what shows.
    $folder = $proj.Replace('\', '\\')
    Set-Content -Path (Join-Path $data "settings.json") -Value "{`"KiroNoticeSeen`":true,`"KiroFolder`":`"$folder`"}" -Encoding utf8NoBOM
    Write-Host "run $i -> $run"
    & $measure run --exe $Exe --script $scriptPath --out $run --data $data --interval-ms $IntervalMs --path-first $fakebin --var "PROJECT=$proj"
    if ($LASTEXITCODE -ne 0) { Write-Warning "run $i failed (see $run\markers.csv)" }
    $dirs += $run
}
& $measure summarize @dirs --md (Join-Path $Out "summary.md")
