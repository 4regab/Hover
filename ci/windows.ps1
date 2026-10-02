# The Windows half of CI. ci/buildspec-windows.yml runs it with PowerShell 7 on
# CodeBuild's Windows Server 2022 image: the tests, and on a release the installer, which
# it copies to the CI bucket for ci/release.sh.
$ErrorActionPreference = 'Stop'
# The progress bar makes Invoke-WebRequest many times slower.
$ProgressPreference = 'SilentlyContinue'
Set-Location $env:CODEBUILD_SRC_DIR

# Runs a program and throws if it fails. PowerShell doesn't stop on a program's exit code
# by itself, and a program's stderr (cargo's progress) isn't an error.
function Invoke-Checked([string]$What, [scriptblock]$Run) {
    $global:LASTEXITCODE = 0
    & { $ErrorActionPreference = 'Continue'; & $Run }
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit code $LASTEXITCODE)" }
}

# A release: a pushed v* tag (it must match Cargo.toml), or a push to rust-port/phase-0-1
# whose Cargo.toml version has no tag yet (so raising the version is what releases it).
# ci/linux.sh makes the same choice.
$crate = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$ref = "$env:CODEBUILD_WEBHOOK_HEAD_REF"
$hookEvent = "$env:CODEBUILD_WEBHOOK_EVENT"
$release = $false
if ($hookEvent -eq 'PUSH' -and $ref -like 'refs/tags/v*') {
    if ($ref -ne "refs/tags/v$crate") { throw "The tag is $($ref -replace '^refs/tags/') but Cargo.toml says $crate." }
    $release = $true
} elseif ($hookEvent -eq 'PUSH' -and $ref -eq 'refs/heads/rust-port/phase-0-1') {
    $tagged = Invoke-Checked "Listing the tags" { git ls-remote --tags $env:CODEBUILD_SOURCE_REPO_URL "refs/tags/v$crate" }
    $release = -not $tagged
}
Write-Host "event '$hookEvent', ref '$ref': version $crate, release: $release"

# Rust on Windows links with MSVC. Its build tools go in only if the image lacks them.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if ((Test-Path $vswhere) -and (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)) {
    Write-Host 'MSVC build tools already installed'
} else {
    $setup = Join-Path $env:TEMP 'vs_buildtools.exe'
    Invoke-WebRequest 'https://aka.ms/vs/17/release/vs_buildtools.exe' -OutFile $setup
    $process = Start-Process $setup -ArgumentList '--quiet', '--wait', '--norestart', '--nocache',
        '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended' -Wait -PassThru
    # 3010: installed, a restart is suggested. Not needed for building.
    if ($process.ExitCode -notin 0, 3010) { throw "Build tools install failed: $($process.ExitCode)" }
}

# Cargo's home has a fixed path, outside the source folder (whose path changes every
# build): the cache (in the buildspec) keeps its registry and git folders by that path,
# and Cargo rebuilds a cached crate whose source path moved. rust-toolchain.toml picks
# the version. rustup isn't let update itself: that once broke the rustup call after it.
$env:CARGO_HOME = 'C:\cargo-home'
$cargoBin = Join-Path $env:CARGO_HOME 'bin'
if (-not (Test-Path (Join-Path $cargoBin 'rustup.exe'))) {
    $init = Join-Path $env:TEMP 'rustup-init.exe'
    Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $init
    Invoke-Checked 'rustup install' { & $init -y --profile minimal --default-toolchain none --no-modify-path }
}
$env:PATH = "$cargoBin;$env:PATH"
Invoke-Checked 'rustup set' { rustup set auto-self-update disable }
# cargo installs rust-toolchain.toml's version the first time it runs.
Invoke-Checked 'rustup toolchain install' { cargo --version }

# Server Core has almost no fonts, so the chat's text laid out empty. The first faces
# its font lists name go in: Inter (the repo's) and Cascadia Code (OFL, Microsoft's).
$zip = Join-Path $env:TEMP 'cascadia.zip'
Invoke-WebRequest 'https://github.com/microsoft/cascadia-code/releases/download/v2407.24/CascadiaCode-2407.24.zip' -OutFile $zip
Expand-Archive $zip (Join-Path $env:TEMP 'cascadia') -Force
$files = @(Get-ChildItem app\assets\Inter-*.ttf) + @(Get-ChildItem (Join-Path $env:TEMP 'cascadia\ttf\static\CascadiaCode-*.ttf'))
if ($files.Count -lt 5) { throw "Expected the Inter and Cascadia Code files, found $($files.Count)" }
$key = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts'
foreach ($f in $files) {
    Copy-Item $f.FullName (Join-Path $env:windir 'Fonts') -Force
    New-ItemProperty -Path $key -Name "$($f.BaseName) (TrueType)" -Value $f.Name -PropertyType String -Force | Out-Null
}

Invoke-Checked 'cargo test' { cargo test --release --workspace --no-fail-fast }

if (-not $release) { exit 0 }

if (-not (Test-Path "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe")) {
    $setup = Join-Path $env:TEMP 'innosetup-6.7.3.exe'
    Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' -OutFile $setup
    $process = Start-Process $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/ALLUSERS' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Inno Setup install failed: $($process.ExitCode)" }
}
.\build.ps1 installer -Version $crate

# The installer goes to installers/<this batch>/ in the CI bucket, where ci/release.sh
# looks for it. The AWS CLI goes in only if the image lacks it.
if (-not (Get-Command aws -ErrorAction SilentlyContinue)) {
    $msi = Join-Path $env:TEMP 'AWSCLIV2.msi'
    Invoke-WebRequest 'https://awscli.amazonaws.com/AWSCLIV2-2.31.0.msi' -OutFile $msi
    $process = Start-Process msiexec.exe -ArgumentList '/i', $msi, '/qn', '/norestart' -Wait -PassThru
    if ($process.ExitCode -notin 0, 3010) { throw "AWS CLI install failed: $($process.ExitCode)" }
    $env:PATH = "$env:ProgramFiles\Amazon\AWSCLIV2;$env:PATH"
}
$batch = Invoke-Checked 'Finding the batch build' { aws codebuild batch-get-builds --ids $env:CODEBUILD_BUILD_ID --query 'builds[0].buildBatchArn' --output text }
if ($batch -notlike 'arn:*') { throw 'This build is not part of a batch build, so the release build would not find the installer.' }
$prefix = "s3://$env:HOVER_CI_BUCKET/installers/$($batch.Split(':')[-1])/"
Invoke-Checked 'Uploading the installer' { aws s3 cp "dist\Hover-Setup-$crate.exe" $prefix }
