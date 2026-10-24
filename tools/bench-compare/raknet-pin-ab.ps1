#requires -Version 7.0
<#
.SYNOPSIS
RakNet loopback A/B: go-raknet old pin vs new pin vs tokio-raknet Vanilla and Optimized.

.DESCRIPTION
Builds `bench-compare` (release), the Go adapter at the pin in tools/bench-compare/go/go.mod
and a second Go adapter re-pinned to -OldPin in a scratch copy (sources unchanged), all on
E-core logical CPUs 16-31 with at most eight jobs. Then pins itself to one logical CPU
(-Processor, a P-core on the measured i9-14900HX) so every child inherits that affinity, and
runs `loopback --count N` for each implementation in a rotating order for -Rounds rounds.
One worker per side (GOMAXPROCS=1, TOKIO_WORKER_THREADS=1) matches the single core.

Rows per size (32/512/4096/65536 B): `echo` (sequential request/echo latency and throughput)
and `setup` (the RakNet handshake of that fresh connection, dial until connected). The
summary holds medians of the per-round statistics; it records the power plan's maximum
processor state, because turbo boost changes absolute timings.

Run only when the coordinator has reserved the machine: concurrent benchmarks corrupt both.
#>
[CmdletBinding()]
param(
    [ValidateRange(3, 99)][int]$Rounds = 5,
    [ValidateRange(1, 100000)][int]$Count = 100,
    [ValidateRange(1, 100000)][int]$SplitCount = 10,
    [ValidateRange(0, 63)][int]$Processor = 4,
    [string]$OldPin = 'v1.15.1-0.20261011161132-87a6b7e53799',
    [string]$Output = (Join-Path $PSScriptRoot 'results'),
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Cache = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force $Cache | Out-Null
$Run = Join-Path $Cache ('raknet-pin-ab-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $Run | Out-Null
Write-Host "Raw evidence: $Run"
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
$Suffix = if ($IsWindows) { '.exe' } else { '' }
$GoSource = Join-Path $PSScriptRoot 'go'
$OldSource = Join-Path $Cache 'go-raknet-old-src'
$OldShort = ($OldPin -split '-')[-1].Substring(0, 7)
$NewVersion = (& go -C $GoSource list -m -f '{{.Replace.Version}}' github.com/sandertv/go-raknet | Out-String).Trim()
if ($LASTEXITCODE -or !$NewVersion) { throw 'Could not resolve the go-raknet replacement of tools/bench-compare/go' }
$NewShort = ($NewVersion -split '-')[-1].Substring(0, 7)
if ($NewShort -eq $OldShort) { throw "-OldPin $OldPin equals the current pin" }
$Sides = [ordered]@{
    "go-$OldShort"    = @{ Exe = (Join-Path $Cache "go-raknet-$OldShort$Suffix"); Policy = $null }
    "go-$NewShort"    = @{ Exe = (Join-Path $Cache "go-raknet-$NewShort$Suffix"); Policy = $null }
    'tokio-vanilla'   = @{ Exe = (Join-Path $Repo "target/release/bench-compare$Suffix"); Policy = 'vanilla' }
    'tokio-optimized' = @{ Exe = (Join-Path $Repo "target/release/bench-compare$Suffix"); Policy = 'optimized' }
}
$Parent = [Diagnostics.Process]::GetCurrentProcess()
$OldAffinity = $Parent.ProcessorAffinity
Push-Location $Repo
try {
    if (!$SkipBuild) {
        # Builds stay on this workstation's E-cores (logical CPUs 16-31), at most eight jobs.
        $Parent.ProcessorAffinity = [IntPtr]4294901760
        & cargo build --release -j 8 -p bench-compare
        if ($LASTEXITCODE) { throw 'Rust build failed' }
        & go -C $GoSource build -p 8 -o $Sides["go-$NewShort"].Exe .
        if ($LASTEXITCODE) { throw 'Go build (current pin) failed' }
        # Scratch copy re-pinned to the old revision; the tracked module is not modified.
        if (Test-Path $OldSource) { Remove-Item -Recurse -Force $OldSource }
        New-Item -ItemType Directory $OldSource | Out-Null
        Copy-Item (Join-Path $GoSource '*.go'), (Join-Path $GoSource 'go.mod'), (Join-Path $GoSource 'go.sum') $OldSource
        & go -C $OldSource mod edit -replace "github.com/sandertv/go-raknet=github.com/lunar-bedrock/go-raknet@$OldPin"
        if ($LASTEXITCODE) { throw 'go mod edit failed' }
        & go -C $OldSource mod tidy
        if ($LASTEXITCODE) { throw 'go mod tidy (old pin) failed' }
        & go -C $OldSource build -p 8 -o $Sides["go-$OldShort"].Exe .
        if ($LASTEXITCODE) { throw 'Go build (old pin) failed' }
    }
    foreach ($side in $Sides.Keys) {
        if (!(Test-Path $Sides[$side].Exe)) { throw "Missing executable for $side; run without -SkipBuild" }
    }
    $maxState = 'not recorded (non-Windows)'
    if ($IsWindows) {
        $powercfg = (& powercfg /query SCHEME_CURRENT SUB_PROCESSOR PROCTHROTTLEMAX | Out-String)
        if ($powercfg -notmatch 'Current AC Power Setting Index:\s*0x([0-9a-fA-F]+)') { throw 'Could not read the maximum processor state from powercfg' }
        $maxState = "$([Convert]::ToInt32($Matches[1], 16))%"
    }
    $metadata = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o'); rounds = $Rounds; count = $Count; split_count = $SplitCount
        processor = $Processor; workers = 1; max_processor_state_ac = $maxState
        go_raknet_old = $OldPin; go_raknet_new = $NewVersion
        commit = (& git rev-parse HEAD | Out-String).Trim()
        rust = (& rustc --version | Out-String).Trim(); go = (& go version | Out-String).Trim()
        cpu = $env:PROCESSOR_IDENTIFIER
    }
    $metadata | ConvertTo-Json | Set-Content (Join-Path $Run 'metadata.json')
    $env:GOMAXPROCS = '1'
    $env:TOKIO_WORKER_THREADS = '1'
    $env:BENCH_SPLIT_COUNT = "$SplitCount"
    $env:BENCH_TRACE_SPLITS = '0'
    # Pin before spawning: children inherit the single-CPU mask with no unpinned window.
    $Parent.ProcessorAffinity = [IntPtr]([long]1 -shl $Processor)
    $names = @($Sides.Keys)
    foreach ($round in 1..$Rounds) {
        $shift = ($round - 1) % $names.Count
        $order = $names[$shift..($names.Count - 1)] + $(if ($shift) { $names[0..($shift - 1)] } else { @() })
        if ($round % 2 -eq 0) { [array]::Reverse($order) }
        foreach ($side in $order) {
            Write-Host "Round $round/$Rounds $side"
            if ($Sides[$side].Policy) { $env:BENCH_RAKNET_POLICY = $Sides[$side].Policy } else { Remove-Item Env:BENCH_RAKNET_POLICY -ErrorAction SilentlyContinue }
            $out = Join-Path $Run "$round-$side.jsonl"
            $err = Join-Path $Run "$round-$side.stderr.log"
            $process = Start-Process -FilePath $Sides[$side].Exe -ArgumentList @('loopback', '--count', "$Count") -WorkingDirectory $Repo `
                -RedirectStandardOutput $out -RedirectStandardError $err -NoNewWindow -PassThru
            # Caching the handle keeps ExitCode available after the process exits.
            $null = $process.Handle
            if (!$process.WaitForExit(600000)) { $process.Kill($true); throw "$side round $round timed out; see $err" }
            if ($process.ExitCode) { throw "$side round $round failed ($($process.ExitCode)); see $err" }
        }
    }
    Remove-Item Env:BENCH_RAKNET_POLICY -ErrorAction SilentlyContinue
    function Median([double[]]$values) {
        $sorted = @($values | Sort-Object)
        $mid = [int][math]::Floor($sorted.Count / 2)
        if ($sorted.Count % 2) { return $sorted[$mid] }
        return ($sorted[$mid - 1] + $sorted[$mid]) / 2
    }
    $rows = foreach ($file in Get-ChildItem $Run -Filter '*.jsonl') {
        $side = ($file.BaseName -split '-', 2)[1]
        foreach ($line in Get-Content $file) {
            if ($line.StartsWith('{')) {
                $row = $line | ConvertFrom-Json
                if ($row.area -eq 'loopback') { [pscustomobject]@{ Side = $side; Size = [int]$row.name; Op = $row.operation; Row = $row } }
            }
        }
    }
    $fmt = { param($v) if ($null -eq $v) { '—' } else { '{0:N2}' -f $v } }
    $lines = @(
        "# RakNet loopback A/B: go-raknet $OldShort vs $NewShort vs tokio-raknet",
        '',
        "Measured UTC $($metadata.utc); $Rounds rotating rounds; logical CPU $Processor only; GOMAXPROCS=1, TOKIO_WORKER_THREADS=1;",
        "$Count echoes for 32/512 B, $SplitCount for 4096/65536 B, one untimed readiness echo; power plan maximum processor state (AC) $maxState",
        "(below 100% disables turbo boost). Source $($metadata.commit); $($metadata.rust); $($metadata.go).",
        'Medians of per-round statistics, microseconds. `setup` is one handshake per fresh connection (dial until connected).',
        '',
        '| Size | Operation | Side | Mean µs | p50 µs | p99 µs | msgs/s | MB/s | Rounds |',
        '| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |'
    )
    foreach ($group in ($rows | Group-Object Size, Op, Side | Sort-Object { $_.Group[0].Size }, { $_.Group[0].Op }, { $names.IndexOf($_.Group[0].Side) })) {
        $first = $group.Group[0]
        $values = $group.Group.Row
        $setup = $first.Op -eq 'setup'
        $p50 = @($values.p50_us | Where-Object { $null -ne $_ })
        $p99 = @($values.p99_us | Where-Object { $null -ne $_ })
        $lines += "| $($first.Size) | $($first.Op) | $($first.Side) | $(& $fmt ((Median $values.ns_per_op) / 1000)) | " +
            "$(& $fmt $(if ($p50.Count) { Median $p50 })) | $(& $fmt $(if ($p99.Count) { Median $p99 })) | " +
            "$(& $fmt $(if (!$setup) { Median $values.msgs_per_sec })) | $(& $fmt $(if (!$setup) { Median $values.mb_per_sec })) | $($group.Count) |"
    }
    $summary = Join-Path $Run 'summary.md'
    $lines | Set-Content $summary -Encoding utf8
    $lines | Write-Host
    Write-Host "Summary: $summary"
} finally {
    Pop-Location
    $Parent.ProcessorAffinity = $OldAffinity
}
