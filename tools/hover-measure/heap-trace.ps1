<#
  Sums a heap-trace (bench `heap-trace N` in a profiling build writes one line per
  allocation of N bytes or more to stderr) by the first frames of its stack.

    .\tools\hover-measure\heap-trace.ps1 -Log evidence\memory\x\run1\hover-stderr.log -Depth 3 -Top 25
#>
param(
    [Parameter(Mandatory = $true)][string]$Log,
    [int]$Depth = 3,
    [int]$Top = 25
)
$rows = @{}
$total = 0
foreach ($l in (Get-Content $Log | Where-Object { $_ -like 'heap-trace *' })) {
    $p = $l.Split(' ', 3)
    $n = [int64]$p[1]
    $total += $n
    $frames = if ($p.Count -gt 2) { ($p[2] -split ' \| ' | Select-Object -First $Depth) -join ' < ' } else { '(no stack)' }
    if (-not $rows.ContainsKey($frames)) { $rows[$frames] = @(0, 0) }
    $rows[$frames] = @(($rows[$frames][0] + $n), ($rows[$frames][1] + 1))
}
"{0:N1} MiB in allocations traced" -f ($total / 1MB)
$rows.GetEnumerator() | Sort-Object { -$_.Value[0] } | Select-Object -First $Top | ForEach-Object {
    "{0,9:N1} MiB  x{1,-5} {2}" -f ($_.Value[0] / 1MB), $_.Value[1], $_.Key
}
