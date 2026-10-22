#requires -Version 7.0
[CmdletBinding()]
param(
    [ValidateSet('vanilla', 'unastar')][string]$Server = 'vanilla',
    # diff: compare gophertunnel and Jolyne traffic (join-idle, join-chat).
    # conformance: capture every scenario and check Valentine decodes and
    # re-encodes every packet exactly (see README).
    [ValidateSet('diff', 'conformance')][string]$Mode = 'diff',
    [string]$Runtime = (Join-Path $PSScriptRoot 'endstone-capture/runtime'),
    [string]$Output = (Join-Path $PSScriptRoot ('runs/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))),
    [string]$UnastarCommand = '',
    # IPv4 game port of the vanilla server (IPv6 uses Port+1); written into the
    # harness-owned runtime server.properties.
    [int]$Port = 19132,
    [int]$StartupTimeoutSeconds = 180
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Runtime = [IO.Path]::GetFullPath($Runtime)
$Output = [IO.Path]::GetFullPath($Output)
$Python = Join-Path $Runtime 'venv/Scripts/python.exe'
$Bds = Join-Path $Runtime 'server'
$Scenarios = Join-Path $PSScriptRoot 'scenarios.json'
$Masks = Join-Path $PSScriptRoot 'masks.toml'
if (!(Test-Path $Python)) { throw 'Run endstone-capture/setup.ps1 first.' }
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
New-Item -ItemType Directory -Force $Output | Out-Null

function Start-Managed([string]$Exe, [string[]]$Arguments, [string]$WorkingDirectory, [string]$Log) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Exe
    $info.WorkingDirectory = $WorkingDirectory
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($arg in $Arguments) { $info.ArgumentList.Add($arg) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    if (!$process.Start()) { throw "Cannot start $Exe" }
    return @{ Process = $process; Stdout = $process.StandardOutput.ReadToEndAsync();
              Stderr = $process.StandardError.ReadToEndAsync(); Log = $Log }
}
function Save-Logs($Job) {
    [IO.File]::WriteAllText($Job.Log + '.stdout.log', $Job.Stdout.GetAwaiter().GetResult())
    [IO.File]::WriteAllText($Job.Log + '.stderr.log', $Job.Stderr.GetAwaiter().GetResult())
}
function Stop-Managed($Job, [bool]$Graceful) {
    if (!$Job.Process.HasExited -and $Graceful) {
        $Job.Process.StandardInput.WriteLine('stop')
        $Job.Process.StandardInput.Flush()
        if (!$Job.Process.WaitForExit(30000)) { $Job.Process.Kill($true) }
    } elseif (!$Job.Process.HasExited) { $Job.Process.Kill($true) }
    $Job.Process.WaitForExit()
    Save-Logs $Job
}
function Invoke-Checked([string]$Exe, [string[]]$Arguments, [string]$WorkingDirectory, [string]$Log, [int]$Timeout = 300) {
    $job = Start-Managed $Exe $Arguments $WorkingDirectory $Log
    if (!$job.Process.WaitForExit($Timeout * 1000)) {
        Stop-Managed $job $false
        throw "Command timed out: $Exe (see $Log.*.log)"
    }
    Save-Logs $job
    if ($job.Process.ExitCode -ne 0) { throw "Command failed ($($job.Process.ExitCode)): $Exe (see $Log.*.log)" }
}
function Test-LocalPort {
    return @((Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue)).Count -gt 0
}
function Set-ServerPort {
    $properties = Join-Path $Bds 'server.properties'
    $text = [IO.File]::ReadAllText($properties)
    $text = $text -replace '(?m)^server-port=\d+', "server-port=$Port" -replace '(?m)^server-portv6=\d+', "server-portv6=$($Port + 1)"
    [IO.File]::WriteAllText($properties, $text)
}
function Initialize-WorldTemplate {
    $template = Join-Path $Runtime 'world-template'
    if (Test-Path $template) { return }
    if (Test-LocalPort) { throw 'UDP port 19132 is already occupied; cannot initialize the world template.' }
    $world = Join-Path $Bds 'worlds/client-diff'
    if (Test-Path $world) { Remove-Item -Recurse -Force $world }
    $captureDir = Join-Path $Output 'template-server'
    New-Item -ItemType Directory $captureDir | Out-Null
    $oldCapture = $env:CLIENT_DIFF_CAPTURE_DIR
    $env:CLIENT_DIFF_CAPTURE_DIR = $captureDir
    $job = $null
    try {
        $job = Start-Managed $Python @('-m', 'endstone', '--server-folder', $Bds, '--no-interactive', '--yes') $Bds (Join-Path $Output 'template')
        $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
        while (!(Test-Path (Join-Path $captureDir 'ready.json'))) {
            if ($job.Process.HasExited) { throw 'BDS exited while initializing world template.' }
            if (Test-Path (Join-Path $captureDir 'error.txt')) { throw (Get-Content -Raw (Join-Path $captureDir 'error.txt')) }
            if ([DateTime]::UtcNow -gt $deadline) { throw 'World template initialization timed out.' }
            Start-Sleep -Milliseconds 200
        }
    } finally {
        if ($null -ne $job) { Stop-Managed $job $true }
        $env:CLIENT_DIFF_CAPTURE_DIR = $oldCapture
    }
    if (!(Test-Path $world)) { throw 'BDS did not save the initialized world.' }
    # Copy only after graceful shutdown: never snapshot an open LevelDB database.
    Copy-Item -Recurse $world $template
}
function Invoke-Capture([string]$Backend, [string]$Client, [string]$Scenario) {
    if (Test-LocalPort) { throw "UDP port $Port is already occupied; refusing to touch an unrelated server." }
    $name = "$Scenario-$Backend-$Client"
    $captureDir = Join-Path $Output "$name-server"
    New-Item -ItemType Directory $captureDir | Out-Null
    $oldCapture = $env:CLIENT_DIFF_CAPTURE_DIR
    $env:CLIENT_DIFF_CAPTURE_DIR = $captureDir
    $job = $null
    try {
        if ($Backend -eq 'vanilla') {
            # Only this harness-owned, fixed-name world is reset.
            $world = Join-Path $Bds 'worlds/client-diff'
            if (Test-Path $world) { Remove-Item -Recurse -Force $world }
            Copy-Item -Recurse (Join-Path $Runtime 'world-template') $world
            $job = Start-Managed $Python @('-m', 'endstone', '--server-folder', $Bds, '--no-interactive', '--yes') $Bds (Join-Path $Output $name)
        } else {
            # Isolated cwd keeps generated world/player data away from the repo.
            $cwd = Join-Path $captureDir 'unastar'
            New-Item -ItemType Directory $cwd | Out-Null
            Copy-Item (Join-Path $Repo 'unastar.toml') (Join-Path $cwd 'unastar.toml')
            if (!$env:VCPKG_ROOT -and (Test-Path 'C:/Users/jvigu/vcpkg')) { $env:VCPKG_ROOT = 'C:/Users/jvigu/vcpkg' }
            if ($UnastarCommand) {
                $job = Start-Managed 'pwsh' @('-NoProfile', '-Command', $UnastarCommand) $cwd (Join-Path $Output $name)
            } else {
                $job = Start-Managed 'cargo' @('run', '--manifest-path', (Join-Path $Repo 'Cargo.toml'), '-p', 'unastar') $cwd (Join-Path $Output $name)
            }
        }
        $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
        while ($true) {
            if ($job.Process.HasExited) { throw "$Backend exited before readiness" }
            if (Test-Path (Join-Path $captureDir 'error.txt')) { throw (Get-Content -Raw (Join-Path $captureDir 'error.txt')) }
            if ($Backend -eq 'vanilla') {
                if (Test-Path (Join-Path $captureDir 'ready.json')) { break }
            } elseif (Test-LocalPort) { break }
            if ([DateTime]::UtcNow -gt $deadline) { throw "$Backend startup timed out" }
            Start-Sleep -Milliseconds 200
        }
        $path = Join-Path $Output "$name.jsonl"
        try {
            if ($Client -eq 'go') {
                Invoke-Checked 'go' @('run', '.', '-address', "127.0.0.1:$Port", '-scenario', $Scenario, '-scenarios', $Scenarios, '-output', $path) (Join-Path $PSScriptRoot 'gt-client') (Join-Path $Output "$name-client")
            } else {
                Invoke-Checked 'cargo' @('run', '-p', 'client-diff', '--', 'jolyne', '--address', "127.0.0.1:$Port", '--scenario', $Scenario, '--scenarios', $Scenarios, '--output', $path) $Repo (Join-Path $Output "$name-client")
            }
        } catch {
            # Preserve and compare rejection traffic too, but never mark a failed join successful.
            Write-Warning $_
            $script:clientFailed = $true
        }
        if (Test-Path (Join-Path $captureDir 'error.txt')) { throw (Get-Content -Raw (Join-Path $captureDir 'error.txt')) }
        if (!(Test-Path $path) -or (Get-Item $path).Length -eq 0) { throw "Missing/empty client capture: $path" }
        if ($Backend -eq 'vanilla' -and !(Test-Path (Join-Path $captureDir 'player-000.jsonl'))) { throw 'Endstone did not observe client packets.' }
        return $path
    } finally {
        if ($null -ne $job) { Stop-Managed $job ($Backend -eq 'vanilla') }
        $env:CLIENT_DIFF_CAPTURE_DIR = $oldCapture
    }
}

$diffFailed = $false
$clientFailed = $false
if ($Server -eq 'unastar' -and $Port -ne 19132) { throw 'unastar listens on the port configured in unastar.toml (19132).' }
Set-ServerPort
Initialize-WorldTemplate
if ($Mode -eq 'conformance') {
    # Interaction scenarios are gophertunnel-only (Jolyne rejects their steps).
    foreach ($scenario in @('join-idle', 'join-chat', 'tour-world', 'tour-interact')) {
        Invoke-Capture 'vanilla' 'go' $scenario | Out-Null
        if ($scenario -ne 'tour-interact') { Invoke-Capture 'vanilla' 'jolyne' $scenario | Out-Null }
    }
    # Every client and Endstone (server-side) capture under $Output.
    $conformanceFailed = $false
    try {
        # .ndjson so reruns over $Output do not re-check the failure dump.
        Invoke-Checked 'cargo' @('run', '-p', 'client-diff', '--', 'conformance', $Output, '--output', (Join-Path $Output 'conformance.md'), '--failures', (Join-Path $Output 'conformance-failures.ndjson')) $Repo (Join-Path $Output 'conformance')
    } catch { Write-Warning $_; $conformanceFailed = $true }
    Write-Host "Captures, conformance.md and command output: $Output"
    if ($conformanceFailed) { throw 'Valentine conformance failed; see conformance.md and conformance-failures.ndjson.' }
    if ($clientFailed) { throw 'One or more client scenarios failed; partial captures were retained for diagnosis.' }
    return
}
foreach ($scenario in @('join-idle', 'join-chat')) {
    $go = Invoke-Capture 'vanilla' 'go' $scenario
    $rust = Invoke-Capture 'vanilla' 'jolyne' $scenario
    foreach ($direction in @('c2s', 's2c')) {
        try {
            Invoke-Checked 'cargo' @('run', '-p', 'client-diff', '--', 'diff', $go, $rust, '--direction', $direction, '--masks', $Masks, '--output', (Join-Path $Output "$scenario-$direction.md")) $Repo (Join-Path $Output "$scenario-$direction-diff")
        } catch { Write-Warning $_; $diffFailed = $true }
    }
    if ($Server -eq 'unastar') {
        $candidate = Invoke-Capture 'unastar' 'go' $scenario
        try {
            Invoke-Checked 'cargo' @('run', '-p', 'client-diff', '--', 'diff', $go, $candidate, '--direction', 's2c', '--masks', $Masks, '--output', (Join-Path $Output "$scenario-unastar-s2c.md")) $Repo (Join-Path $Output "$scenario-unastar-s2c-diff")
        } catch { Write-Warning $_; $diffFailed = $true }
    }
}
Write-Host "Captures and command output: $Output"
if ($diffFailed) { throw 'One or more differential comparisons failed; inspect the diff logs.' }
if ($clientFailed) { throw 'One or more client scenarios failed; partial captures/reports were retained for diagnosis.' }
