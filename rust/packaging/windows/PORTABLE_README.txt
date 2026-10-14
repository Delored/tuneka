Tuneka Rust - Windows x64 development artifact

Extract the entire ZIP into one directory. Keep every DLL beside the EXE files.
This unsigned CI artifact has not been validated on a clean consumer Windows
desktop. Windows 10/11 x64 with a compatible Direct3D driver is required for GUI.
Windows N editions may require Microsoft's Media Feature Pack for FFmpeg's
Windows media libraries. Windows ARM64 is outside the current target matrix.

tuneka.exe --help            Command-line downloader
tuneka-tui.exe               Interactive terminal interface (use a real console)
tuneka-mcp.exe               MCP stdio server (configure it in an MCP client)
tuneka-gui.exe               GPUI desktop interface

State: %APPDATA%\TunekaRust
Overrides: TUNEKA_RUST_DATA_DIR and TUNEKA_DOWNLOADS
No Python, yt-dlp, ffmpeg.exe or ffprobe.exe is required to run these applications.

The bundled FFmpeg, codec libraries and app-local MSVC C++ runtime are dynamically
linked. licenses/native contains installed native-port licenses and exact vcpkg
package versions; licenses/rust contains the Cargo dependency license inventory.
The pinned vcpkg registry linked from build-info.json records native source URLs,
archive checksums and build patches. No external FFmpeg programs are bundled.

MSVC redistributable DLLs come from the build toolset's x64 Microsoft.VC*.CRT
directory. They remain subject to Microsoft's Visual Studio license terms:
https://visualstudio.microsoft.com/license-terms/
https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files

Tuneka is GPL-3.0-only; see LICENSE and THIRD_PARTY_NOTICES.md. Complete
corresponding-source and redistribution obligations must be satisfied before a
public release. This package is a development build, not a signed public release.

native-imports.json records the DLL import closure. validation.json records
portable CLI and MCP checks run with build tools removed from PATH. It does not
claim GUI interaction, TUI console interaction or live YouTube download coverage.
