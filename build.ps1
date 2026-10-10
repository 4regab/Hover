<#
  Hover for Windows - build helper (Go, from the repo root).

    .\build.ps1              hoverai.exe and wgpu_native.dll in .\publish
    .\build.ps1 run          build, then start it
    .\build.ps1 publish      the same, with LICENSE and THIRD-PARTY-NOTICES.txt beside them
    .\build.ps1 installer    Inno Setup installer in .\dist (the version in VERSION)
    .\build.ps1 installer -Version 5.0.3
    .\build.ps1 test         the Go tests (CI does not run them)

  Needs Go (the version go.mod names; winget install GoLang.Go). No C compiler: the Windows
  build is plain Go. The installer needs Inno Setup 6 or 7.
#>
param(
    [string]$Mode = "build",
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$publishDir = Join-Path $PSScriptRoot "publish"
$installerScript = Join-Path $PSScriptRoot "packaging\windows\Hover.iss"
$distDir = Join-Path $PSScriptRoot "dist"
# The office draws with wgpu-native; the exe looks for its DLL beside itself.
$wgpuUrl = "https://github.com/gfx-rs/wgpu-native/releases/download/v29.0.0.0/wgpu-windows-x86_64-msvc-release.zip"

if ((-not [string]::IsNullOrWhiteSpace($Version)) -and
    $Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must have three numeric parts, for example 5.0.3"
}

function Stop-Hover {
    # Hover: an install from before the rename, which would hold the single-instance lock.
    Get-Process hoverai, Hover -ErrorAction SilentlyContinue | Stop-Process -Force
}

function Get-ProjectVersion {
    if (-not [string]::IsNullOrWhiteSpace($Version)) { return $Version }
    $line = (Get-Content (Join-Path $PSScriptRoot "VERSION") -TotalCount 1).Trim()
    if ($line -notmatch '^\d+\.\d+\.\d+$') { throw "Could not read the version from VERSION" }
    return $line
}

function Build-Hover {
    Stop-Hover
    $v = Get-ProjectVersion
    New-Item -ItemType Directory -Path $publishDir -Force | Out-Null
    $env:CGO_ENABLED = "0"
    Push-Location $PSScriptRoot
    try {
        # Go writes "go: downloading ..." to stderr; Windows PowerShell turns that into a
        # terminating error under "Stop" whenever the output is redirected (a log, CI).
        $ErrorActionPreference = "Continue"
        & go build -ldflags "-H=windowsgui -X github.com/4regab/Hover/internal/shell.Version=$v" -o (Join-Path $publishDir "hoverai.exe") .\cmd\hover
        $code = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($code -ne 0) { throw "go build failed" }
    } finally { Pop-Location }
    $dll = Join-Path $publishDir "wgpu_native.dll"
    if (-not (Test-Path $dll)) {
        $zip = Join-Path ([IO.Path]::GetTempPath()) "wgpu-native.zip"
        $tmp = Join-Path ([IO.Path]::GetTempPath()) "wgpu-native"
        Invoke-WebRequest $wgpuUrl -OutFile $zip
        Expand-Archive $zip $tmp -Force
        $found = Get-ChildItem $tmp -Recurse -Filter wgpu_native.dll | Select-Object -First 1
        if (-not $found) { throw "no wgpu_native.dll in the wgpu-native release zip" }
        Copy-Item $found.FullName $dll -Force
    }
    return (Join-Path $publishDir "hoverai.exe")
}

function Publish-Hover {
    $exe = Build-Hover
    Copy-Item (Join-Path $PSScriptRoot "LICENSE") $publishDir -Force
    Copy-Item (Join-Path $PSScriptRoot "THIRD-PARTY-NOTICES.txt") $publishDir -Force
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

switch ($Mode.ToLower()) {
    "publish" {
        Publish-Hover | Out-Null
        Write-Host "publish\hoverai.exe"
    }
    "installer" {
        Publish-Hover | Out-Null
        $resolvedVersion = Get-ProjectVersion
        $iscc = Find-InnoCompiler
        New-Item -ItemType Directory -Path $distDir -Force | Out-Null
        & $iscc "/DMyAppVersion=$resolvedVersion" "/DExeDir=$publishDir" "/O$distDir" $installerScript
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }
        Write-Host (Join-Path $distDir "Hover-Setup-$resolvedVersion.exe")
    }
    "test" {
        Push-Location $PSScriptRoot
        try {
            & go test .\cmd\... .\internal\...
            if ($LASTEXITCODE -ne 0) { throw "go test failed" }
        } finally { Pop-Location }
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
