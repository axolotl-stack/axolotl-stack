# Dot-sourced by the runner. Evidence is its unique output directory.
function Save-NetherNet($Job, [string]$Name) {
    $stdout = $Job.Out.GetAwaiter().GetResult()
    $stderr = $Job.Prefix + $Job.Err.GetAwaiter().GetResult()
    [IO.File]::WriteAllText((Join-Path $Evidence ($Name + '.jsonl')), $stdout)
    [IO.File]::WriteAllText((Join-Path $Evidence ($Name + '.stderr.log')), $stderr)
    return $stderr
}
function Stop-NetherNet($Job, [string]$Name) {
    if ($null -eq $Job) { return }
    if (!$Job.Process.HasExited) {
        $Job.Process.Kill($true)
        $Job.Process.WaitForExit()
    }
    Save-NetherNet $Job $Name | Out-Null
}
function Finish-NetherNet($Job, [string]$Name, [int]$Seconds = 300) {
    $timedOut = !$Job.Process.WaitForExit($Seconds * 1000)
    if ($timedOut) {
        $Job.Process.Kill($true)
        $Job.Process.WaitForExit()
    }
    # Persist before throwing: failure is the case where diagnostics matter most.
    $stderr = Save-NetherNet $Job $Name
    if ($timedOut) { throw "NetherNet timeout: $Name`n$stderr" }
    if ($Job.Process.ExitCode -ne 0) { throw "Exit $($Job.Process.ExitCode): $Name`n$stderr" }
}
