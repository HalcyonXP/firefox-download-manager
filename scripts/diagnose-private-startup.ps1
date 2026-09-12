# Post-failure observation only. Never reruns qualification or changes permissions/state.
param([Parameter(Mandatory = $true)][string]$Report)
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
$root = [IO.Path]::GetFullPath([IO.Path]::Combine($PSScriptRoot, '..'))
function Read-Source([string]$name) {
    $path = [IO.Path]::Combine($root, 'crates', 'setup', 'src', $name)
    if ([IO.FileInfo]::new($path).Length -gt 65536) { throw 'source bound exceeded' }
    return [IO.File]::ReadAllText($path, $utf8)
}
$source = Read-Source 'private_file.rs'
$match = [regex]::Match($source, '(?s)const BOOTSTRAP: &str = r#"(.*?)"#;')
if (!$match.Success) { throw 'fixed bootstrap unavailable' }
$bootstrap = $match.Groups[1].Value
# Independent stop before either production body, even if opcode validation changes.
# No valid operation/path is supplied and no file/ACL operation is executed.
$programs = @{
    minimal = @'
$ErrorActionPreference = 'Stop'
$r = [IO.StreamReader]::new([Console]::OpenStandardInput(), [Text.UTF8Encoding]::new($false, $true), $false)
[Console]::Out.Write("start`n")
[Console]::Out.Flush()
$r.Dispose()
'@
}
foreach ($kind in @('file', 'directory')) {
    $body = Read-Source "private_$kind.ps1"
    $programs[$kind] = $bootstrap + "`nexit 1`ntry {`n" + $body + '`n} finally { $dmInput.Dispose() }'.Replace('`n', "`n")
}
$cases = @(
    @{ kind = 'minimal'; input = 'open'; encoding = 'command' },
    @{ kind = 'minimal'; input = 'closed'; encoding = 'command' }
)
foreach ($kind in @('file', 'directory')) {
    $cases += @{ kind = $kind; input = 'after-marker'; encoding = 'command' }
    $cases += @{ kind = $kind; input = 'prefed'; encoding = 'command' }
    $cases += @{ kind = $kind; input = 'after-marker'; encoding = 'encoded' }
}
$reportFile = [IO.File]::Open($Report, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
try {
    $results = @()
    foreach ($case in $cases) {
        $program = $programs[$case.kind]
        $process = [Diagnostics.Process]::new()
        $process.StartInfo.FileName = [IO.Path]::Combine([Environment]::SystemDirectory, 'WindowsPowerShell\v1.0\powershell.exe')
        $arguments = @('-NoProfile', '-NonInteractive')
        if ($case.encoding -eq 'encoded') {
            $arguments += @('-EncodedCommand', [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($program)))
        } else { $arguments += @('-Command', $program) }
        foreach ($arg in $arguments) { $process.StartInfo.ArgumentList.Add($arg) }
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
        $inputBytes = $utf8.GetBytes("startup-probe-refuse`nC:\synthetic-unused`n")
        $inputSent = $false
        $startupMs = $null
        $cpuMs = $null
        $outcome = 'refused'
        try {
            $started = $process.Start()
            if (!$started) { throw 'refused' }
            if ($case.input -eq 'closed') { $process.StandardInput.Close() }
            if ($case.input -eq 'prefed') {
                $process.StandardInput.BaseStream.Write($inputBytes, 0, $inputBytes.Length)
                $process.StandardInput.BaseStream.Flush()
                $inputSent = $true
            }
            $errors = $process.StandardError.BaseStream.ReadAsync($errorBuffer, 0, $errorBuffer.Length)
            while ($count -lt $buffer.Length) {
                # Read only to the marker boundary first, not through a pending input wait.
                $length = if ($count -lt 6) { 6 - $count } else { 1 }
                $pending = $process.StandardOutput.BaseStream.ReadAsync($buffer, $count, $length)
                $remaining = [Math]::Max(0, 5000 - $clock.ElapsedMilliseconds)
                if (!$pending.Wait([int]$remaining)) { throw 'refused' }
                $read = $pending.GetAwaiter().GetResult()
                $pending = $null
                if ($read -eq 0) { break }
                $count += $read
                if ($count -eq 6) {
                    if ([Text.Encoding]::ASCII.GetString($buffer, 0, 6) -cne "start`n") { throw 'refused' }
                    $startupMs = $clock.ElapsedMilliseconds
                    if ($case.input -eq 'after-marker') {
                        $process.StandardInput.BaseStream.Write($inputBytes, 0, $inputBytes.Length)
                        $process.StandardInput.BaseStream.Flush()
                        $inputSent = $true
                    }
                }
            }
            $remaining = [Math]::Max(0, 5000 - $clock.ElapsedMilliseconds)
            if (!$process.WaitForExit([int]$remaining)) { throw 'refused' }
            $expectedExit = if ($case.kind -eq 'minimal') { 0 } else { 1 }
            if ($count -ne 6 -or $null -eq $startupMs -or $process.ExitCode -ne $expectedExit) { throw 'refused' }
            $outcome = 'observed'
        } catch {
            # No raw exceptions, environment values or child output in the report.
            $outcome = 'refused'
        } finally {
            $observedMs = $clock.ElapsedMilliseconds
            $retired = $false
            $joinFailed = $false
            if ($started) {
                try { $cpuMs = $process.TotalProcessorTime.TotalMilliseconds } catch { }
                if (!$process.HasExited) {
                    try { $process.Kill(); $retired = $true } catch { $outcome = 'refused' }
                }
                # Retain/join the exact process and both reads before reporting.
                try { $process.WaitForExit() } catch { $joinFailed = $true; $outcome = 'refused' }
                if ($null -ne $pending) { try { [void]$pending.GetAwaiter().GetResult() } catch { $outcome = 'refused' } }
                if ($null -ne $errors) {
                    try { if ($errors.GetAwaiter().GetResult() -ne 0) { $outcome = 'refused' } } catch { $outcome = 'refused' }
                }
            }
            $process.Dispose()
            if ($joinFailed) { throw 'startup observation cleanup failed' }
        }
        $results += [ordered]@{ kind = $case.kind; input = $case.input; encoding = $case.encoding;
            program_sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($utf8.GetBytes($program))).ToLowerInvariant();
            outcome = $outcome; startup_ms = $startupMs; observation_ms = $observedMs; child_cpu_ms = $cpuMs;
            input_written = $inputSent; cleanup_ms = ($clock.ElapsedMilliseconds - $observedMs); retired_owned_child = $retired; joined = $started }
    }
    $bytes = $utf8.GetBytes((ConvertTo-Json -Depth 4 -InputObject ([ordered]@{ qualification = $false; version = 2; cases = $results })))
    $reportFile.Write($bytes, 0, $bytes.Length)
    $reportFile.Flush($true)
} finally {
    $reportFile.Dispose()
}
