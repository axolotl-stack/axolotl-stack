#requires -Version 7.0
<#
.SYNOPSIS
Records Mojang RakNet (BDS) wire behaviour through a loopback recording relay.

.DESCRIPTION
For each scenario: restores the world, starts bedrock_server.exe (plain BDS, no
Endstone hooks), starts `raknet_capture relay` on 127.0.0.1:<RelayPort> in front
of BDS with the scenario's faults, drives the gophertunnel/go-raknet bot
(`gt-client`) through the relay, then stops everything. Captures land in
`<Output>/<scenario>.jsonl`; `-Commit` also copies them to
`crates/tokio-raknet/tests/captures/bds-1.26.51/`.

Never binds beyond loopback itself. Note: BDS 1.26.51 ignores `server-ip` and
binds 0.0.0.0/:: on its ports; only loopback traffic is generated.
#>
[CmdletBinding()]
param(
    [string]$Runtime = (Join-Path $PSScriptRoot '../endstone-capture/runtime'),
    [string]$Output = (Join-Path $PSScriptRoot ('../runs/raknet-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [int]$Port = 39132,
    [int]$RelayPort = 39140,
    [string[]]$Scenario = @('probe', 'baseline', 'mtu-1400', 'mtu-1200', 'mtu-576', 'loss-5', 'reorder-5', 'kick', 'client-silence'),
    [int]$StartupTimeoutSeconds = 120,
    [switch]$Commit
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$Runtime = [IO.Path]::GetFullPath($Runtime)
# `pwsh -File` passes `-Scenario a,b` as one string.
$Scenario = @($Scenario | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Output = [IO.Path]::GetFullPath($Output)
$Bds = Join-Path $Runtime 'server'
$Scenarios = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scenarios.json'))
$Committed = Join-Path $Repo 'crates/tokio-raknet/tests/captures/bds-1.26.51'
if (!(Test-Path (Join-Path $Bds 'bedrock_server.exe'))) { throw "No BDS at $Bds (copy the runtime there first)." }
if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
    $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
}
New-Item -ItemType Directory -Force $Output | Out-Null

# Relay arguments, bot scenario and whether the bot may fail (kicked/silenced).
$Table = @{
    'baseline'       = @{ Relay = @('--mtu', '1492'); Bot = 'raknet-idle'; MayFail = $false }
    'mtu-1400'       = @{ Relay = @('--mtu', '1400'); Bot = 'join-idle'; MayFail = $false }
    'mtu-1200'       = @{ Relay = @('--mtu', '1200'); Bot = 'join-idle'; MayFail = $false }
    'mtu-576'        = @{ Relay = @('--mtu', '576'); Bot = 'join-idle'; MayFail = $false }
    'loss-5'         = @{ Relay = @('--mtu', '1492', '--loss', '0.05', '--seed', '20261022'); Bot = 'join-idle'; MayFail = $false }
    'reorder-5'      = @{ Relay = @('--mtu', '1492', '--reorder', '0.05', '--seed', '20261022'); Bot = 'join-idle'; MayFail = $false }
    'kick'           = @{ Relay = @('--mtu', '1492'); Bot = 'raknet-kick'; MayFail = $true }
    'client-silence' = @{ Relay = @('--mtu', '1492', '--blackhole-c2s-after-ms', '8000'); Bot = 'raknet-idle'; MayFail = $true }
}

function Set-Properties {
    $path = Join-Path $Bds 'server.properties'
    $text = [IO.File]::ReadAllText($path)
    $text = $text -replace '(?m)^server-port=\d+', "server-port=$Port" -replace '(?m)^server-portv6=\d+', "server-portv6=$($Port + 1)"
    $text = $text -replace '(?m)^view-distance=\d+', 'view-distance=10'
    if ($text -notmatch '(?m)^server-ip=') { $text = "server-ip=127.0.0.1`n" + $text }
    [IO.File]::WriteAllText($path, $text)
}
function Test-Port([int]$P) { @((Get-NetUDPEndpoint -LocalPort $P -ErrorAction SilentlyContinue)).Count -gt 0 }
function Start-Process2([string]$Exe, [string[]]$Arguments, [string]$Cwd) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Exe
    $info.WorkingDirectory = $Cwd
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($a in $Arguments) { $info.ArgumentList.Add($a) }
    $p = [Diagnostics.Process]::Start($info)
    if ($null -eq $p) { throw "Cannot start $Exe" }
    return $p
}
function Wait-Line($Process, [System.IO.StreamReader]$Reader, [string]$Pattern, [int]$Seconds, [Collections.Generic.List[string]]$Log) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ($true) {
        $task = $Reader.ReadLineAsync()
        while (!$task.Wait(200)) {
            if ([DateTime]::UtcNow -gt $deadline) { throw "Timed out waiting for '$Pattern'" }
        }
        $line = $task.Result
        if ($null -eq $line) { throw "Process exited before '$Pattern'" }
        $Log.Add($line)
        if ($line -match $Pattern) { return $line }
    }
}
function Start-Bds {
    if (Test-Port $Port) { throw "UDP port $Port is in use; refusing to start a second server." }
    $world = Join-Path $Bds 'worlds/client-diff'
    if (Test-Path $world) { Remove-Item -Recurse -Force $world }
    Copy-Item -Recurse (Join-Path $Runtime 'world-template') $world
    $p = Start-Process2 (Join-Path $Bds 'bedrock_server.exe') @() $Bds
    $log = [Collections.Generic.List[string]]::new()
    Wait-Line $p $p.StandardOutput 'Server started\.' $StartupTimeoutSeconds $log | Out-Null
    return @{ Process = $p; Log = $log; Rest = $p.StandardOutput.ReadToEndAsync(); Err = $p.StandardError.ReadToEndAsync() }
}
function Stop-Bds($Server, [string]$LogPath) {
    if (!$Server.Process.HasExited) {
        $Server.Process.StandardInput.WriteLine('stop')
        $Server.Process.StandardInput.Flush()
        if (!$Server.Process.WaitForExit(30000)) { $Server.Process.Kill($true) }
    }
    $Server.Process.WaitForExit()
    [IO.File]::WriteAllText($LogPath, (($Server.Log -join "`n") + "`n" + $Server.Rest.GetAwaiter().GetResult() + $Server.Err.GetAwaiter().GetResult()))
}

Push-Location $Repo
try {
    cargo build -p tokio-raknet --example raknet_capture
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    $capture = Join-Path $Repo 'target/debug/examples/raknet_capture.exe'
    $bot = Join-Path $Repo 'target/gt-client.exe'
    Push-Location (Join-Path $PSScriptRoot '../gt-client')
    go build -o $bot .
    if ($LASTEXITCODE -ne 0) { throw 'go build failed' }
    Pop-Location
} finally { Pop-Location }
Set-Properties

foreach ($name in $Scenario) {
    Write-Host "== $name"
    $file = Join-Path $Output "$name.jsonl"
    $server = Start-Bds
    try {
        if ($name -eq 'probe') {
            & $capture probe --server "127.0.0.1:$Port" --out $file --server-name 'bds-1.26.51'
            if ($LASTEXITCODE -ne 0) { throw 'probe failed' }
            continue
        }
        if (!$Table.ContainsKey($name)) { throw "Unknown scenario $name" }
        $spec = $Table[$name]
        $relayArgs = @('relay', '--listen', "127.0.0.1:$RelayPort", '--upstream', "127.0.0.1:$Port", '--out', $file,
            '--scenario', $name, '--server', 'bds-1.26.51', '--client', 'gophertunnel 80c811b/go-raknet 87a6b7e') + $spec.Relay
        $relay = Start-Process2 $capture $relayArgs $Repo
        $relayLog = [Collections.Generic.List[string]]::new()
        Wait-Line $relay $relay.StandardError '^READY ' 30 $relayLog | Out-Null
        $relayErr = $relay.StandardError.ReadToEndAsync()
        try {
            # Linger lets go-raknet finish its close handshake (ACKs, bare 0x15)
            # instead of vanishing; client-silence models a vanished client.
            $linger = $(if ($name -eq 'client-silence') { '0s' } else { '7s' })
            $botProcess = Start-Process2 $bot @('-address', "127.0.0.1:$RelayPort", '-scenario', $spec.Bot, '-scenarios', $Scenarios, '-output', (Join-Path $Output "$name-bot.jsonl"), '-linger', $linger) (Split-Path $bot)
            $botOut = $botProcess.StandardOutput.ReadToEndAsync()
            $botErr = $botProcess.StandardError.ReadToEndAsync()
            if (!$botProcess.WaitForExit(120000)) { $botProcess.Kill($true); throw "bot timed out in $name" }
            [IO.File]::WriteAllText((Join-Path $Output "$name-bot.log"), $botOut.Result + $botErr.Result)
            if ($botProcess.ExitCode -ne 0 -and !$spec.MayFail) { throw "bot failed in $name (see $name-bot.log)" }
            # Let BDS finish its side (disconnect, resends, timeout) before stopping the relay.
            Start-Sleep -Seconds ($(if ($name -eq 'client-silence') { 20 } else { 3 }))
        } finally {
            if (!$relay.HasExited) {
                $relay.StandardInput.WriteLine('stop')
                $relay.StandardInput.Flush()
                if (!$relay.WaitForExit(10000)) { $relay.Kill($true) }
            }
            [IO.File]::WriteAllText((Join-Path $Output "$name-relay.log"), (($relayLog -join "`n") + "`n" + $relayErr.Result))
        }
    } finally {
        Stop-Bds $server (Join-Path $Output "$name-bds.log")
    }
    Write-Host (& $capture analyze $file | Out-String)
}

if ($Commit) {
    New-Item -ItemType Directory -Force $Committed | Out-Null
    foreach ($name in $Scenario) { Copy-Item (Join-Path $Output "$name.jsonl") (Join-Path $Committed "$name.jsonl") -Force }
    Write-Host "Committed captures: $Committed"
}
Write-Host "Captures: $Output"
