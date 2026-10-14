# Repository Guidelines

## Project Structure & Module Organization

Tuneka is a native Python 3/Tkinter desktop application. `app.py` contains the entry point, download workflows, tool bootstrapping, settings, and application controller. `ui.py` holds reusable widgets and drawing helpers; `i18n.py` provides Ukrainian/English translations. Application icons live in `assets/`, with packaging icons under `packaging/icons/`. Tests are in `tests/test_core.py` and `tests/test_ui.py`. See `docs/design.md` for UI principles and `packaging/README.md` for distribution details. Downloaded executables belong in `resources/bin/`.

## Build, Test, and Development Commands

Run commands from the repository root:

- `python3 app.py` — launch locally; use `python app.py` on Windows. Python must include Tkinter. First launch downloads yt-dlp and FFmpeg.
- `python3 -m unittest discover -s tests -p 'test_core.py' -v` — run core tests without a display; Tkinter must still be importable.
- `python3 -m unittest discover -s tests -p 'test_*.py' -v` — run the full suite with a desktop display.
- `xvfb-run --auto-servernum python3 -m unittest discover -s tests -p 'test_*.py' -v` — run the full suite on headless Linux; install `python3-tk` and `xvfb` first.
- `sh packaging/debian/build-deb.sh 0.1.0` — build a `.deb` in `dist/` on matching Debian/Ubuntu hardware.

GitHub Actions tests changes targeting `master` and builds Debian amd64/arm64 packages plus Windows x64 packages using PyInstaller and Inno Setup.

## Coding Style & Naming Conventions

Use four-space indentation, `snake_case` functions and variables, `PascalCase` classes, and `UPPER_SNAKE_CASE` constants. Match surrounding formatting; no formatter or lint configuration is currently checked in. Keep reusable widgets in `ui.py` and update `i18n.py` when changing interface text. Preserve the standard-library/Tkinter runtime approach.

## Testing Guidelines

Use `unittest`, `test_*.py` files, and descriptive `test_*` methods. Mock network calls, external tools, and settings writes; use temporary directories for filesystem cases. Add regression coverage for behavior changes. UI tests skip when no display is available, so verify they actually run. No numeric coverage threshold is configured.

## Commit & Pull Request Guidelines

Follow the history's short, imperative subjects, such as `Fix Linux application dock icon`. Keep commits focused. Target `master`; describe the problem, resulting behavior, test results, and relevant platform checks. Link related issues and include screenshots for visible UI changes.

## Local Configuration

Use `TUNEKA_DATA_DIR` and `TUNEKA_DOWNLOADS` to override local paths. Keep settings, downloaded media, tool binaries, credentials, and generated `build/` or `dist/` output out of commits.
