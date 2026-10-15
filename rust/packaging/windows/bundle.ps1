#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$VcpkgRoot,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'Windows x64 is required to inspect and test native PE binaries.' }
$RustRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$VersionMatch = [regex]::Match((Get-Content (Join-Path $RustRoot 'Cargo.toml') -Raw), '(?m)^version = "([^"]+)"')
if (-not $VersionMatch.Success) { throw 'Workspace version was not found.' }
$Version = $VersionMatch.Groups[1].Value
$Name = "Tuneka-$Version-windows-x64"
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$Stage = Join-Path $OutputDirectory $Name
$Zip = Join-Path $OutputDirectory "$Name.zip"
if ((Test-Path $Stage) -or (Test-Path $Zip)) { throw "Refusing to overwrite an existing artifact in $OutputDirectory" }
$TargetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RustRoot 'target' }
if (-not [IO.Path]::IsPathRooted($TargetDirectory)) { $TargetDirectory = Join-Path $RustRoot $TargetDirectory }
$Release = Join-Path $TargetDirectory 'x86_64-pc-windows-msvc/release'
$Native = Join-Path $VcpkgRoot 'installed/x64-windows'
$Manifest = Get-Content (Join-Path $PSScriptRoot 'vcpkg.json') -Raw | ConvertFrom-Json
$Baseline = & git -C $VcpkgRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $Baseline.Trim() -ne $Manifest.'builtin-baseline') { throw 'Unexpected vcpkg checkout.' }

New-Item -ItemType Directory -Path $Stage -Force | Out-Null
foreach ($Binary in @('tuneka.exe', 'tuneka-tui.exe', 'tuneka-mcp.exe', 'tuneka-gui.exe')) {
    Copy-Item (Join-Path $Release $Binary) $Stage
}
$NativeDlls = @(Get-ChildItem (Join-Path $Native 'bin') -Filter '*.dll' -File)
if ($NativeDlls.Count -eq 0) { throw 'No shared vcpkg native libraries were found.' }
$NativeDlls | Copy-Item -Destination $Stage

# App-local runtime deployment uses the selected Visual Studio toolset's
# redistributable directory, never arbitrary DLLs copied from System32.
if (-not $env:VCToolsRedistDir) { throw 'Initialize the Visual Studio developer shell before bundling.' }
$CrtDirectories = @(Get-ChildItem (Join-Path $env:VCToolsRedistDir 'x64') -Directory -Filter 'Microsoft.VC*.CRT')
if ($CrtDirectories.Count -ne 1) { throw 'Expected one x64 MSVC CRT redistributable directory.' }
$CrtDlls = @(Get-ChildItem $CrtDirectories[0].FullName -File -Filter '*.dll')
if ($CrtDlls.Count -eq 0) { throw 'The MSVC CRT redistributable directory is empty.' }
$CrtDlls | Copy-Item -Destination $Stage

$Licenses = Join-Path $Stage 'licenses'
New-Item -ItemType Directory -Path (Join-Path $Licenses 'native') -Force | Out-Null
$Share = Join-Path $Native 'share'
foreach ($Copyright in Get-ChildItem $Share -Recurse -File -Filter 'copyright') {
    $Relative = [IO.Path]::GetRelativePath($Share, $Copyright.FullName)
    $Destination = Join-Path (Join-Path $Licenses 'native') $Relative
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($Destination)) -Force | Out-Null
    Copy-Item $Copyright.FullName $Destination
}
if (-not (Test-Path (Join-Path $Licenses 'native/ffmpeg/copyright'))) { throw 'Missing FFmpeg license.' }
Copy-Item (Join-Path $VcpkgRoot 'installed/vcpkg/status') (Join-Path $Licenses 'native/vcpkg-status.txt')
Copy-Item (Join-Path $PSScriptRoot 'vcpkg.json') (Join-Path $Licenses 'native/vcpkg.json')
Copy-Item (Join-Path $RustRoot '../LICENSE') (Join-Path $Stage 'LICENSE')
Copy-Item (Join-Path $RustRoot 'THIRD_PARTY_NOTICES.md') $Stage
Copy-Item (Join-Path $RustRoot 'README.md') (Join-Path $Stage 'RUST_README.md')
Copy-Item (Join-Path $PSScriptRoot 'PORTABLE_README.txt') (Join-Path $Stage 'README.txt')
& python (Join-Path $RustRoot 'packaging/collect_licenses.py') (Join-Path $Licenses 'rust')
if ($LASTEXITCODE -ne 0) { throw 'Rust dependency license collection failed.' }

# Record imports of every bundled PE file. Dependencies must resolve either
# beside the executable or to a named Windows OS component. Runtime libraries
# from a development machine do not count as portable dependencies.
$SystemDlls = @(
    'advapi32.dll', 'avicap32.dll', 'avrt.dll', 'bcrypt.dll', 'bcryptprimitives.dll',
    'cfgmgr32.dll', 'comctl32.dll', 'comdlg32.dll', 'crypt32.dll', 'cryptbase.dll',
    'd2d1.dll', 'd3d11.dll', 'd3d12.dll', 'd3dcompiler_47.dll', 'dcomp.dll',
    'devobj.dll', 'dnsapi.dll', 'dwmapi.dll', 'dwrite.dll', 'dxgi.dll', 'dxva2.dll',
    'gdi32.dll', 'hid.dll', 'imm32.dll', 'iphlpapi.dll', 'icuuc.dll', 'kernel32.dll', 'kernelbase.dll',
    'mf.dll', 'mfplat.dll', 'mfreadwrite.dll', 'mfuuid.dll', 'mmdevapi.dll', 'msacm32.dll',
    'msasn1.dll', 'msimg32.dll', 'msvcrt.dll', 'msvfw32.dll', 'mswsock.dll', 'ncrypt.dll', 'netapi32.dll',
    'normaliz.dll', 'ntdll.dll', 'ole32.dll', 'oleacc.dll', 'oleaut32.dll', 'opengl32.dll',
    'powrprof.dll', 'profapi.dll', 'propsys.dll', 'psapi.dll', 'rpcrt4.dll', 'secur32.dll',
    'setupapi.dll', 'shell32.dll', 'shlwapi.dll', 'sspicli.dll', 'ucrtbase.dll',
    'user32.dll', 'userenv.dll', 'usp10.dll', 'uxtheme.dll', 'version.dll',
    'winhttp.dll', 'wininet.dll', 'winmm.dll', 'windowscodecs.dll', 'winspool.drv',
    'wintrust.dll', 'wldap32.dll', 'ws2_32.dll', 'wtsapi32.dll'
)
$Imports = [ordered]@{}
foreach ($Pe in Get-ChildItem $Stage -File | Where-Object { $_.Extension -in @('.exe', '.dll') }) {
    $Headers = & dumpbin.exe /nologo /headers $Pe.FullName
    if ($LASTEXITCODE -ne 0 -or -not ($Headers -match '^\s*8664 machine')) { throw "Not an x64 PE file: $($Pe.Name)" }
    $Raw = & dumpbin.exe /nologo /dependents $Pe.FullName
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect imports of $($Pe.Name)" }
    $Dependencies = @($Raw | ForEach-Object {
        if ($_ -match '^\s+([a-zA-Z0-9_.-]+\.(?:dll|drv))\s*$') { $Matches[1].ToLowerInvariant() }
    } | Sort-Object -Unique)
    if ($Dependencies.Count -eq 0) { throw "No PE imports found for $($Pe.Name)" }
    foreach ($Dll in $Dependencies) {
        if (Test-Path (Join-Path $Stage $Dll)) { continue }
        if (($Dll -match '^(api-ms-win-|ext-ms-win-)') -or
            (($Dll -in $SystemDlls) -and (Test-Path (Join-Path $env:WINDIR "System32/$Dll")))) { continue }
        throw "Unbundled dependency: $($Pe.Name) -> $Dll"
    }
    $Imports[$Pe.Name] = $Dependencies
}
$Imports | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Stage 'native-imports.json') -Encoding utf8
$Evidence = Join-Path $Stage 'validation.json'
& python (Join-Path $PSScriptRoot 'smoke_portable.py') $Stage --evidence $Evidence
if ($LASTEXITCODE -ne 0) { throw 'Portable CLI/MCP runtime checks failed.' }
$GitCommit = & git -C $RustRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot record the application revision.' }
$NativeSources = [ordered]@{
    vcpkg_commit = $Baseline.Trim()
    registry_url = "https://github.com/microsoft/vcpkg/tree/$($Baseline.Trim())"
    source_downloads = 'vcpkg portfile.cmake files pin source archives by SHA512; patches are in the pinned registry.'
    msvc_redist_directory = $CrtDirectories[0].Name
    msvc_runtime_files = @($CrtDlls | ForEach-Object { @{ name = $_.Name; version = $_.VersionInfo.FileVersion } })
    app_commit = $GitCommit.Trim()
    target = 'x86_64-pc-windows-msvc'
    signing = 'unsigned development CI artifact'
}
$NativeSources | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Stage 'build-info.json') -Encoding utf8
$Hashes = @(Get-ChildItem $Stage -Recurse -File | Sort-Object FullName | ForEach-Object {
    $Relative = [IO.Path]::GetRelativePath($Stage, $_.FullName).Replace('\', '/')
    (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $Relative
})
$Hashes | Set-Content (Join-Path $Stage 'SHA256SUMS.txt') -Encoding ascii
Compress-Archive -LiteralPath $Stage -DestinationPath $Zip -CompressionLevel Optimal
((Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + [IO.Path]::GetFileName($Zip)) |
    Set-Content (Join-Path $OutputDirectory 'SHA256SUMS.txt') -Encoding ascii
Write-Host "Built $Zip"
