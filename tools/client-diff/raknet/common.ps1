# Shared helpers of the RakNet capture drivers (`raknet/capture.ps1`,
# `real-client/capture.ps1`). Dot-source this file.

$CaptureRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))

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

function Test-UdpPort([int]$Port) {
    @((Get-NetUDPEndpoint -LocalPort $Port -ErrorAction SilentlyContinue)).Count -gt 0
}

# Builds the capture tool (and, when asked, the gophertunnel bot) and returns
# their paths.
function Build-CaptureTools([switch]$Bot) {
    Push-Location $CaptureRepo
    try {
        cargo build -p tokio-raknet --example raknet_capture
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
        $tools = @{ Capture = (Join-Path $CaptureRepo 'target/debug/examples/raknet_capture.exe') }
        if ($Bot) {
            if (!(Get-Command go -ErrorAction SilentlyContinue) -and (Test-Path 'C:/Program Files/Go/bin/go.exe')) {
                $env:PATH = 'C:\Program Files\Go\bin;' + $env:PATH
            }
            $tools.Bot = Join-Path $CaptureRepo 'target/gt-client.exe'
            Push-Location (Join-Path $CaptureRepo 'tools/client-diff/gt-client')
            try {
                go build -o $tools.Bot .
                if ($LASTEXITCODE -ne 0) { throw 'go build failed' }
            } finally { Pop-Location }
        }
        return $tools
    } finally { Pop-Location }
}

# Plain bedrock_server.exe (no Endstone hooks) on `Port` (IPv6 `Port + 1`).
# BDS 1.26.51 ignores `server-ip` and binds every interface; only loopback
# traffic is generated.
function Set-BdsProperties([string]$Runtime, [int]$Port) {
    $path = Join-Path $Runtime 'server/server.properties'
    $text = [IO.File]::ReadAllText($path)
    $text = $text -replace '(?m)^server-port=\d+', "server-port=$Port" -replace '(?m)^server-portv6=\d+', "server-portv6=$($Port + 1)"
    $text = $text -replace '(?m)^view-distance=\d+', 'view-distance=10'
    if ($text -notmatch '(?m)^server-ip=') { $text = "server-ip=127.0.0.1`n" + $text }
    [IO.File]::WriteAllText($path, $text)
}

function Start-Bds([string]$Runtime, [int]$Port, [int]$TimeoutSeconds = 120) {
    $bds = Join-Path $Runtime 'server'
    if (!(Test-Path (Join-Path $bds 'bedrock_server.exe'))) { throw "No BDS at $bds (copy the runtime there first)." }
    if (Test-UdpPort $Port) { throw "UDP port $Port is in use; refusing to start a second server." }
    $world = Join-Path $bds 'worlds/client-diff'
    if (Test-Path $world) { Remove-Item -Recurse -Force $world }
    Copy-Item -Recurse (Join-Path $Runtime 'world-template') $world
    $p = Start-Process2 (Join-Path $bds 'bedrock_server.exe') @() $bds
    $log = [Collections.Generic.List[string]]::new()
    Wait-Line $p $p.StandardOutput 'Server started\.' $TimeoutSeconds $log | Out-Null
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

# `raknet_capture relay` on 127.0.0.1:RelayPort in front of 127.0.0.1:Port.
function Start-Relay([string]$Capture, [int]$RelayPort, [int]$Port, [string]$Out, [string[]]$Arguments) {
    $all = @('relay', '--listen', "127.0.0.1:$RelayPort", '--upstream', "127.0.0.1:$Port", '--out', $Out) + $Arguments
    $process = Start-Process2 $Capture $all $CaptureRepo
    $log = [Collections.Generic.List[string]]::new()
    Wait-Line $process $process.StandardError '^READY ' 30 $log | Out-Null
    return @{ Process = $process; Log = $log; Err = $process.StandardError.ReadToEndAsync() }
}

function Stop-Relay($Relay, [string]$LogPath) {
    if (!$Relay.Process.HasExited) {
        $Relay.Process.StandardInput.WriteLine('stop')
        $Relay.Process.StandardInput.Flush()
        if (!$Relay.Process.WaitForExit(10000)) { $Relay.Process.Kill($true) }
    }
    [IO.File]::WriteAllText($LogPath, (($Relay.Log -join "`n") + "`n" + $Relay.Err.Result))
}
