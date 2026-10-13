# Tuneka

Tuneka is a desktop application for downloading music mixes, playlists, and videos. It can split a chaptered mix into separate music files, keeping the chapter titles and cover art when supported. A video without chapters is saved as a single track.

## Features

- **Music by chapters:** Choose chapters and save them as individual MP3, M4A, FLAC, or WAV files. MP3, M4A, and FLAC can include embedded cover art.
- **Multiple links:** Add several video links at once, combine their chapters into one selection, and process video downloads in sequence.
- **Playlists:** Select the videos you want and download each as music (MP3, M4A, FLAC, or WAV) or as a video file.
- **Single videos:** Choose a resolution and a container: MP4, MKV, or WebM.
- **Download controls:** View progress, pause and resume, or cancel an active download.
- **Preferences:** Set the download folder, default video quality and format, playlist output and music format, color theme, and interface language. Video quality and file format choices are saved automatically and shared by the video and playlist modes.
- **Localization:** Ukrainian and English interface languages.
- **Themes:** Light by default, with olive and purple alternatives.

## Requirements

- Python 3
- Tkinter (usually included with Python; on Ubuntu/Debian, install `python3-tk` if it is missing)
- An internet connection for downloading media and the required tools

You do not need to install `yt-dlp` or FFmpeg system-wide. Tuneka downloads platform-specific binaries on first launch and stores them in `resources/bin/`.

## Run from source

Clone the repository, then run:

```bash
python3 app.py
```

On Windows, use:

```powershell
python app.py
```

## Windows downloads

The GitHub Actions workflow creates both a Windows x64 installer and a portable ZIP. Download `Tuneka-Setup-x64.exe` from a successful workflow run's artifacts, or from a GitHub Release, and follow the setup wizard. It installs Tuneka under Program Files and adds a Start menu shortcut; a desktop shortcut is optional. Uninstall it from Windows Settings → Apps. The installer does not remove your settings or downloaded media.

If you prefer not to install the app, download `Tuneka-windows-x64.zip`, extract it, and launch `Tuneka.exe`. No Python installation is required for either build. On first launch, Tuneka downloads the required `yt-dlp` and FFmpeg tools. Both Windows builds store tools and settings in `%LOCALAPPDATA%\Tuneka`, and downloads default to your Windows Downloads folder.

Tuneka downloads `yt-dlp` from its [official GitHub releases](https://github.com/yt-dlp/yt-dlp/releases/latest) and FFmpeg from [imageio-binaries](https://github.com/imageio/imageio-binaries/tree/master/ffmpeg). Use **Update tools** in the app to download them again.

Supported platforms: Windows x64 and ARM64, macOS x64 and Apple Silicon, and Linux x64 and ARM64.

## Build a Debian package

Experimental Debian packages for amd64 and arm64 can be built on matching Debian or Ubuntu systems with:

```sh
sh packaging/debian/build-deb.sh 0.1.0
```

The architecture-specific `.deb` is written to `dist/`. It uses the system Python and Tkinter packages; Tuneka continues to download its own `yt-dlp` and FFmpeg binaries on first launch and stores them in the user's data directory. See [`packaging/README.md`](packaging/README.md) for installation notes.

## Tests

Run the test suite on Debian or Ubuntu with Tkinter and a virtual display installed:

```sh
sudo apt install python3-tk xvfb
xvfb-run --auto-servernum python3 -m unittest discover --start-directory tests --pattern 'test_*.py' --verbose
```

## Downloads and settings

By default, source runs save files under a `downloads/` folder beside the application. Windows builds default to the user's Downloads folder. A chaptered music mix gets its own folder named after the video; a mix without chapters is saved as one music file in the selected format directly in the selected folder. Standalone videos are also saved directly in the selected folder. Playlist downloads get one folder named after the playlist, containing the selected videos or music files. The destination folder can be changed in the app, and the selection is remembered between launches.

Temporary source audio and cover files used while splitting a mix are removed when processing finishes. Settings, including the download folder, theme, language, and default formats, are saved in `settings.json` beside the application for source runs and under `%LOCALAPPDATA%\Tuneka` in Windows builds.

## Legal and responsible use

Tuneka is an independent open-source project.

You are responsible for ensuring that you have the rights, permission, or another lawful basis to download and use content, and for complying with applicable laws and the terms of the services you use. The project authors do not endorse copyright infringement or other unlawful use of Tuneka and are not responsible for a user's unlawful use of the application.

Tuneka is provided under the GNU General Public License v3.0. The license's warranty disclaimer and limitation-of-liability terms are included in [`LICENSE`](LICENSE). This notice does not change the terms of that license.

## License

Tuneka is licensed under the **GNU General Public License v3.0**. See [`LICENSE`](LICENSE) for the full text. The separately downloaded `yt-dlp` and FFmpeg binaries retain their own licenses.
