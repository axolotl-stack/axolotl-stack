#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateRange(5, 99)][int]$Rounds = 5,
    [ValidateRange(1, 100000)][int]$Count = 100,
    [ValidateRange(1, 100000)][int]$SplitCount = 10,
    [string]$Output = (Join-Path $PSScriptRoot 'results'),
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Cache = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force $Cache | Out-Null
$Evidence = Join-Path $Cache ('nethernet-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $Evidence | Out-Null
Write-Host "Raw NetherNet evidence: $Evidence"
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
$Suffix = if ($IsWindows) { '.exe' } else { '' }
$CinnabarManifest = Join-Path $PSScriptRoot 'cinnabar/Cargo.toml'
$Executables = @{
    rust = Join-Path $Repo "target/release/bench-compare$Suffix"
    cinnabar = Join-Path $PSScriptRoot "cinnabar/target/release/cinnabar-bench$Suffix"
    go = Join-Path $Cache "go-bench$Suffix"
}
$ParentProcess = [Diagnostics.Process]::GetCurrentProcess()
$OldAffinity = $ParentProcess.ProcessorAffinity
$OldEnvironment = @{}
foreach ($key in @('GOMAXPROCS', 'TOKIO_WORKER_THREADS', 'BENCH_SPLIT_COUNT', 'CARGO_BUILD_JOBS')) {
    $OldEnvironment[$key] = [Environment]::GetEnvironmentVariable($key)
}
$env:GOMAXPROCS = '8'
$env:TOKIO_WORKER_THREADS = '8'
$env:BENCH_SPLIT_COUNT = "$SplitCount"
$env:CARGO_BUILD_JOBS = '8'
. (Join-Path $PSScriptRoot 'nethernet-process.ps1')
function Start-NetherNet([string]$Side, [string[]]$Arguments, [bool]$Ready = $false, [string]$Name = '') {
    # Set before Process.Start: child inherits the allowed mask from its first instruction.
    $ParentProcess.ProcessorAffinity = if ($Arguments[0] -eq 'nethernet-micro') { [IntPtr]1 } else { [IntPtr]16711680 }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executables[$Side]
    $info.WorkingDirectory = $Repo
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    if (!$process.Start()) { throw "Cannot start $Side" }
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $prefix = ''
    if ($Ready) {
        $first = $process.StandardError.ReadLineAsync()
        $readyInTime = $first.Wait(30000)
        if (!$readyInTime) {
            if (!$process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        }
        $prefix = $first.GetAwaiter().GetResult() + [Environment]::NewLine
        if (!$readyInTime -or $prefix -notmatch '^READY') {
            $job = @{ Process = $process; Out = $stdout; Err = $process.StandardError.ReadToEndAsync(); Prefix = $prefix }
            Stop-NetherNet $job $Name
            throw "NetherNet server failed readiness: $Side"
        }
    }
    return @{ Process = $process; Out = $stdout; Err = $process.StandardError.ReadToEndAsync(); Prefix = $prefix }
}
Push-Location $Repo
try {
    # Only logical CPUs 24..31, including dependency builds and all toolchain queries.
    $ParentProcess.ProcessorAffinity = [IntPtr]4278190080
    if (!$SkipBuild) {
        & git submodule update --init crates/valentine_gen/protocolgen
        if ($LASTEXITCODE) { throw 'Submodule initialization failed' }
        & cargo build --release -j 8 -p bench-compare
        if ($LASTEXITCODE) { throw 'Rust build failed' }
        & cargo build --release -j 8 --manifest-path $CinnabarManifest --locked
        if ($LASTEXITCODE) { throw 'Cinnabar build failed' }
        Push-Location (Join-Path $PSScriptRoot 'go')
        try {
            & go build -p 8 -o $Executables.go .
            if ($LASTEXITCODE) { throw 'Go build failed' }
        } finally { Pop-Location }
    }
    $metadata = @{
        utc = [DateTime]::UtcNow.ToString('o'); rounds = $Rounds; count = $Count; split_count = $SplitCount
        warmup_echoes = 1; build_affinity_logical_cpus = @(24..31); build_jobs = 8
        affinity_logical_cpu = 0; loopback_affinity_logical_cpus = @(16..23)
        go_cpu = 8; rust_workers = 8
        commit = (& git rev-parse HEAD | Out-String).Trim()
        conditions = 'Shared i9-14900HX Windows workstation. Micro logical CPU 0; all three loopback/interop implementations on identical logical CPUs 16-23 (E-cores acceptable for this WebRTC comparison), eight Tokio workers/GOMAXPROCS. Builds only logical CPUs 24-31, at most eight jobs. No timing on logical CPUs 2,4,6,8,10,12,14. Alternating order; no priority, power-plan or timer-resolution adjustment.'
        versions = @{
            rust = (& rustc --version | Out-String).Trim()
            go_toolchain = (& go version | Out-String).Trim()
            go_nethernet = 'df-mc/go-nethernet 215e46422b5831d6ddc6146003617dc3f1ff7c26'
            cinnabar_nethernet = 'cinnabar-mc/nethernet fed09fc05afb3b0fe34d0e7b18702f7ea86c3290; Apache-2.0'
            ours_stack = 'webrtc-rs 0.12.0; webrtc-dtls 0.11.0; webrtc-sctp 0.11.0; webrtc-data 0.10.0'
            go_stack = 'Pion webrtc v4.2.21-0.20260920133716-91bfc6c2039f; ICE v4.4.4; DTLS v3.1.9; SCTP v1.11.3; datachannel v1.6.3'
            cinnabar_stack = 'rtc / rtc-dtls / rtc-sctp / rtc-datachannel 0.21.1; rtc-turn git ceef0780c54c4f5fdc08e7e343403f8026653171'
            channel_parameters = 'ReliableDataChannel: ordered, reliable, default retransmission policy; second unreliable channel also opens; no DTLS cipher, certificate, retransmission or SCTP congestion/RTO overrides'
            sctp_limits = 'Advertised max-message-size: ours 65535 (segment payload65534); Go/Cinnabar262144 (segment payload262143); outgoing segmentation follows peer limit; Cinnabar framing micro separately uses16383 payload'
            buffers = 'Ours one-slot inbound/outbound handoffs; Go unbuffered completed-packet handoff; Cinnabar requests2MiB UDP receive buffer; other socket/DTLS/SCTP buffers remain stack defaults (OS-effective sizes not queried)'
            identities = 'Go listener requires default self-signed server identity; ours/Cinnabar permit unsigned local answers; Go setup therefore includes identity key issuance/signing, which is not patched out'
            release_profile = 'Both Rust binaries: opt-level=3; lto=thin; codegen-units=16; strip=true'
            signaling = 'Benchmark-local JSON-string TCP signals; server network ID 1, client ID 2; no external network signaling/STUN/TURN'
        }
    }
    $metadata | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Evidence 'metadata.json')
    foreach ($round in 1..$Rounds) {
        $order = switch (($round - 1) % 3) {
            0 { @('rust', 'cinnabar', 'go') }
            1 { @('go', 'cinnabar', 'rust') }
            2 { @('cinnabar', 'rust', 'go') }
        }
        foreach ($side in $order) {
            foreach ($area in @('micro', 'loopback')) {
                Write-Host "NetherNet round $round/$Rounds $side $area"
                $arguments = @("nethernet-$area")
                if ($area -eq 'loopback') { $arguments += @('--count', "$Count") }
                $job = Start-NetherNet $side $arguments
                Finish-NetherNet $job "$round-$side-nethernet-$area"
            }
        }
    }
    $interop = @()
    foreach ($direction in @('rust-to-go', 'go-to-rust', 'rust-to-cinnabar', 'cinnabar-to-rust', 'cinnabar-to-go', 'go-to-cinnabar')) {
        $clientSide, $serverSide = $direction -split '-to-'
        $reservation = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
        $reservation.Start()
        $port = $reservation.LocalEndpoint.Port
        $reservation.Stop()
        $address = "127.0.0.1:$port"
        $server = $null
        $client = $null
        try {
            $server = Start-NetherNet $serverSide @('nethernet-serve', '--address', $address, '--count', "$Count") $true "interop-$direction-server"
            $client = Start-NetherNet $clientSide @('nethernet-client', '--address', $address, '--count', "$Count")
            Finish-NetherNet $client "interop-$direction-client"
            Finish-NetherNet $server "interop-$direction-server"
            $interop += @{ direction = $direction; status = 'PASS'; sizes = @(32, 512, 4096, 65536); count = $Count; split_count = $SplitCount }
        } catch {
            $interop += @{ direction = $direction; status = 'FAIL'; error = "$_"; sizes = @(32, 512, 4096, 65536); count = $Count; split_count = $SplitCount }
        } finally {
            Stop-NetherNet $client "interop-$direction-client"
            Stop-NetherNet $server "interop-$direction-server"
        }
    }
    $interop | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Evidence 'interop.json')
    # Report generation is not timed, and remains on the build mask.
    $ParentProcess.ProcessorAffinity = [IntPtr]4278190080
    & python (Join-Path $PSScriptRoot 'nethernet_report.py') $Evidence (Join-Path $PSScriptRoot 'report.md')
    if ($LASTEXITCODE) { throw 'NetherNet report generation failed' }
    Write-Host "NetherNet report updated; raw evidence: $Evidence"
    if (@($interop | Where-Object status -eq 'FAIL').Count) { throw 'NetherNet interop failures retained in report; no retries or silent fallbacks' }
} finally {
    Pop-Location
    $ParentProcess.ProcessorAffinity = $OldAffinity
    foreach ($key in $OldEnvironment.Keys) { [Environment]::SetEnvironmentVariable($key, $OldEnvironment[$key]) }
}
