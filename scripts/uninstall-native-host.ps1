[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "HalcyonXP\FirefoxDownloadManager\host")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$HostName = "com.halcyonxp.firefox_download_manager"
$RegistryKey = "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"

if ([string]::IsNullOrWhiteSpace($InstallRoot) -or -not [System.IO.Path]::IsPathRooted($InstallRoot)) {
    throw "InstallRoot must be a non-empty absolute path."
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)
$ManifestPath = Join-Path $InstallRoot "$HostName.json"
$BinDirectory = Join-Path $InstallRoot "bin"
$InstalledExecutable = Join-Path $BinDirectory "download-manager-native-host.exe"

if ($PSCmdlet.ShouldProcess("the current-user Firefox Native Messaging registry", "Uninstall $HostName")) {
    if (Test-Path -LiteralPath $RegistryKey) {
        $RegisteredManifest = (Get-Item -LiteralPath $RegistryKey).GetValue("")
        if (-not [string]::IsNullOrWhiteSpace($RegisteredManifest)) {
            $RegisteredCanonical = [System.IO.Path]::GetFullPath([string]$RegisteredManifest)
            $ExpectedCanonical = [System.IO.Path]::GetFullPath($ManifestPath)
            if (-not [string]::Equals($RegisteredCanonical, $ExpectedCanonical, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "The current-user registration belongs to a different installation."
            }
        }
        Remove-Item -LiteralPath $RegistryKey -Force
    }

    Remove-Item -LiteralPath $ManifestPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $InstalledExecutable -Force -ErrorAction SilentlyContinue
    if ((Test-Path -LiteralPath $BinDirectory -PathType Container) -and
        -not (Get-ChildItem -LiteralPath $BinDirectory -Force | Select-Object -First 1)) {
        Remove-Item -LiteralPath $BinDirectory -Force
    }
    if ((Test-Path -LiteralPath $InstallRoot -PathType Container) -and
        -not (Get-ChildItem -LiteralPath $InstallRoot -Force | Select-Object -First 1)) {
        Remove-Item -LiteralPath $InstallRoot -Force
    }
}

Write-Output "Removed the current-user Firefox Download Manager native host registration."
