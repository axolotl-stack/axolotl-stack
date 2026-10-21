# Manual-only launcher. The automated differential runner never invokes this file.
[CmdletBinding()]
param(
    [string]$Address = '127.0.0.1',
    [ValidateRange(1, 65535)][int]$Port = 19132,
    [ValidateRange(1, 3600)][int]$WaitSeconds = 30
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$existing = @(Get-Process -Name 'Minecraft.Windows' -ErrorAction SilentlyContinue)
if ($existing.Count -gt 0) { throw 'Minecraft is already running; close it first so this script cannot kill an unrelated session.' }
$uri = 'minecraft://connect/?serverUrl=' + [Uri]::EscapeDataString($Address) + '&serverPort=' + $Port
Start-Process $uri
try {
    Start-Sleep -Seconds $WaitSeconds
} finally {
    Get-Process -Name 'Minecraft.Windows' -ErrorAction SilentlyContinue | Stop-Process -Force
}
