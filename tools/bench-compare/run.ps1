#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateRange(3, 99)][int]$Rounds = 5,
    [ValidateRange(1, 100000)][int]$Count = 100,
    [ValidateRange(1, 100000)][int]$SplitCount = 10,
    [ValidateRange(1, 256)][int]$GoCpu = 4,
    [string]$Output = (Join-Path $PSScriptRoot 'results'),
    [switch]$SkipBuild,
    [ValidateRange(0, 63)][int]$Processor = 2,
    [int[]]$LoopbackProcessors = @(0, 2, 4, 6)
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Cache = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force $Cache | Out-Null
$Output = Join-Path $Cache ('run-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $Output | Out-Null
Write-Host "Raw evidence: $Output"
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
$env:GOMAXPROCS = "$GoCpu"
$env:TOKIO_WORKER_THREADS = "$GoCpu"
$env:BENCH_SPLIT_COUNT = "$SplitCount"
# Diagnostic packet observation changes scheduling and is never a timed input.
$env:BENCH_TRACE_SPLITS = '0'
$Suffix = if ($IsWindows) { '.exe' } else { '' }
$Rust = Join-Path $Repo "target/release/bench-compare$Suffix"
$Go = Join-Path $Cache "go-bench$Suffix"
$CinnabarManifest = Join-Path $PSScriptRoot 'cinnabar/Cargo.toml'
$Cinnabar = Join-Path $PSScriptRoot "cinnabar/target/release/cinnabar-bench$Suffix"
$Executables = @{ rust = $Rust; cinnabar = $Cinnabar; go = $Go }
# Pin the parent before spawning: child processes inherit the mask, with no
# unpinned startup window. Logical CPU 2 is a P-core on the measured i9-14900HX.
$ParentProcess = [Diagnostics.Process]::GetCurrentProcess()
$OldAffinity = $ParentProcess.ProcessorAffinity
$Affinity = [IntPtr]([long]1 -shl $Processor)
if (@($LoopbackProcessors | Sort-Object -Unique).Count -ne $GoCpu) {
    throw 'Select one distinct P-core logical CPU per Tokio worker / GOMAXPROCS in -LoopbackProcessors'
}
$LoopbackMask = [long]0
foreach ($cpu in $LoopbackProcessors) {
    if ($cpu -lt 0 -or $cpu -gt 63) { throw 'LoopbackProcessors must be logical CPUs 0..63' }
    $LoopbackMask = $LoopbackMask -bor ([long]1 -shl $cpu)
}
function Start-Runner([string]$Exe, [string[]]$Arguments, [string]$Cwd, [bool]$Ready = $false) {
    $ParentProcess.ProcessorAffinity = if ($Arguments[0] -in @('loopback','serve','client')) {
        [IntPtr]$LoopbackMask
    } else { $Affinity }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Exe
    $info.WorkingDirectory = $Cwd
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($a in $Arguments) { $info.ArgumentList.Add($a) }
    $p = [Diagnostics.Process]::new()
    $p.StartInfo = $info
    if (!$p.Start()) { throw "Cannot start $Exe" }
    $stdout = $p.StandardOutput.ReadToEndAsync()
    if ($Ready) {
        $line = $p.StandardError.ReadLineAsync()
        if (!$line.Wait(30000) -or $line.GetAwaiter().GetResult() -notmatch '^READY') {
            if (!$p.HasExited) { $p.Kill($true); $p.WaitForExit() }
            throw "Listener failed readiness: $Exe"
        }
    }
    return @{ Process = $p; Out = $stdout; Err = $p.StandardError.ReadToEndAsync() }
}
function Finish-Runner($Job, [string]$Log, [int]$Seconds = 600) {
    if (!$Job.Process.WaitForExit($Seconds * 1000)) {
        $Job.Process.Kill($true)
        $Job.Process.WaitForExit()
        throw "Timeout: $Log"
    }
    $stdout = $Job.Out.GetAwaiter().GetResult()
    $stderr = $Job.Err.GetAwaiter().GetResult()
    [IO.File]::WriteAllText($Log + '.jsonl', $stdout)
    [IO.File]::WriteAllText($Log + '.stderr.log', $stderr)
    if ($Job.Process.ExitCode -ne 0) { throw "Exit $($Job.Process.ExitCode): $Log`n$stderr" }
    return $stdout
}
Push-Location $Repo
try {
    # Reserve this workstation's logical CPUs 16..31 for compilation.
    # Children inherit affinity before startup; Cargo also caps parallel jobs.
    $ParentProcess.ProcessorAffinity = [IntPtr]4294901760
    if (!$SkipBuild) {
        & git submodule update --init crates/valentine_gen/protocolgen
        if ($LASTEXITCODE) { throw 'Submodule initialization failed' }
        & cargo build --release -j 8 -p bench-compare
        if ($LASTEXITCODE) { throw 'Rust build failed' }
        & cargo build --release -j 8 --manifest-path $CinnabarManifest --locked
        if ($LASTEXITCODE) { throw 'Cinnabar standalone build failed' }
        Push-Location (Join-Path $PSScriptRoot 'go')
        try { & go build -p 8 -o $Go .; if ($LASTEXITCODE) { throw 'Go build failed' } } finally { Pop-Location }
    }
    $ParentProcess.ProcessorAffinity = $Affinity
    $meta = @{
        utc = [DateTime]::UtcNow.ToString('o'); rounds = $Rounds; count = $Count; go_cpu = $GoCpu
        warmup_echoes = 1
        split_count = $SplitCount
        os = [Runtime.InteropServices.RuntimeInformation]::OSDescription
        architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        processors = [Environment]::ProcessorCount
        cpu = $env:PROCESSOR_IDENTIFIER
        rust_workers = $GoCpu
        rust_release_profile = 'opt-level=3; lto=thin; codegen-units=16; strip=true (both Rust executables)'
        build_affinity_logical_cpus = @(16..31)
        build_jobs = 8
        rust = (& rustc --version | Out-String).Trim(); go = (& go version | Out-String).Trim()
        commit = (& git rev-parse HEAD | Out-String).Trim()
        corpus_commit = (& git -C crates/valentine_gen/protocolgen rev-parse HEAD | Out-String).Trim()
        affinity_logical_cpu = $Processor
        loopback_affinity_logical_cpus = $LoopbackProcessors
        cinnabar_raknet_commit = '7eb32a31f5715ed6cd69120ef19118aa03fcab28'
        cinnabar_bedrock_net_commit = '0eaf6d4cb38ac1b52328815302c0964645150d17'
        conditions = "Shared Windows workstation; codec/micro inherit affinity to logical CPU $Processor; loopback/interop inherit logical CPUs $($LoopbackProcessors -join ',') (distinct P-cores on measured i9-14900HX), matching $GoCpu workers; other workers may run concurrently; no priority, power-plan or timer-resolution adjustment."
    }
    $meta | ConvertTo-Json | Set-Content (Join-Path $Output 'metadata.json')
    foreach ($round in 1..$Rounds) {
        $order = switch (($round - 1) % 3) {
            0 { @('rust','cinnabar','go') }
            1 { @('go','cinnabar','rust') }
            2 { @('cinnabar','rust','go') }
        }
        foreach ($side in $order) {
            $exe = $Executables[$side]
            foreach ($area in @('codec','micro','loopback')) {
                Write-Host "Round $round/$Rounds $side $area"
                $arguments = @($area)
                if ($area -eq 'loopback') { $arguments += @('--count', "$Count") }
                $job = Start-Runner $exe $arguments $Repo
                Finish-Runner $job (Join-Path $Output "$round-$side-$area") | Out-Null
            }
        }
    }
    $interop = @()
    foreach ($direction in @('go-to-rust','rust-to-go','cinnabar-to-rust','rust-to-cinnabar','cinnabar-to-go','go-to-cinnabar')) {
        $clientSide, $serverSide = $direction -split '-to-'
        $server = $Executables[$serverSide]
        $client = $Executables[$clientSide]
        # Bind an ephemeral UDP socket to obtain an unreserved candidate; bind errors fail explicitly.
        $udp = [Net.Sockets.UdpClient]::new(0)
        $port = $udp.Client.LocalEndPoint.Port
        $udp.Dispose()
        $address = "127.0.0.1:$port"
        $job = Start-Runner $server @('serve','--address',$address,'--count',"$Count") $Repo $true
        try {
            $cj = Start-Runner $client @('client','--address',$address,'--count',"$Count") $Repo
            Finish-Runner $cj (Join-Path $Output "interop-$direction-client") | Out-Null
            Finish-Runner $job (Join-Path $Output "interop-$direction-server") | Out-Null
            $interop += @{ direction = $direction; status = 'PASS'; sizes = @(32,512,4096,65536); count = $Count; split_count = $SplitCount }
        } catch {
            $interop += @{ direction = $direction; status = 'FAIL'; error = "$_"; sizes = @(32,512,4096,65536); count = $Count; split_count = $SplitCount }
        } finally {
            if (!$job.Process.HasExited) { $job.Process.Kill($true); $job.Process.WaitForExit() }
        }
    }
    $interop | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Output 'interop.json')
    & python (Join-Path $PSScriptRoot 'report.py') $Output (Join-Path $PSScriptRoot 'report.md')
    if ($LASTEXITCODE) { throw 'Report generation failed' }
    if (@($interop | Where-Object status -eq 'FAIL').Count) { throw 'Interop failed; report and raw evidence retained' }
} finally {
    Pop-Location
    $ParentProcess.ProcessorAffinity = $OldAffinity
}
