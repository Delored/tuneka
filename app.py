from __future__ import annotations

import json
import os
import platform
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import tkinter as tk
from pathlib import Path
from tkinter import filedialog, ttk
from tkinter import font as tkfont
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent
RESOURCES = ROOT / "resources"
BIN_DIR = RESOURCES / "bin"
DOWNLOADS = ROOT / "downloads"
SETTINGS_PATH = ROOT / "settings.json"
DOWNLOADS.mkdir(exist_ok=True)
IS_WINDOWS = sys.platform.startswith("win")
YTDLP_NAME = "yt-dlp.exe" if IS_WINDOWS else "yt-dlp"
FFMPEG_NAME = "ffmpeg.exe" if IS_WINDOWS else "ffmpeg"
YTDLP_PATH = BIN_DIR / YTDLP_NAME
FFMPEG_PATH = BIN_DIR / FFMPEG_NAME
YTDLP_LATEST = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}"
FFMPEG_BASE = "https://raw.githubusercontent.com/imageio/imageio-binaries/master/ffmpeg/{}"

THEMES = {
    "olive": {
        "label": "Оливкова", "bg": "#151911", "card": "#20261b", "ink": "#f0f3e9",
        "muted": "#a1aa95", "accent": "#c9ed72", "accent_text": "#263218", "accent_hover": "#d6f68b",
        "field": "#181d14", "border": "#343d2d", "outline": "#3a4430", "heading": "#282f23",
        "table_fg": "#dce3d2", "selected": "#3c4d2d", "selected_fg": "#f2f7e9",
        "scrollbar": "#343d2d", "progress_trough": "#2a3124", "disabled": "#343c2b",
        "disabled_text": "#89917d", "focus": "#8fae52", "entry_select": "#506b32",
        "secondary": "#2a3025", "secondary_hover": "#353e2d", "secondary_text": "#e8eddf",
        "secondary_disabled": "#252a21", "secondary_disabled_text": "#79806f",
    },
    "purple": {
        "label": "Фіолетова", "bg": "#17131f", "card": "#211a2d", "ink": "#f2eafa",
        "muted": "#b1a4bf", "accent": "#c49aff", "accent_text": "#2d1d3c", "accent_hover": "#d3b4ff",
        "field": "#1b1625", "border": "#392d49", "outline": "#49395b", "heading": "#2a2238",
        "table_fg": "#e3d9ed", "selected": "#49345f", "selected_fg": "#fff8ff",
        "scrollbar": "#40334f", "progress_trough": "#30263d", "disabled": "#393143",
        "disabled_text": "#9589a2", "focus": "#aa7de2", "entry_select": "#624783",
        "secondary": "#30283a", "secondary_hover": "#40354d", "secondary_text": "#eee5f5",
        "secondary_disabled": "#292330", "secondary_disabled_text": "#8c8198",
    },
}


def tool_assets() -> tuple[str, str]:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "windows":
        if machine in ("arm64", "aarch64"):
            return "yt-dlp_arm64.exe", "ffmpeg-win-x86_64-v7.1.exe"
        return "yt-dlp.exe", "ffmpeg-win-x86_64-v7.1.exe"
    if system == "darwin":
        arch = "aarch64" if machine in ("arm64", "aarch64") else "x86_64"
        return "yt-dlp_macos", f"ffmpeg-macos-{arch}-v7.1"
    if system == "linux":
        if machine in ("aarch64", "arm64"):
            return "yt-dlp_linux_aarch64", "ffmpeg-linux-aarch64-v7.0.2"
        if machine in ("x86_64", "amd64"):
            return "yt-dlp_linux", "ffmpeg-linux-x86_64-v7.0.2"
    raise RuntimeError(f"Платформа {system}/{machine} поки не підтримується.")


def hidden_console_kwargs() -> dict:
    if IS_WINDOWS:
        return {"creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0)}
    return {}


def validate_binary(path: Path, version_arg: str) -> bool:
    try:
        result = subprocess.run([str(path), version_arg], capture_output=True, timeout=15, **hidden_console_kwargs())
        return result.returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def download_file(url: str, target: Path, progress) -> None:
    temporary = target.with_name(target.name + ".download")
    request = urllib.request.Request(url, headers={"User-Agent": "Tracksplit/1.0"})
    try:
        with urllib.request.urlopen(request, timeout=45) as response, temporary.open("wb") as output:
            total = int(response.headers.get("Content-Length", "0"))
            received = 0
            while True:
                chunk = response.read(1024 * 256)
                if not chunk:
                    break
                output.write(chunk)
                received += len(chunk)
                progress(received, total)
        temporary.replace(target)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise
    if not IS_WINDOWS:
        target.chmod(target.stat().st_mode | 0o755)


def ensure_tools(progress, force: bool = False) -> None:
    BIN_DIR.mkdir(parents=True, exist_ok=True)
    ytdlp_asset, ffmpeg_asset = tool_assets()
    for path, url, label, version_arg in (
        (YTDLP_PATH, YTDLP_LATEST.format(ytdlp_asset), "yt-dlp", "--version"),
        (FFMPEG_PATH, FFMPEG_BASE.format(ffmpeg_asset), "ffmpeg", "-version"),
    ):
        if not force and validate_binary(path, version_arg):
            continue
        progress(f"Завантажую {label} у resources/bin…", 0)
        download_file(url, path, lambda done, total, name=label: progress(
            f"Завантажую {name}…" if not total else f"Завантажую {name}… {done * 100 // total}%",
            0 if not total else done * 100 // total,
        ))
        if not validate_binary(path, version_arg):
            path.unlink(missing_ok=True)
            raise RuntimeError(f"Завантажений файл {label} не запускається на цій системі.")
    progress("Інструменти готові.", 100)


def valid_youtube_url(value: str) -> bool:
    try:
        parsed = urlparse(value)
        host = (parsed.hostname or "").lower().rstrip(".")
        return parsed.scheme == "https" and (host == "youtu.be" or host == "youtube.com" or host.endswith(".youtube.com"))
    except ValueError:
        return False


def run(args: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=False, **hidden_console_kwargs())


class DownloadCancelled(Exception):
    """Raised when the user cancels an active download."""


def process_group_kwargs() -> dict:
    options = hidden_console_kwargs()
    if os.name != "nt":
        options["start_new_session"] = True
    return options


class DownloadController:
    def __init__(self):
        self._lock = threading.Lock()
        self.process = None
        self.paused = False
        self.cancelled = False

    def attach(self, process):
        with self._lock:
            self.process = process
            paused, cancelled = self.paused, self.cancelled
        if cancelled:
            self._terminate(process)
        elif paused:
            self._send_signal(process, signal.SIGSTOP)

    def detach(self, process):
        with self._lock:
            if self.process is process:
                self.process = None

    def pause(self):
        if os.name == "nt":
            raise OSError("Пауза процесу наразі підтримується в Linux та macOS.")
        with self._lock:
            if self.cancelled or self.paused:
                return
            self.paused = True
            process = self.process
        if process is not None:
            self._send_signal(process, signal.SIGSTOP)

    def resume(self):
        if os.name == "nt":
            raise OSError("Продовження процесу наразі підтримується в Linux та macOS.")
        with self._lock:
            if self.cancelled or not self.paused:
                return
            self.paused = False
            process = self.process
        if process is not None:
            self._send_signal(process, signal.SIGCONT)

    def cancel(self):
        with self._lock:
            if self.cancelled:
                return
            self.cancelled = True
            self.paused = False
            process = self.process
        if process is not None:
            self._terminate(process)

    def check_cancelled(self):
        with self._lock:
            if self.cancelled:
                raise DownloadCancelled("Завантаження скасовано.")

    @staticmethod
    def _send_signal(process, sig):
        try:
            if os.name == "nt":
                process.send_signal(sig)
            else:
                os.killpg(process.pid, sig)
        except (OSError, ProcessLookupError):
            pass

    @classmethod
    def _terminate(cls, process):
        if os.name != "nt":
            cls._send_signal(process, signal.SIGCONT)
            cls._send_signal(process, signal.SIGTERM)
        else:
            try:
                process.terminate()
            except OSError:
                pass


def run_controlled(args: list[str], timeout: int, controller: DownloadController):
    controller.check_cancelled()
    process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, **process_group_kwargs())
    controller.attach(process)
    try:
        deadline = time.monotonic() + timeout
        while True:
            try:
                stdout, stderr = process.communicate(timeout=0.25)
                break
            except subprocess.TimeoutExpired:
                controller.check_cancelled()
                if not controller.paused and time.monotonic() >= deadline:
                    DownloadController._terminate(process)
                    process.communicate()
                    raise subprocess.TimeoutExpired(args, timeout)
    finally:
        if process.poll() is None:
            DownloadController._terminate(process)
            process.wait()
        controller.detach(process)
    controller.check_cancelled()
    return subprocess.CompletedProcess(args, process.returncode, stdout, stderr)


def info_for(url: str, control: DownloadController | None = None) -> dict:
    command = [str(YTDLP_PATH), "--no-warnings", "--dump-single-json", "--skip-download", url]
    result = run_controlled(command, 180, control) if control else run(command, timeout=180)
    if result.returncode:
        raise RuntimeError((result.stderr or result.stdout).strip()[-1200:] or "Не вдалося прочитати відео.")
    data = json.loads(result.stdout)
    chapters = data.get("chapters") or []
    title = data.get("title") or "YouTube відео"
    duration = data.get("duration")
    if not isinstance(duration, (int, float)) or duration <= 0:
        duration = None
    if not chapters:
        chapters = [{"title": title, "start_time": 0, "end_time": duration}]
    normalized_chapters = []
    for index, chapter in enumerate(chapters):
        start = chapter.get("start_time") or 0
        end = chapter.get("end_time")
        if end is None or end <= start:
            if index + 1 < len(chapters):
                next_start = chapters[index + 1].get("start_time")
                end = next_start if next_start is not None and next_start > start else None
            elif duration is not None and duration > start:
                end = duration
            else:
                end = None
        normalized_chapters.append({
            "index": index,
            "title": chapter.get("title") or f"Трек {index + 1}",
            "start": start,
            "end": end,
        })
    return {
        "title": title, "thumbnail": data.get("thumbnail"),
        "duration": duration,
        "chapters": normalized_chapters,
    }


def playlist_info_for(url: str) -> dict:
    result = run([str(YTDLP_PATH), "--no-warnings", "--flat-playlist", "--dump-single-json",
                  "--skip-download", url], timeout=600)
    if result.returncode:
        raise RuntimeError((result.stderr or result.stdout).strip()[-1200:] or "Не вдалося прочитати плейлист.")
    data = json.loads(result.stdout)
    raw_entries = data.get("entries") or []
    entries = []
    for item in raw_entries:
        if not item:
            continue
        item_id = item.get("id")
        item_url = item.get("webpage_url")
        if not item_url or not item_url.startswith("http"):
            item_url = f"https://www.youtube.com/watch?v={item_id}" if item_id else item.get("url")
        if not item_url:
            continue
        entries.append({
            "index": len(entries),
            "id": item_id,
            "url": item_url,
            "title": item.get("title") or f"Відео {len(entries) + 1}",
            "duration": item.get("duration"),
        })
    if not entries:
        raise RuntimeError("У плейлисті не знайдено доступних відео.")
    return {"title": data.get("title") or "YouTube плейлист", "entries": entries}


def download_video_file(url: str, title: str, output_dir: Path, progress, *,
                        resolution: str = "best", container: str = "mp4", prefix: str = "",
                        control: DownloadController | None = None) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    if not FFMPEG_PATH.is_file():
        raise RuntimeError("Не знайдено вбудований ffmpeg. Онови інструменти й повтори завантаження.")
    safe_title = safe_filename(title)
    stem = f"{prefix}{safe_title}"
    output_template = str(output_dir / f"{stem}.%(ext)s")
    height = re.search(r"(\d+)p", resolution.lower())
    height_filter = f"[height<={height.group(1)}]" if height else ""
    container = container.lower()
    if container not in {"mp4", "mkv", "webm"}:
        raise ValueError("Підтримуються формати MP4, MKV та WebM.")
    format_selector = f"bv*{height_filter}+ba/b{height_filter}"
    command = [str(YTDLP_PATH), "--no-warnings", "--no-colors", "--newline", "--progress",
               "--no-simulate", "--no-playlist",
               "--ffmpeg-location", str(FFMPEG_PATH),
               "--progress-template",
               "download:TRACKSPLIT_PROGRESS %(progress._percent_str)s · %(progress._speed_str)s · ETA %(progress._eta_str)s",
               "--progress-template", "postprocess:TRACKSPLIT_POSTPROCESS %(progress._default_template)s",
               "-f", format_selector, "--merge-output-format", container, "--remux-video", container,
               "-o", output_template, "--print", "after_move:filepath", url]
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                               bufsize=1, **process_group_kwargs())
    if control:
        control.attach(process)
    output_lines = []
    error_lines = []
    downloaded = None
    progress("Отримую інформацію з YouTube…", 0)
    last_phase = None
    try:
        for line in process.stdout or []:
            if control:
                control.check_cancelled()
            line = line.strip()
            if not line:
                continue
            output_lines.append(line)
            output_lines = output_lines[-12:]
            if line.startswith("ERROR:") or line.startswith("ERROR "):
                error_lines.append(line)
                error_lines = error_lines[-8:]
            if line.startswith("TRACKSPLIT_PROGRESS"):
                match = re.search(r"(\d+(?:\.\d+)?)%", line)
                if match:
                    percent = float(match.group(1))
                    detail = line.partition("TRACKSPLIT_PROGRESS")[2].strip()
                    progress(f"Завантажую відео… {detail}", max(1, min(98, int(percent))))
                continue
            if line.startswith("TRACKSPLIT_POSTPROCESS"):
                detail = line.partition("TRACKSPLIT_POSTPROCESS")[2].strip()
                progress(f"Обробляю відео… {detail}", 98)
                continue
            match = re.search(r"(\d+(?:\.\d+)?)%", line)
            if match:
                percent = float(match.group(1))
                progress(f"Завантажую відео… {percent:.1f}%", max(1, min(98, int(percent))))
            elif line.startswith(("[youtube]", "[youtube:tab]", "[generic]")):
                phase = "Отримую інформацію про відео…"
                if phase != last_phase:
                    progress(phase, 0)
                    last_phase = phase
            elif line.startswith(("[Merger]", "[VideoRemuxer]", "[Fixup")):
                phase = "Обробляю відео та готую файл…"
                if phase != last_phase:
                    progress(phase, 98)
                    last_phase = phase
            candidate_text = line.strip("\"'")
            if candidate_text and (candidate_text.startswith("/") or re.match(r"^[A-Za-z]:\\", candidate_text)):
                candidate = Path(candidate_text)
                if candidate.is_file():
                    downloaded = candidate
        code = process.wait()
    finally:
        if control:
            if process.poll() is None:
                DownloadController._terminate(process)
                process.wait()
            control.detach(process)
    if control:
        try:
            control.check_cancelled()
        except DownloadCancelled:
            for partial in output_dir.iterdir():
                if partial.name.startswith(f"{stem}.") and partial.name.endswith((".part", ".ytdl")):
                    partial.unlink(missing_ok=True)
            raise
    expected_path = output_dir / f"{stem}.{container}"
    if expected_path.is_file() and expected_path.stat().st_size > 0:
        downloaded = expected_path
    if downloaded is None:
        candidates = [path for path in output_dir.iterdir()
                      if path.is_file() and path.name.startswith(f"{stem}.") and path.suffix != ".part"]
        if candidates:
            downloaded = max(candidates, key=lambda path: path.stat().st_mtime)
    if code or downloaded is None:
        diagnostics = [line for line in output_lines if not line.startswith((
            "TRACKSPLIT_PROGRESS", "TRACKSPLIT_POSTPROCESS", "[Merger] Merging formats into:",
        )) and not (line.startswith("/") or re.match(r"^[A-Za-z]:\\", line))]
        details = "\n".join(error_lines or diagnostics).strip()[-1200:]
        raise RuntimeError(details or f"yt-dlp не завантажив відео (код {code}).")
    if downloaded.name != expected_path.name:
        raise RuntimeError(
            "yt-dlp зберіг окремі аудіо- та відеопотоки замість готового відео. "
            "Перевір наявність ffmpeg і повтори завантаження."
        )
    progress("Відео завантажено.", 100)
    return downloaded


def safe_filename(value: str) -> str:
    value = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "", value).strip().rstrip(".")
    return re.sub(r"\s+", " ", value)[:160] or "track"


def safe_foldername(value: str) -> str:
    # Replace characters forbidden in Windows/macOS/Linux path components with spaces.
    value = re.sub(r'[<>:"/\\|?*\x00-\x1f]', " ", value)
    value = re.sub(r"\s+", " ", value).strip().rstrip(". ")
    value = value[:120].strip().rstrip(". ") or "YouTube відео"
    if value.upper() in {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}:
        value = f"_{value}"
    return value


def timestamp(seconds: float) -> str:
    millis = round(seconds * 1000)
    hours, rest = divmod(millis, 3_600_000)
    minutes, rest = divmod(rest, 60_000)
    secs, ms = divmod(rest, 1000)
    return f"{hours:02}:{minutes:02}:{secs:02}.{ms:03}"


def download_cover(url: str | None, target: Path) -> Path | None:
    if not url:
        return None
    request = urllib.request.Request(url, headers={"User-Agent": "Tracksplit/1.0"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            if not response.headers.get("Content-Type", "").lower().startswith("image/"):
                return None
            with target.open("wb") as output:
                shutil.copyfileobj(response, output)
        return target if target.stat().st_size else None
    except (OSError, ValueError):
        target.unlink(missing_ok=True)
        return None


def split_video(url: str, video: dict, selected: list[int], download_parent: Path, progress,
                jobdir_override: Path | None = None, filename_prefix: str = "",
                control: DownloadController | None = None) -> tuple[Path, list[Path], bool]:
    jobdir = jobdir_override or download_parent / safe_foldername(video["title"])
    jobdir.mkdir(parents=True, exist_ok=True)
    # Keep the downloaded source out of the user's output folder and always
    # remove it, including when yt-dlp or ffmpeg fails partway through.
    with tempfile.TemporaryDirectory(prefix=".tracksplit-", dir=jobdir) as temporary_dir:
        cover_warning = False
        cover_path = Path(temporary_dir) / "video-cover"
        progress("Завантажую обкладинку відео…", 0)
        cover_path = download_cover(video.get("thumbnail"), cover_path)
        if control:
            control.check_cancelled()
        if cover_path is None:
            cover_warning = True
        source_template = str(Path(temporary_dir) / "source.%(ext)s")
        command = [str(YTDLP_PATH), "--no-warnings", "--no-progress", "--no-colors", "--no-playlist", "-f", "bestaudio/best",
                   "-o", source_template, "--print", "after_move:filepath", url]
        progress("Завантажую аудіо з YouTube…", 0)
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                   bufsize=1, **process_group_kwargs())
        if control:
            control.attach(process)
        source = None
        output_lines = []
        try:
            for line in process.stdout or []:
                if control:
                    control.check_cancelled()
                line = line.strip()
                if line:
                    output_lines.append(line)
                    output_lines = output_lines[-12:]
                match = re.search(r"(\d+(?:\.\d+)?)%", line)
                if match:
                    progress(f"Завантажую аудіо… {match.group(1)}%", min(90, int(float(match.group(1)) * .9)))
                candidate_text = line.strip('"\'')
                if candidate_text and (candidate_text.startswith("/") or re.match(r"^[A-Za-z]:\\", candidate_text)):
                    candidate = Path(candidate_text)
                    if candidate.is_file():
                        source = candidate
            code = process.wait()
        finally:
            if control:
                if process.poll() is None:
                    DownloadController._terminate(process)
                    process.wait()
                control.detach(process)
        if control:
            control.check_cancelled()
        if source is None:
            # --print output can differ between yt-dlp releases and extractors;
            # recover the media from the temporary output directory instead.
            candidates = [path for path in Path(temporary_dir).glob("source.*")
                          if path.is_file() and not path.name.endswith((".part", ".ytdl"))]
            if candidates:
                source = max(candidates, key=lambda path: path.stat().st_mtime)
        if source is None:
            details = "\n".join(output_lines).strip()
            if details:
                details = details[-1200:]
                raise RuntimeError(f"yt-dlp не завантажив аудіо (код {code}):\n{details}")
            raise RuntimeError(f"yt-dlp не завантажив аудіо (код {code}). Перевірте доступ до відео та інтернет-з'єднання.")

        by_index = {chapter["index"]: chapter for chapter in video["chapters"]}
        tracks = []
        for order, index in enumerate(selected, 1):
            if control:
                control.check_cancelled()
            chapter = by_index[index]
            chapter_prefix = f"{order:02d} - " if not filename_prefix else ""
            filename = f"{filename_prefix}{chapter_prefix}{safe_filename(chapter['title'])}.mp3"
            target = jobdir / filename
            progress(f"Нарізаю треки… {order}/{len(selected)}", 90 + int(10 * (order - 1) / max(1, len(selected))))
            segment_duration = None
            if chapter["end"] is not None:
                segment_duration = max(0.01, chapter["end"] - chapter["start"])
            # Apply seek and duration to the audio input. Putting -t on the
            # output also counts the one-frame cover stream and can truncate
            # the MP3 to only a fraction of a second.
            command = [str(FFMPEG_PATH), "-hide_banner", "-loglevel", "error", "-y", "-ss",
                       timestamp(chapter["start"])]
            if segment_duration is not None:
                command += ["-t", timestamp(segment_duration)]
            command += ["-i", str(source)]
            if cover_path is not None:
                command += ["-i", str(cover_path)]
            command += ["-map", "0:a:0",
                        "-c:a", "libmp3lame", "-q:a", "2", "-metadata", f"title={chapter['title']}",
                        "-metadata", f"album={video['title']}"]
            if cover_path is not None:
                command += ["-map", "1:v:0", "-c:v", "mjpeg", "-id3v2_version", "3",
                            "-metadata:s:v:0", "title=Album cover", "-metadata:s:v:0", "comment=Cover (front)",
                            "-disposition:v:0", "attached_pic"]
            else:
                command.append("-vn")
            command.append(str(target))
            try:
                result = run_controlled(command, 1800, control) if control else run(command, timeout=1800)
            except DownloadCancelled:
                target.unlink(missing_ok=True)
                raise
            if result.returncode and cover_path is not None:
                # Still produce usable MP3s when a platform's ffmpeg build
                # cannot decode or embed the thumbnail format.
                cover_warning = True
                command = [str(FFMPEG_PATH), "-hide_banner", "-loglevel", "error", "-y", "-ss",
                           timestamp(chapter["start"]), "-i", str(source)]
                if segment_duration is not None:
                    command += ["-t", timestamp(segment_duration)]
                command += ["-vn", "-c:a", "libmp3lame", "-q:a", "2", "-metadata",
                            f"title={chapter['title']}", "-metadata", f"album={video['title']}", str(target)]
                try:
                    result = run_controlled(command, 1800, control) if control else run(command, timeout=1800)
                except DownloadCancelled:
                    target.unlink(missing_ok=True)
                    raise
            if result.returncode:
                raise RuntimeError((result.stderr or "Помилка ffmpeg").strip()[-1200:])
            tracks.append(target)
    progress(f"Готово: {len(tracks)} треків", 100)
    return jobdir, tracks, cover_warning


def draw_round_rect(canvas: tk.Canvas, width: int, height: int, radius: int, fill: str, outline: str = ""):
    radius = min(radius, max(1, width // 2), max(1, height // 2))
    points = [radius, 1, width - radius, 1, width - 1, 1, width - 1, radius,
              width - 1, height - radius, width - 1, height - 1, width - radius, height - 1,
              radius, height - 1, 1, height - 1, 1, height - radius, 1, radius, 1, 1]
    return canvas.create_polygon(points, smooth=True, splinesteps=18, fill=fill, outline=outline,
                                 width=1 if outline else 0, tags="shape")


class RoundedButton(tk.Frame):
    PALETTES = {
        "primary": {"fill": "#c9ed72", "hover": "#d6f68b", "text": "#263218", "disabled": "#343c2b", "disabled_text": "#89917d"},
        "secondary": {"fill": "#2a3025", "hover": "#353e2d", "text": "#e8eddf", "disabled": "#252a21", "disabled_text": "#79806f"},
    }

    def __init__(self, master, text: str, command, *, variant="secondary", height=38, bg="#151911"):
        super().__init__(master, bg=bg, bd=0, highlightthickness=0)
        self.label = text
        self.command = command
        self.variant = variant
        self.button_height = height
        self.enabled = True
        self.hovered = False
        self.font = tkfont.Font(family="TkDefaultFont", size=10, weight="bold" if variant == "primary" else "normal")
        self.button_width = self.font.measure(text) + (36 if variant == "primary" else 30)
        self.canvas = tk.Canvas(self, width=self.button_width, height=height, bg=bg,
                                bd=0, highlightthickness=0, takefocus=1)
        self.canvas.pack(fill="both", expand=True)
        self.canvas.bind("<Configure>", self._draw)
        self.canvas.bind("<Enter>", self._on_enter)
        self.canvas.bind("<Leave>", self._on_leave)
        self.canvas.bind("<Button-1>", self._on_click)
        self.canvas.bind("<ButtonRelease-1>", self._on_release)
        self.canvas.bind("<Return>", self._on_key)
        self.canvas.bind("<space>", self._on_key)
        self.canvas.bind("<FocusIn>", self._draw)
        self.canvas.bind("<FocusOut>", self._draw)
        self._draw()

    def _draw(self, _event=None):
        if not hasattr(self, "canvas"):
            return
        width = max(self.button_width, self.canvas.winfo_width())
        height = max(self.button_height, self.canvas.winfo_height())
        palette = self.PALETTES[self.variant]
        fill = palette["disabled"] if not self.enabled else palette["hover"] if self.hovered else palette["fill"]
        text_color = palette["disabled_text"] if not self.enabled else palette["text"]
        self.canvas.delete("all")
        draw_round_rect(self.canvas, width, height, 14, fill)
        try:
            has_focus = self.canvas.focus_displayof() is self.canvas
        except (KeyError, tk.TclError):
            # Native dialogs can temporarily expose Tcl-only widgets (for
            # example __tk_choosedir) that Tkinter cannot resolve to a Python
            # widget while mapping focus_displayof().
            has_focus = False
        if has_focus:
            draw_round_rect(self.canvas, width, height, 14, "", getattr(self, "FOCUS_COLOR", "#8fae52"))
        self.canvas.create_text(width / 2, height / 2, text=self.label, fill=text_color,
                                font=self.font, tags="button-text")
        self.canvas.configure(cursor="hand2" if self.enabled else "arrow")

    def _on_enter(self, _event=None):
        self.hovered = True
        self._draw()

    def _on_leave(self, _event=None):
        self.hovered = False
        self._draw()

    def _on_click(self, _event=None):
        if self.enabled:
            self.canvas.focus_set()

    def _on_release(self, event):
        if self.enabled and 0 <= event.x <= self.canvas.winfo_width() and 0 <= event.y <= self.canvas.winfo_height():
            self.command()

    def _on_key(self, _event=None):
        if self.enabled:
            self.command()
        return "break"

    def set_state(self, state: str):
        self.enabled = state != "disabled"
        self._draw()

    def set_text(self, text: str):
        self.label = text
        self.button_width = self.font.measure(text) + (36 if self.variant == "primary" else 30)
        self.canvas.configure(width=self.button_width)
        self._draw()

    def set_variant(self, variant: str):
        self.variant = variant
        self.font.configure(weight="bold" if variant == "primary" else "normal")
        self.set_text(self.label)


class RoundedEntry(tk.Frame):
    def __init__(self, master, textvariable: tk.StringVar, *, bg="#20261b", field="#181d14", fg="#edf1e6"):
        super().__init__(master, bg=bg, bd=0, highlightthickness=0, height=46)
        self.pack_propagate(False)
        self.field = field
        self.border = "#3a4430"
        self.default_border = self.border
        self.focus_color = "#90b64c"
        self.text_color = fg
        self.selection_color = "#506b32"
        self.canvas = tk.Canvas(self, height=46, bg=bg, bd=0, highlightthickness=0)
        self.canvas.pack(fill="both", expand=True)
        self.canvas.bind("<Configure>", self._draw)
        self.entry = tk.Entry(self.canvas, textvariable=textvariable, relief="flat", bd=0, highlightthickness=0,
                              bg=field, fg=fg, insertbackground=fg, selectbackground=self.selection_color,
                              selectforeground="#ffffff", font=("TkDefaultFont", 11))
        self.entry.place(x=13, y=9, relwidth=1, width=-26, height=28)
        self.entry.bind("<FocusIn>", self._focus_in)
        self.entry.bind("<FocusOut>", self._focus_out)
        self._draw()

    def _draw(self, _event=None):
        if hasattr(self, "canvas"):
            width = max(self.canvas.winfo_width(), 200)
            self.canvas.delete("all")
            draw_round_rect(self.canvas, width, 46, 14, self.field, self.border)

    def _focus_in(self, _event=None):
        self.border = self.focus_color
        self._draw()

    def _focus_out(self, _event=None):
        self.border = self.default_border
        self._draw()


class AnimatedProgressBar(tk.Canvas):
    def __init__(self, master, *, bg, trough, fill, height=12):
        super().__init__(master, height=height, bg=bg, bd=0, highlightthickness=0)
        self.trough = trough
        self.fill = fill
        self.current = 0.0
        self.target = 0.0
        self.height = height
        self.indeterminate = False
        self.pulse_offset = 0
        self.bind("<Configure>", self._draw)

    def set_theme(self, *, trough, fill):
        self.trough, self.fill = trough, fill
        self._draw()

    def set_value(self, value: int):
        self.indeterminate = False
        self.target = max(0.0, min(100.0, float(value)))
        if not getattr(self, "animating", False):
            self.animating = True
            self.after(16, self._animate)

    def start_indeterminate(self):
        if self.indeterminate:
            return
        self.indeterminate = True
        self.target = self.current = 0.0
        self._animate_pulse()

    def _animate_pulse(self):
        if not self.indeterminate:
            return
        width = max(1, self.winfo_width())
        chunk = max(32, width // 5)
        self.pulse_offset = (self.pulse_offset + max(6, width // 55)) % (width + chunk)
        self._draw()
        self.after(24, self._animate_pulse)

    def _animate(self):
        difference = self.target - self.current
        self.current = self.target if abs(difference) < 0.6 else self.current + difference * 0.22
        self._draw()
        if abs(self.target - self.current) >= 0.6:
            self.after(16, self._animate)
        else:
            self.current = self.target
            self.animating = False
            self._draw()

    def _draw(self, _event=None):
        width = max(1, self.winfo_width())
        height = self.height
        self.delete("all")
        draw_round_rect(self, width, height, height // 2, self.trough)
        if self.indeterminate:
            chunk = max(32, width // 5)
            left = self.pulse_offset - chunk
            right = self.pulse_offset
            self.create_rectangle(max(0, left), 1, min(width, right), height - 1,
                                  fill=self.fill, outline="")
            return
        fill_width = max(0, int(width * self.current / 100))
        if fill_width:
            draw_round_rect(self, fill_width, height, min(height // 2, fill_width // 2), self.fill)


class RoundedCard(tk.Frame):
    def __init__(self, master, *, bg, fill, padding=14, radius=18, expand_content=False):
        super().__init__(master, bg=bg, bd=0, highlightthickness=0)
        self.bg_color = bg
        self.fill = fill
        self.padding = padding
        self.radius = radius
        self.expand_content = expand_content
        self.canvas = tk.Canvas(self, bg=bg, bd=0, highlightthickness=0)
        self.canvas.pack(fill="both", expand=True)
        self.content = tk.Frame(self.canvas, bg=fill)
        self.window = self.canvas.create_window(padding, padding, anchor="nw", window=self.content)
        self.canvas.bind("<Configure>", self._resize)
        if not expand_content:
            self.after(50, self._fit_height)

    def _fit_height(self):
        if self.winfo_exists():
            self.update_idletasks()
            self.canvas.configure(height=self.content.winfo_reqheight() + self.padding * 2)

    def _resize(self, event):
        inner_width = max(1, event.width - self.padding * 2)
        inner_height = max(1, event.height - self.padding * 2)
        self.canvas.coords(self.window, self.padding, self.padding)
        if self.expand_content:
            self.canvas.itemconfigure(self.window, width=inner_width, height=inner_height)
        else:
            self.canvas.itemconfigure(self.window, width=inner_width)
        self.canvas.delete("shape")
        draw_round_rect(self.canvas, event.width, event.height, self.radius, self.fill)
        self.canvas.tag_lower("shape")
        self.canvas.itemconfigure(self.window, tags="card-content")

    def set_theme(self, *, bg, fill):
        self.bg_color, self.fill = bg, fill
        self.configure(bg=bg)
        self.canvas.configure(bg=bg)
        self.content.configure(bg=fill)
        self._resize(type("Size", (), {"width": self.canvas.winfo_width(), "height": self.canvas.winfo_height()})())


class TrackSplitApp:
    def __init__(self, root: tk.Tk):
        self.root = root
        self.video: dict | None = None
        self.busy = False
        self.download_parent = self._load_download_parent()
        self.theme_name = self._load_theme()
        preferences = self._load_preferences()
        self.default_video_format = preferences.get("default_video_format", "mp4")
        self.default_video_resolution = preferences.get("default_video_resolution", "best")
        self.default_playlist_output = preferences.get("default_playlist_output", "mp3")
        self.applied_theme = "olive"
        self.last_output_dir: Path | None = None
        self.download_controller: DownloadController | None = None
        self.root.title("Tracksplit")
        self.root.geometry("1180x820")
        self.root.minsize(980, 680)
        self.root.configure(bg="#151911")
        self.style = ttk.Style()
        try:
            self.style.theme_use("clam")
        except tk.TclError:
            pass
        self.style.configure("TFrame", background="#151911")
        self.style.configure("Treeview", rowheight=36, font=("TkDefaultFont", 10),
                             background="#20261b", fieldbackground="#20261b", foreground="#dce3d2",
                             borderwidth=0, relief="flat", bordercolor="#20261b",
                             lightcolor="#20261b", darkcolor="#20261b")
        self.style.map("Treeview", background=[("selected", "#3c4d2d")],
                       foreground=[("selected", "#f2f7e9")])
        self.style.configure("Treeview.Heading", font=("TkDefaultFont", 9, "bold"),
                             background="#282f23", foreground="#a5ae98", borderwidth=0,
                             bordercolor="#20261b", lightcolor="#282f23", darkcolor="#282f23", padding=(12, 10))
        self.style.map("Treeview.Heading", background=[("active", "#333b2d")])
        self.style.configure("TScrollbar", background="#343d2d", troughcolor="#20261b", borderwidth=0,
                             bordercolor="#20261b", arrowcolor="#a1aa95", lightcolor="#343d2d",
                             darkcolor="#343d2d", arrowsize=12)
        self.style.configure("Horizontal.TProgressbar", troughcolor="#2a3124", background="#a9d65d",
                             bordercolor="#2a3124", lightcolor="#a9d65d", darkcolor="#a9d65d", thickness=6)
        self.style.configure("TCombobox", fieldbackground="#181d14", background="#2a3025",
                             foreground="#f0f3e9", arrowcolor="#a1aa95", bordercolor="#343d2d",
                             lightcolor="#343d2d", darkcolor="#343d2d", padding=8)
        self.style.map("TCombobox", fieldbackground=[("readonly", "#181d14")],
                       foreground=[("readonly", "#f0f3e9")], selectbackground=[("readonly", "#3c4d2d")])
        self._build_ui()
        self._apply_theme(self.theme_name, save=False)
        self.root.after(100, self._bootstrap_tools)

    def _build_ui(self):
        bg, card, ink, muted, accent = "#151911", "#20261b", "#f0f3e9", "#a1aa95", "#c9ed72"
        shell = tk.Frame(self.root, bg=bg)
        shell.pack(fill="both", expand=True)

        self.sidebar = tk.Frame(shell, bg=card, width=210, padx=16, pady=20)
        self.sidebar.pack(side="left", fill="y")
        self.sidebar.pack_propagate(False)
        brand = tk.Frame(self.sidebar, bg=card)
        brand.pack(fill="x", pady=(4, 30))
        tk.Label(brand, text="♫", bg=accent, fg="#263218", width=2, height=1,
                 font=("TkDefaultFont", 16, "bold")).pack(side="left", padx=(0, 10), ipady=3)
        tk.Label(brand, text="TRACKSPLIT", bg=card, fg=ink,
                 font=("TkDefaultFont", 12, "bold")).pack(side="left")

        tk.Label(self.sidebar, text="РОБОЧІ РЕЖИМИ", bg=card, fg=muted,
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", padx=8, pady=(0, 8))
        self.nav_buttons = {}
        for key, label in (
            ("music", "♫   Музика по розділах"),
            ("playlist", "▤   Плейлист"),
            ("video", "▶   Відео"),
            ("options", "⚙   Опції"),
        ):
            button = RoundedButton(self.sidebar, text=label, command=lambda page=key: self._show_page(page),
                                   variant="secondary", height=42, bg=card)
            button.pack(fill="x", pady=4)
            self.nav_buttons[key] = button
        tk.Label(self.sidebar, text="yt-dlp  ·  ffmpeg", bg=card, fg=muted,
                 font=("TkDefaultFont", 9)).pack(side="bottom", anchor="w", padx=8, pady=8)

        self.workspace = tk.Frame(shell, bg=bg, padx=28, pady=14)
        self.workspace.pack(side="left", fill="both", expand=True)
        topbar = tk.Frame(self.workspace, bg=bg)
        topbar.pack(fill="x", pady=(0, 18))
        self.page_title = tk.Label(topbar, text="Музика по розділах", bg=bg, fg=ink,
                                   font=("TkDefaultFont", 16, "bold"), anchor="w")
        self.page_title.pack(side="left")
        self.update_button = RoundedButton(topbar, text="Оновити інструменти", command=self._update_tools,
                                           variant="secondary", height=36, bg=bg)
        self.update_button.pack(side="right")
        self.theme_button = RoundedButton(topbar, text="Тема: Оливкова", command=self._toggle_theme,
                                          variant="secondary", height=36, bg=bg)
        self.theme_button.pack(side="right", padx=(0, 9))

        self.page_host = tk.Frame(self.workspace, bg=bg)
        self.page_host.pack(fill="both", expand=True)
        self.pages = {}
        for key, builder in (
            ("music", self._build_music_page),
            ("playlist", self._build_playlist_page),
            ("video", self._build_video_page),
            ("options", self._build_options_page),
        ):
            page = tk.Frame(self.page_host, bg=bg)
            self.pages[key] = page
            builder(page)

        footer = tk.Frame(self.workspace, bg=bg)
        footer.pack(side="bottom", fill="x", pady=(6, 0))
        destination = RoundedCard(footer, bg=bg, fill=card, padding=9, radius=18)
        destination.pack(fill="x")
        destination_row = tk.Frame(destination.content, bg=card)
        destination_row.pack(fill="x")
        tk.Label(destination_row, text="ЗБЕРІГАТИ В", bg=card, fg=muted,
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 14))
        self.destination_label = tk.Label(destination_row, text=str(self.download_parent), bg=card, fg="#b5beaa",
                                          font=("TkDefaultFont", 9), anchor="w")
        self.destination_label.pack(side="left", fill="x", expand=True)
        self.choose_folder_button = RoundedButton(destination_row, text="Вибрати теку…",
                                                  command=self._choose_download_parent,
                                                  variant="secondary", height=34, bg=card)
        self.choose_folder_button.pack(side="right", padx=(8, 0))
        self.open_folder_button = RoundedButton(destination_row, text="Відкрити",
                                                command=self._open_downloads,
                                                variant="secondary", height=34, bg=card)
        self.open_folder_button.pack(side="right")

        self.status = tk.StringVar(value="Перевіряю інструменти застосунку…")
        self.status_label = tk.Label(footer, textvariable=self.status, bg=bg, fg=muted,
                                     font=("TkDefaultFont", 9), anchor="w", justify="left", wraplength=900)
        self.status_label.pack(anchor="w", fill="x", pady=(10, 5))
        self.progress = AnimatedProgressBar(footer, bg=bg, trough="#2a3124", fill="#a9d65d", height=9)
        self.progress.pack(fill="x", pady=(0, 2))

        self.download_controls = tk.Frame(footer, bg=bg)
        self.pause_download_button = RoundedButton(
            self.download_controls, text="Пауза", command=self._toggle_download_pause,
            variant="secondary", height=30, bg=bg,
        )
        self.pause_download_button.pack(side="right", padx=(8, 0))
        self.cancel_download_button = RoundedButton(
            self.download_controls, text="Скасувати", command=self._cancel_download,
            variant="secondary", height=30, bg=bg,
        )
        self.cancel_download_button.pack(side="right")
        self.download_controls.pack(fill="x", pady=(4, 0))
        self.download_controls.pack_forget()
        self._show_page("music")

    def _page_intro(self, parent, title, description):
        tk.Label(parent, text=title, bg="#151911", fg="#f0f3e9",
                 font=("TkDefaultFont", 23, "bold")).pack(anchor="w")
        tk.Label(parent, text=description, bg="#151911", fg="#a1aa95",
                 font=("TkDefaultFont", 10), wraplength=880, justify="left").pack(anchor="w", pady=(5, 16))

    def _source_card(self, parent, variable, button_text, command):
        card = RoundedCard(parent, bg="#151911", fill="#20261b", padding=18, radius=20)
        card.pack(fill="x")
        tk.Label(card.content, text="ПОСИЛАННЯ НА YOUTUBE", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 9, "bold")).pack(anchor="w", pady=(0, 9))
        row = tk.Frame(card.content, bg="#20261b")
        row.pack(fill="x")
        entry = RoundedEntry(row, textvariable=variable, bg="#20261b", field="#181d14", fg="#f0f3e9")
        entry.pack(side="left", fill="x", expand=True, padx=(0, 10))
        button = RoundedButton(row, text=button_text, command=command, variant="primary", height=44, bg="#20261b")
        button.pack(side="right")
        entry.entry.bind("<Return>", lambda _event: command())
        return card, entry, button

    def _make_table(self, parent, columns, headings, widths):
        card = RoundedCard(parent, bg="#151911", fill="#20261b", padding=9, radius=20, expand_content=True)
        card.pack(fill="both", expand=True)
        tree = ttk.Treeview(card.content, columns=columns, show="headings", selectmode="extended")
        for column, heading, width in zip(columns, headings, widths):
            tree.heading(column, text=heading)
            tree.column(column, anchor="w" if column not in ("start", "duration") else "e",
                        width=width, minwidth=100 if column in ("title", "video") else width,
                        stretch=column in ("title", "video"))
        scroll = ttk.Scrollbar(card.content, orient="vertical", command=tree.yview)
        tree.configure(yscrollcommand=scroll.set)
        tree.pack(side="left", fill="both", expand=True, padx=(1, 0), pady=1)
        scroll.pack(side="right", fill="y")
        return tree

    def _build_music_page(self, page):
        self._page_intro(page, "Музика по розділах",
                         "Встав посилання на відео з розділами — отримай окремі MP3. Якщо розділів немає, відео стане одним треком.")
        self.url = tk.StringVar(value="")
        self.url.trace_add("write", self._music_url_changed)
        _, self.url_entry, self.find_button = self._source_card(page, self.url, "Знайти розділи  →",
                                                                self._find_chapters)
        row = tk.Frame(page, bg="#151911")
        row.pack(fill="x", pady=(17, 8))
        self.video_label = tk.Label(row, text="Треки з відео з’являться тут", bg="#151911", fg="#f0f3e9",
                                    font=("TkDefaultFont", 12, "bold"), anchor="w")
        self.video_label.pack(side="left", fill="x", expand=True)
        self.chapter_count = tk.Label(row, text="РОЗДІЛИ", bg="#151911", fg="#a1aa95",
                                      font=("TkDefaultFont", 9, "bold"))
        self.chapter_count.pack(side="right", padx=(12, 14))
        self.select_button = RoundedButton(row, text="Зняти вибір", command=self._toggle_selection,
                                           variant="secondary", height=34, bg="#151911")
        self.select_button.set_state("disabled")
        self.select_button.pack(side="right")
        self.tree = self._make_table(page, ("title", "start"), ("НАЗВА ТРЕКУ", "ПОЧАТОК"), (650, 100))
        self.download_button = RoundedButton(page, text="Завантажити вибрані MP3",
                                             command=self._download, variant="primary", height=42,
                                             bg="#151911")
        self.download_button.set_state("disabled")
        self.download_button.pack(anchor="w", pady=(12, 0))

    def _build_playlist_page(self, page):
        self._page_intro(page, "Завантажити плейлист",
                         "Вибери відео з плейлиста й завантаж їх як окремі MP3 або відеофайли.")
        self.playlist_url = tk.StringVar(value="")
        self.playlist = None
        self.playlist_url.trace_add("write", self._playlist_url_changed)
        _, self.playlist_url_entry, self.playlist_find_button = self._source_card(
            page, self.playlist_url, "Знайти відео  →", self._find_playlist)
        self.playlist_controls_card = RoundedCard(page, bg="#151911", fill="#20261b", padding=14, radius=18)
        self.playlist_controls_card.pack(fill="x", pady=(12, 0))
        line = tk.Frame(self.playlist_controls_card.content, bg="#20261b")
        line.pack(fill="x")
        tk.Label(line, text="ФОРМАТ ЗАВАНТАЖЕННЯ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 12))
        self.playlist_output = tk.StringVar(value=self.default_playlist_output)
        self.playlist_mp3_button = RoundedButton(line, text="MP3 · 1 файл на відео",
                                                 command=lambda: self._set_playlist_output("mp3", persist=False),
                                                 variant="primary", height=36, bg="#20261b")
        self.playlist_mp3_button.pack(side="left", padx=(0, 7))
        self.playlist_video_button = RoundedButton(line, text="Відео",
                                                   command=lambda: self._set_playlist_output("video", persist=False),
                                                   variant="secondary", height=36, bg="#20261b")
        self.playlist_video_button.pack(side="left")
        self.playlist_video_options = tk.Frame(self.playlist_controls_card.content, bg="#20261b")
        self.playlist_video_options.pack(fill="x", pady=(12, 0))
        tk.Label(self.playlist_video_options, text="ЯКІСТЬ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 8))
        self.playlist_resolution = ttk.Combobox(self.playlist_video_options, state="readonly", width=20,
                                                values=("Автоматично", "2160p", "1440p", "1080p", "720p", "480p", "360p"))
        self.playlist_resolution.pack(side="left", padx=(0, 18))
        tk.Label(self.playlist_video_options, text="ФОРМАТ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 8))
        self.playlist_format = ttk.Combobox(self.playlist_video_options, state="readonly", width=12,
                                            values=("MP4", "MKV", "WebM"))
        self.playlist_format.pack(side="left")
        self.playlist_resolution.set("Автоматично" if self.default_video_resolution == "best" else self.default_video_resolution)
        self.playlist_format.set(self._video_format_label(self.default_video_format))
        self._set_playlist_output(self.playlist_output.get(), persist=False)

        playlist_head = tk.Frame(page, bg="#151911")
        playlist_head.pack(fill="x", pady=(16, 8))
        self.playlist_title_label = tk.Label(playlist_head, text="Відео плейлиста з’являться тут",
                                             bg="#151911", fg="#f0f3e9", font=("TkDefaultFont", 12, "bold"),
                                             anchor="w")
        self.playlist_title_label.pack(side="left", fill="x", expand=True)
        self.playlist_count_label = tk.Label(playlist_head, text="", bg="#151911", fg="#a1aa95",
                                             font=("TkDefaultFont", 9, "bold"))
        self.playlist_count_label.pack(side="right", padx=(12, 14))
        self.playlist_select_button = RoundedButton(playlist_head, text="Зняти вибір",
                                                    command=self._toggle_playlist_selection,
                                                    variant="secondary", height=34, bg="#151911")
        self.playlist_select_button.set_state("disabled")
        self.playlist_select_button.pack(side="right")
        self.playlist_tree = self._make_table(page, ("video", "duration"),
                                              ("ВІДЕО", "ТРИВАЛІСТЬ"), (650, 110))
        self.playlist_download_button = RoundedButton(page, text="Завантажити вибрані",
                                                      command=self._download_playlist,
                                                      variant="primary", height=42, bg="#151911")
        self.playlist_download_button.set_state("disabled")
        self.playlist_download_button.pack(anchor="w", pady=(12, 0))

    def _build_video_page(self, page):
        self._page_intro(page, "Завантажити відео",
                         "Збережи окреме відео у вибраній якості та стандартному контейнері.")
        self.video_url = tk.StringVar(value="")
        self.video_info = None
        self.video_url.trace_add("write", self._video_url_changed)
        _, self.video_url_entry, self.video_check_button = self._source_card(
            page, self.video_url, "Перевірити посилання", self._prepare_video)
        settings = RoundedCard(page, bg="#151911", fill="#20261b", padding=18, radius=20)
        settings.pack(fill="x", pady=(14, 0))
        self.video_resolution, self.video_format = self._build_video_options(settings.content, "ВІДЕО")
        self.video_download_button = RoundedButton(page, text="Завантажити відео",
                                                   command=self._download_video,
                                                   variant="primary", height=42, bg="#151911")
        self.video_download_button.set_state("disabled")
        self.video_download_button.pack(anchor="w", pady=(14, 0))

    def _build_video_options(self, parent, prefix):
        tk.Label(parent, text=f"{prefix} · ЯКІСТЬ І ФОРМАТ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", pady=(0, 10))
        row = tk.Frame(parent, bg="#20261b")
        row.pack(fill="x")
        tk.Label(row, text="РОЗДІЛЬНІСТЬ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 8))
        resolution = ttk.Combobox(row, state="readonly", width=20,
                                  values=("Автоматично", "2160p", "1440p", "1080p", "720p", "480p", "360p"))
        resolution.pack(side="left", padx=(0, 20))
        resolution.set("Автоматично" if self.default_video_resolution == "best" else self.default_video_resolution)
        tk.Label(row, text="ФОРМАТ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(side="left", padx=(0, 8))
        video_format = ttk.Combobox(row, state="readonly", width=12, values=("MP4", "MKV", "WebM"))
        video_format.pack(side="left")
        video_format.set(self._video_format_label(self.default_video_format))
        return resolution, video_format

    @staticmethod
    def _video_format_label(value):
        return {"mp4": "MP4", "mkv": "MKV", "webm": "WebM"}.get(str(value).lower(), "MP4")

    def _build_options_page(self, page):
        self._page_intro(page, "Опції", "Задай теки та формати, які Tracksplit використовуватиме за замовчуванням.")
        folder_card = RoundedCard(page, bg="#151911", fill="#20261b", padding=18, radius=20)
        folder_card.pack(fill="x")
        tk.Label(folder_card.content, text="ТЕКА ДЛЯ ЗАВАНТАЖЕНЬ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", pady=(0, 9))
        folder_row = tk.Frame(folder_card.content, bg="#20261b")
        folder_row.pack(fill="x")
        self.options_folder_label = tk.Label(folder_row, text=str(self.download_parent), bg="#20261b",
                                             fg="#f0f3e9", font=("TkDefaultFont", 10), anchor="w")
        self.options_folder_label.pack(side="left", fill="x", expand=True)
        RoundedButton(folder_row, text="Вибрати теку…", command=self._choose_download_parent,
                      variant="secondary", height=36, bg="#20261b").pack(side="right")

        defaults = RoundedCard(page, bg="#151911", fill="#20261b", padding=18, radius=20)
        defaults.pack(fill="x", pady=(14, 0))
        tk.Label(defaults.content, text="ЗАВАНТАЖЕННЯ ВІДЕО", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", pady=(0, 10))
        self.options_resolution, self.options_format = self._build_video_options(defaults.content, "ЗА ЗАМОВЧУВАННЯМ")
        tk.Label(defaults.content, text="ФОРМАТ ДЛЯ ПЛЕЙЛИСТІВ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", pady=(18, 8))
        self.options_playlist_output = ttk.Combobox(defaults.content, state="readonly", width=25,
                                                    values=("MP3 · один файл на відео", "Відео"))
        self.options_playlist_output.pack(anchor="w")
        self.options_playlist_output.set("Відео" if self.default_playlist_output == "video" else "MP3 · один файл на відео")
        tk.Label(defaults.content, text="ТЕМА ЗАСТОСУНКУ", bg="#20261b", fg="#a1aa95",
                 font=("TkDefaultFont", 8, "bold")).pack(anchor="w", pady=(18, 8))
        self.options_theme = ttk.Combobox(defaults.content, state="readonly", width=25,
                                          values=("Оливкова", "Фіолетова"))
        self.options_theme.pack(anchor="w")
        self.options_theme.set("Фіолетова" if self.theme_name == "purple" else "Оливкова")
        self.options_theme.bind("<<ComboboxSelected>>", self._change_theme_from_options)
        self.options_save_button = RoundedButton(page, text="Зберегти опції", command=self._save_preferences_from_ui,
                                                 variant="primary", height=42, bg="#151911")
        self.options_save_button.pack(anchor="w", pady=(14, 0))
    def _show_page(self, page_key):
        if page_key not in self.pages:
            return
        for page in self.pages.values():
            page.pack_forget()
        self.pages[page_key].pack(fill="both", expand=True)
        titles = {"music": "Музика по розділах", "playlist": "Плейлист",
                  "video": "Відео", "options": "Опції"}
        self.page_title.configure(text=titles[page_key])
        for key, button in self.nav_buttons.items():
            button.set_variant("primary" if key == page_key else "secondary")
        self.active_page = page_key

    def _music_url_changed(self, *_):
        self.video = None
        self.video_label.configure(text="Треки з відео з’являться тут")
        self.chapter_count.configure(text="РОЗДІЛИ")
        for item in self.tree.get_children():
            self.tree.delete(item)
        self.select_button.set_state("disabled")
        self.download_button.set_state("disabled")

    def _playlist_url_changed(self, *_):
        self.playlist = None
        for item in self.playlist_tree.get_children():
            self.playlist_tree.delete(item)
        self.playlist_title_label.configure(text="Відео плейлиста з’являться тут")
        self.playlist_count_label.configure(text="")
        self.playlist_select_button.set_state("disabled")
        self.playlist_download_button.set_state("disabled")

    def _video_url_changed(self, *_):
        self.video_info = None
        self.video_download_button.set_state("disabled")

    def _set_playlist_output(self, output, persist=True):
        if output not in ("mp3", "video"):
            output = "mp3"
        self.playlist_output.set(output)
        self.playlist_mp3_button.set_variant("primary" if output == "mp3" else "secondary")
        self.playlist_video_button.set_variant("primary" if output == "video" else "secondary")
        if output == "video":
            self.playlist_video_options.pack(fill="x", pady=(12, 0))
        else:
            self.playlist_video_options.pack_forget()
        self.playlist_controls_card.after_idle(self.playlist_controls_card._fit_height)
        if persist:
            self.default_playlist_output = output
            self._save_settings()

    def _change_theme_from_options(self, _event=None):
        self._apply_theme("purple" if self.options_theme.get() == "Фіолетова" else "olive")

    def _toggle_playlist_selection(self):
        children = self.playlist_tree.get_children()
        if len(self.playlist_tree.selection()) == len(children):
            self.playlist_tree.selection_remove(*children)
            self.playlist_select_button.set_text("Вибрати всі")
        else:
            self.playlist_tree.selection_set(children)
            self.playlist_select_button.set_text("Зняти вибір")

    @staticmethod
    def _duration_text(duration):
        if not isinstance(duration, (int, float)) or duration < 0:
            return "—"
        seconds = int(duration)
        hours, remainder = divmod(seconds, 3600)
        minutes, seconds = divmod(remainder, 60)
        return f"{hours:02}:{minutes:02}:{seconds:02}" if hours else f"{minutes:02}:{seconds:02}"

    def _find_playlist(self):
        url = self.playlist_url.get().strip()
        if not valid_youtube_url(url):
            self._show_dialog("Перевір посилання", "Встав коректне HTTPS-посилання на YouTube плейлист.", "warning")
            return
        if self.busy:
            return
        self.busy = True
        self.playlist_find_button.set_state("disabled")
        self.playlist_download_button.set_state("disabled")
        self._set_status("Читаю плейлист…", 0)
        def worker():
            try:
                playlist = playlist_info_for(url)
                self.root.after(0, lambda: self._playlist_ready(playlist, url))
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._playlist_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _playlist_ready(self, playlist, url):
        self.busy = False
        if self.playlist_url.get().strip() != url:
            self.playlist_find_button.set_state("normal")
            return
        self.playlist = playlist
        self.playlist_url_loaded = url
        self.playlist_find_button.set_state("normal")
        for item in self.playlist_tree.get_children():
            self.playlist_tree.delete(item)
        for entry in playlist["entries"]:
            self.playlist_tree.insert("", "end", iid=str(entry["index"]),
                                      values=(entry["title"], self._duration_text(entry.get("duration"))))
        children = self.playlist_tree.get_children()
        self.playlist_tree.selection_set(children)
        self.playlist_title_label.configure(text=playlist["title"])
        self.playlist_count_label.configure(text=f"{len(children)} ВІДЕО")
        self.playlist_select_button.set_state("normal")
        self.playlist_select_button.set_text("Зняти вибір")
        self.playlist_download_button.set_state("normal")
        self._set_status(f"У плейлисті {len(children)} відео. Вибери потрібні й завантаж.", 0)

    def _playlist_failed(self, error):
        self.busy = False
        self.playlist_find_button.set_state("normal")
        self._set_status("Не вдалося прочитати плейлист.", 0)
        self._show_dialog("Не вдалося прочитати плейлист", error, "error")

    def _download_playlist(self):
        if (not getattr(self, "playlist", None) or self.busy
                or getattr(self, "playlist_url_loaded", None) != self.playlist_url.get().strip()):
            return
        selected_ids = sorted(int(item) for item in self.playlist_tree.selection())
        if not selected_ids:
            self._show_dialog("Не вибрано відео", "Вибери хоча б одне відео з плейлиста.", "warning")
            return
        entries = [self.playlist["entries"][index] for index in selected_ids]
        output_kind = self.playlist_output.get()
        resolution = self.playlist_resolution.get() if output_kind == "video" else "Автоматично"
        video_format = self.playlist_format.get().lower() if output_kind == "video" else "mp4"
        playlist = dict(self.playlist)
        parent = self.download_parent
        self.busy = True
        self.playlist_find_button.set_state("disabled")
        self.playlist_download_button.set_state("disabled")
        control = self._begin_download_controls()
        self._set_status("Готую завантаження плейлиста…", 0)
        def worker():
            try:
                result = self._playlist_download_worker(
                    playlist, entries, output_kind, resolution, video_format, parent, control,
                )
                self.root.after(0, lambda: self._playlist_download_done(result))
            except DownloadCancelled:
                self.root.after(0, self._download_cancelled)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._playlist_download_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _playlist_download_worker(self, playlist, entries, output_kind, resolution, video_format, parent, control):
        output_dir = parent / safe_foldername(playlist["title"])
        output_dir.mkdir(parents=True, exist_ok=True)
        completed, failures, cover_warning = [], [], False
        total = len(entries)
        for position, entry in enumerate(entries, 1):
            control.check_cancelled()
            def item_progress(text, value, position=position, title=entry["title"]):
                fraction = max(0, min(value, 100)) / 100
                overall = int(((position - 1) + fraction) * 100 / total)
                self._thread_progress(f"{position}/{total} · {title} — {text}", overall)
            try:
                if output_kind == "mp3":
                    details = info_for(entry["url"], control)
                    single_track = {
                        "title": details["title"], "thumbnail": details.get("thumbnail"),
                        "chapters": [{"index": 0, "title": details["title"], "start": 0,
                                      "end": details.get("duration")}],
                    }
                    _, tracks, missing_cover = split_video(
                        entry["url"], single_track, [0], output_dir, item_progress,
                        jobdir_override=output_dir, filename_prefix=f"{position:02d} - ",
                        control=control,
                    )
                    completed.extend(tracks)
                    cover_warning = cover_warning or missing_cover
                else:
                    target = download_video_file(entry["url"], entry["title"], output_dir, item_progress,
                                                 resolution=resolution, container=video_format,
                                                 prefix=f"{position:02d} - ", control=control)
                    completed.append(target)
            except DownloadCancelled:
                raise
            except Exception as exc:
                failures.append(f"{entry['title']}: {str(exc).strip()[-300:]}")
        if not completed and failures:
            raise RuntimeError("Не вдалося завантажити жодне відео.\n\n" + "\n".join(failures[:5]))
        return {"output_dir": output_dir, "files": completed, "failures": failures,
                "cover_warning": cover_warning, "kind": output_kind}

    def _playlist_download_done(self, result):
        self.busy = False
        self._finish_download_controls()
        self.last_output_dir = result["output_dir"]
        self.playlist_find_button.set_state("normal")
        self.playlist_download_button.set_state("normal")
        count = len(result["files"])
        failed = len(result["failures"])
        label = "MP3-файлів" if result["kind"] == "mp3" else "відео"
        summary = f"Збережено {count} {label}."
        if failed:
            summary += f" Не вдалося завантажити: {failed}.\n\n" + "\n".join(result["failures"][:5])
        if result["cover_warning"]:
            summary += "\n\nДля частини MP3 не вдалося додати обкладинку."
        self._set_status(f"{count} {label} збережено. Тека: {result['output_dir']}", 100)
        kind = "warning" if failed or result["cover_warning"] else "success"
        self._show_dialog("Завантаження плейлиста завершено", f"{summary}\n\nТека: {result['output_dir']}", kind)

    def _playlist_download_failed(self, error):
        self.busy = False
        self._finish_download_controls()
        self.playlist_find_button.set_state("normal")
        self.playlist_download_button.set_state("normal")
        self._set_status("Не вдалося завантажити плейлист.", 0)
        self._show_dialog("Помилка завантаження плейлиста", error, "error")

    def _prepare_video(self):
        url = self.video_url.get().strip()
        if not valid_youtube_url(url):
            self._show_dialog("Перевір посилання", "Встав коректне HTTPS-посилання на YouTube відео.", "warning")
            return
        if self.busy:
            return
        self.busy = True
        self.video_download_button.set_state("disabled")
        self._set_status("Перевіряю відео…", 0)
        def worker():
            try:
                video = info_for(url)
                self.root.after(0, lambda: self._video_ready(video, url))
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._video_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _video_ready(self, video, url):
        self.busy = False
        if self.video_url.get().strip() != url:
            self.video_check_button.set_state("normal")
            return
        self.video_info = video
        self.video_info_url = url
        self.video_download_button.set_state("normal")
        self._set_status(f"Готово до завантаження: {video['title']}", 0)

    def _video_failed(self, error):
        self.busy = False
        self.video_download_button.set_state("disabled")
        self._set_status("Не вдалося прочитати відео.", 0)
        self._show_dialog("Не вдалося прочитати відео", error, "error")

    def _download_video(self):
        url = self.video_url.get().strip()
        if not valid_youtube_url(url):
            self._show_dialog("Перевір посилання", "Встав коректне HTTPS-посилання на YouTube відео.", "warning")
            return
        if self.busy:
            return
        if not getattr(self, "video_info", None) or getattr(self, "video_info_url", None) != url:
            self._prepare_video()
            return
        video = self.video_info
        output_dir = self.download_parent / safe_foldername(video["title"])
        resolution, container = self.video_resolution.get(), self.video_format.get().lower()
        self.busy = True
        self.video_download_button.set_state("disabled")
        control = self._begin_download_controls()
        self._set_status("Завантажую відео…", 0)
        self.progress.start_indeterminate()
        def worker():
            try:
                path = download_video_file(url, video["title"], output_dir, self._thread_progress,
                                           resolution=resolution, container=container, control=control)
                self.root.after(0, lambda: self._video_download_done(path))
            except DownloadCancelled:
                self.root.after(0, self._download_cancelled)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._video_download_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _video_download_done(self, path):
        self.busy = False
        self._finish_download_controls()
        self.last_output_dir = path.parent
        self.video_download_button.set_state("normal")
        self._set_status(f"Відео збережено: {path}", 100)
        self._show_dialog("Відео завантажено", f"Файл збережено:\n{path}", "success")

    def _video_download_failed(self, error):
        self.busy = False
        self._finish_download_controls()
        self.video_download_button.set_state("normal")
        self._set_status("Не вдалося завантажити відео.", 0)
        self._show_dialog("Помилка завантаження відео", error, "error")

    def _save_preferences_from_ui(self):
        resolution = self.options_resolution.get()
        self.default_video_resolution = "best" if resolution == "Автоматично" else resolution
        self.default_video_format = self.options_format.get().lower()
        self.default_playlist_output = "video" if self.options_playlist_output.get() == "Відео" else "mp3"
        self._set_playlist_output(self.default_playlist_output, persist=False)
        self.playlist_resolution.set("Автоматично" if self.default_video_resolution == "best"
                                     else self.default_video_resolution)
        self.playlist_format.set(self._video_format_label(self.default_video_format))
        self.video_resolution.set("Автоматично" if self.default_video_resolution == "best"
                                  else self.default_video_resolution)
        self.video_format.set(self._video_format_label(self.default_video_format))
        self._save_settings()
        self._show_dialog("Опції збережено", "Значення збережено як параметри за замовчуванням.", "success")

    def _begin_download_controls(self):
        self.download_controller = DownloadController()
        self.pause_download_button.set_text("Пауза")
        self.pause_download_button.set_state("normal" if os.name != "nt" else "disabled")
        self.cancel_download_button.set_state("normal")
        self.download_controls.pack(fill="x", pady=(4, 0))
        return self.download_controller

    def _toggle_download_pause(self):
        controller = self.download_controller
        if controller is None or controller.cancelled:
            return
        try:
            if controller.paused:
                controller.resume()
                self.pause_download_button.set_text("Пауза")
                self.status.set("Продовжую завантаження…")
            else:
                controller.pause()
                self.pause_download_button.set_text("Продовжити")
                self.status.set("Завантаження призупинено.")
        except OSError as exc:
            self._show_dialog("Керування завантаженням", str(exc), "warning")

    def _cancel_download(self):
        controller = self.download_controller
        if controller is None or controller.cancelled:
            return
        controller.cancel()
        self.cancel_download_button.set_state("disabled")
        self.pause_download_button.set_state("disabled")
        self.status.set("Скасовую завантаження…")

    def _finish_download_controls(self):
        self.download_controls.pack_forget()
        self.download_controller = None

    def _download_cancelled(self):
        self.busy = False
        self._finish_download_controls()
        self._set_tool_actions_enabled(True)
        self._set_status("Завантаження скасовано. Вже готові файли збережено.", 0)

    def _set_status(self, text: str, value: int | None = None):
        self.status.set(text)
        if value is not None:
            self.progress.set_value(value)

    def _show_dialog(self, title: str, message: str, kind: str = "info"):
        theme = THEMES[self.theme_name]
        dialog = tk.Toplevel(self.root)
        dialog.title(title)
        dialog.configure(bg=theme["bg"])
        dialog.resizable(False, False)
        dialog.transient(self.root)

        card = RoundedCard(dialog, bg=theme["bg"], fill=theme["card"], padding=22, radius=20)
        card.pack(fill="both", expand=True, padx=12, pady=12)
        content = card.content
        header = tk.Frame(content, bg=theme["card"])
        header.pack(fill="x", pady=(0, 12))
        badge_color = theme["accent"] if kind in ("info", "success") else "#f1b95f" if kind == "warning" else "#f07888"
        badge_text = theme["accent_text"] if kind in ("info", "success") else "#30220d" if kind == "warning" else "#32151b"
        icon = {"info": "i", "success": "✓", "warning": "!", "error": "×"}.get(kind, "i")
        icon_canvas = tk.Canvas(header, width=34, height=34, bg=theme["card"], bd=0, highlightthickness=0)
        icon_canvas.pack(side="left", padx=(0, 11))
        icon_canvas.create_oval(2, 2, 32, 32, fill=badge_color, outline="")
        icon_canvas.create_text(17, 17, text=icon, fill=badge_text, font=("TkDefaultFont", 14, "bold"))
        tk.Label(header, text=title, bg=theme["card"], fg=theme["ink"],
                 font=("TkDefaultFont", 13, "bold"), anchor="w").pack(side="left", fill="x", expand=True)
        tk.Label(content, text=message, bg=theme["card"], fg=theme["muted"],
                 font=("TkDefaultFont", 10), anchor="w", justify="left", wraplength=440).pack(fill="x")
        footer = tk.Frame(content, bg=theme["card"])
        footer.pack(fill="x", pady=(20, 0))
        RoundedButton(footer, text="Гаразд", command=dialog.destroy, variant="primary", height=38,
                      bg=theme["card"]).pack(side="right")
        dialog.bind("<Return>", lambda _event: dialog.destroy())
        dialog.bind("<Escape>", lambda _event: dialog.destroy())
        dialog.protocol("WM_DELETE_WINDOW", dialog.destroy)
        width = 520
        x = self.root.winfo_rootx() + max(0, (self.root.winfo_width() - width) // 2)
        y = self.root.winfo_rooty() + max(0, (self.root.winfo_height() - 220) // 2)
        dialog.geometry(f"{width}x220+{x}+{y}")
        dialog.update_idletasks()
        card._fit_height()
        dialog.update_idletasks()
        height = max(170, card.canvas.winfo_reqheight() + 24)
        y = self.root.winfo_rooty() + max(0, (self.root.winfo_height() - height) // 2)
        dialog.geometry(f"{width}x{height}+{x}+{y}")
        dialog.grab_set()
        dialog.focus_set()
        self.root.wait_window(dialog)

    @staticmethod
    def _load_download_parent() -> Path:
        try:
            saved = json.loads(SETTINGS_PATH.read_text(encoding="utf-8")).get("download_parent")
            if saved:
                return Path(saved).expanduser()
        except (OSError, ValueError, AttributeError):
            pass
        return DOWNLOADS

    @staticmethod
    def _load_theme() -> str:
        try:
            saved = json.loads(SETTINGS_PATH.read_text(encoding="utf-8")).get("theme")
            if saved in THEMES:
                return saved
        except (OSError, ValueError, AttributeError):
            pass
        return "olive"

    @staticmethod
    def _load_preferences() -> dict:
        try:
            settings = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
            return {
                "default_video_format": settings.get("default_video_format", "mp4"),
                "default_video_resolution": settings.get("default_video_resolution", "best"),
                "default_playlist_output": settings.get("default_playlist_output", "mp3"),
            }
        except (OSError, ValueError, AttributeError):
            return {}

    def _save_settings(self):
        try:
            settings = {}
            if SETTINGS_PATH.exists():
                try:
                    settings = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
                except (OSError, ValueError):
                    pass
            settings["download_parent"] = str(self.download_parent)
            settings["theme"] = self.theme_name
            settings["default_video_format"] = self.default_video_format
            settings["default_video_resolution"] = self.default_video_resolution
            settings["default_playlist_output"] = self.default_playlist_output
            SETTINGS_PATH.write_text(json.dumps(settings, ensure_ascii=False, indent=2), encoding="utf-8")
        except OSError:
            pass

    def _toggle_theme(self):
        self._apply_theme("purple" if self.theme_name == "olive" else "olive")

    def _apply_theme(self, theme_name: str, *, save=True):
        if theme_name not in THEMES:
            theme_name = "olive"
        theme = THEMES[theme_name]
        previous = THEMES[self.applied_theme]
        color_map = {previous[key]: theme[key] for key in theme if key in previous and key != "label"}

        def recolor(widget):
            for option in ("bg", "fg", "highlightbackground", "insertbackground", "selectbackground",
                           "troughcolor", "background", "foreground"):
                try:
                    value = widget.cget(option)
                    if value in color_map:
                        widget.configure(**{option: color_map[value]})
                except (tk.TclError, TypeError):
                    continue
            for child in widget.winfo_children():
                recolor(child)

        recolor(self.root)
        RoundedButton.PALETTES = {
            "primary": {"fill": theme["accent"], "hover": theme["accent_hover"], "text": theme["accent_text"],
                        "disabled": theme["disabled"], "disabled_text": theme["disabled_text"]},
            "secondary": {"fill": theme["secondary"], "hover": theme["secondary_hover"],
                          "text": theme["secondary_text"], "disabled": theme["secondary_disabled"],
                          "disabled_text": theme["secondary_disabled_text"]},
        }
        RoundedButton.FOCUS_COLOR = theme["focus"]
        for widget in self._walk_widgets(self.root):
            if isinstance(widget, RoundedCard):
                widget.set_theme(bg=theme["bg"], fill=theme["card"])
            elif isinstance(widget, RoundedButton):
                widget._draw()
            elif isinstance(widget, RoundedEntry):
                widget.field = theme["field"]
                widget.default_border = theme["outline"]
                widget.border = theme["focus"] if widget.entry.focus_get() else theme["outline"]
                widget.focus_color = theme["focus"]
                widget.text_color = theme["ink"]
                widget.selection_color = theme["entry_select"]
                widget.entry.configure(bg=theme["field"], fg=theme["ink"], insertbackground=theme["ink"],
                                       selectbackground=theme["entry_select"])
                widget._draw()

        self.root.configure(bg=theme["bg"])
        self.style.configure("Treeview", background=theme["card"], fieldbackground=theme["card"],
                             foreground=theme["table_fg"], bordercolor=theme["card"],
                             lightcolor=theme["card"], darkcolor=theme["card"])
        self.style.map("Treeview", background=[("selected", theme["selected"])],
                       foreground=[("selected", theme["selected_fg"])])
        self.style.configure("Treeview.Heading", background=theme["heading"], foreground=theme["muted"],
                             bordercolor=theme["card"], lightcolor=theme["heading"], darkcolor=theme["heading"])
        self.style.map("Treeview.Heading", background=[("active", theme["border"])])
        self.style.configure("TScrollbar", background=theme["scrollbar"], troughcolor=theme["card"],
                             bordercolor=theme["card"], arrowcolor=theme["muted"],
                             lightcolor=theme["scrollbar"], darkcolor=theme["scrollbar"])
        self.style.configure("Horizontal.TProgressbar", troughcolor=theme["progress_trough"],
                             background=theme["accent"], bordercolor=theme["progress_trough"],
                             lightcolor=theme["accent"], darkcolor=theme["accent"])
        self.style.configure("TCombobox", fieldbackground=theme["field"], background=theme["secondary"],
                             foreground=theme["ink"], arrowcolor=theme["muted"], bordercolor=theme["border"],
                             lightcolor=theme["border"], darkcolor=theme["border"])
        self.style.map("TCombobox", fieldbackground=[("readonly", theme["field"])],
                       foreground=[("readonly", theme["ink"])], selectbackground=[("readonly", theme["selected"])])
        self.progress.set_theme(trough=theme["progress_trough"], fill=theme["accent"])
        self.status_label.configure(fg=theme["muted"])
        self.theme_name = self.applied_theme = theme_name
        self.theme_button.set_text(f"Тема: {theme['label']}")
        self.options_theme.set("Фіолетова" if theme_name == "purple" else "Оливкова")
        if save:
            self._save_settings()

    @staticmethod
    def _walk_widgets(parent):
        for child in parent.winfo_children():
            yield child
            yield from TrackSplitApp._walk_widgets(child)

    def _choose_download_parent(self):
        selected = filedialog.askdirectory(
            title="Виберіть теку для збереження відео", initialdir=str(self.download_parent), mustexist=True
        )
        if not selected:
            return
        self.download_parent = Path(selected)
        self.destination_label.configure(text=str(self.download_parent))
        self.options_folder_label.configure(text=str(self.download_parent))
        try:
            self._save_settings()
        except OSError as exc:
            self._show_dialog("Налаштування", f"Теку вибрано, але не вдалося зберегти вибір: {exc}", "warning")

    def _thread_progress(self, text: str, value: int):
        def update():
            self.status.set(text)
            if value > 0 or not self.progress.indeterminate:
                self.progress.set_value(value)
        self.root.after(0, update)

    def _bootstrap_tools(self):
        self.busy = True
        self._set_tool_actions_enabled(False)
        def worker():
            try:
                ensure_tools(self._thread_progress)
                self.root.after(0, self._tools_ready)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._tools_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _tools_ready(self):
        self.busy = False
        self._set_tool_actions_enabled(True)
        self._set_status(f"Готово. yt-dlp {self._version(YTDLP_PATH)} · ffmpeg {self._version(FFMPEG_PATH)}", 100)

    def _tools_failed(self, error: str):
        self.busy = False
        self._set_tool_actions_enabled(True)
        self._set_status("Не вдалося підготувати інструменти.", 0)
        self._show_dialog("Не вдалося підготувати інструменти",
                          f"{error}\n\nПеревірте підключення до інтернету й повторіть через «Оновити інструменти».",
                          "error")

    @staticmethod
    def _version(path: Path) -> str:
        try:
            version_arg = "--version" if path == YTDLP_PATH else "-version"
            result = run([str(path), version_arg], timeout=10)
            return (result.stdout.splitlines()[0] if result.stdout else "готовий")
        except Exception:
            return "готовий"

    def _update_tools(self):
        if self.busy:
            return
        self.busy = True
        self._set_tool_actions_enabled(False)
        self._set_status("Оновлюю бінарники…", 0)
        def worker():
            try:
                ensure_tools(self._thread_progress, force=True)
                self.root.after(0, self._tools_ready)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._tools_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _set_tool_actions_enabled(self, enabled):
        state = "normal" if enabled else "disabled"
        for button in (self.find_button, self.playlist_find_button, self.video_check_button):
            button.set_state(state)
        if not enabled:
            self.download_button.set_state("disabled")
            self.playlist_download_button.set_state("disabled")
            self.video_download_button.set_state("disabled")
        else:
            self.download_button.set_state("normal" if self.video else "disabled")
            self.playlist_download_button.set_state("normal" if getattr(self, "playlist", None) else "disabled")
            self.video_download_button.set_state("normal" if getattr(self, "video_info", None) else "disabled")

    def _find_chapters(self):
        url = self.url.get().strip()
        if not valid_youtube_url(url):
            self._show_dialog("Перевір посилання", "Встав коректне HTTPS-посилання на YouTube.", "warning")
            return
        if self.busy:
            return
        self.busy = True
        self.find_button.set_state("disabled")
        self.download_button.set_state("disabled")
        self._set_status("Читаю розділи відео…", 0)
        def worker():
            try:
                video = info_for(url)
                self.root.after(0, lambda: self._show_video(video, url))
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._info_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _show_video(self, video: dict, url: str):
        self.busy = False
        if self.url.get().strip() != url:
            self.find_button.set_state("normal")
            return
        self.video = video
        self.find_button.set_state("normal")
        for item in self.tree.get_children():
            self.tree.delete(item)
        for chapter in video["chapters"]:
            start = int(chapter["start"])
            self.tree.insert("", "end", iid=str(chapter["index"]), values=(chapter["title"], f"{start // 60:02}:{start % 60:02}"))
        self.tree.selection_set(self.tree.get_children())
        self.video_label.configure(text=video["title"])
        chapter_total = len(video["chapters"])
        if chapter_total % 10 == 1 and chapter_total % 100 != 11:
            chapter_word = "РОЗДІЛ"
        elif 2 <= chapter_total % 10 <= 4 and not 12 <= chapter_total % 100 <= 14:
            chapter_word = "РОЗДІЛИ"
        else:
            chapter_word = "РОЗДІЛІВ"
        self.chapter_count.configure(text=f"{chapter_total} {chapter_word}")
        self.select_button.set_state("normal")
        self.select_button.set_text("Зняти вибір")
        self.download_button.set_state("normal")
        self._set_status("Вибери треки й натисни «Завантажити вибрані MP3».", 0)

    def _info_failed(self, error: str):
        self.busy = False
        self.find_button.set_state("normal")
        self._set_status("Не вдалося прочитати відео.", 0)
        self._show_dialog("Не вдалося прочитати відео", error, "error")

    def _toggle_selection(self):
        children = self.tree.get_children()
        if len(self.tree.selection()) == len(children):
            self.tree.selection_remove(*children)
            self.select_button.set_text("Вибрати всі")
        else:
            self.tree.selection_set(children)
            self.select_button.set_text("Зняти вибір")

    def _download(self):
        if not self.video or self.busy:
            return
        selected = sorted(int(item) for item in self.tree.selection())
        if not selected:
            self._show_dialog("Не вибрано треки", "Вибери хоча б один розділ.", "warning")
            return
        self.busy = True
        self.find_button.set_state("disabled")
        self.download_button.set_state("disabled")
        url, video, download_parent = self.url.get().strip(), self.video, self.download_parent
        control = self._begin_download_controls()
        def worker():
            try:
                output_dir, tracks, cover_warning = split_video(
                    url, video, selected, download_parent, self._thread_progress, control=control,
                )
                self.root.after(0, lambda: self._download_done(output_dir, tracks, cover_warning))
            except DownloadCancelled:
                self.root.after(0, self._download_cancelled)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._download_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _download_done(self, output_dir: Path, tracks: list[Path], cover_warning: bool):
        self.busy = False
        self._finish_download_controls()
        self.last_output_dir = output_dir
        self.find_button.set_state("normal")
        self.download_button.set_state("normal")
        note = " Деякі обкладинки не вдалося вбудувати." if cover_warning else " Обкладинку відео додано до треків."
        self._set_status(f"Збережено {len(tracks)} MP3 у {output_dir}.{note}", 100)
        self._show_dialog("Завантаження завершено",
                          f"Створено {len(tracks)} треків.{note}\n\nТека: {output_dir}", "success")

    def _download_failed(self, error: str):
        self.busy = False
        self._finish_download_controls()
        self.find_button.set_state("normal")
        self.download_button.set_state("normal")
        self._set_status("Помилка під час завантаження.", 0)
        self._show_dialog("Помилка завантаження", error, "error")

    def _open_path(self, path: Path):
        path.mkdir(parents=True, exist_ok=True)
        try:
            if sys.platform.startswith("win"):
                os.startfile(str(path))
            elif sys.platform == "darwin":
                subprocess.Popen(["open", str(path)])
            else:
                subprocess.Popen(["xdg-open", str(path)])
        except OSError as exc:
            self._show_dialog("Не вдалося відкрити теку", str(exc), "error")

    def _open_downloads(self):
        self._open_path(self.last_output_dir or self.download_parent)


def main():
    root = tk.Tk()
    TrackSplitApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
