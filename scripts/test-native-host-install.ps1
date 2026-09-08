[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallRoot,
    [string]$TestProfileRoot = (Join-Path $env:TEMP "download-manager-native-host-test-profile")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$HostName = "com.halcyonxp.firefox_download_manager"
$ExtensionId = "download-manager@halcyonxp.local"
$RegistryKey = "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"
$MaximumMessageBytes = 1MB

function Assert-Condition {
    param(
        [Parameter(Mandatory = $true)]
        [bool]$Condition,
        [Parameter(Mandatory = $true)]
        [string]$Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Read-NativeMessages {
    param([byte[]]$Bytes)
    $Offset = 0
    $Messages = @()
    $StrictUtf8 = New-Object System.Text.UTF8Encoding($false, $true)
    while ($Offset -lt $Bytes.Length) {
        Assert-Condition ($Bytes.Length - $Offset -ge 4) "Native output has a truncated prefix."
        $Length = [System.BitConverter]::ToUInt32($Bytes, $Offset)
        $Offset += 4
        Assert-Condition ($Length -le $MaximumMessageBytes) "Native output exceeds the message limit."
        Assert-Condition ($Bytes.Length - $Offset -ge $Length) "Native output has a truncated body."
        $Json = $StrictUtf8.GetString($Bytes, $Offset, [int]$Length)
        $Offset += [int]$Length
        $Messages += ,($Json | ConvertFrom-Json)
    }
    return $Messages
}

$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$ExpectedManifestPath = Join-Path $InstallRoot "$HostName.json"
Assert-Condition (Test-Path -LiteralPath $RegistryKey) "The current-user native host key is missing."
$RegistryItem = Get-Item -LiteralPath $RegistryKey
Assert-Condition ($RegistryItem.GetValueKind("") -eq [Microsoft.Win32.RegistryValueKind]::String) "The registration is not REG_SZ."
$RegisteredManifestPath = [string]$RegistryItem.GetValue("")
Assert-Condition ([string]::Equals(
        [System.IO.Path]::GetFullPath($RegisteredManifestPath),
        $ExpectedManifestPath,
        [System.StringComparison]::OrdinalIgnoreCase
    )) "The registry does not point to the expected manifest."
Assert-Condition (Test-Path -LiteralPath $RegisteredManifestPath -PathType Leaf) "The registered manifest is missing."

$Manifest = Get-Content -LiteralPath $RegisteredManifestPath -Raw | ConvertFrom-Json
Assert-Condition ($Manifest.name -ceq $HostName) "The manifest host name is incorrect."
Assert-Condition ($Manifest.type -ceq "stdio") "The manifest transport is not stdio."
Assert-Condition ($Manifest.allowed_extensions.Count -eq 1) "The manifest allows an unexpected extension count."
Assert-Condition ($Manifest.allowed_extensions[0] -ceq $ExtensionId) "The manifest extension principal is incorrect."
Assert-Condition ([System.IO.Path]::IsPathRooted([string]$Manifest.path)) "The helper path is not absolute."
Assert-Condition (-not ([string]$Manifest.path).Contains('"')) "The helper path contains command-line quote characters."
Assert-Condition (Test-Path -LiteralPath ([string]$Manifest.path) -PathType Leaf) "The helper executable is missing."

$TestLocalAppData = Join-Path $TestProfileRoot "Local App Data"
$TestUserProfile = Join-Path $TestProfileRoot "User Profile"
New-Item -ItemType Directory -Path (Join-Path $TestUserProfile "Downloads") -Force | Out-Null
New-Item -ItemType Directory -Path $TestLocalAppData -Force | Out-Null

$Hello = [ordered]@{
    protocol_version = 2
    correlation_id = "install-test-hello"
    kind = "command"
    command = "hello"
    payload = [ordered]@{
        supported_versions = @(2)
        client_name = "windows-install-test"
        client_version = "0.1.0"
    }
} | ConvertTo-Json -Depth 8 -Compress
$Add = [ordered]@{
    protocol_version = 2
    correlation_id = "install-test-add"
    kind = "command"
    command = "add"
    payload = [ordered]@{
        url = "http://127.0.0.1:9/native-install-test.bin"
        destination = (Join-Path $TestUserProfile "Downloads")
        suggested_filename = "native-install-test.bin"
        workers = 2
    }
} | ConvertTo-Json -Depth 8 -Compress
$List = [ordered]@{
    protocol_version = 2
    correlation_id = "install-test-list"
    kind = "command"
    command = "list"
    payload = [ordered]@{
        cursor = $null
        limit = 200
        include_terminal = $true
    }
} | ConvertTo-Json -Depth 8 -Compress
$Utf8 = New-Object System.Text.UTF8Encoding($false)

$StartInfo = New-Object System.Diagnostics.ProcessStartInfo
$StartInfo.FileName = [string]$Manifest.path
$StartInfo.UseShellExecute = $false
$StartInfo.CreateNoWindow = $true
$StartInfo.RedirectStandardInput = $true
$StartInfo.RedirectStandardOutput = $true
$StartInfo.RedirectStandardError = $true
$StartInfo.Environment["LOCALAPPDATA"] = $TestLocalAppData
$StartInfo.Environment["USERPROFILE"] = $TestUserProfile
$Process = New-Object System.Diagnostics.Process
$Process.StartInfo = $StartInfo
Assert-Condition ($Process.Start()) "The registered helper did not start."
foreach ($MessageJson in @($Hello, $Add, $List)) {
    $MessageBytes = $Utf8.GetBytes($MessageJson)
    $Prefix = [System.BitConverter]::GetBytes([uint32]$MessageBytes.Length)
    $Process.StandardInput.BaseStream.Write($Prefix, 0, $Prefix.Length)
    $Process.StandardInput.BaseStream.Write($MessageBytes, 0, $MessageBytes.Length)
}
$Process.StandardInput.BaseStream.Flush()
$Process.StandardInput.Close()

$Output = New-Object System.IO.MemoryStream
$Process.StandardOutput.BaseStream.CopyTo($Output)
$StandardError = $Process.StandardError.ReadToEnd()
$Process.WaitForExit()
Assert-Condition ($Process.ExitCode -eq 0) "The registered helper exited unsuccessfully."
Assert-Condition ([string]::IsNullOrEmpty($StandardError)) "The registered helper wrote a diagnostic during a clean session."
$Messages = @(Read-NativeMessages -Bytes ($Output.ToArray()))
Assert-Condition ($Messages.Count -ge 4) "The clean command session returned too few messages: $($Messages.Count)."
Assert-Condition ($Messages[0].kind -ceq "response") "The first native message is not a response."
Assert-Condition ($Messages[0].command -ceq "hello") "The first native response is not hello."
Assert-Condition ($Messages[0].ok -eq $true) "Protocol negotiation failed."
Assert-Condition ($Messages[0].result.selected_version -eq 2) "The helper selected an unexpected protocol version."
Assert-Condition ($Messages[0].result.max_message_bytes -eq $MaximumMessageBytes) "The helper advertised an unexpected message limit."
Assert-Condition ($Messages[1].kind -ceq "event") "The second native message is not an event."
Assert-Condition ($Messages[1].event -ceq "snapshot") "The reconnect bootstrap event is not a snapshot."
Assert-Condition ($Messages[1].data.complete -eq $true) "The initial snapshot did not complete."
$AddResponse = @($Messages | Where-Object { $_.kind -ceq "response" -and $_.command -ceq "add" })
Assert-Condition ($AddResponse.Count -eq 1) "The installed helper did not return one add response."
Assert-Condition ($AddResponse[0].ok -eq $true) "The installed helper rejected a valid add command."
Assert-Condition ($AddResponse[0].result.workers -eq 2) "The installed helper lost the selected worker count."
$ListResponse = @($Messages | Where-Object { $_.kind -ceq "response" -and $_.command -ceq "list" })
Assert-Condition ($ListResponse.Count -eq 1) "The installed helper did not return one list response."
Assert-Condition ($ListResponse[0].ok -eq $true) "The installed helper rejected a valid list command."
Assert-Condition ($ListResponse[0].result.complete -eq $true) "The installed helper did not complete its bounded list page."
Assert-Condition ($ListResponse[0].result.tasks.Count -eq 1) "The helper list snapshot lost the created task."

$SessionMessagesPath = Join-Path $TestProfileRoot "native-session-messages.json"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText(
    $SessionMessagesPath,
    (ConvertTo-Json -InputObject $Messages -Depth 32),
    $Utf8NoBom
)
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepositoryRoot
try {
    & node ./scripts/validate-protocol.mjs $SessionMessagesPath
    Assert-Condition ($LASTEXITCODE -eq 0) "The live helper messages failed protocol schema validation."
}
finally {
    Pop-Location
}

Remove-Item -LiteralPath $TestProfileRoot -Recurse -Force -ErrorAction SilentlyContinue
Write-Output "Verified registration, path handling, launch, negotiation, and initial snapshot."
