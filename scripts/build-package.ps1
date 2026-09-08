# Build-only tooling downloads are pinned; the product itself has no downloader/updater.
[CmdletBinding()]
param([string]$Output = 'artifacts/package', [switch]$Development, [switch]$TestRust, [switch]$Rebuild)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$variables = @('PATH', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER', 'CC_x86_64_pc_windows_gnullvm', 'CXX_x86_64_pc_windows_gnullvm', 'AR_x86_64_pc_windows_gnullvm', 'CFLAGS_x86_64_pc_windows_gnullvm', 'CMAKE_GENERATOR')
$saved = @{}
foreach ($name in $variables) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
Push-Location $root
try {
    $tool = python scripts/prepare-toolchain.py
    if ($LASTEXITCODE) { throw 'Reviewed toolchain verification failed' }
    rustup target add x86_64-pc-windows-gnullvm
    if ($LASTEXITCODE) { throw 'Rust target preparation failed' }
    $cmakePaths = @()
    if (!(Get-Command cmake.exe -ErrorAction SilentlyContinue) -or !(Get-Command ninja.exe -ErrorAction SilentlyContinue)) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        if (Test-Path $vswhere) {
            $vs = & $vswhere -latest -property installationPath
            $cmake = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake'
            $cmakePaths = @((Join-Path $cmake 'CMake\bin'), (Join-Path $cmake 'Ninja'))
        }
    }
    $env:PATH = (@((Join-Path $tool 'bin')) + $cmakePaths + @($env:PATH)) -join ';'
    if (!(Get-Command cmake.exe -ErrorAction SilentlyContinue) -or !(Get-Command ninja.exe -ErrorAction SilentlyContinue)) { throw 'CMake and Ninja are required development tools; no system feature is changed to install them' }
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER = Join-Path $tool 'bin\x86_64-w64-mingw32-clang.exe'
    $env:CC_x86_64_pc_windows_gnullvm = $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER
    $env:CXX_x86_64_pc_windows_gnullvm = Join-Path $tool 'bin\x86_64-w64-mingw32-clang++.exe'
    $env:AR_x86_64_pc_windows_gnullvm = Join-Path $tool 'bin\llvm-ar.exe'
    $env:CMAKE_GENERATOR = 'Ninja'
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $env:CFLAGS_x86_64_pc_windows_gnullvm = "-ffile-prefix-map=`"$root=workspace`" -ffile-prefix-map=`"$cargoHome=cargo-home`""
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C', 'target-feature=+crt-static', '-C', 'link-self-contained=no', '-C', 'debuginfo=0', '-C', 'link-arg=-Wl,--no-insert-timestamp', '-C', 'link-arg=-static', "--remap-path-prefix=$root=workspace", "--remap-path-prefix=$cargoHome=cargo-home") -join [char]31
    if ($Rebuild) {
        cargo clean --target-dir target/package-build
        if ($LASTEXITCODE) { throw 'Explicit release build-cache cleanup failed' }
    }
    npm run build
    if ($LASTEXITCODE) { throw 'Extension build failed' }
    npm run extension:check
    if ($LASTEXITCODE) { throw 'Extension policy check failed' }
    if ($TestRust) {
        cargo test --target-dir target/package-build --workspace --all-features --locked --release --target x86_64-pc-windows-gnullvm
        if ($LASTEXITCODE) { throw 'Release-target tests failed' }
    }
    $maps = Join-Path $root 'target\package-maps'
    New-Item -ItemType Directory -Path $maps -Force | Out-Null
    foreach ($binary in @('download-manager-native-host', 'download-manager-setup')) {
        # Local link maps support runtime-object review; never include absolute-path maps in release inputs.
        $map = Join-Path $maps "$binary.map"
        cargo rustc --target-dir target/package-build --release --target x86_64-pc-windows-gnullvm -p $binary --bin $binary --locked -- -C "link-arg=-Wl,-Map,$map"
        if ($LASTEXITCODE) { throw 'Release build failed' }
    }
    $options = @('scripts/build-package.py', '--binary-dir', 'target/package-build/x86_64-pc-windows-gnullvm/release', '--output', $Output)
    if ($Development) { $options += '--development' }
    python @options
    if ($LASTEXITCODE) { throw 'Package construction failed' }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Pop-Location
}
