"""Exercise actual failed/timed-out child processes, not source-string assertions."""
from pathlib import Path
import subprocess
import tempfile
import unittest


class NetherNetProcessEvidence(unittest.TestCase):
    def test_failed_client_and_killed_server_both_retain_diagnostics(self):
        helpers = Path(__file__).with_name('nethernet-process.ps1').resolve()
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'check.ps1'
            script.write_text('''param($Helpers, $Evidence)
$ErrorActionPreference = 'Stop'
. $Helpers
function Child($Text, $Code, $Wait) {
    $info = [Diagnostics.ProcessStartInfo]::new('pwsh')
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($a in @('-NoProfile', '-Command', "[Console]::Out.WriteLine('payload'); [Console]::Error.WriteLine('$Text'); Start-Sleep -Seconds $Wait; exit $Code")) { $info.ArgumentList.Add($a) }
    $p = [Diagnostics.Process]::Start($info)
    $first = $p.StandardError.ReadLineAsync()
    if (!$first.Wait(10000)) { throw 'fixture did not start' }
    return @{ Process = $p; Out = $p.StandardOutput.ReadToEndAsync(); Err = $p.StandardError.ReadToEndAsync(); Prefix = $first.GetAwaiter().GetResult() + [Environment]::NewLine }
}
$client = Child 'client-failed' 77 0
$server = Child 'server-originating-error' 0 30
try {
    Finish-NetherNet $client 'client' 10
    throw 'expected client failure'
} catch {
    if ($_ -notmatch 'Exit 77') { throw }
} finally {
    Stop-NetherNet $client 'client'
    Stop-NetherNet $server 'server'
}
$timeout = Child 'timeout-diagnostic' 0 30
try {
    Finish-NetherNet $timeout 'timeout' 1
    throw 'expected timeout'
} catch {
    if ($_ -notmatch 'timeout') { throw }
}
''', encoding='utf-8')
            result = subprocess.run(['pwsh', '-NoProfile', '-File', str(script),
                                     str(helpers), directory], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            for name, diagnostic in [('client', 'client-failed'),
                                     ('server', 'server-originating-error'),
                                     ('timeout', 'timeout-diagnostic')]:
                self.assertIn(diagnostic, (Path(directory) / f'{name}.stderr.log').read_text())
                self.assertIn('payload', (Path(directory) / f'{name}.jsonl').read_text())


if __name__ == '__main__':
    unittest.main()
