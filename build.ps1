<#
  Hover for Windows — build helper (the native build: Rust, in .\native).

    .\build.ps1              debug build
    .\build.ps1 release      optimised build
    .\build.ps1 release run  build, then relaunch
    .\build.ps1 test         the workspace's tests
    .\build.ps1 publish      Hover.exe in .\publish
    .\build.ps1 installer    Inno Setup installer in .\dist (the version in native\Cargo.toml)
    .\build.ps1 installer -Version 3.0.1

  Needs Rust (winget install Rustlang.Rustup) with the MSVC build tools.
#>
param(
    [string]$Mode = "debug",
    [string]$Then = "",
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$native = Join-Path $PSScriptRoot "native"
$manifest = Join-Path $native "Cargo.toml"
$publishDir = Join-Path $PSScriptRoot "publish"
$installerScript = Join-Path $native "installer\Hover.iss"
$distDir = Join-Path $PSScriptRoot "dist"

if ((-not [string]::IsNullOrWhiteSpace($Version)) -and
    $Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must have three numeric parts, for example 3.0.1"
}

function Stop-Hover {
    Get-Process Hover -ErrorAction SilentlyContinue | Stop-Process -Force
}

function Build-Hover([string]$profile) {
    Stop-Hover
    $cargoArgs = @("build", "--manifest-path", $manifest, "-p", "hover")
    if ($profile -eq "release") { $cargoArgs += "--release" }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    return (Join-Path $native "target\$profile\hover.exe")
}

function Publish-Hover {
    $exe = Build-Hover "release"
    New-Item -ItemType Directory -Path $publishDir -Force | Out-Null
    # Hover.exe, as the C# build named it: shortcuts and the Run value point there.
    Copy-Item $exe (Join-Path $publishDir "Hover.exe") -Force
    Copy-Item (Join-Path $PSScriptRoot "LICENSE") $publishDir -Force
    Copy-Item (Join-Path $PSScriptRoot "THIRD-PARTY-NOTICES.txt") $publishDir -Force
}

function Get-ProjectVersion {
    if (-not [string]::IsNullOrWhiteSpace($Version)) { return $Version }
    $line = Select-String -Path $manifest -Pattern '^version = "(.+)"' | Select-Object -First 1
    if (-not $line) { throw "Could not read the version from native\Cargo.toml" }
    return $line.Matches[0].Groups[1].Value
}

function Find-InnoCompiler {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 7\ISCC.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 7\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 7\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    throw "Inno Setup 6 or 7 is required. Install it from https://jrsoftware.org/isdl.php"
}

switch ($Mode.ToLower()) {
    "publish" {
        Publish-Hover
        Write-Host "publish\Hover.exe"
    }
    "installer" {
        Publish-Hover
        $resolvedVersion = Get-ProjectVersion
        $iscc = Find-InnoCompiler
        New-Item -ItemType Directory -Path $distDir -Force | Out-Null
        & $iscc "/DMyAppVersion=$resolvedVersion" "/DExeDir=$publishDir" "/O$distDir" $installerScript
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }
        Write-Host (Join-Path $distDir "Hover-Setup-$resolvedVersion.exe")
    }
    "test" {
        & cargo test --manifest-path $manifest --release --workspace
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
    }
    "release" {
        $exe = Build-Hover "release"
        if ($Then -eq "run") { Start-Process $exe }
    }
    default {
        $exe = Build-Hover "debug"
        if ($Then -eq "run") { Start-Process $exe }
    }
}
