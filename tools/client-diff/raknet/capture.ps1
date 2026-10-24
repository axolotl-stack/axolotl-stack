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
`crates/tokio-raknet/tests/captures/bds-1.26.51/`. See README.md.

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
. (Join-Path $PSScriptRoot 'common.ps1')
$Runtime = [IO.Path]::GetFullPath($Runtime)
# `pwsh -File` passes `-Scenario a,b` as one string.
$Scenario = @($Scenario | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Output = [IO.Path]::GetFullPath($Output)
$Scenarios = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scenarios.json'))
$Committed = Join-Path $CaptureRepo 'crates/tokio-raknet/tests/captures/bds-1.26.51'
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

$tools = Build-CaptureTools -Bot
Set-BdsProperties $Runtime $Port

foreach ($name in $Scenario) {
    Write-Host "== $name"
    $file = Join-Path $Output "$name.jsonl"
    $server = Start-Bds $Runtime $Port $StartupTimeoutSeconds
    try {
        if ($name -eq 'probe') {
            & $tools.Capture probe --server "127.0.0.1:$Port" --out $file --server-name 'bds-1.26.51'
            if ($LASTEXITCODE -ne 0) { throw 'probe failed' }
            continue
        }
        if (!$Table.ContainsKey($name)) { throw "Unknown scenario $name" }
        $spec = $Table[$name]
        $relay = Start-Relay $tools.Capture $RelayPort $Port $file (@('--scenario', $name, '--server', 'bds-1.26.51',
                '--client', 'gophertunnel 80c811b/go-raknet 6124f52') + $spec.Relay)
        try {
            # Linger lets go-raknet finish its close handshake (ACKs, bare 0x15)
            # instead of vanishing; client-silence models a vanished client.
            $linger = $(if ($name -eq 'client-silence') { '0s' } else { '7s' })
            $botProcess = Start-Process2 $tools.Bot @('-address', "127.0.0.1:$RelayPort", '-scenario', $spec.Bot, '-scenarios', $Scenarios, '-output', (Join-Path $Output "$name-bot.jsonl"), '-linger', $linger) (Split-Path $tools.Bot)
            $botOut = $botProcess.StandardOutput.ReadToEndAsync()
            $botErr = $botProcess.StandardError.ReadToEndAsync()
            if (!$botProcess.WaitForExit(120000)) { $botProcess.Kill($true); throw "bot timed out in $name" }
            [IO.File]::WriteAllText((Join-Path $Output "$name-bot.log"), $botOut.Result + $botErr.Result)
            if ($botProcess.ExitCode -ne 0 -and !$spec.MayFail) { throw "bot failed in $name (see $name-bot.log)" }
            # Let BDS finish its side (disconnect, resends, timeout) before stopping the relay.
            Start-Sleep -Seconds ($(if ($name -eq 'client-silence') { 20 } else { 3 }))
        } finally {
            Stop-Relay $relay (Join-Path $Output "$name-relay.log")
        }
    } finally {
        Stop-Bds $server (Join-Path $Output "$name-bds.log")
    }
    Write-Host (& $tools.Capture analyze $file | Out-String)
}

if ($Commit) {
    New-Item -ItemType Directory -Force $Committed | Out-Null
    foreach ($name in $Scenario) { Copy-Item (Join-Path $Output "$name.jsonl") (Join-Path $Committed "$name.jsonl") -Force }
    Write-Host "Committed captures: $Committed"
}
Write-Host "Captures: $Output"
