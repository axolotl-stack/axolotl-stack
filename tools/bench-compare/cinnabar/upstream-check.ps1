#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateSet('Build', 'Run', 'All')][string]$Stage = 'All',
    [ValidateRange(3, 99)][int]$Rounds = 3,
    [string]$Output = (Join-Path $PSScriptRoot '../results/upstream-check')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Revision = '7eb32a31f5715ed6cd69120ef19118aa03fcab28'
$Output = [IO.Path]::GetFullPath($Output)
$Source = Join-Path $Output 'source'
New-Item -ItemType Directory -Force $Output | Out-Null
$Logs = Join-Path $Output ((Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $Stage)
New-Item -ItemType Directory $Logs | Out-Null
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
$env:GOMAXPROCS = '4'
$env:TOKIO_WORKER_THREADS = '4'
$env:CARGO_TARGET_DIR = Join-Path $Output 'target'
$ParentProcess = [Diagnostics.Process]::GetCurrentProcess()
$OldAffinity = $ParentProcess.ProcessorAffinity
$Commands = [Collections.Generic.List[object]]::new()
function Invoke-Logged([string]$Exe, [string[]]$Arguments, [string]$Cwd, [string]$Name, [int]$TimeoutSeconds = 1200) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Exe
    $info.WorkingDirectory = $Cwd
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    $entry = @{ executable = $Exe; arguments = $Arguments; cwd = $Cwd; utc = [DateTime]::UtcNow.ToString('o'); affinity_mask = $ParentProcess.ProcessorAffinity.ToInt64() }
    $Commands.Add($entry)
    $Commands | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Logs 'commands.json')
    Write-Host "$Name : $Exe $($Arguments -join ' ')"
    $p = [Diagnostics.Process]::new()
    $p.StartInfo = $info
    if (!$p.Start()) { throw "Cannot start $Exe" }
    $stdout = $p.StandardOutput.ReadToEndAsync()
    $stderr = $p.StandardError.ReadToEndAsync()
    $timedOut = !$p.WaitForExit($TimeoutSeconds * 1000)
    if ($timedOut) { $p.Kill($true); $p.WaitForExit() }
    $text = $stdout.GetAwaiter().GetResult()
    [IO.File]::WriteAllText((Join-Path $Logs "$Name.stdout.log"), $text)
    [IO.File]::WriteAllText((Join-Path $Logs "$Name.stderr.log"), $stderr.GetAwaiter().GetResult())
    $entry.exit_code = $p.ExitCode
    $entry.timed_out = $timedOut
    $Commands | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Logs 'commands.json')
    if ($timedOut -or $p.ExitCode -ne 0) { throw "$Name failed; see $Logs" }
    return $text
}
try {
    $ParentProcess.ProcessorAffinity = [IntPtr]4294901760 # build/test CPUs 16..31
    if (!(Test-Path $Source)) {
        Invoke-Logged 'git' @('clone', '--no-checkout', 'https://github.com/cinnabar-mc/raknet', $Source) $Output 'clone' | Out-Null
        Invoke-Logged 'git' @('-C', $Source, 'checkout', '--detach', $Revision) $Output 'checkout' | Out-Null
    }
    $actual = (Invoke-Logged 'git' @('rev-parse', 'HEAD') $Source 'revision').Trim()
    if ($actual -ne $Revision) { throw "Wrong source revision: $actual" }
    # The pinned tree has no .gitmodules; no submodules or cached Cargo sources are modified.
    $status = Invoke-Logged 'git' @('status', '--porcelain', '--untracked-files=no') $Source 'source-status'
    if ($status.Trim()) { throw 'Upstream tracked source is modified' }
    $rustVersion = (Invoke-Logged 'rustc' @('--version') $Source 'rust-version').Trim()
    $goVersion = (Invoke-Logged 'go' @('version') $Source 'go-version').Trim()
    $GoExe = Join-Path $Output 'upstream-go.test.exe'
    $GoDir = Join-Path $Source 'tools/go-raknet'
    # Build on E-cores even for Run, then invoke the resulting binaries directly.
    # Cargo must never start a compiler after the parent switches to timed P-cores.
    $rustBuild = Invoke-Logged 'cargo' @('bench', '-j', '8', '--locked', '-p', 'raknet-tokio', '--bench', 'loopback', '--no-run', '--message-format=json') $Source 'rust-build'
    $artifacts = @($rustBuild -split '\r?\n' | Where-Object { $_.StartsWith('{') } | ForEach-Object { ConvertFrom-Json $_ } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'loopback' -and $_.executable })
    if ($artifacts.Count -ne 1) { throw 'Expected exactly one compiled upstream loopback benchmark executable' }
    $RustExe = $artifacts[0].executable
    Invoke-Logged 'go' @('test', '-p', '8', '-c', '-o', $GoExe, '.') $GoDir 'go-build' | Out-Null
    if ($Stage -in @('Run', 'All')) {
        # Set before spawning so every timed child inherits affinity from startup.
        $ParentProcess.ProcessorAffinity = [IntPtr]85 # logical CPUs 0,2,4,6
        $metadata = @{ source_commit = $actual; rust = $rustVersion; go = $goVersion; utc = [DateTime]::UtcNow.ToString('o'); rounds = $Rounds; logical_cpus = @(0,2,4,6); affinity_mask = 85; GOMAXPROCS = 4; TOKIO_WORKER_THREADS = 4; os = [Runtime.InteropServices.RuntimeInformation]::OSDescription; cpu = $env:PROCESSOR_IDENTIFIER; criterion_sample_size = 10; criterion_warmup_seconds = 1; criterion_measurement_seconds = 10; go_benchtime = '10s'; settings = 'Original upstream defaults and API; no priority, power-plan, or timer-resolution adjustment. Release bench profile; Go default optimized test build.' }
        $metadata | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Logs 'metadata.json')
        foreach ($round in 1..$Rounds) {
            $order = if ($round % 2) { @('rust', 'go') } else { @('go', 'rust') }
            foreach ($side in $order) {
                if ($side -eq 'rust') {
                    Invoke-Logged $RustExe @('loopback/', '--sample-size', '10', '--warm-up-time', '1', '--measurement-time', '10', '--noplot', '--bench') $Source "round-$round-rust" | Out-Null
                    $criterion = Join-Path $env:CARGO_TARGET_DIR 'criterion'
                    Copy-Item -Recurse $criterion (Join-Path $Logs "round-$round-criterion")
                } else {
                    Invoke-Logged $GoExe @('-test.run=^$', '-test.bench=^BenchmarkLoopback$', '-test.benchtime=10s', '-test.count=1', '-test.cpu=4') $GoDir "round-$round-go" | Out-Null
                }
            }
        }
    }
    Write-Host "Evidence: $Logs"
} finally {
    $ParentProcess.ProcessorAffinity = $OldAffinity
}
