<#
  The memory scenarios on Windows, N times, against one Hover build, with the fake
  agent standing in for Kiro, Codex and Cursor. Each run gets a fresh data folder and
  project folder under -Out; nothing of the user's own Hover data is read or written.

    .\native\tools\hover-measure\run-memory.ps1 -Exe native\target\release\hoverai.exe -Out evidence\memory\after -Runs 3
    .\native\tools\hover-measure\run-memory.ps1 -Exe ... -Script office.hms -Runs 5
    .\native\tools\hover-measure\run-memory.ps1 -Exe ... -Env SLINT_WGPU_CPU=1   (a machine whose only adapter is WARP)

  Needs a release build of hover-measure (cargo build --release -p hover-measure).
  A Hover already running takes the single-instance lock: quit it first.
  Runs in Windows PowerShell 5.1 and PowerShell 7 alike (the file is ASCII on purpose).
#>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Runs = 3,
    [string]$Script = "memory.hms",
    [int]$IntervalMs = 250,
    # Extra environment for Hover, K=V each (passed to hover-measure run --env).
    [string[]]$Env = @(),
    # The folder hover-measure.exe and fake-agent.exe are in (default: native\target\release).
    [string]$Tools = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = Resolve-Path (Join-Path $here "..\..\..")
$bin = if ($Tools) { (Resolve-Path $Tools).Path } else { Join-Path $root "native\target\release" }
$measure = Join-Path $bin "hover-measure.exe"
$fake = Join-Path $bin "fake-agent.exe"
if (-not (Test-Path $measure) -or -not (Test-Path $fake)) { throw "Build the tools first: cargo build --manifest-path native/Cargo.toml --release -p hover-measure" }
if (Get-Process hoverai, Hover -ErrorAction SilentlyContinue) { throw "Hover is running; quit it first (it holds the single-instance lock)." }
$Exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

# The stand-in tools, first on PATH for Hover only.
$fakebin = Join-Path $Out "fakebin"
New-Item -ItemType Directory -Force -Path $fakebin | Out-Null
foreach ($n in "kiro-cli", "codex-acp", "codex", "cursor-agent") { Copy-Item $fake (Join-Path $fakebin "$n.exe") -Force }
$fakeOc = Join-Path $bin "fake-opencode.exe"
if (Test-Path $fakeOc) { Copy-Item $fakeOc (Join-Path $fakebin "opencode.exe") -Force }

$scriptPath = if (Test-Path $Script) { (Resolve-Path $Script).Path } else { Join-Path $here "scenarios\$Script" }
# The project folder has a space and a non-ASCII letter in it (u with diaeresis), as
# users' folders do; written as a code point so the script itself stays ASCII.
$projName = "project dir A" + [char]0x00FC
$utf8 = New-Object System.Text.UTF8Encoding($false)
$envArgs = @()
# "-File" hands a list over as one string: K=V,K=V is split here.
foreach ($kv in (($Env -join ",").Split(",") | Where-Object { $_ })) { $envArgs += @("--env", $kv) }
$dirs = @()
for ($i = 1; $i -le $Runs; $i++) {
    $run = Join-Path $Out ("run{0}" -f $i)
    if (Test-Path $run) { Remove-Item -Recurse -Force $run }
    $data = Join-Path $run "data"
    $proj = Join-Path $run $projName
    New-Item -ItemType Directory -Force -Path $data, $proj | Out-Null
    # The first-use note is taken as read, so the office itself is what shows.
    $folder = $proj.Replace('\', '\\')
    [System.IO.File]::WriteAllText((Join-Path $data "settings.json"), "{`"KiroNoticeSeen`":true,`"KiroFolder`":`"$folder`"}", $utf8)
    Write-Host "run $i -> $run"
    & $measure run --exe $Exe --script $scriptPath --out $run --data $data --interval-ms $IntervalMs --path-first $fakebin --var "PROJECT=$proj" @envArgs
    if ($LASTEXITCODE -ne 0) { Write-Warning "run $i failed (see $run\markers.csv)" }
    $dirs += $run
}
& $measure summarize @dirs --md (Join-Path $Out "summary.md")
