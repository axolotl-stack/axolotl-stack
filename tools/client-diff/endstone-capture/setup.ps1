[CmdletBinding()]
param([string]$Python = 'python', [string]$Runtime = (Join-Path $PSScriptRoot 'runtime'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Runtime = [IO.Path]::GetFullPath($Runtime)
New-Item -ItemType Directory -Force $Runtime | Out-Null
$VenvPython = Join-Path $Runtime 'venv/Scripts/python.exe'
if (!(Test-Path $VenvPython)) {
    & $Python -m venv (Join-Path $Runtime 'venv')
    if ($LASTEXITCODE -ne 0) { throw 'Python venv creation failed (use CPython 3.10+ x64).' }
}
& $VenvPython -m pip install --disable-pip-version-check 'endstone==0.11.12' --editable $PSScriptRoot
if ($LASTEXITCODE -ne 0) { throw 'Endstone/plugin installation failed.' }
& $VenvPython (Join-Path $PSScriptRoot 'provision.py') (Join-Path $Runtime 'server')
if ($LASTEXITCODE -ne 0) { throw 'BDS provisioning failed.' }
Write-Host "Ready: $VenvPython -m endstone --server-folder `"$Runtime/server`" --no-interactive --yes"
