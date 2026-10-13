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
from tkinter import ttk
from tkinter import font as tkfont
from urllib.parse import urlparse
from ui import FONT, WaveMark, MediaList, EmptyState, Choice, rounded as draw_aa_rounded
from i18n import UK_TO_EN, translate

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
        "label": "Оливкова", "bg": "#111511", "card": "#1b211c", "ink": "#f0f4ed",
        "muted": "#9aa797", "accent": "#c4e88a", "accent_text": "#223017", "accent_hover": "#d6f4a5",
        "field": "#141a15", "border": "#343e32", "outline": "#3c4937", "heading": "#202821",
        "table_fg": "#dce3d2", "selected": "#34442c", "selected_fg": "#f2f7e9",
        "scrollbar": "#343e32", "progress_trough": "#2a3124", "disabled": "#343c2b",
        "disabled_text": "#89917d", "focus": "#8fae52", "entry_select": "#506b32",
        "secondary": "#2a3025", "secondary_hover": "#353e2d", "secondary_text": "#e8eddf",
        "secondary_disabled": "#252a21", "secondary_disabled_text": "#79806f",
    },
    "purple": {
        "label": "Фіолетова", "bg": "#111018", "card": "#1c1927", "ink": "#f4f0ff",
        "muted": "#aaa1bc", "accent": "#bfa0f5", "accent_text": "#241739", "accent_hover": "#d2b9ff",
        "field": "#15121e", "border": "#342b45", "outline": "#483959", "heading": "#211d30",
        "table_fg": "#e3d9ed", "selected": "#372b4c", "selected_fg": "#fff8ff",
        "scrollbar": "#453658", "progress_trough": "#30263f", "disabled": "#393143",
        "disabled_text": "#9589a2", "focus": "#aa7de2", "entry_select": "#624783",
        "secondary": "#2a2437", "secondary_hover": "#3c314c", "secondary_text": "#eee5f5",
        "secondary_disabled": "#292330", "secondary_disabled_text": "#8c8198",
    },
    "light": {
        "label": "Світла", "bg": "#f3f2f7", "card": "#ffffff", "ink": "#242132",
        "muted": "#686174", "accent": "#7150b8", "accent_text": "#fffefe", "accent_hover": "#60409f",
        "field": "#faf9fc", "border": "#ddd7e6", "outline": "#c6bcd5", "heading": "#f4f0f8",
        "table_fg": "#383140", "selected": "#e8ddfa", "selected_fg": "#3e2469",
        "scrollbar": "#bdb2ce", "progress_trough": "#e5dfee", "disabled": "#e6e0ed",
        "disabled_text": "#80758d", "focus": "#8664ca", "entry_select": "#d9c6f5",
        "secondary": "#eee9f5", "secondary_hover": "#e0d6ee", "secondary_text": "#443451",
        "secondary_disabled": "#f1edf5", "secondary_disabled_text": "#8b8098",
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
    request = urllib.request.Request(url, headers={"User-Agent": "Tuneka/1.0"})
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
               "download:TUNEKA_PROGRESS %(progress._percent_str)s · %(progress._speed_str)s · ETA %(progress._eta_str)s",
               "--progress-template", "postprocess:TUNEKA_POSTPROCESS %(progress._default_template)s",
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
            if line.startswith("TUNEKA_PROGRESS"):
                match = re.search(r"(\d+(?:\.\d+)?)%", line)
                if match:
                    percent = float(match.group(1))
                    detail = line.partition("TUNEKA_PROGRESS")[2].strip()
                    progress(f"Завантажую відео… {detail}", max(1, min(98, int(percent))))
                continue
            if line.startswith("TUNEKA_POSTPROCESS"):
                detail = line.partition("TUNEKA_POSTPROCESS")[2].strip()
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
            "TUNEKA_PROGRESS", "TUNEKA_POSTPROCESS", "[Merger] Merging formats into:",
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
    request = urllib.request.Request(url, headers={"User-Agent": "Tuneka/1.0"})
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
    with tempfile.TemporaryDirectory(prefix=".tuneka-", dir=jobdir) as temporary_dir:
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
    return draw_aa_rounded(canvas, 0, 0, width, height, radius, fill, outline)


class RoundedButton(tk.Frame):
    PALETTES = {
        "primary": {"fill": "#c4e88a", "hover": "#d6f4a5", "text": "#223017", "disabled": "#343c2b", "disabled_text": "#89917d"},
        "secondary": {"fill": "#2a3025", "hover": "#353e2d", "text": "#e8eddf", "disabled": "#252a21", "disabled_text": "#79806f"},
    }

    def __init__(self, master, text: str, command, *, variant="secondary", height=38, bg="#111511"):
        super().__init__(master, bg=bg, bd=0, highlightthickness=0)
        self.source_label = text
        self.translate = lambda value: value
        self.label = text
        self.command = command
        self.variant = variant
        self.button_height = height
        self.enabled = True
        self.hovered = False
        self.font = tkfont.Font(family=FONT, size=10, weight="bold" if variant == "primary" else "normal")
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
        width = self.canvas.winfo_width() if self.canvas.winfo_width() > 1 else self.button_width
        height = max(self.button_height, self.canvas.winfo_height())
        palette = self.PALETTES[self.variant]
        fill = palette["disabled"] if not self.enabled else palette["hover"] if self.hovered else palette["fill"]
        text_color = palette["disabled_text"] if not self.enabled else palette["text"]
        self.canvas.delete("all")
        if getattr(self, "navigation", False):
            nav = getattr(self, "NAV_PALETTE", {"selected": "#34442c", "accent": "#c4e88a"})
            fill = nav["selected"] if self.variant == "primary" else palette["hover"] if self.hovered else self.cget("bg")
            text_color = nav["accent"] if self.variant == "primary" else palette["text"]
        draw_round_rect(self.canvas, width, height, 18 if getattr(self, "navigation", False) else height // 2, fill)
        try:
            has_focus = self.canvas.focus_displayof() is self.canvas
        except (KeyError, tk.TclError):
            # Native dialogs can temporarily expose Tcl-only widgets (for
            # example __tk_choosedir) that Tkinter cannot resolve to a Python
            # widget while mapping focus_displayof().
            has_focus = False
        if has_focus and not getattr(self, "navigation", False):
            # Redraw the button fill under its focus outline. An outline-only
            # rounded shape blends its corner pixels with the canvas background,
            # which creates dark gaps over the colored button.
            radius = 18 if getattr(self, "navigation", False) else height // 2
            draw_round_rect(self.canvas, width, height, radius, fill,
                            getattr(self, "FOCUS_COLOR", "#8fae52"))
        self.canvas.create_text(16 if getattr(self, "navigation", False) else width / 2, height / 2,
                                anchor="w" if getattr(self, "navigation", False) else "center", text=self.label, fill=text_color,
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
        self.source_label = text
        self.label = self.translate(text)
        self.button_width = self.font.measure(self.label) + (36 if self.variant == "primary" else 30)
        self.canvas.configure(width=self.button_width)
        self._draw()

    def set_variant(self, variant: str):
        self.variant = variant
        self.font.configure(weight="bold" if variant == "primary" else "normal")
        self.button_width = self.font.measure(self.label) + (36 if self.variant == "primary" else 30)
        self.canvas.configure(width=self.button_width)
        self._draw()


class RoundedEntry(tk.Frame):
    def __init__(self, master, textvariable: tk.StringVar, *, bg="#1b211c", field="#141a15", fg="#edf1e6"):
        super().__init__(master, bg=bg, bd=0, highlightthickness=0, height=46)
        self.pack_propagate(False)
        self.field = field
        self.border = "#3c4937"
        self.default_border = self.border
        self.focus_color = "#90b64c"
        self.text_color = fg
        self.selection_color = "#506b32"
        self.canvas = tk.Canvas(self, height=46, bg=bg, bd=0, highlightthickness=0)
        self.canvas.pack(fill="both", expand=True)
        self.canvas.bind("<Configure>", self._draw)
        self.entry = tk.Entry(self.canvas, textvariable=textvariable, relief="flat", bd=0, highlightthickness=0,
                              bg=field, fg=fg, insertbackground=fg, selectbackground=self.selection_color,
                              selectforeground="#ffffff", font=(FONT, 11))
        self.entry.place(x=13, y=9, relwidth=1, width=-26, height=28)
        self.variable = textvariable
        self.placeholder = tk.Label(self.entry, text="Встав посилання на YouTube…", bg=field,
                                    fg="#9aa797", font=(FONT, 10), anchor="w", cursor="xterm")
        self.placeholder.bind("<Button-1>", lambda _e: self.entry.focus_set())
        self.variable.trace_add("write", self._update_placeholder)
        self._update_placeholder()
        self.entry.bind("<FocusIn>", self._focus_in)
        self.entry.bind("<FocusOut>", self._focus_out)
        self._draw()

    def _draw(self, _event=None):
        if hasattr(self, "canvas"):
            width = max(self.canvas.winfo_width(), 200)
            self.canvas.delete("all")
            draw_round_rect(self.canvas, width, 46, 14, self.field, self.border)

    def _update_placeholder(self, *_):
        if self.variable.get() or str(self.tk.call("focus")) == str(self.entry):
            self.placeholder.place_forget()
        else:
            self.placeholder.place(x=0, y=0, relwidth=1, relheight=1)

    def _focus_in(self, _event=None):
        self._update_placeholder()
        self.border = self.focus_color
        self._draw()

    def _focus_out(self, _event=None):
        self._update_placeholder()
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
        draw_round_rect(self.canvas, event.width, event.height, self.radius, self.fill, getattr(self, "border", ""))
        self.canvas.tag_lower("shape")
        self.canvas.itemconfigure(self.window, tags="card-content")

    def set_theme(self, *, bg, fill):
        self.bg_color, self.fill = bg, fill
        self.configure(bg=bg)
        self.canvas.configure(bg=bg)
        self.content.configure(bg=fill)
        self._resize(type("Size", (), {"width": self.canvas.winfo_width(), "height": self.canvas.winfo_height()})())


class TunekaApp:
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
        self.language = preferences.get("language", "uk")
        if self.language not in ("uk", "en"):
            self.language = "uk"
        self.applied_theme = "olive"
        self.last_output_dir: Path | None = None
        self.download_controller: DownloadController | None = None
        self.compact_elements = []
        self.intro_titles = []
        self._compact_pack = {}
        self.compact = False
        self.root.title("Tuneka")
        self.root.geometry(f"{min(1220, self.root.winfo_screenwidth()-60)}x{min(840, self.root.winfo_screenheight()-100)}")
        self.root.minsize(980, 700)
        self.root.configure(bg="#111511")
        self.style = ttk.Style()
        try:
            self.style.theme_use("clam")
        except tk.TclError:
            pass
        self.style.configure("TFrame", background="#111511")
        self.style.configure("Treeview", rowheight=46, font=(FONT, 10),
                             background="#1b211c", fieldbackground="#1b211c", foreground="#dce3d2",
                             borderwidth=0, relief="flat", bordercolor="#1b211c",
                             lightcolor="#1b211c", darkcolor="#1b211c")
        self.style.map("Treeview", background=[("selected", "#34442c")],
                       foreground=[("selected", "#f2f7e9")])
        self.style.configure("Treeview.Heading", font=(FONT, 9, "bold"),
                             background="#202821", foreground="#a5ae98", borderwidth=0,
                             bordercolor="#1b211c", lightcolor="#202821", darkcolor="#202821", padding=(12, 9))
        self.style.map("Treeview.Heading", background=[("active", "#333b2d")])
        self.style.layout("Vertical.TScrollbar", [("Vertical.Scrollbar.trough", {
            "sticky": "ns", "children": [("Vertical.Scrollbar.thumb", {"expand": True, "sticky": "nswe"})]})])
        self.style.configure("Vertical.TScrollbar", width=6, borderwidth=0)
        self.style.configure("TScrollbar", background="#343e32", troughcolor="#1b211c", borderwidth=0,
                             bordercolor="#1b211c", arrowcolor="#9aa797", lightcolor="#343e32",
                             darkcolor="#343e32", arrowsize=12)
        self.style.configure("Horizontal.TProgressbar", troughcolor="#2a3124", background="#a9d65d",
                             bordercolor="#2a3124", lightcolor="#a9d65d", darkcolor="#a9d65d", thickness=6)
        self.style.configure("TCombobox", fieldbackground="#141a15", background="#2a3025",
                             foreground="#f0f4ed", arrowcolor="#9aa797", bordercolor="#343e32",
                             lightcolor="#343e32", darkcolor="#343e32", padding=8)
        self.style.map("TCombobox", fieldbackground=[("readonly", "#141a15")],
                       foreground=[("readonly", "#f0f4ed")], selectbackground=[("readonly", "#34442c")])
        self._build_ui()
        self._apply_theme(self.theme_name, save=False)
        self._apply_language(self.language, persist=False)
        self.root.after(100, self._bootstrap_tools)

    def _build_ui(self):
        bg, card, ink, muted, accent = "#111511", "#1b211c", "#f0f4ed", "#9aa797", "#c4e88a"
        shell = tk.Frame(self.root, bg=bg)
        shell.pack(fill="both", expand=True)
        self.sidebar = tk.Frame(shell, bg=card, width=204, padx=14, pady=24)
        self.sidebar.pack(side="left", fill="y")
        self.sidebar.pack_propagate(False)
        brand = tk.Frame(self.sidebar, bg=card)
        brand.pack(fill="x", padx=8, pady=(2, 34))
        self.brand_mark = WaveMark(brand, bg=card, accent=accent, ink="#223017", size=36)
        self.brand_mark.pack(side="left", padx=(0, 10))
        tk.Label(brand, text="tuneka", bg=card, fg=ink,
                 font=(FONT, 16, "bold")).pack(side="left")
        tk.Label(self.sidebar, text="ВАША КОЛЕКЦІЯ", bg=card, fg=muted,
                 font=(FONT, 8, "bold")).pack(anchor="w", padx=14, pady=(0, 12))
        self.nav_buttons = {}
        for key, label in (("music", "♫   Музика"), ("playlist", "≡   Плейлисти"),
                           ("video", "▷   Відео"), ("options", "⚙   Налаштування")):
            button = RoundedButton(self.sidebar, text=label, command=lambda page=key: self._show_page(page),
                                   variant="secondary", height=46, bg=card)
            button.navigation = True
            button.pack(fill="x", pady=4)
            self.nav_buttons[key] = button
        bottom = tk.Frame(self.sidebar, bg=card)
        bottom.pack(side="bottom", fill="x", padx=14)
        tk.Label(bottom, text="Медіа, які завжди поруч.", bg=card, fg=ink,
                 font=(FONT, 8)).pack(anchor="w", pady=(0, 5))
        tk.Label(bottom, text="На вашому пристрої", bg=card, fg=muted,
                 font=(FONT, 7)).pack(anchor="w")

        self.workspace = tk.Frame(shell, bg=bg, padx=26, pady=18)
        self.workspace.pack(side="left", fill="both", expand=True)
        self.workspace.columnconfigure(0, weight=1)
        self.workspace.rowconfigure(1, weight=1)
        topbar = self.topbar = tk.Frame(self.workspace, bg=bg)
        topbar.grid(row=0, column=0, sticky="ew", pady=(0, 22))
        self.page_title = tk.Label(topbar, text="", bg=bg, fg=muted, font=(FONT, 10), anchor="w")
        self.page_title.pack(side="left")
        self.update_button = RoundedButton(topbar, text="↻  Оновити інструменти", command=self._update_tools,
                                           variant="secondary", height=32, bg=bg)
        self.update_button.pack(side="right")
        self.theme_selector = Choice(topbar, width=14, values=tuple(t["label"] for t in THEMES.values()))
        self.theme_selector.pack(side="right", padx=(0, 8))
        self.theme_selector.bind("<<ComboboxSelected>>", self._change_theme_from_selection)
        self.page_host = tk.Frame(self.workspace, bg=bg)
        self.page_host.grid(row=1, column=0, sticky="nsew")
        self.page_host.pack_propagate(False)

        # The dock receives its natural height before the flexible media list.
        # Its buttons stay visible even when the window becomes shorter.
        self.footer_card = RoundedCard(self.workspace, bg=bg, fill=card, padding=16, radius=24)
        self.footer_card.grid(row=2, column=0, sticky="ew", pady=(16, 0))
        footer = self.footer_card.content
        destination_row = tk.Frame(footer, bg=card)
        destination_row.pack(fill="x")
        self.action_host = tk.Frame(destination_row, bg=card)
        self.action_host.pack(side="right", padx=(14, 0))
        self.choose_folder_button = RoundedButton(destination_row, text="Змінити…",
                                                  command=self._choose_download_parent,
                                                  variant="secondary", height=38, bg=card)
        self.choose_folder_button.pack(side="right", padx=(10, 0))
        destination_text = tk.Frame(destination_row, bg=card)
        destination_text.pack(side="left", fill="x", expand=True)
        tk.Label(destination_text, text="ЗБЕРЕГТИ В", bg=card, fg=muted,
                 font=(FONT, 8, "bold")).pack(anchor="w")
        self.destination_label = tk.Label(destination_text, text=str(self.download_parent), bg=card, fg=ink,
                                          font=(FONT, 9), anchor="w")
        self.destination_label.pack(fill="x", pady=(4, 0))
        self.destination_label.bind("<Configure>", self._fit_destination)
        self.status = tk.StringVar(value="Перевіряю інструменти…")
        self.status_label = tk.Label(footer, textvariable=self.status, bg=card, fg=muted,
                                     font=(FONT, 9), anchor="w", cursor="hand2")
        self.status_label.pack(anchor="w", fill="x", pady=(14, 8))
        self.status_label.bind("<Button-1>", lambda _e: self._show_dialog("Стан завантаження", self.status.get()))
        self.progress = AnimatedProgressBar(footer, bg=card, trough="#2a3124", fill=accent, height=6)
        self.progress.pack(fill="x")
        self.download_controls = tk.Frame(footer, bg=card)
        self.pause_download_button = RoundedButton(self.download_controls, text="Пауза",
            command=self._toggle_download_pause, variant="secondary", height=32, bg=card)
        self.pause_download_button.pack(side="left")
        self.cancel_download_button = RoundedButton(self.download_controls, text="Скасувати",
            command=self._cancel_download, variant="secondary", height=32, bg=card)
        self.cancel_download_button.pack(side="left", padx=(8, 0))
        self.open_folder_button = RoundedButton(self.download_controls, text="Відкрити теку  ↗",
            command=self._open_downloads, variant="secondary", height=32, bg=card)
        self.open_folder_button.pack(side="right")
        self.download_controls.pack(fill="x", pady=(10, 0))
        self.pause_download_button.set_state("disabled")
        self.cancel_download_button.set_state("disabled")

        self.pages = {}
        for key, builder in (("music", self._build_music_page), ("playlist", self._build_playlist_page),
                             ("video", self._build_video_page), ("options", self._build_options_page)):
            page = tk.Frame(self.page_host, bg=bg)
            self.pages[key] = page
            builder(page)
        self._show_page("music")
        self.workspace.bind("<Configure>", self._adapt_layout)

    def _fit_destination(self, _event=None):
        path = str(self.download_parent)
        font = tkfont.Font(font=self.destination_label.cget("font"))
        width = max(30, self.destination_label.winfo_width() - 6)
        display = path
        while len(display) > 4 and font.measure(display) > width:
            display = "…" + display[2:]
        self.destination_label.configure(text=display)

    def _adapt_layout(self, _event=None):
        compact = self.root.winfo_height() < 780
        for title in self.intro_titles:
            title.configure(font=(FONT, 21 if compact else 25, "bold"))
        if compact == self.compact:
            return
        self.compact = compact
        self.topbar.grid_configure(pady=(0, 12 if compact else 22))
        for widget in self.compact_elements:
            if compact:
                siblings = widget.master.pack_slaves()
                index = siblings.index(widget)
                following = siblings[index+1] if index+1 < len(siblings) else None
                self._compact_pack[widget] = (widget.pack_info(), following)
                widget.pack_forget()
            else:
                options, following = self._compact_pack[widget]
                options.pop("in", None)
                if following is not None:
                    options["before"] = following
                widget.pack(**options)
        for widget in self._walk_widgets(self.root):
            if isinstance(widget, RoundedCard) and not widget.expand_content:
                widget.after_idle(widget._fit_height)

    def _page_intro(self, parent, title, description):
        label = tk.Label(parent, text=title, bg="#111511", fg="#f0f4ed",
                         font=(FONT, 25, "bold"))
        label.pack(anchor="w", pady=(0, 8))
        self.intro_titles.append(label)
        subtitle = tk.Label(parent, text=description, bg="#111511", fg="#9aa797",
                            font=(FONT, 10), wraplength=880, justify="left")
        subtitle.pack(anchor="w", pady=(0, 16))
        self.compact_elements.append(subtitle)

    def _source_card(self, parent, variable, button_text, command):
        card = RoundedCard(parent, bg="#111511", fill="#1b211c", padding=16, radius=24)
        card.pack(fill="x")
        caption = tk.Label(card.content, text="ПОСИЛАННЯ НА YOUTUBE", bg="#1b211c", fg="#9aa797",
                           font=(FONT, 9, "bold"))
        caption.pack(anchor="w", pady=(0, 9))
        self.compact_elements.append(caption)
        row = tk.Frame(card.content, bg="#1b211c")
        row.pack(fill="x")
        entry = RoundedEntry(row, textvariable=variable, bg="#1b211c", field="#141a15", fg="#f0f4ed")

        button = RoundedButton(row, text=button_text, command=command, variant="primary", height=40, bg="#1b211c")
        button.pack(side="right")
        paste = RoundedButton(row, text="Вставити", command=lambda: self._paste_url(variable, entry),
                              variant="secondary", height=40, bg="#1b211c")
        paste.pack(side="right", padx=(0, 8))
        entry.pack(side="left", fill="x", expand=True, padx=(0, 10))
        entry.entry.bind("<Return>", lambda _event: command())
        return card, entry, button

    def _paste_url(self, variable, entry):
        try:
            variable.set(self.root.clipboard_get().strip())
            entry.entry.focus_set()
        except tk.TclError:
            self._set_status("Буфер обміну порожній. Скопіюй посилання з YouTube.")

    def _make_table(self, parent, columns, headings, widths):
        card = RoundedCard(parent, bg="#111511", fill="#1b211c", padding=12, radius=24, expand_content=True)
        card.pack(fill="both", expand=True)
        music = columns[0] == "title"
        tree = MediaList(card.content, columns=columns,
                         empty_title="Кожен мікс починається з посилання" if music else "Улюблене — цілим плейлистом",
                         empty_description="Додай посилання вище — тут з’являться треки для вибору." if music
                         else "Встав посилання на YouTube-плейлист, щоб вибрати відео.")
        for column, heading, width in zip(columns, headings, widths):
            tree.heading(column, text=heading.capitalize(), anchor="w")
            tree.column(column, anchor="w" if column not in ("start", "duration") else "e",
                        width=100 if column in ("title", "video") else width,
                        minwidth=100 if column in ("title", "video") else width,
                        stretch=column in ("title", "video"))
        scroll = ttk.Scrollbar(card.content, orient="vertical", command=tree.yview)
        def update_scroll(first, last):
            scroll.set(first, last)
            if float(first) <= 0 and float(last) >= 1:
                scroll.pack_forget()
            elif not scroll.winfo_manager():
                scroll.pack(side="right", fill="y", padx=(6, 0), before=tree)
        tree.configure(yscrollcommand=update_scroll)
        scroll.pack(side="right", fill="y", padx=(6, 0))
        tree.pack(side="left", fill="both", expand=True)
        return tree

    def _build_music_page(self, page):
        self._page_intro(page, "Твій наступний плейлист.",
                         "Мікс із YouTube → окремі MP3 з назвами й обкладинкою. Без розділів — один трек.")
        self.url = tk.StringVar(value="")
        self.url.trace_add("write", self._music_url_changed)
        _, self.url_entry, self.find_button = self._source_card(page, self.url, "Знайти розділи  →",
                                                                self._find_chapters)
        row = tk.Frame(page, bg="#111511")
        row.pack(fill="x", pady=(17, 8))
        self.video_label = tk.Label(row, text="Треки", bg="#111511", fg="#f0f4ed",
                                    font=(FONT, 12, "bold"), anchor="w")
        self.chapter_count = tk.Label(row, text="MP3 · з обкладинкою", bg="#111511", fg="#9aa797",
                                      font=(FONT, 9, "bold"))
        self.chapter_count.pack(side="right", padx=(12, 14))
        self.select_button = RoundedButton(row, text="Зняти вибір", command=self._toggle_selection,
                                           variant="secondary", height=34, bg="#111511")
        self.select_button.set_state("disabled")
        self.select_button.pack(side="right")
        self.video_label.pack(side="left", fill="x", expand=True)
        self.tree = self._make_table(page, ("title", "start"), ("НАЗВА ТРЕКУ", "ПОЧАТОК"), (650, 100))
        self.download_button = RoundedButton(self.action_host, text="↓  Завантажити MP3",
                                             command=self._download, variant="primary", height=42,
                                             bg="#111511")
        self.download_button.set_state("disabled")

    def _build_playlist_page(self, page):
        self._page_intro(page, "Уся добірка. Одним рухом.",
                         "Вибери потрібне з плейлиста. Збережи як музику або відео.")
        self.playlist_url = tk.StringVar(value="")
        self.playlist = None
        self.playlist_url.trace_add("write", self._playlist_url_changed)
        _, self.playlist_url_entry, self.playlist_find_button = self._source_card(
            page, self.playlist_url, "Знайти відео  →", self._find_playlist)
        self.playlist_controls_card = RoundedCard(page, bg="#111511", fill="#1b211c", padding=14, radius=18)
        self.playlist_controls_card.pack(fill="x", pady=(12, 0))
        line = tk.Frame(self.playlist_controls_card.content, bg="#1b211c")
        line.pack(fill="x")
        tk.Label(line, text="ФОРМАТ ЗАВАНТАЖЕННЯ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(side="left", padx=(0, 12))
        self.playlist_output = tk.StringVar(value=self.default_playlist_output)
        self.playlist_mp3_button = RoundedButton(line, text="MP3 · 1 файл на відео",
                                                 command=lambda: self._set_playlist_output("mp3", persist=False),
                                                 variant="primary", height=36, bg="#1b211c")
        self.playlist_mp3_button.pack(side="left", padx=(0, 7))
        self.playlist_video_button = RoundedButton(line, text="Відео",
                                                   command=lambda: self._set_playlist_output("video", persist=False),
                                                   variant="secondary", height=36, bg="#1b211c")
        self.playlist_video_button.pack(side="left")
        self.playlist_video_options = tk.Frame(self.playlist_controls_card.content, bg="#1b211c")
        self.playlist_video_options.pack(fill="x", pady=(12, 0))
        tk.Label(self.playlist_video_options, text="ЯКІСТЬ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(side="left", padx=(0, 8))
        self.playlist_resolution = Choice(self.playlist_video_options, state="readonly", width=20,
                                                values=("Автоматично", "2160p", "1440p", "1080p", "720p", "480p", "360p"))
        self.playlist_resolution.pack(side="left", padx=(0, 18))
        tk.Label(self.playlist_video_options, text="ФОРМАТ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(side="left", padx=(0, 8))
        self.playlist_format = Choice(self.playlist_video_options, state="readonly", width=12,
                                            values=("MP4", "MKV", "WebM"))
        self.playlist_format.pack(side="left")
        self.playlist_resolution.set("Автоматично" if self.default_video_resolution == "best" else self.default_video_resolution)
        self.playlist_format.set(self._video_format_label(self.default_video_format))
        self._set_playlist_output(self.playlist_output.get(), persist=False)

        playlist_head = tk.Frame(page, bg="#111511")
        playlist_head.pack(fill="x", pady=(16, 8))
        self.playlist_title_label = tk.Label(playlist_head, text="Відео плейлиста",
                                             bg="#111511", fg="#f0f4ed", font=(FONT, 12, "bold"),
                                             anchor="w")
        self.playlist_count_label = tk.Label(playlist_head, text="", bg="#111511", fg="#9aa797",
                                             font=(FONT, 9, "bold"))
        self.playlist_count_label.pack(side="right", padx=(12, 14))
        self.playlist_select_button = RoundedButton(playlist_head, text="Зняти вибір",
                                                    command=self._toggle_playlist_selection,
                                                    variant="secondary", height=34, bg="#111511")
        self.playlist_select_button.set_state("disabled")
        self.playlist_select_button.pack(side="right")
        self.playlist_title_label.pack(side="left", fill="x", expand=True)
        self.playlist_tree = self._make_table(page, ("video", "duration"),
                                              ("ВІДЕО", "ТРИВАЛІСТЬ"), (650, 110))
        self.playlist_download_button = RoundedButton(self.action_host, text="↓  Завантажити вибрані",
                                                      command=self._download_playlist,
                                                      variant="primary", height=42, bg="#111511")
        self.playlist_download_button.set_state("disabled")

    def _build_video_page(self, page):
        self._page_intro(page, "Відео, яке лишається з тобою.",
                         "Обери якість і формат. Дивись улюблене, коли зручно.")
        self.video_url = tk.StringVar(value="")
        self.video_info = None
        self.video_url.trace_add("write", self._video_url_changed)
        _, self.video_url_entry, self.video_check_button = self._source_card(
            page, self.video_url, "Перевірити посилання", self._prepare_video)
        settings = RoundedCard(page, bg="#111511", fill="#1b211c", padding=18, radius=20)
        settings.pack(fill="x", pady=(14, 0))
        self.video_resolution, self.video_format = self._build_video_options(settings.content, "ВІДЕО")
        self.video_download_button = RoundedButton(self.action_host, text="↓  Завантажити відео",
                                                   command=self._download_video,
                                                   variant="primary", height=42, bg="#111511")
        self.video_download_button.set_state("disabled")
        preview = RoundedCard(page, bg="#111511", fill="#1b211c", padding=18, radius=24, expand_content=True)
        preview.pack(fill="both", expand=True, pady=(14, 0))
        self.video_preview = EmptyState(preview.content, "Місце для твого наступного відео",
                                       "Перевір посилання, щоб побачити назву й тривалість.")
        self.video_preview.pack(fill="both", expand=True)

    def _build_video_options(self, parent, prefix):
        tk.Label(parent, text=f"{prefix} · ЯКІСТЬ І ФОРМАТ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(anchor="w", pady=(0, 10))
        row = tk.Frame(parent, bg="#1b211c")
        row.pack(fill="x")
        tk.Label(row, text="РОЗДІЛЬНІСТЬ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(side="left", padx=(0, 8))
        resolution = Choice(row, state="readonly", width=20,
                                  values=("Автоматично", "2160p", "1440p", "1080p", "720p", "480p", "360p"))
        resolution.pack(side="left", padx=(0, 20))
        resolution.set("Автоматично" if self.default_video_resolution == "best" else self.default_video_resolution)
        tk.Label(row, text="ФОРМАТ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(side="left", padx=(0, 8))
        video_format = Choice(row, state="readonly", width=12, values=("MP4", "MKV", "WebM"))
        video_format.pack(side="left")
        video_format.set(self._video_format_label(self.default_video_format))
        return resolution, video_format

    @staticmethod
    def _video_format_label(value):
        return {"mp4": "MP4", "mkv": "MKV", "webm": "WebM"}.get(str(value).lower(), "MP4")

    def _build_options_page(self, page):
        self._page_intro(page, "Опції", "Задай теки та формати, які Tuneka використовуватиме за замовчуванням.")
        folder_card = RoundedCard(page, bg="#111511", fill="#1b211c", padding=18, radius=20)
        folder_card.pack(fill="x")
        tk.Label(folder_card.content, text="ТЕКА ДЛЯ ЗАВАНТАЖЕНЬ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).pack(anchor="w", pady=(0, 9))
        folder_row = tk.Frame(folder_card.content, bg="#1b211c")
        folder_row.pack(fill="x")
        self.options_folder_label = tk.Label(folder_row, text=str(self.download_parent), bg="#1b211c",
                                             fg="#f0f4ed", font=(FONT, 10), anchor="w")
        self.options_folder_label.pack(side="left", fill="x", expand=True)
        RoundedButton(folder_row, text="Вибрати теку…", command=self._choose_download_parent,
                      variant="secondary", height=36, bg="#1b211c").pack(side="right")

        defaults = RoundedCard(page, bg="#111511", fill="#1b211c", padding=18, radius=20)
        defaults.pack(fill="x", pady=(14, 0))
        self.options_resolution, self.options_format = self._build_video_options(defaults.content, "ЗА ЗАМОВЧУВАННЯМ")
        preferences = tk.Frame(defaults.content, bg="#1b211c")
        preferences.pack(fill="x", pady=(20, 0))
        for column in range(3):
            preferences.grid_columnconfigure(column, weight=1, uniform="settings_preferences")

        tk.Label(preferences, text="ФОРМАТ ПЛЕЙЛИСТІВ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).grid(row=0, column=0, sticky="w", pady=(0, 8))
        self.options_playlist_output = Choice(preferences, width=25,
                                              values=("MP3 · один файл на відео", "Відео"))
        self.options_playlist_output.grid(row=1, column=0, sticky="w")
        self.options_playlist_output.set("Відео" if self.default_playlist_output == "video" else "MP3 · один файл на відео")

        tk.Label(preferences, text="КОЛІРНА ТЕМА", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).grid(row=0, column=1, sticky="w", pady=(0, 8))
        self.options_theme = Choice(preferences, width=18, values=tuple(t["label"] for t in THEMES.values()))
        self.options_theme.grid(row=1, column=1, sticky="w")
        self.options_theme.set(THEMES[self.theme_name]["label"])
        self.options_theme.bind("<<ComboboxSelected>>", self._change_theme_from_selection)

        tk.Label(preferences, text="МОВА ІНТЕРФЕЙСУ", bg="#1b211c", fg="#9aa797",
                 font=(FONT, 8, "bold")).grid(row=0, column=2, sticky="w", pady=(0, 8))
        self.options_language = Choice(preferences, width=18, values=("uk", "en"))
        self.options_language.grid(row=1, column=2, sticky="w")
        self.options_language.set(self.language)
        self.options_language.bind("<<ComboboxSelected>>", self._change_language_from_selection)
        self.options_save_button = RoundedButton(self.action_host, text="Зберегти опції", command=self._save_preferences_from_ui,
                                                 variant="primary", height=42, bg="#111511")

    def _show_page(self, page_key):
        if page_key not in self.pages:
            return
        for page in self.pages.values():
            page.pack_forget()
        self.pages[page_key].pack(fill="both", expand=True)
        for action in self.action_host.winfo_children():
            action.pack_forget()
        actions = {"music": self.download_button, "playlist": self.playlist_download_button,
                   "video": self.video_download_button, "options": self.options_save_button}
        actions[page_key].configure(bg=THEMES[self.applied_theme]["card"])
        actions[page_key].canvas.configure(bg=THEMES[self.applied_theme]["card"])
        actions[page_key].pack()
        self.footer_card.after_idle(self.footer_card._fit_height)
        titles = {"music": "Музика по розділах", "playlist": "Плейлист",
                  "video": "Відео", "options": "Опції"}
        self._set_label(self.page_title, f"Колекція  /  {titles[page_key]}")
        for key, button in self.nav_buttons.items():
            button.set_variant("primary" if key == page_key else "secondary")
        self.active_page = page_key

    def _tr(self, text):
        return translate(text, self.language)

    def _set_label(self, widget, text):
        widget._i18n_source = text
        widget.configure(text=self._tr(text))

    @staticmethod
    def _set_dynamic_label(widget, text):
        if hasattr(widget, "_i18n_source"):
            del widget._i18n_source
        widget.configure(text=text)

    def _apply_language(self, language, *, persist=True):
        self.language = language if language in ("uk", "en") else "uk"
        for widget in self._walk_widgets(self.root):
            if isinstance(widget, RoundedButton):
                widget.translate = self._tr
                widget.set_text(widget.source_label)
            elif isinstance(widget, Choice):
                widget.set_translator(self._tr)
            elif isinstance(widget, EmptyState):
                widget.translate = self._tr
                widget._draw()
            elif isinstance(widget, ttk.Treeview):
                sources = getattr(widget, "_i18n_headings", {})
                for column in ("#0", *widget["columns"]):
                    heading = widget.heading(column)
                    source = sources.get(column, heading.get("text", ""))
                    if source:
                        sources[column] = source
                        widget.heading(column, text=self._tr(source))
                widget._i18n_headings = sources
            elif isinstance(widget, tk.Label) and not widget.cget("textvariable"):
                source = getattr(widget, "_i18n_source", None)
                if source is None:
                    candidate = widget.cget("text")
                    # Only claim known interface strings. Paths and media titles
                    # are user data and must never be translated.
                    if candidate not in UK_TO_EN:
                        continue
                    source = candidate
                    widget._i18n_source = source
                widget.configure(text=self._tr(source))
        if hasattr(self, "status"):
            self.status.set(self._tr(getattr(self, "_status_source", "Перевіряю інструменти…")))
        if hasattr(self, "options_language"):
            self.options_language.set(self.language)
        if getattr(self, "active_page", None):
            self._show_page(self.active_page)
        if persist:
            self._save_settings()

    def _change_language_from_selection(self, _event=None):
        self._apply_language(self.options_language.get())

    def _music_url_changed(self, *_):
        self.video = None
        self._set_label(self.video_label, "Треки")
        self._set_label(self.chapter_count, "MP3 · з обкладинкою")
        for item in self.tree.get_children():
            self.tree.delete(item)
        self.select_button.set_state("disabled")
        self.download_button.set_state("disabled")

    def _playlist_url_changed(self, *_):
        self.playlist = None
        for item in self.playlist_tree.get_children():
            self.playlist_tree.delete(item)
        self._set_label(self.playlist_title_label, "Відео плейлиста")
        self.playlist_count_label.configure(text="")
        self.playlist_select_button.set_state("disabled")
        self.playlist_download_button.set_state("disabled")

    def _video_url_changed(self, *_):
        self.video_info = None
        self.video_preview.title = "Місце для твого наступного відео"
        self.video_preview.description = "Перевір посилання, щоб побачити назву й тривалість."
        self.video_preview._draw()
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

    def _change_theme_from_selection(self, event):
        label = event.widget.get()
        for name, theme in THEMES.items():
            if theme["label"] == label:
                self._apply_theme(name)
                break

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
        self._set_dynamic_label(self.playlist_title_label, playlist["title"])
        self._set_label(self.playlist_count_label, f"{len(children)} ВІДЕО")
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
        self.video_preview.title = video["title"]
        self.video_preview.description = (
            f"YouTube  ·  {self._duration_text(video.get('duration'))}  ·  {self._tr('Готове до завантаження')}"
        )
        self.video_preview._draw()
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
        self.download_controls.pack(fill="x", pady=(10, 0))
        return self.download_controller

    def _toggle_download_pause(self):
        controller = self.download_controller
        if controller is None or controller.cancelled:
            return
        try:
            if controller.paused:
                controller.resume()
                self.pause_download_button.set_text("Пауза")
                self._set_status("Продовжую завантаження…")
            else:
                controller.pause()
                self.pause_download_button.set_text("Продовжити")
                self._set_status("Завантаження призупинено.")
        except OSError as exc:
            self._show_dialog("Керування завантаженням", str(exc), "warning")

    def _cancel_download(self):
        controller = self.download_controller
        if controller is None or controller.cancelled:
            return
        controller.cancel()
        self.cancel_download_button.set_state("disabled")
        self.pause_download_button.set_state("disabled")
        self._set_status("Скасовую завантаження…")

    def _finish_download_controls(self):
        self.pause_download_button.set_text("Пауза")
        self.pause_download_button.set_state("disabled")
        self.cancel_download_button.set_state("disabled")
        self.download_controller = None

    def _download_cancelled(self):
        self.busy = False
        self._finish_download_controls()
        self._set_tool_actions_enabled(True)
        self._set_status("Завантаження скасовано. Вже готові файли збережено.", 0)

    def _set_status(self, text: str, value: int | None = None):
        self._status_source = text
        self.status.set(self._tr(text))
        if value is not None:
            self.progress.set_value(value)

    def _show_dialog(self, title: str, message: str, kind: str = "info"):
        theme = THEMES[self.theme_name]
        dialog = tk.Toplevel(self.root)
        dialog.title(self._tr(title))
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
        icon_canvas.create_text(17, 17, text=icon, fill=badge_text, font=(FONT, 14, "bold"))
        tk.Label(header, text=self._tr(title), bg=theme["card"], fg=theme["ink"],
                 font=(FONT, 13, "bold"), anchor="w").pack(side="left", fill="x", expand=True)
        tk.Label(content, text=self._tr(message), bg=theme["card"], fg=theme["muted"],
                 font=(FONT, 10), anchor="w", justify="left", wraplength=440).pack(fill="x")
        footer = tk.Frame(content, bg=theme["card"])
        footer.pack(fill="x", pady=(20, 0))
        RoundedButton(footer, text=self._tr("Гаразд"), command=dialog.destroy, variant="primary", height=38,
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
                "language": settings.get("language", "uk"),
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
            settings["language"] = self.language
            SETTINGS_PATH.write_text(json.dumps(settings, ensure_ascii=False, indent=2), encoding="utf-8")
        except OSError:
            pass

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
        RoundedButton.NAV_PALETTE = theme
        self.brand_mark.set_theme(theme["accent"], theme["accent_text"])
        for widget in self._walk_widgets(self.root):
            if isinstance(widget, Choice):
                widget.set_theme(theme)
            elif isinstance(widget, MediaList):
                widget.set_theme(theme)
            elif isinstance(widget, EmptyState):
                widget.set_theme(theme)
            elif isinstance(widget, RoundedCard):
                widget.border = theme["border"]
                widget.set_theme(bg=theme["bg"], fill=theme["card"])
            elif isinstance(widget, RoundedButton):
                widget._draw()
            elif isinstance(widget, RoundedEntry):
                widget.field = theme["field"]
                widget.default_border = theme["outline"]
                widget.border = theme["focus"] if str(widget.tk.call("focus")) == str(widget.entry) else theme["outline"]
                widget.focus_color = theme["focus"]
                widget.text_color = theme["ink"]
                widget.selection_color = theme["entry_select"]
                widget.placeholder.configure(bg=theme["field"], fg=theme["muted"])
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
        self.style.configure("Vertical.TScrollbar", background=theme["scrollbar"], troughcolor=theme["card"], relief="flat",
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
        self.theme_selector.set(theme["label"])
        self.options_theme.set(theme["label"])
        if save:
            self._save_settings()

    @staticmethod
    def _walk_widgets(parent):
        for child in parent.winfo_children():
            yield child
            yield from TunekaApp._walk_widgets(child)

    def _choose_download_parent(self):
        theme = THEMES[self.theme_name]
        start = self.download_parent.expanduser()
        if not start.is_dir():
            start = Path.home()

        dialog = tk.Toplevel(self.root)
        dialog.title(self._tr("Тека для завантажень"))
        dialog.configure(bg=theme["bg"])
        dialog.transient(self.root)
        dialog.resizable(True, True)
        dialog.minsize(600, 440)

        card = RoundedCard(dialog, bg=theme["bg"], fill=theme["card"],
                           padding=20, radius=22, expand_content=True)
        card.border = theme["border"]
        card.pack(fill="both", expand=True, padx=12, pady=12)
        content = card.content

        tk.Label(content, text=self._tr("Куди зберігати файли?"), bg=theme["card"], fg=theme["ink"],
                 font=(FONT, 17, "bold"), anchor="w").pack(fill="x")
        tk.Label(content, text=self._tr("Обери теку зі списку або введи шлях вручну."),
                 bg=theme["card"], fg=theme["muted"], font=(FONT, 10),
                 anchor="w").pack(fill="x", pady=(4, 16))

        path_row = tk.Frame(content, bg=theme["card"])
        path_row.pack(fill="x", pady=(0, 12))
        path_var = tk.StringVar(value=str(start))
        path_entry = RoundedEntry(path_row, path_var, bg=theme["card"],
                                  field=theme["field"], fg=theme["ink"])
        path_entry.pack(side="left", fill="x", expand=True, padx=(0, 8))
        path_entry.placeholder.configure(text=self._tr("Шлях до теки…"), bg=theme["field"], fg=theme["muted"])
        path_entry.border = path_entry.default_border = theme["outline"]
        path_entry.focus_color = theme["focus"]
        path_entry.selection_color = theme["entry_select"]
        path_entry.entry.configure(selectbackground=theme["entry_select"])

        browser = RoundedCard(content, bg=theme["card"], fill=theme["field"],
                              padding=7, radius=15, expand_content=True)
        browser.border = theme["border"]
        browser.pack(fill="both", expand=True)

        style_name = "Folder.Treeview"
        self.style.configure(style_name, rowheight=36, font=(FONT, 10),
                             background=theme["field"], fieldbackground=theme["field"],
                             foreground=theme["ink"], borderwidth=0, relief="flat")
        self.style.map(style_name, background=[("selected", theme["selected"])],
                       foreground=[("selected", theme["selected_fg"])])
        listing = ttk.Treeview(browser.content, show="tree", selectmode="browse",
                               style=style_name, takefocus=True)
        listing.column("#0", stretch=True, width=500)
        scrollbar = ttk.Scrollbar(browser.content, orient="vertical", command=listing.yview,
                                  style="Vertical.TScrollbar")
        listing.configure(yscrollcommand=scrollbar.set)
        listing.pack(side="left", fill="both", expand=True)
        scrollbar.pack(side="right", fill="y", padx=(5, 0))

        status = tk.Label(content, text=self._tr("Подвійне натискання відкриває теку."),
                          bg=theme["card"], fg=theme["muted"], font=(FONT, 9), anchor="w")
        status.pack(fill="x", pady=(10, 12))
        footer = tk.Frame(content, bg=theme["card"])
        footer.pack(fill="x")
        result = {"path": None}
        current = {"path": start.resolve()}
        child_paths = {}

        def navigate(path):
            candidate = Path(path).expanduser()
            if not candidate.is_absolute():
                candidate = current["path"] / candidate
            try:
                candidate = candidate.resolve(strict=True)
                if not candidate.is_dir():
                    raise NotADirectoryError(str(candidate))
                folders = sorted((item for item in candidate.iterdir() if item.is_dir()),
                                 key=lambda item: item.name.casefold())
            except (OSError, RuntimeError) as exc:
                status.configure(text=self._tr(f"Не вдалося відкрити теку: {exc}"), fg="#f07888")
                return
            current["path"] = candidate
            path_var.set(str(candidate))
            listing.delete(*listing.get_children())
            child_paths.clear()
            for index, folder in enumerate(folders):
                item_id = str(index)
                child_paths[item_id] = folder
                listing.insert("", "end", iid=item_id, text=f"📁  {folder.name}")
            status.configure(
                text=(self._tr(f"Підпапок: {len(folders)}") if folders else
                      self._tr("У цій теці немає підпапок. Її можна вибрати.")),
                fg=theme["muted"],
            )

        def enter_selected(_event=None):
            selection = listing.selection()
            if selection:
                navigate(child_paths[selection[0]])
            return "break"

        def go_to_parent():
            parent = current["path"].parent
            if parent != current["path"]:
                navigate(parent)

        def confirm():
            result["path"] = current["path"]
            dialog.destroy()

        def cancel():
            dialog.destroy()

        RoundedButton(path_row, text=self._tr("Вгору ↑"), command=go_to_parent,
                      variant="secondary", height=38, bg=theme["card"]).pack(side="right")
        RoundedButton(footer, text=self._tr("Скасувати"), command=cancel,
                      variant="secondary", height=38, bg=theme["card"]).pack(side="right")
        RoundedButton(footer, text=self._tr("Вибрати цю теку"), command=confirm,
                      variant="primary", height=38, bg=theme["card"]).pack(side="right", padx=(0, 8))

        listing.bind("<Double-1>", enter_selected)
        listing.bind("<Return>", enter_selected)
        path_entry.entry.bind("<Return>", lambda _event: navigate(path_var.get()))
        dialog.bind("<Escape>", lambda _event: cancel())
        dialog.protocol("WM_DELETE_WINDOW", cancel)
        navigate(start)

        dialog.geometry(f"700x570+{self.root.winfo_rootx()+max(0, (self.root.winfo_width()-700)//2)}"
                        f"+{self.root.winfo_rooty()+max(0, (self.root.winfo_height()-570)//2)}")
        dialog.grab_set()
        path_entry.entry.focus_set()
        self.root.wait_window(dialog)

        selected = result["path"]
        if selected is None:
            return
        self.download_parent = selected
        self._fit_destination()
        self._set_dynamic_label(self.options_folder_label, str(self.download_parent))
        try:
            self._save_settings()
        except OSError as exc:
            self._show_dialog("Налаштування", f"Теку вибрано, але не вдалося зберегти вибір: {exc}", "warning")

    def _thread_progress(self, text: str, value: int):
        def update():
            self._set_status(text)
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
        self._set_status("Готово до роботи. Додай посилання, щоб почати.", 0)

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
        self._set_dynamic_label(self.video_label, video["title"])
        chapter_total = len(video["chapters"])
        if chapter_total % 10 == 1 and chapter_total % 100 != 11:
            chapter_word = "РОЗДІЛ"
        elif 2 <= chapter_total % 10 <= 4 and not 12 <= chapter_total % 100 <= 14:
            chapter_word = "РОЗДІЛИ"
        else:
            chapter_word = "РОЗДІЛІВ"
        self._set_label(self.chapter_count, f"{chapter_total} {chapter_word}")
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
    TunekaApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
