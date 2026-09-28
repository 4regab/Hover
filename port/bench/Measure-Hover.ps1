<#
.SYNOPSIS
  The frozen benchmark (port/phase0/BENCHMARK.md) for one Hover build.

.EXAMPLE
  pwsh port/bench/Measure-Hover.ps1 -Exe .\publish\Hover.exe -Impl csharp -Out .\bench-out
  pwsh port/bench/Measure-Hover.ps1 -Exe .\native\target\release\hover.exe -Impl native -Out .\bench-out

  Needs PowerShell 7, and the fixture tools built (see port/README.md):
    -Fixture  port/tools/HoverFixture/bin/Release/.../HoverFixture.exe
    -FakeAcp  port/tools/FakeAcp/bin/Release/.../FakeAcp.exe
  Takes over the keyboard (Alt+N, Esc) and the pointer while it runs.
#>
param(
    [Parameter(Mandatory)] [string] $Exe,
    [ValidateSet('csharp', 'native')] [string] $Impl = 'csharp',
    [string] $Out = (Join-Path $PWD 'bench-out'),
    [string[]] $Scenarios = @('S1', 'S2', 'S3', 'S4', 'S5', 'S6'),
    [int] $Runs = 5,
    [string] $Fixture,
    [string] $FakeAcp,
    [int] $Turns = 200,
    # CI: shorter waits, and fewer repeats. Never used for the numbers of record.
    [switch] $Quick
)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$Exe = (Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$cdpPort = 9333
$settle = if ($Quick) { 12 } else { 45 }        # S2: WebView2's 30 s drop timer + 15 s
$sampleSeconds = if ($Quick) { 4 } else { 10 }
$loops = if ($Quick) { 4 } else { 20 }

Add-Type -Namespace Bench -Name Win -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
public static void Chord(byte mod, byte key) {
    if (mod != 0) keybd_event(mod, 0, 0, UIntPtr.Zero);
    keybd_event(key, 0, 0, UIntPtr.Zero); keybd_event(key, 0, 2, UIntPtr.Zero);
    if (mod != 0) keybd_event(mod, 0, 2, UIntPtr.Zero);
}
'@
function AltN { [Bench.Win]::Chord(0x12, 0x4E) }   # VK_MENU, 'N'
function Esc { [Bench.Win]::Chord(0, 0x1B) }
function NowMs { [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }

# ---- environment -------------------------------------------------------------
function Write-Env {
    $os = Get-CimInstance Win32_OperatingSystem
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $gpu = Get-CimInstance Win32_VideoController | ForEach-Object { [ordered]@{ name = $_.Name; driver = $_.DriverVersion; ramMB = [math]::Round($_.AdapterRAM / 1MB) } }
    Add-Type -AssemblyName System.Windows.Forms
    $screens = [System.Windows.Forms.Screen]::AllScreens | ForEach-Object { [ordered]@{ device = $_.DeviceName; primary = $_.Primary; bounds = "$($_.Bounds)"; work = "$($_.WorkingArea)" } }
    $dpi = (Get-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics' -Name AppliedDPI -ErrorAction SilentlyContinue).AppliedDPI
    $wv2 = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv
    [ordered]@{
        when = (Get-Date).ToString('o'); impl = $Impl; exe = $Exe; exeSha256 = (Get-FileHash $Exe).Hash
        commit = (git -C $repo rev-parse HEAD 2>$null); os = "$($os.Caption) $($os.Version) build $($os.BuildNumber)"
        cpu = $cpu.Name; cores = $cpu.NumberOfLogicalProcessors; ramGB = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
        gpu = @($gpu); screens = @($screens); appliedDpi = $dpi; webview2 = $wv2
        ci = [bool]$env:GITHUB_ACTIONS; quick = [bool]$Quick
    } | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Out "env-$Impl.json")
}

# ---- process tree and samples ------------------------------------------------
function Get-Tree([int] $root) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, CreationDate
    $kids = @{}
    foreach ($p in $all) { if (-not $kids[[int]$p.ParentProcessId]) { $kids[[int]$p.ParentProcessId] = @() }; $kids[[int]$p.ParentProcessId] += $p }
    $out = @(); $stack = [System.Collections.Stack]::new(); $stack.Push($root)
    $byId = @{}; foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }
    while ($stack.Count) {
        $id = $stack.Pop(); if ($byId[$id]) { $out += $byId[$id] }
        foreach ($k in @($kids[$id])) { if ($k -and $k.ProcessId -ne $id) { $stack.Push([int]$k.ProcessId) } }
    }
    $out
}

# Drawing helpers Hover owns count as the application; every other child is a provider.
function Group-Of($name, [int] $id, [int] $root) {
    if ($id -eq $root -or $name -ieq 'msedgewebview2.exe') { 'app' } else { 'provider' }
}

function Get-GpuMemory {
    $gpu = @{}
    try {
        $c = Get-Counter '\GPU Process Memory(*)\Dedicated Usage', '\GPU Process Memory(*)\Shared Usage' -ErrorAction Stop
        foreach ($s in $c.CounterSamples) {
            if ($s.InstanceName -match '^pid_(\d+)_') { $gpu[[int]$Matches[1]] += $s.CookedValue }
        }
    } catch { }
    $gpu
}

function Sample([int] $root) {
    $tree = @(Get-Tree $root)
    if (-not $tree) { return $null }
    $ids = $tree | ForEach-Object { [int]$_.ProcessId }
    $filter = ($ids | ForEach-Object { "IDProcess=$_" }) -join ' OR '
    $pws = @{}
    Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter $filter -Property IDProcess, WorkingSetPrivate |
        ForEach-Object { $pws[[int]$_.IDProcess] = [double]$_.WorkingSetPrivate }
    $gpu = Get-GpuMemory
    $rows = foreach ($p in $tree) {
        $gp = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
        if (-not $gp) { continue }
        [pscustomobject]@{
            pid = [int]$p.ProcessId; name = $p.Name; group = Group-Of $p.Name $p.ProcessId $root
            privateWS = $pws[[int]$p.ProcessId]; privateCommit = [double]$gp.PrivateMemorySize64
            workingSet = [double]$gp.WorkingSet64; handles = $gp.HandleCount
            cpuMs = $gp.TotalProcessorTime.TotalMilliseconds; gpu = [double]$gpu[[int]$p.ProcessId]
        }
    }
    $sum = { param($g, $f) ($rows | Where-Object group -eq $g | Measure-Object -Property $f -Sum).Sum }
    [pscustomobject]@{
        t = NowMs; processes = @($rows)
        app = [ordered]@{ privateWS = & $sum 'app' privateWS; privateCommit = & $sum 'app' privateCommit; handles = & $sum 'app' handles; cpuMs = & $sum 'app' cpuMs; gpu = & $sum 'app' gpu; count = @($rows | Where-Object group -eq 'app').Count }
        providers = [ordered]@{ privateWS = & $sum 'provider' privateWS; privateCommit = & $sum 'provider' privateCommit; count = @($rows | Where-Object group -eq 'provider').Count }
    }
}

function Samples([int] $root, [int] $seconds) {
    $list = @()
    for ($i = 0; $i -lt $seconds; $i++) { $t = NowMs; $s = Sample $root; if ($s) { $list += $s }; Start-Sleep -Milliseconds ([math]::Max(0, 1000 - ((NowMs) - $t))) }
    $list
}

function Median($xs) { $a = @($xs | Where-Object { $_ -ne $null } | Sort-Object); if (-not $a) { return $null }; $a[[int][math]::Floor(($a.Count - 1) / 2)] }

function Summary($samples) {
    if (-not $samples) { return $null }
    $first = $samples[0]; $last = $samples[-1]; $secs = [math]::Max(1, ($last.t - $first.t) / 1000)
    [ordered]@{
        appPrivateWS = Median ($samples | ForEach-Object { $_.app.privateWS })
        appPrivateCommit = Median ($samples | ForEach-Object { $_.app.privateCommit })
        appHandles = Median ($samples | ForEach-Object { $_.app.handles })
        appGpu = Median ($samples | ForEach-Object { $_.app.gpu })
        appCpuPerSecMs = ($last.app.cpuMs - $first.app.cpuMs) / $secs
        appProcesses = $last.app.count
        providerPrivateWS = Median ($samples | ForEach-Object { $_.providers.privateWS })
        providerPrivateCommit = Median ($samples | ForEach-Object { $_.providers.privateCommit })
        providerProcesses = $last.providers.count
    }
}

# ---- the office page over DevTools (C# only) -------------------------------------
function Invoke-Page([string] $expr, [int] $timeoutSec = 20) {
    if ($Impl -ne 'csharp') { return $null }
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        try {
            $targets = Invoke-RestMethod "http://127.0.0.1:$cdpPort/json" -TimeoutSec 2
            $page = $targets | Where-Object { $_.type -eq 'page' -and $_.url -like 'https://hover.office/*' } | Select-Object -First 1
            if ($page) { break }
        } catch { }
        Start-Sleep -Milliseconds 250
    }
    if (-not $page) { throw 'the office page did not appear on DevTools' }
    $ws = [System.Net.WebSockets.ClientWebSocket]::new()
    $ws.ConnectAsync([uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
    $msg = @{ id = 1; method = 'Runtime.evaluate'; params = @{ expression = $expr; awaitPromise = $true; returnByValue = $true } } | ConvertTo-Json -Depth 5 -Compress
    $bytes = [Text.Encoding]::UTF8.GetBytes($msg)
    $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, [Threading.CancellationToken]::None).Wait()
    $buf = [byte[]]::new(1MB); $sb = [Text.StringBuilder]::new()
    do {
        $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), [Threading.CancellationToken]::None).Result
        [void]$sb.Append([Text.Encoding]::UTF8.GetString($buf, 0, $r.Count))
    } until ($r.EndOfMessage)
    $ws.Dispose()
    ($sb.ToString() | ConvertFrom-Json).result.result.value
}

# Frame and input probes, injected into the page: render times from its FRAMES counter,
# and pointermove -> the next rendered frame. `visible` (Hover showing the page) -> the
# next rendered frame is the reopen time.
$probe = @'
(() => { if (window.__probe) return true; window.__probe = { frames: [], input: [], shown: [] };
  let seen = window.FRAMES || 0, pend = null, vis = null;
  addEventListener('pointermove', e => { if (pend == null) pend = performance.now(); }, { passive: true });
  window.chrome?.webview?.addEventListener('message', e => { if (e.data?.type === 'visible' && e.data.on) vis = Date.now(); });
  (function f() { const n = window.FRAMES || 0; if (n !== seen) { seen = n; const t = performance.now();
      __probe.frames.push(t); if (pend != null) { __probe.input.push(t - pend); pend = null; }
      if (vis != null) { __probe.shown.push([vis, Date.now()]); vis = null; } }
    requestAnimationFrame(f); })();
  return true; })()
'@

function Frame-Stats($times) {
    if (-not $times -or $times.Count -lt 3) { return $null }
    $d = @(); for ($i = 1; $i -lt $times.Count; $i++) { $d += $times[$i] - $times[$i - 1] }
    $s = $d | Sort-Object
    $p = { param($q) $s[[int][math]::Min($s.Count - 1, [math]::Floor($q * $s.Count))] }
    [ordered]@{ count = $d.Count; p50 = & $p 0.5; p95 = & $p 0.95; p99 = & $p 0.99 }
}

# ---- one run ---------------------------------------------------------------------
function New-DataDir([string] $run) {
    $dir = Join-Path $Out "data-$Impl-$run"
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    $proj = Join-Path $Out 'project'
    $key = if ($Fixture) { (& $Fixture $dir $proj $Turns (Join-Path $repo 'native\golden\fixtures\rich.md')) | Select-Object -Last 1 } else { $null }
    if (-not $Fixture) {
        New-Item -ItemType Directory -Force $dir, $proj | Out-Null
        @{ HoverOpensWorkspace = $false; NotchItems = @(); KiroFolder = "$proj"; KiroNoticeSeen = $true } | ConvertTo-Json | Set-Content (Join-Path $dir 'settings.json')
    }
    @{ dir = $dir; key = $key; project = $proj }
}

function Start-Hover($data) {
    Get-Process Hover, hover-notch-proto -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep 1
    $bin = Join-Path $Out 'fakebin'
    if ($FakeAcp) {
        New-Item -ItemType Directory -Force $bin | Out-Null
        foreach ($n in 'kiro-cli', 'codex-acp', 'cursor-agent') { "@`"$FakeAcp`" %*" | Set-Content (Join-Path $bin "$n.cmd") -Encoding ascii }
    }
    $psi = [Diagnostics.ProcessStartInfo]::new($Exe)
    $psi.UseShellExecute = $false
    $psi.Environment['HOVER_DATA_DIR'] = $data.dir
    $psi.Environment['PATH'] = "$bin;$env:PATH"
    $psi.Environment['FAKEACP_ANSWER'] = Join-Path $repo 'native\golden\fixtures\rich.md'
    $psi.Environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = "--remote-debugging-port=$cdpPort"
    $t0 = NowMs
    $p = [Diagnostics.Process]::Start($psi)
    $deadline = (Get-Date).AddSeconds(30); $visible = $null
    while ((Get-Date) -lt $deadline) {
        $h = [Bench.Win]::FindWindow($null, 'Hover notch')
        if ($h -ne [IntPtr]::Zero -and [Bench.Win]::IsWindowVisible($h)) { $visible = NowMs; break }
        Start-Sleep -Milliseconds 20
    }
    [pscustomobject]@{ process = $p; startupMs = if ($visible) { $visible - $t0 } else { $null } }
}

function Open-Office { AltN; Start-Sleep -Milliseconds 300 }

function Run-One([string] $run) {
    $r = [ordered]@{ run = $run; impl = $Impl }
    $data = New-DataDir $run
    $h = Start-Hover $data
    $root = $h.process.Id
    $r.S1 = [ordered]@{ startupMs = $h.startupMs }
    Start-Sleep 10
    $r.S1.memory = Summary @(Sample $root)

    if ('S3' -in $Scenarios -or 'S2' -in $Scenarios) {
        Open-Office; Start-Sleep 5
        if ($Impl -eq 'csharp') { try { Invoke-Page "localStorage.setItem('office.time','night'); localStorage.setItem('office.beats','off'); true" | Out-Null } catch { $r.pageError = "$_" } }
        Invoke-Page $probe | Out-Null
        Start-Sleep ($sampleSeconds - 5 + 5)
        # The pointer moves over the office while samples run, for input response.
        $mover = Start-ThreadJob { param($n) Add-Type -Namespace B -Name W -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);'; for ($i = 0; $i -lt $n * 10; $i++) { [B.W]::SetCursorPos(700 + ($i % 40) * 5, 200 + ($i % 7) * 3); Start-Sleep -Milliseconds 100 } } -ArgumentList $sampleSeconds
        $r.S3 = [ordered]@{ memory = Summary (Samples $root $sampleSeconds) }
        Wait-Job $mover | Remove-Job
        $pr = Invoke-Page 'JSON.stringify({ frames: __probe.frames, input: __probe.input })'
        if ($pr) { $pj = $pr | ConvertFrom-Json; $r.S3.frames = Frame-Stats $pj.frames; $r.S3.inputMs = Median $pj.input }
        Esc
        Start-Sleep $settle
        $r.S2 = [ordered]@{ settleSeconds = $settle; memory = Summary (Samples $root $sampleSeconds) }
    }

    if ('S4' -in $Scenarios -and $data.key -and $Impl -eq 'csharp') {
        Open-Office; Start-Sleep 4
        Invoke-Page $probe | Out-Null
        $t = Invoke-Page @"
new Promise(res => { const t0 = performance.now(); document.querySelector('#histBtn').click();
  const want = $Turns; const tick = () => { const card = document.querySelector('#pBody [data-key="$($data.key)"]');
    if (card && !window.__clicked) { window.__clicked = 1; card.click(); }
    if (document.querySelectorAll('#thread .ans').length >= want) { const th = document.querySelector('#thread'); th.scrollTop = 0;
      requestAnimationFrame(() => { th.scrollTop = 1e9; res(performance.now() - t0); }); } else setTimeout(tick, 16); }; tick(); })
"@ 60
        Start-Sleep 5
        $r.S4 = [ordered]@{ openMs = $t; memory = Summary (Samples $root $sampleSeconds) }
        Esc; Esc
    }

    if ('S5' -in $Scenarios -and $FakeAcp -and $Impl -eq 'csharp') {
        Open-Office; Start-Sleep 3
        $env:FAKEACP_SECONDS = '30'
        for ($k = 0; $k -lt 3; $k++) {
            Invoke-Page @"
new Promise(res => { document.querySelector('#fabMain').click(); setTimeout(() => { document.querySelector('#fabTools [data-tool="kiro"]').click();
  setTimeout(() => { const i = document.querySelector('#nInput'); i.value = 'Benchmark task $k'; i.dispatchEvent(new Event('input'));
    document.querySelector('#nGo').click(); res(true); }, 150); }, 150); })
"@ | Out-Null
            Start-Sleep 1
        }
        Invoke-Page "document.querySelector('.tag .nm')?.click(); true" | Out-Null
        $r.S5 = [ordered]@{ during = Summary (Samples $root $sampleSeconds) }
        Start-Sleep ([math]::Max(0, 36 - $sampleSeconds))
        $r.S5.after = Summary (Samples $root $sampleSeconds)
        Esc; Esc
    }

    if ('S6' -in $Scenarios) {
        Open-Office; Start-Sleep 3
        Invoke-Page $probe | Out-Null
        for ($k = 0; $k -lt $loops; $k++) { Esc; Start-Sleep 2; AltN; Start-Sleep 2 }
        $shown = Invoke-Page 'JSON.stringify(__probe.shown)'
        $r.S6 = [ordered]@{ loops = $loops }
        if ($shown) { $r.S6.reopenMs = Median (($shown | ConvertFrom-Json) | ForEach-Object { $_[1] - $_[0] }) }
        $r.S6.memoryAfterLoops = Summary (Samples $root $sampleSeconds)
        Esc
    }

    Stop-Process -Id $root -Force -ErrorAction SilentlyContinue
    Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "*$($data.dir)*" } | Stop-Process -Force -ErrorAction SilentlyContinue
    $r
}

Write-Env
$results = @()
for ($i = 1; $i -le $Runs; $i++) {
    Write-Host "run $i of $Runs ($Impl)"
    try { $results += Run-One "$i" } catch { $results += [ordered]@{ run = "$i"; error = "$_" }; Write-Warning "$_" }
}
$results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Out "results-$Impl.json")
Write-Host "wrote $(Join-Path $Out "results-$Impl.json")"
