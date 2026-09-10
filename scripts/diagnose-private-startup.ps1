# Post-failure observation only. Does not rerun tests or modify permissions/state.
param([Parameter(Mandatory = $true)][string]$Report)
$ErrorActionPreference = 'Stop'
$reportFile = [IO.File]::Open($Report, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
try {
    $results = @()
    foreach ($closeInput in @($false, $true)) {
        $process = [Diagnostics.Process]::new()
        $process.StartInfo.FileName = [IO.Path]::Combine([Environment]::SystemDirectory, 'WindowsPowerShell\v1.0\powershell.exe')
        foreach ($arg in @('-NoProfile', '-NonInteractive', '-Command', @'
$ErrorActionPreference = 'Stop'
$r = [IO.StreamReader]::new([Console]::OpenStandardInput(), [Text.UTF8Encoding]::new($false, $true), $false)
[Console]::Out.Write("start`n")
[Console]::Out.Flush()
$r.Dispose()
'@)) { $process.StartInfo.ArgumentList.Add($arg) }
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.CreateNoWindow = $true
        $process.StartInfo.RedirectStandardInput = $true
        $process.StartInfo.RedirectStandardOutput = $true
        $process.StartInfo.RedirectStandardError = $true
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $started = $false
        $pending = $null
        $errors = $null
        $count = 0
        $buffer = [byte[]]::new(7)
        $errorBuffer = [byte[]]::new(512)
        $outcome = 'refused'
        try {
            $started = $process.Start()
            if (!$started) { throw 'refused' }
            if ($closeInput) { $process.StandardInput.Close() }
            $errors = $process.StandardError.BaseStream.ReadAsync($errorBuffer, 0, $errorBuffer.Length)
            while ($count -lt $buffer.Length) {
                $pending = $process.StandardOutput.BaseStream.ReadAsync($buffer, $count, $buffer.Length - $count)
                $remaining = [Math]::Max(0, 5000 - $clock.ElapsedMilliseconds)
                if (!$pending.Wait([int]$remaining)) { throw 'refused' }
                $read = $pending.GetAwaiter().GetResult()
                $pending = $null
                if ($read -eq 0) { break }
                $count += $read
            }
            $remaining = [Math]::Max(0, 5000 - $clock.ElapsedMilliseconds)
            if (!$process.WaitForExit([int]$remaining)) { throw 'refused' }
            if ($count -ne 6 -or [Text.Encoding]::ASCII.GetString($buffer, 0, $count) -cne "start`n" -or $process.ExitCode -ne 0) { throw 'refused' }
            $outcome = 'observed'
        } catch {
            # Never echo exceptions, environment values or child output.
            $outcome = 'refused'
        } finally {
            $observedMs = $clock.ElapsedMilliseconds
            $retired = $false
            $joinFailed = $false
            if ($started) {
                if (!$process.HasExited) {
                    try { $process.Kill(); $retired = $true } catch { $outcome = 'refused' }
                }
                # Join the exact process and retained reads before reporting.
                try { $process.WaitForExit() } catch { $joinFailed = $true; $outcome = 'refused' }
                if ($null -ne $pending) { try { [void]$pending.GetAwaiter().GetResult() } catch { $outcome = 'refused' } }
                if ($null -ne $errors) {
                    try { if ($errors.GetAwaiter().GetResult() -ne 0) { $outcome = 'refused' } } catch { $outcome = 'refused' }
                }
            }
            $process.Dispose()
            if ($joinFailed) { throw 'startup observation cleanup failed' }
        }
        $results += [ordered]@{ stdin_closed = $closeInput; outcome = $outcome; observation_ms = $observedMs; cleanup_ms = ($clock.ElapsedMilliseconds - $observedMs); retired_owned_child = $retired; joined = $started }
    }
    $bytes = [Text.UTF8Encoding]::new($false, $true).GetBytes((ConvertTo-Json -Depth 4 -InputObject ([ordered]@{ qualification = $false; cases = $results })))
    $reportFile.Write($bytes, 0, $bytes.Length)
    $reportFile.Flush($true)
} finally {
    $reportFile.Dispose()
}
