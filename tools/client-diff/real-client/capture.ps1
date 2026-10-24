#requires -Version 7.0
<#
.SYNOPSIS
Records the real Minecraft client's RakNet (Mojang, client side) through the
loopback recording relay in front of BDS 1.26.51.

.DESCRIPTION
One command: starts plain BDS (as raknet/capture.ps1 does), starts
`raknet_capture relay` on 127.0.0.1:<RelayPort> with the requested path MTU and
loss, opens `minecraft://connect/?serverUrl=127.0.0.1&serverPort=<RelayPort>`,
lets the session run for -Seconds (play, idle or disconnect from the pause
menu), closes the game, stops everything and prints the analysis. The capture
is `<Output>/<Name>.jsonl`; -Commit copies it to
`crates/tokio-raknet/tests/captures/real-client/<Name>.jsonl`, where
`cargo test -p tokio-raknet --test capture_diff` compares it with
`RaknetStream` (Vanilla).

Run it only when a person is at the machine: the game must be installed and
signed in, and must not be running already (the script closes the game process
it started). -NoLaunch leaves the game to that person: the script only starts
BDS and the relay, prints the address to join and records for -Seconds without
opening or closing the game. -Client gophertunnel replaces the game with the
gophertunnel bot to check the plumbing without the game.

Prerequisites: the BDS runtime in tools/client-diff/endstone-capture/runtime
(see raknet/README.md). The GDK (Microsoft Store/Xbox app) build connects to
127.0.0.1 directly; the retired UWP build needed a loopback exemption
(`CheckNetIsolation LoopbackExempt -a -n=Microsoft.MinecraftUWP_8wekyb3d8bbwe`,
administrator) which this script never applies.
#>
[CmdletBinding()]
param(
    [string]$Name = 'baseline',
    [ValidateRange(400, 1500)][int]$Mtu = 1492,
    [ValidateRange(0.0, 0.5)][double]$Loss = 0,
    [ValidateRange(10, 3600)][int]$Seconds = 60,
    [ValidateSet('minecraft', 'gophertunnel')][string]$Client = 'minecraft',
    [string]$Runtime = (Join-Path $PSScriptRoot '../endstone-capture/runtime'),
    [string]$Output = (Join-Path $PSScriptRoot ('../runs/real-client-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [int]$Port = 39132,
    [int]$RelayPort = 39140,
    [switch]$NoLaunch,
    [switch]$Commit
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../raknet/common.ps1')
$Runtime = [IO.Path]::GetFullPath($Runtime)
$Output = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force $Output | Out-Null
$GameProcess = 'Minecraft.Windows'
if ($Client -eq 'minecraft' -and !$NoLaunch -and @(Get-Process -Name $GameProcess -ErrorAction SilentlyContinue).Count -gt 0) {
    throw 'Minecraft is already running; close it first so this script cannot kill an unrelated session.'
}

$tools = Build-CaptureTools -Bot:($Client -eq 'gophertunnel')
Set-BdsProperties $Runtime $Port
$file = Join-Path $Output "$Name.jsonl"
$clientName = $(if ($Client -eq 'minecraft') { 'minecraft (real client)' } else { 'gophertunnel 80c811b/go-raknet 6124f52' })
# The game joins the IPv4 port advertised in the pong; BDS 1.26.51 sends
# RakNet pongs without an advertisement, and the game never joined through
# the relay with them. The relay therefore advertises itself (protocol 2193,
# 1.26.51). The gophertunnel bot joins the address it dialed either way.
$relayArgs = @('--scenario', "real-client-$Name", '--server', 'bds-1.26.51', '--client', $clientName, '--mtu', "$Mtu", '--advertise-relay', '2193;1.26.51')
if ($Loss -gt 0) { $relayArgs += @('--loss', "$Loss", '--seed', '20261022') }

$server = Start-Bds $Runtime $Port
try {
    $relay = Start-Relay $tools.Capture $RelayPort $Port $file $relayArgs
    try {
        if ($Client -eq 'minecraft' -and $NoLaunch) {
            Write-Host "Relay ready: join 127.0.0.1 port $RelayPort in Minecraft now; recording for $Seconds s."
            Start-Sleep -Seconds $Seconds
        } elseif ($Client -eq 'minecraft') {
            Start-Process ('minecraft://connect/?serverUrl=127.0.0.1&serverPort=' + $RelayPort)
            Write-Host "Minecraft is connecting to 127.0.0.1:$RelayPort; recording for $Seconds s."
            try {
                Start-Sleep -Seconds $Seconds
            } finally {
                Get-Process -Name $GameProcess -ErrorAction SilentlyContinue | Stop-Process -Force
            }
        } else {
            $scenarios = Join-Path $PSScriptRoot '../scenarios.json'
            $bot = Start-Process2 $tools.Bot @('-address', "127.0.0.1:$RelayPort", '-scenario', 'raknet-idle', '-scenarios', $scenarios, '-output', (Join-Path $Output "$Name-bot.jsonl"), '-linger', '7s') (Split-Path $tools.Bot)
            $botOut = $bot.StandardOutput.ReadToEndAsync()
            $botErr = $bot.StandardError.ReadToEndAsync()
            if (!$bot.WaitForExit(($Seconds + 60) * 1000)) { $bot.Kill($true); throw 'bot timed out' }
            [IO.File]::WriteAllText((Join-Path $Output "$Name-bot.log"), $botOut.Result + $botErr.Result)
            if ($bot.ExitCode -ne 0) { throw "bot failed (see $Name-bot.log)" }
        }
        # BDS's side of the disconnect (or its 10 s timeout) after the client is gone.
        Start-Sleep -Seconds 12
    } finally {
        Stop-Relay $relay (Join-Path $Output "$Name-relay.log")
    }
} finally {
    Stop-Bds $server (Join-Path $Output "$Name-bds.log")
}
Write-Host (& $tools.Capture analyze $file | Out-String)

if ($Commit) {
    $committed = Join-Path $CaptureRepo 'crates/tokio-raknet/tests/captures/real-client'
    New-Item -ItemType Directory -Force $committed | Out-Null
    Copy-Item $file (Join-Path $committed "$Name.jsonl") -Force
    Write-Host "Committed capture: $(Join-Path $committed "$Name.jsonl")"
}
Write-Host "Capture: $file"
