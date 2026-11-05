<#
  Hover for Windows - build helper (from the repo root).

    .\build.ps1              hoverai.exe (the C# app) and hover-backend.exe in .\publish
    .\build.ps1 run          build, then start it
    .\build.ps1 publish      the same, with LICENSE and THIRD-PARTY-NOTICES.txt beside them
    .\build.ps1 installer    Inno Setup installer in .\dist (the version in VERSION)
    .\build.ps1 installer -Version 5.0.3

    .\build.ps1 legacy [publish|run|installer]
                             the Go Windows app, as these modes were before the C# app: hoverai.exe
                             and wgpu_native.dll in .\publish-legacy; its installer is
                             dist\Hover-Legacy-Setup-<version>.exe. Kept until the Go app is retired.

  Needs the .NET 10 SDK (winget install Microsoft.DotNet.SDK.10) and Go (the version go.mod
  names; winget install GoLang.Go): the app is C#, the backend it starts is Go. No Visual Studio
  and no C compiler. The installer needs Inno Setup 6 or 7. The legacy build needs only Go.

  Set HOVER_DATA_DIR to an empty folder before `run`, so the build cannot alter your own settings.
#>
param(
    [Parameter(Position = 0)][string]$Mode = "build",
    [Parameter(Position = 1)][string]$Target = "",
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$publishDir = Join-Path $PSScriptRoot "publish"
$legacyDir = Join-Path $PSScriptRoot "publish-legacy"
$installerScript = Join-Path $PSScriptRoot "packaging\windows\Hover.iss"
$distDir = Join-Path $PSScriptRoot "dist"
$outDir = Join-Path $PSScriptRoot "out"
# The Go app's office draws with wgpu-native; its exe looks for the DLL beside itself.
$wgpuUrl = "https://github.com/gfx-rs/wgpu-native/releases/download/v29.0.0.0/wgpu-windows-x86_64-msvc-release.zip"

if ((-not [string]::IsNullOrWhiteSpace($Version)) -and
    $Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must have three numeric parts, for example 5.0.3"
}

function Stop-Hover {
    # Hover: an install from before the rename, which would hold the single-instance lock.
    # hover-backend: the C# app's backend, which dies with its app but would lock the file.
    Get-Process hoverai, Hover, hover-backend -ErrorAction SilentlyContinue | Stop-Process -Force
}

function Get-ProjectVersion {
    if (-not [string]::IsNullOrWhiteSpace($Version)) { return $Version }
    $line = (Get-Content (Join-Path $PSScriptRoot "VERSION") -TotalCount 1).Trim()
    if ($line -notmatch '^\d+\.\d+\.\d+$') { throw "Could not read the version from VERSION" }
    return $line
}

function Require-Tool([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name was not found on PATH. $Hint"
    }
}

function Require-Go {
    Require-Tool go "Go is needed for hover-backend (the version go.mod names): winget install GoLang.Go"
}

function Require-DotNet {
    Require-Tool dotnet "The .NET 10 SDK builds the app: winget install Microsoft.DotNet.SDK.10"
    $sdk = (& dotnet --version 2>$null | Select-Object -First 1)
    if ($sdk -notmatch '^(\d+)\.' -or [int]$Matches[1] -lt 10) {
        throw "The .NET 10 SDK or newer is needed (dotnet --version says '$sdk'): winget install Microsoft.DotNet.SDK.10"
    }
}

# Runs a program and returns its exit code. Go and dotnet write progress to stderr; Windows
# PowerShell turns that into a terminating error under "Stop" whenever the output is
# redirected (a log, CI), so the preference is relaxed for the call.
function Invoke-Native([string]$File, [string[]]$Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        # Out-Host keeps the program's own output out of the value this function returns.
        & $File @Arguments | Out-Host
        return $LASTEXITCODE
    } finally { $ErrorActionPreference = $previous }
}

function Invoke-Checked([string]$File, [string[]]$Arguments) {
    $code = Invoke-Native $File $Arguments
    if ($code -ne 0) { throw "$File $($Arguments[0]) failed (exit code $code)" }
}

function Set-Env([string]$Name, [string]$Value) {
    [Environment]::SetEnvironmentVariable($Name, $Value, "Process")
}

# Empties a folder file by file. After a program ran from the folder, Windows' font cache holds the
# font files it drew with for a while, and deleting the folder as a whole then fails. A file that
# stays is tried again, then kept with a warning: the build writes the same file over it.
function Clear-Folder([string]$Path) {
    if (-not (Test-Path $Path)) { return }
    $left = @()
    for ($try = 1; $try -le 5; $try++) {
        $left = @()
        foreach ($file in Get-ChildItem $Path -Recurse -File -Force) {
            try { Remove-Item -LiteralPath $file.FullName -Force -ErrorAction Stop } catch { $left += $file.FullName }
        }
        if ($left.Count -eq 0) { break }
        Start-Sleep -Seconds 2
    }
    $folders = Get-ChildItem $Path -Recurse -Directory -Force | Sort-Object { $_.FullName.Length } -Descending
    foreach ($folder in $folders) {
        if (-not (Get-ChildItem $folder.FullName -Force)) { Remove-Item -LiteralPath $folder.FullName -Force -ErrorAction SilentlyContinue }
    }
    if ($left.Count -gt 0) { Write-Warning "in use, so kept: $($left -join ', ')" }
}

# The C# app: dotnet publish (self-contained, Release, x64) and the Go backend beside it.
function Build-Hover {
    Require-Go
    Require-DotNet
    Stop-Hover
    $v = Get-ProjectVersion
    # A clean folder, so nothing an earlier build (or the legacy build) left is ever installed.
    Clear-Folder $publishDir
    New-Item -ItemType Directory -Path $publishDir -Force | Out-Null
    Push-Location $PSScriptRoot
    try {
        Invoke-Checked dotnet @("publish", "windows\Hover\Hover.csproj", "-c", "Release", "-r", "win-x64",
            "--self-contained", "-p:Version=$v", "-o", $publishDir, "--nologo", "-v", "minimal")
        Set-Env "CGO_ENABLED" "0"
        Invoke-Checked go @("build", "-o", (Join-Path $publishDir "hover-backend.exe"), "./cmd/hover-backend")
    } finally { Pop-Location }
    $exe = Join-Path $publishDir "hoverai.exe"
    if (-not (Test-Path $exe)) { throw "dotnet publish did not make hoverai.exe" }
    return $exe
}

function Publish-Hover {
    $exe = Build-Hover
    Copy-Item (Join-Path $PSScriptRoot "LICENSE") $publishDir -Force
    Copy-Item (Join-Path $PSScriptRoot "THIRD-PARTY-NOTICES.txt") $publishDir -Force
    return $exe
}

# The Go Windows app, as the default build was before the C# app.
function Build-Legacy {
    Require-Go
    Stop-Hover
    $v = Get-ProjectVersion
    New-Item -ItemType Directory -Path $legacyDir -Force | Out-Null
    Set-Env "CGO_ENABLED" "0"
    Push-Location $PSScriptRoot
    try {
        Invoke-Checked go @("build", "-ldflags", "-H=windowsgui -X github.com/4regab/Hover/internal/shell.Version=$v",
            "-o", (Join-Path $legacyDir "hoverai.exe"), ".\cmd\hover")
    } finally { Pop-Location }
    $dll = Join-Path $legacyDir "wgpu_native.dll"
    if (-not (Test-Path $dll)) {
        $zip = Join-Path ([IO.Path]::GetTempPath()) "wgpu-native.zip"
        $tmp = Join-Path ([IO.Path]::GetTempPath()) "wgpu-native"
        Invoke-WebRequest $wgpuUrl -OutFile $zip
        Expand-Archive $zip $tmp -Force
        $found = Get-ChildItem $tmp -Recurse -Filter wgpu_native.dll | Select-Object -First 1
        if (-not $found) { throw "no wgpu_native.dll in the wgpu-native release zip" }
        Copy-Item $found.FullName $dll -Force
    }
    return (Join-Path $legacyDir "hoverai.exe")
}

function Publish-Legacy {
    $exe = Build-Legacy
    Copy-Item (Join-Path $PSScriptRoot "LICENSE") $legacyDir -Force
    Copy-Item (Join-Path $PSScriptRoot "THIRD-PARTY-NOTICES.txt") $legacyDir -Force
    return $exe
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

# The setup: one script for both apps. The legacy one is told so with /DLegacy.
function Build-Installer([string]$SourceDir, [bool]$IsLegacy) {
    $resolvedVersion = Get-ProjectVersion
    $iscc = Find-InnoCompiler
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
    $defines = @("/DMyAppVersion=$resolvedVersion", "/DExeDir=$SourceDir")
    if ($IsLegacy) { $defines += "/DLegacy" }
    Invoke-Checked $iscc ($defines + @("/O$distDir", $installerScript))
    $name = if ($IsLegacy) { "Hover-Legacy-Setup-$resolvedVersion.exe" } else { "Hover-Setup-$resolvedVersion.exe" }
    Write-Host (Join-Path $distDir $name)
}

switch ($Mode.ToLower()) {
    "legacy" {
        switch ($Target.ToLower()) {
            "publish" {
                Publish-Legacy | Out-Null
                Write-Host "publish-legacy\hoverai.exe"
            }
            "installer" {
                Publish-Legacy | Out-Null
                Build-Installer $legacyDir $true
            }
            "run" {
                $exe = Build-Legacy
                Start-Process $exe
            }
            "" {
                Build-Legacy | Out-Null
                Write-Host "publish-legacy\hoverai.exe"
            }
            default { throw "legacy takes publish, run, installer, not '$Target'" }
        }
    }
    "publish" {
        Publish-Hover | Out-Null
        Write-Host "publish\hoverai.exe"
    }
    "installer" {
        Publish-Hover | Out-Null
        Build-Installer $publishDir $false
    }
    "run" {
        $exe = Build-Hover
        Start-Process $exe
    }
    default {
        Build-Hover | Out-Null
        Write-Host "publish\hoverai.exe"
    }
}
