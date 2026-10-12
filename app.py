from __future__ import annotations

import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import threading
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


def info_for(url: str) -> dict:
    result = run([str(YTDLP_PATH), "--no-warnings", "--dump-single-json", "--skip-download", url], timeout=180)
    if result.returncode:
        raise RuntimeError((result.stderr or result.stdout).strip()[-1200:] or "Не вдалося прочитати відео.")
    data = json.loads(result.stdout)
    chapters = data.get("chapters") or []
    if not chapters:
        raise RuntimeError("У цьому відео не знайдено розділів із часовими мітками.")
    return {
        "title": data.get("title", "YouTube відео"), "thumbnail": data.get("thumbnail"),
        "duration": data.get("duration"),
        "chapters": [{"index": i, "title": c.get("title") or f"Трек {i + 1}",
                      "start": c.get("start_time", 0), "end": c.get("end_time", 0)}
                     for i, c in enumerate(chapters)],
    }


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


def split_video(url: str, video: dict, selected: list[int], download_parent: Path, progress) -> tuple[Path, list[Path], bool]:
    jobdir = download_parent / safe_foldername(video["title"])
    jobdir.mkdir(parents=True, exist_ok=True)
    # Keep the downloaded source out of the user's output folder and always
    # remove it, including when yt-dlp or ffmpeg fails partway through.
    with tempfile.TemporaryDirectory(prefix=".tracksplit-", dir=jobdir) as temporary_dir:
        cover_warning = False
        cover_path = Path(temporary_dir) / "video-cover"
        progress("Завантажую обкладинку відео…", 0)
        cover_path = download_cover(video.get("thumbnail"), cover_path)
        if cover_path is None:
            cover_warning = True
        source_template = str(Path(temporary_dir) / "source.%(ext)s")
        command = [str(YTDLP_PATH), "--no-warnings", "--no-progress", "--no-colors", "--no-playlist", "-f", "bestaudio/best",
                   "-o", source_template, "--print", "after_move:filepath", url]
        progress("Завантажую аудіо з YouTube…", 0)
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                   bufsize=1, **hidden_console_kwargs())
        source = None
        output_lines = []
        for line in process.stdout or []:
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
            chapter = by_index[index]
            filename = f"{order:02d} - {safe_filename(chapter['title'])}.mp3"
            target = jobdir / filename
            progress(f"Нарізаю треки… {order}/{len(selected)}", 90 + int(10 * (order - 1) / max(1, len(selected))))
            # Apply seek and duration to the audio input. Putting -t on the
            # output also counts the one-frame cover stream and can truncate
            # the MP3 to only a fraction of a second.
            command = [str(FFMPEG_PATH), "-hide_banner", "-loglevel", "error", "-y", "-ss",
                       timestamp(chapter["start"]), "-t", timestamp(chapter["end"] - chapter["start"]),
                       "-i", str(source)]
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
            result = run(command, timeout=1800)
            if result.returncode and cover_path is not None:
                # Still produce usable MP3s when a platform's ffmpeg build
                # cannot decode or embed the thumbnail format.
                cover_warning = True
                command = [str(FFMPEG_PATH), "-hide_banner", "-loglevel", "error", "-y", "-ss",
                           timestamp(chapter["start"]), "-i", str(source), "-t",
                           timestamp(chapter["end"] - chapter["start"]), "-vn", "-c:a", "libmp3lame",
                           "-q:a", "2", "-metadata", f"title={chapter['title']}", "-metadata",
                           f"album={video['title']}", str(target)]
                result = run(command, timeout=1800)
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
        if self.canvas.focus_displayof() is self.canvas:
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
        self.bind("<Configure>", self._draw)

    def set_theme(self, *, trough, fill):
        self.trough, self.fill = trough, fill
        self._draw()

    def set_value(self, value: int):
        self.target = max(0.0, min(100.0, float(value)))
        if not getattr(self, "animating", False):
            self.animating = True
            self.after(16, self._animate)

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
        self.applied_theme = "olive"
        self.last_output_dir: Path | None = None
        self.root.title("Tracksplit")
        self.root.geometry("980x800")
        self.root.minsize(760, 620)
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
        self._build_ui()
        self._apply_theme(self.theme_name, save=False)
        self.root.after(100, self._bootstrap_tools)

    def _build_ui(self):
        bg, card, ink, muted, accent = "#151911", "#20261b", "#f0f3e9", "#a1aa95", "#c9ed72"
        outer = tk.Frame(self.root, bg=bg, padx=30, pady=24)
        outer.pack(fill="both", expand=True)

        # Keep actions and download destination outside the expanding track list.
        # The list can then shrink with the window instead of pushing buttons off-screen.
        footer = tk.Frame(outer, bg=bg)
        footer.pack(side="bottom", fill="x")

        destination = RoundedCard(footer, bg=bg, fill=card, padding=16, radius=20)
        destination.pack(fill="x", pady=(12, 0))
        destination_row = tk.Frame(destination.content, bg=card)
        destination_row.pack(fill="x")
        tk.Label(destination_row, text="ЗБЕРІГАТИ В", bg=card, fg=muted,
                 font=("TkDefaultFont", 9, "bold")).pack(side="left", padx=(0, 15))
        self.destination_label = tk.Label(destination_row, text=str(self.download_parent), bg=card, fg="#b5beaa",
                                          font=("TkDefaultFont", 9), anchor="w")
        self.destination_label.pack(side="left", fill="x", expand=True)
        RoundedButton(destination_row, text="Вибрати теку…", command=self._choose_download_parent,
                      variant="secondary", height=36, bg=card).pack(side="right")

        controls = tk.Frame(footer, bg=bg)
        controls.pack(fill="x", pady=(10, 0))
        self.download_button = RoundedButton(controls, text="Завантажити вибрані MP3", command=self._download,
                                             variant="primary", height=42, bg=bg)
        self.download_button.set_state("disabled")
        self.download_button.pack(side="left")
        self.open_folder_button = RoundedButton(controls, text="Відкрити теку", command=self._open_downloads,
                                                variant="secondary", height=38, bg=bg)
        self.open_folder_button.pack(side="right")
        self.status = tk.StringVar(value="Перевіряю інструменти застосунку…")
        self.status_label = tk.Label(footer, textvariable=self.status, bg=bg, fg=muted,
                                     font=("TkDefaultFont", 9), anchor="w", justify="left", wraplength=880)
        self.status_label.pack(anchor="w", fill="x", pady=(12, 6))
        status_row = tk.Frame(footer, bg=bg)
        status_row.pack(fill="x", pady=(0, 2))
        self.progress = AnimatedProgressBar(status_row, bg=bg, trough="#2a3124", fill="#a9d65d", height=10)
        self.progress.pack(fill="x")

        header = tk.Frame(outer, bg=bg)
        header.pack(fill="x", pady=(0, 19))
        brand = tk.Frame(header, bg=bg)
        brand.pack(side="left")
        tk.Label(brand, text="♫", bg=accent, fg="#263218", width=2, height=1,
                 font=("TkDefaultFont", 16, "bold")).pack(side="left", padx=(0, 11), ipady=3)
        tk.Label(brand, text="TRACKSPLIT", bg=bg, fg=ink,
                 font=("TkDefaultFont", 13, "bold")).pack(side="left")
        RoundedButton(header, text="Оновити інструменти", command=self._update_tools,
                      variant="secondary", height=36, bg=bg).pack(side="right", anchor="s")
        self.theme_button = RoundedButton(header, text="Тема: Оливкова", command=self._toggle_theme,
                                          variant="secondary", height=36, bg=bg)
        self.theme_button.pack(side="right", anchor="s", padx=(0, 9))

        tk.Label(outer, text="Мікс, який стає плейлистом.", bg=bg, fg=ink,
                 font=("TkDefaultFont", 25, "bold")).pack(anchor="w")
        tk.Label(outer, text="Встав посилання на відео з розділами — вибери треки та збережи їх з обкладинкою.",
                 bg=bg, fg=muted, font=("TkDefaultFont", 10)).pack(anchor="w", pady=(5, 17))

        source = RoundedCard(outer, bg=bg, fill=card, padding=20, radius=20)
        source.pack(fill="x")
        source_head = tk.Frame(source.content, bg=card)
        source_head.pack(fill="x", pady=(0, 9))
        tk.Label(source_head, text="01", bg=accent, fg="#33451d", width=3,
                 font=("TkDefaultFont", 9, "bold")).pack(side="left", padx=(0, 9), ipady=3)
        tk.Label(source_head, text="ПОСИЛАННЯ НА YOUTUBE", bg=card, fg=ink,
                 font=("TkDefaultFont", 9, "bold")).pack(side="left")
        row = tk.Frame(source.content, bg=card)
        row.pack(fill="x")
        self.url = tk.StringVar(value="")
        self.url_entry = RoundedEntry(row, textvariable=self.url, bg=card, field="#181d14", fg=ink)
        self.url_entry.pack(side="left", fill="x", expand=True, padx=(0, 10))
        self.url_entry.entry.bind("<Return>", lambda _event: self._find_chapters())
        self.find_button = RoundedButton(row, text="Знайти розділи  →", command=self._find_chapters,
                                         variant="primary", height=46, bg=card)
        self.find_button.pack(side="right")

        video_head = tk.Frame(outer, bg=bg)
        video_head.pack(fill="x", pady=(19, 8))
        self.video_label = tk.Label(video_head, text="Треки з відео з’являться тут", bg=bg, fg=ink,
                                    font=("TkDefaultFont", 13, "bold"), anchor="w")
        self.chapter_count = tk.Label(video_head, text="РОЗДІЛИ", bg=bg, fg=muted,
                                      font=("TkDefaultFont", 9, "bold"))
        self.chapter_count.pack(side="right", padx=(12, 15))
        self.select_button = RoundedButton(video_head, text="Зняти вибір", command=self._toggle_selection,
                                           variant="secondary", height=34, bg=bg)
        self.select_button.set_state("disabled")
        self.select_button.pack(side="right")
        self.video_label.pack(side="left", fill="x", expand=True)

        list_frame = RoundedCard(outer, bg=bg, fill=card, padding=10, radius=20, expand_content=True)
        list_frame.pack(fill="both", expand=True)
        self.tree = ttk.Treeview(list_frame.content, columns=("title", "start"), show="headings", selectmode="extended")
        self.tree.heading("title", text="НАЗВА ТРЕКУ")
        self.tree.heading("start", text="ПОЧАТОК")
        self.tree.column("title", anchor="w", width=650, minwidth=260, stretch=True)
        self.tree.column("start", anchor="e", width=100, stretch=False)
        scrollbar = ttk.Scrollbar(list_frame.content, orient="vertical", command=self.tree.yview)
        self.tree.configure(yscrollcommand=scrollbar.set)
        self.tree.pack(side="left", fill="both", expand=True, padx=(1, 0), pady=1)
        scrollbar.pack(side="right", fill="y")

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
        self.progress.set_theme(trough=theme["progress_trough"], fill=theme["accent"])
        self.status_label.configure(fg=theme["muted"])
        self.theme_name = self.applied_theme = theme_name
        self.theme_button.set_text(f"Тема: {theme['label']}")
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
        try:
            self._save_settings()
        except OSError as exc:
            self._show_dialog("Налаштування", f"Теку вибрано, але не вдалося зберегти вибір: {exc}", "warning")

    def _thread_progress(self, text: str, value: int):
        self.root.after(0, lambda: self._set_status(text, value))

    def _bootstrap_tools(self):
        self.busy = True
        self.find_button.set_state("disabled")
        def worker():
            try:
                ensure_tools(self._thread_progress)
                self.root.after(0, self._tools_ready)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._tools_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _tools_ready(self):
        self.busy = False
        self.find_button.set_state("normal")
        self._set_status(f"Готово. yt-dlp {self._version(YTDLP_PATH)} · ffmpeg {self._version(FFMPEG_PATH)}", 100)

    def _tools_failed(self, error: str):
        self.busy = False
        self.find_button.set_state("normal")
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
        self.find_button.set_state("disabled")
        self._set_status("Оновлюю бінарники…", 0)
        def worker():
            try:
                ensure_tools(self._thread_progress, force=True)
                self.root.after(0, self._tools_ready)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._tools_failed(error))
        threading.Thread(target=worker, daemon=True).start()

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
                self.root.after(0, lambda: self._show_video(video))
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._info_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _show_video(self, video: dict):
        self.busy = False
        self.video = video
        self.find_button.set_state("normal")
        for item in self.tree.get_children():
            self.tree.delete(item)
        for chapter in video["chapters"]:
            start = int(chapter["start"])
            self.tree.insert("", "end", iid=str(chapter["index"]), values=(chapter["title"], f"{start // 60:02}:{start % 60:02}"))
        self.tree.selection_set(self.tree.get_children())
        self.video_label.configure(text=video["title"])
        self.chapter_count.configure(text=f"{len(video['chapters'])} РОЗДІЛІВ")
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
        def worker():
            try:
                output_dir, tracks, cover_warning = split_video(url, video, selected, download_parent, self._thread_progress)
                self.root.after(0, lambda: self._download_done(output_dir, tracks, cover_warning))
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._download_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _download_done(self, output_dir: Path, tracks: list[Path], cover_warning: bool):
        self.busy = False
        self.last_output_dir = output_dir
        self.find_button.set_state("normal")
        self.download_button.set_state("normal")
        note = " Деякі обкладинки не вдалося вбудувати." if cover_warning else " Обкладинку відео додано до треків."
        self._set_status(f"Збережено {len(tracks)} MP3 у {output_dir}.{note}", 100)
        self._show_dialog("Завантаження завершено",
                          f"Створено {len(tracks)} треків.{note}\n\nТека: {output_dir}", "success")

    def _download_failed(self, error: str):
        self.busy = False
        self.find_button.set_state("normal")
        self.download_button.set_state("normal")
        self._set_status("Помилка під час завантаження.", 0)
        self._show_dialog("Помилка завантаження", error, "error")

    @staticmethod
    def _open_path(path: Path):
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
