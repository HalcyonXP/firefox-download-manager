[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$ExecutablePath,
    [string]$InstallRoot = (Join-Path $env:LOCALAPPDATA "HalcyonXP\FirefoxDownloadManager\host")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$HostName = "com.halcyonxp.firefox_download_manager"
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$TemplatePath = Join-Path $RepositoryRoot "native-host\manifest.template.json"
$RegistryKey = "HKCU:\Software\Mozilla\NativeMessagingHosts\$HostName"

if ([string]::IsNullOrWhiteSpace($InstallRoot) -or -not [System.IO.Path]::IsPathRooted($InstallRoot)) {
    throw "InstallRoot must be a non-empty absolute path."
}
$InstallRoot = [System.IO.Path]::GetFullPath($InstallRoot)

if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    Push-Location $RepositoryRoot
    try {
        & cargo build --release --locked -p download-manager-native-host
        if ($LASTEXITCODE -ne 0) {
            throw "The native host release build failed."
        }
    }
    finally {
        Pop-Location
    }
    $ExecutablePath = Join-Path $RepositoryRoot "target\release\download-manager-native-host.exe"
}
if (-not [System.IO.Path]::IsPathRooted($ExecutablePath)) {
    $ExecutablePath = Join-Path (Get-Location) $ExecutablePath
}
$ExecutablePath = [System.IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "The native host executable does not exist."
}
if (-not (Test-Path -LiteralPath $TemplatePath -PathType Leaf)) {
    throw "The native host manifest template does not exist."
}

$BinDirectory = Join-Path $InstallRoot "bin"
$InstalledExecutable = Join-Path $BinDirectory "download-manager-native-host.exe"
$ManifestPath = Join-Path $InstallRoot "$HostName.json"
$ExecutableTemporary = "$InstalledExecutable.installing-$PID"
$ManifestTemporary = "$ManifestPath.installing-$PID"

if ($PSCmdlet.ShouldProcess("the current-user Firefox Native Messaging registry", "Install $HostName")) {
    New-Item -ItemType Directory -Path $BinDirectory -Force | Out-Null
    try {
        $SourceCanonical = [System.IO.Path]::GetFullPath($ExecutablePath)
        $DestinationCanonical = [System.IO.Path]::GetFullPath($InstalledExecutable)
        if (-not [string]::Equals($SourceCanonical, $DestinationCanonical, [System.StringComparison]::OrdinalIgnoreCase)) {
            Copy-Item -LiteralPath $ExecutablePath -Destination $ExecutableTemporary -Force
            Move-Item -LiteralPath $ExecutableTemporary -Destination $InstalledExecutable -Force
        }

        $SourceHash = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash
        $InstalledHash = (Get-FileHash -LiteralPath $InstalledExecutable -Algorithm SHA256).Hash
        if (-not [string]::Equals($SourceHash, $InstalledHash, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "The installed native host executable failed verification."
        }

        $Manifest = Get-Content -LiteralPath $TemplatePath -Raw | ConvertFrom-Json
        $Manifest.path = $InstalledExecutable
        $ManifestJson = $Manifest | ConvertTo-Json -Depth 8
        $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($ManifestTemporary, $ManifestJson + [Environment]::NewLine, $Utf8NoBom)
        Move-Item -LiteralPath $ManifestTemporary -Destination $ManifestPath -Force

        New-Item -Path $RegistryKey -Force | Out-Null
        Set-Item -LiteralPath $RegistryKey -Value $ManifestPath
    }
    finally {
        Remove-Item -LiteralPath $ExecutableTemporary -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $ManifestTemporary -Force -ErrorAction SilentlyContinue
    }
}

Write-Output "Installed the current-user Firefox Download Manager native host."
