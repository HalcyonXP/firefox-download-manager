[CmdletBinding(SupportsShouldProcess = $true)]
param([string]$ExecutablePath, [string]$InstallRoot, [string]$TestProfileRoot)
$ErrorActionPreference = 'Stop'
throw 'These development registration scripts are retired: no changes were made. Use download-manager-setup.exe from a verified package; see docs/INSTALLATION.md. For isolated CI lifecycle testing use scripts/test-package-install.py.'
