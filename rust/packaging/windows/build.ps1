#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$VcpkgRoot,
    [string]$OutputDirectory = 'dist/windows-x64',
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'Windows x64 is required; this is not a cross-compilation script.' }
$RustRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$VcpkgRoot = [IO.Path]::GetFullPath($VcpkgRoot)
if ($VcpkgRoot -match '\s') { throw 'FFmpeg requires a vcpkg build path without spaces.' }
if (-not [IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $RustRoot $OutputDirectory
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$Target = 'x86_64-pc-windows-msvc'
$Manifest = Get-Content (Join-Path $PSScriptRoot 'vcpkg.json') -Raw | ConvertFrom-Json
$Baseline = $Manifest.'builtin-baseline'

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

# Initialize the compiler/SDK without printing an environment dump.
$VsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path $VsWhere)) { throw 'Visual Studio Installer/vswhere.exe is required.' }
$VsPath = & $VsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $VsPath) { throw 'Visual Studio C++ x64 build tools were not found.' }
$VsPath = ($VsPath | Select-Object -First 1).Trim()
& (Join-Path $VsPath 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
if ($env:VSCMD_ARG_TGT_ARCH -notin @('x64', 'amd64')) { throw 'Failed to initialize the x64 MSVC environment.' }
Get-Command cl.exe, link.exe, dumpbin.exe, rc.exe, cargo.exe, python.exe, git.exe -ErrorAction Stop | Out-Null

$SdkBin = Join-Path $env:WindowsSdkDir ('bin/' + $env:WindowsSDKVersion.TrimEnd('\') + '/x64/fxc.exe')
if (-not (Test-Path $SdkBin)) { throw "The selected Windows SDK has no x64 fxc.exe: $SdkBin" }
$env:GPUI_FXC_PATH = $SdkBin
if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM/bin' }
if (-not (Test-Path (Join-Path $env:LIBCLANG_PATH 'libclang.dll'))) {
    throw 'Install x64 LLVM and set LIBCLANG_PATH to its bin directory containing libclang.dll.'
}

# Use a dedicated checkout at the same commit as the manifest registry. Do not
# reset a global vcpkg installation or accept whatever happens to be on PATH.
if (-not (Test-Path $VcpkgRoot)) {
    Invoke-Checked git @('clone', '--no-checkout', '--filter=blob:none', 'https://github.com/microsoft/vcpkg.git', $VcpkgRoot)
    Invoke-Checked git @('-C', $VcpkgRoot, 'checkout', '--detach', $Baseline)
}
$ActualBaseline = & git -C $VcpkgRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $ActualBaseline.Trim() -ne $Baseline) {
    throw "Use a dedicated vcpkg checkout at $Baseline; existing checkout was not changed."
}
$env:VCPKG_ROOT = $VcpkgRoot
$env:VCPKGRS_TRIPLET = 'x64-windows'
$env:VCPKGRS_DYNAMIC = '1'
$env:VCPKG_DISABLE_METRICS = '1'
# Native dependencies use hashed source archives, with an optional explicit
# local cache populated by previous builds of this workflow.
$env:VCPKG_BINARY_SOURCES = 'clear'
if ($env:VCPKG_DEFAULT_BINARY_CACHE) {
    New-Item -ItemType Directory -Force $env:VCPKG_DEFAULT_BINARY_CACHE | Out-Null
    $env:VCPKG_BINARY_SOURCES += ";files,$env:VCPKG_DEFAULT_BINARY_CACHE,readwrite"
}
Remove-Item Env:FFMPEG_DIR -ErrorAction SilentlyContinue
Remove-Item Env:FFMPEG_STATIC -ErrorAction SilentlyContinue
Invoke-Checked (Join-Path $VcpkgRoot 'bootstrap-vcpkg.bat') @('-disableMetrics')
Invoke-Checked (Join-Path $VcpkgRoot 'vcpkg.exe') @(
    'install', '--triplet=x64-windows', '--host-triplet=x64-windows',
    "--x-manifest-root=$PSScriptRoot", "--x-install-root=$VcpkgRoot/installed"
)
$env:PATH = (Join-Path $VcpkgRoot 'installed/x64-windows/bin') + ';' + $env:PATH

Push-Location $RustRoot
try {
    Invoke-Checked cargo @('build', '--workspace', '--locked', '--release', '--target', $Target)
    if (-not $SkipTests) {
        # These include actual FFmpeg output tests and an MCP child-process lifecycle.
        # No live YouTube probes or GUI desktop interaction are run by this command.
        Invoke-Checked cargo @('test', '--workspace', '--locked', '--release', '--target', $Target)
    }
    & (Join-Path $PSScriptRoot 'bundle.ps1') -VcpkgRoot $VcpkgRoot -OutputDirectory $OutputDirectory
} finally {
    Pop-Location
}
