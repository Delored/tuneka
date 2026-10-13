# Tuneka

Tuneka is a desktop application for downloading YouTube music mixes, playlists, and videos. It can split a chaptered mix into separate MP3 tracks, keeping the chapter titles and video cover art. A video without chapters is saved as a single track.

## Features

- **Music by chapters:** Choose chapters and save them as individual MP3 files with embedded cover art.
- **Multiple links:** Add several YouTube video links at once, combine their chapters into one selection, and process video downloads in sequence.
- **Playlists:** Select the videos you want and download each as an MP3 or a video file.
- **Single videos:** Choose a resolution and a container: MP4, MKV, or WebM.
- **Download controls:** View progress, pause and resume, or cancel an active download.
- **Preferences:** Set the download folder, default video quality and format, playlist format, color theme, and interface language.
- **Localization:** Ukrainian and English interface languages.
- **Themes:** Olive, purple, and light themes.

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

Tuneka downloads `yt-dlp` from its [official GitHub releases](https://github.com/yt-dlp/yt-dlp/releases/latest) and FFmpeg from [imageio-binaries](https://github.com/imageio/imageio-binaries/tree/master/ffmpeg). Use **Update tools** in the app to download them again.

Supported platforms: Windows x64 and ARM64, macOS x64 and Apple Silicon, and Linux x64 and ARM64.

## Build a Debian package

An experimental Debian package for amd64 can be built on Debian or Ubuntu with:

```sh
sh packaging/debian/build-deb.sh 0.1.0
```

The `.deb` is written to `dist/`. It uses the system Python and Tkinter packages; Tuneka continues to download its own `yt-dlp` and FFmpeg binaries on first launch and stores them in the user's data directory. See [`packaging/README.md`](packaging/README.md) for installation notes.

## Downloads and settings

By default, files are saved under a `downloads/` folder beside the application. Music tracks and individual videos are placed in a subfolder named after the video; playlist downloads go into a folder named after the playlist. The destination folder can be changed in the app, and the selection is remembered between launches.

Temporary source audio and cover files used while splitting a mix are removed when processing finishes. Cover art is embedded in the MP3 metadata. Settings, including the download folder, theme, language, and default formats, are saved in `settings.json` beside the application.

## License

Tuneka is licensed under the **GNU General Public License v3.0**. See [`LICENSE`](LICENSE) for the full text. The separately downloaded `yt-dlp` and FFmpeg binaries retain their own licenses.
