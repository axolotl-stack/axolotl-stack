#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateRange(3, 99)][int]$Rounds = 3,
    [ValidateRange(1, 100000)][int]$Count = 100,
    [ValidateRange(1, 256)][int]$GoCpu = 4,
    [string]$Output = (Join-Path $PSScriptRoot 'results'),
    [switch]$SkipBuild,
    [switch]$Contract
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
$Suffix = if ($IsWindows) { '.exe' } else { '' }
$Rust = Join-Path $Repo "target/release/bench-compare$Suffix"
$Go = Join-Path $Cache "go-bench$Suffix"
function Start-Runner([string]$Exe, [string[]]$Arguments, [string]$Cwd, [bool]$Ready = $false) {
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
    if (!$SkipBuild) {
        & git submodule update --init crates/valentine_gen/protocolgen
        if ($LASTEXITCODE) { throw 'Submodule initialization failed' }
        & cargo build --release -p bench-compare
        if ($LASTEXITCODE) { throw 'Rust build failed' }
        Push-Location (Join-Path $PSScriptRoot 'go')
        try { & go build -o $Go .; if ($LASTEXITCODE) { throw 'Go build failed' } } finally { Pop-Location }
    }
    $meta = @{
        utc = [DateTime]::UtcNow.ToString('o'); rounds = $Rounds; count = $Count; go_cpu = $GoCpu
        warmup_echoes = 1
        os = [Runtime.InteropServices.RuntimeInformation]::OSDescription
        architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        processors = [Environment]::ProcessorCount
        cpu = $env:PROCESSOR_IDENTIFIER
        rust_workers = $GoCpu
        rust = (& rustc --version | Out-String).Trim(); go = (& go version | Out-String).Trim()
        commit = (& git rev-parse HEAD | Out-String).Trim()
        corpus_commit = (& git -C crates/valentine_gen/protocolgen rev-parse HEAD | Out-String).Trim()
        conditions = 'Shared Windows workstation; other workers may build or benchmark concurrently; no affinity, priority, power-plan or timer-resolution adjustment.'
    }
    $meta | ConvertTo-Json | Set-Content (Join-Path $Output 'metadata.json')
    foreach ($round in 1..$Rounds) {
        $order = if ($round % 2) { @('rust','go') } else { @('go','rust') }
        foreach ($side in $order) {
            $exe = if ($side -eq 'rust') { $Rust } else { $Go }
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
    foreach ($direction in @('go-to-rust','rust-to-go')) {
        $server = if ($direction -eq 'go-to-rust') { $Rust } else { $Go }
        $client = if ($direction -eq 'go-to-rust') { $Go } else { $Rust }
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
            $interop += @{ direction = $direction; status = 'PASS'; sizes = @(32,512,4096,65536); count = $Count }
        } catch {
            $interop += @{ direction = $direction; status = 'FAIL'; error = "$_"; sizes = @(32,512,4096,65536); count = $Count }
        } finally {
            if (!$job.Process.HasExited) { $job.Process.Kill($true); $job.Process.WaitForExit() }
        }
    }
    $interop | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Output 'interop.json')
    if ($Contract) {
        $env:CRITERION_HOME = Join-Path $Output 'contract-criterion'
        & cargo bench --manifest-path crates/valentine_gen/protocolgen/differential/retail/bench/Cargo.toml --target-dir (Join-Path $Repo 'target/contract-bench') --bench codec -- --warm-up-time 0.1 --measurement-time 0.2 --sample-size 10
        if ($LASTEXITCODE) { throw 'Protocolgen contract benchmarks failed' }
    }
    & python (Join-Path $PSScriptRoot 'report.py') $Output (Join-Path $PSScriptRoot 'report.md')
    if ($LASTEXITCODE) { throw 'Report generation failed' }
    if (@($interop | Where-Object status -eq 'FAIL').Count) { throw 'Interop failed; report and raw evidence retained' }
} finally { Pop-Location }
