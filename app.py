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
from tkinter import filedialog, messagebox, ttk
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


class TrackSplitApp:
    def __init__(self, root: tk.Tk):
        self.root = root
        self.video: dict | None = None
        self.busy = False
        self.download_parent = self._load_download_parent()
        self.last_output_dir: Path | None = None
        self.root.title("Tracksplit")
        self.root.geometry("900x720")
        self.root.minsize(720, 560)
        self.root.configure(bg="#f4f3ed")
        self.style = ttk.Style()
        try:
            self.style.theme_use("clam")
        except tk.TclError:
            pass
        self.style.configure("TFrame", background="#f4f3ed")
        self.style.configure("Panel.TFrame", background="#fffefa")
        self.style.configure("TLabel", background="#f4f3ed", foreground="#23251f", font=("TkDefaultFont", 10))
        self.style.configure("Title.TLabel", font=("TkDefaultFont", 26, "bold"))
        self.style.configure("Hint.TLabel", foreground="#777970", font=("TkDefaultFont", 9))
        self.style.configure("Treeview", rowheight=31, font=("TkDefaultFont", 10), background="#fffefa", fieldbackground="#fffefa")
        self.style.configure("Treeview.Heading", font=("TkDefaultFont", 9, "bold"))
        self.style.configure("Accent.TButton", font=("TkDefaultFont", 10, "bold"))
        self._build_ui()
        self.root.after(100, self._bootstrap_tools)

    def _build_ui(self):
        outer = ttk.Frame(self.root, padding=(28, 24))
        outer.pack(fill="both", expand=True)

        # Keep actions and download destination outside the expanding track list.
        # The list can then shrink with the window instead of pushing buttons off-screen.
        footer = ttk.Frame(outer)
        footer.pack(side="bottom", fill="x")

        destination = ttk.Frame(footer, style="Panel.TFrame", padding=(14, 10))
        destination.pack(fill="x", pady=(12, 0))
        destination_row = ttk.Frame(destination, style="Panel.TFrame")
        destination_row.pack(fill="x")
        ttk.Label(destination_row, text="ТЕКА ДЛЯ ЗАВАНТАЖЕННЯ", background="#fffefa",
                  font=("TkDefaultFont", 9, "bold")).pack(side="left", padx=(0, 12))
        self.destination_label = ttk.Label(destination_row, text=str(self.download_parent),
                                           background="#fffefa", style="Hint.TLabel")
        self.destination_label.pack(side="left", fill="x", expand=True)
        ttk.Button(destination_row, text="Вибрати теку…", command=self._choose_download_parent).pack(side="right")

        controls = ttk.Frame(footer)
        controls.pack(fill="x", pady=(10, 0))
        self.download_button = ttk.Button(controls, text="Завантажити вибрані MP3", command=self._download, style="Accent.TButton", state="disabled")
        self.download_button.pack(side="left", ipady=6)
        self.open_folder_button = ttk.Button(controls, text="Відкрити теку", command=self._open_downloads)
        self.open_folder_button.pack(side="right")
        self.progress = ttk.Progressbar(footer, maximum=100, mode="determinate")
        self.progress.pack(fill="x", pady=(10, 4))
        self.status = tk.StringVar(value="Перевіряю інструменти застосунку…")
        ttk.Label(footer, textvariable=self.status, style="Hint.TLabel", wraplength=820).pack(anchor="w")

        header = ttk.Frame(outer)
        header.pack(fill="x", pady=(0, 18))
        ttk.Label(header, text="Tracksplit", style="Title.TLabel").pack(side="left")
        ttk.Button(header, text="Оновити інструменти", command=self._update_tools).pack(side="right", anchor="s")
        ttk.Label(outer, text="Знайди відео з розділами — отримай окремі MP3 з правильними назвами.",
                  style="Hint.TLabel").pack(anchor="w", pady=(0, 18))

        source = ttk.Frame(outer, style="Panel.TFrame", padding=16)
        source.pack(fill="x")
        ttk.Label(source, text="ПОСИЛАННЯ НА YOUTUBE", background="#fffefa", font=("TkDefaultFont", 9, "bold")).pack(anchor="w", pady=(0, 8))
        row = ttk.Frame(source, style="Panel.TFrame")
        row.pack(fill="x")
        self.url = tk.StringVar(value="https://www.youtube.com/watch?v=G8NpktMnLKU")
        self.url_entry = ttk.Entry(row, textvariable=self.url, font=("TkDefaultFont", 11))
        self.url_entry.pack(side="left", fill="x", expand=True, ipady=8, padx=(0, 10))
        self.find_button = ttk.Button(row, text="Знайти розділи", command=self._find_chapters, style="Accent.TButton")
        self.find_button.pack(side="right", ipady=5)

        self.video_label = ttk.Label(outer, text="Встав посилання на відео з розділами.", font=("TkDefaultFont", 12, "bold"))
        self.video_label.pack(anchor="w", pady=(22, 8))
        tools_row = ttk.Frame(outer)
        tools_row.pack(fill="x", pady=(0, 6))
        self.chapter_count = ttk.Label(tools_row, text="РОЗДІЛИ", style="Hint.TLabel")
        self.chapter_count.pack(side="left")
        self.select_button = ttk.Button(tools_row, text="Зняти вибір", command=self._toggle_selection, state="disabled")
        self.select_button.pack(side="right")

        list_frame = ttk.Frame(outer)
        list_frame.pack(fill="both", expand=True)
        self.tree = ttk.Treeview(list_frame, columns=("title", "start"), show="headings", selectmode="extended")
        self.tree.heading("title", text="НАЗВА ТРЕКУ")
        self.tree.heading("start", text="ПОЧАТОК")
        self.tree.column("title", anchor="w", width=650)
        self.tree.column("start", anchor="e", width=100, stretch=False)
        scrollbar = ttk.Scrollbar(list_frame, orient="vertical", command=self.tree.yview)
        self.tree.configure(yscrollcommand=scrollbar.set)
        self.tree.pack(side="left", fill="both", expand=True)
        scrollbar.pack(side="right", fill="y")

    def _set_status(self, text: str, value: int | None = None):
        self.status.set(text)
        if value is not None:
            self.progress["value"] = value

    @staticmethod
    def _load_download_parent() -> Path:
        try:
            saved = json.loads(SETTINGS_PATH.read_text(encoding="utf-8")).get("download_parent")
            if saved:
                return Path(saved).expanduser()
        except (OSError, ValueError, AttributeError):
            pass
        return DOWNLOADS

    def _choose_download_parent(self):
        selected = filedialog.askdirectory(
            title="Виберіть теку для збереження відео", initialdir=str(self.download_parent), mustexist=True
        )
        if not selected:
            return
        self.download_parent = Path(selected)
        self.destination_label.configure(text=str(self.download_parent))
        try:
            SETTINGS_PATH.write_text(
                json.dumps({"download_parent": str(self.download_parent)}, ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
        except OSError as exc:
            messagebox.showwarning("Налаштування", f"Теку вибрано, але не вдалося зберегти вибір: {exc}")

    def _thread_progress(self, text: str, value: int):
        self.root.after(0, lambda: self._set_status(text, value))

    def _bootstrap_tools(self):
        self.busy = True
        self.find_button.configure(state="disabled")
        def worker():
            try:
                ensure_tools(self._thread_progress)
                self.root.after(0, self._tools_ready)
            except Exception as exc:
                self.root.after(0, lambda error=str(exc): self._tools_failed(error))
        threading.Thread(target=worker, daemon=True).start()

    def _tools_ready(self):
        self.busy = False
        self.find_button.configure(state="normal")
        self._set_status(f"Готово. yt-dlp {self._version(YTDLP_PATH)} · ffmpeg {self._version(FFMPEG_PATH)}", 100)

    def _tools_failed(self, error: str):
        self.busy = False
        self.find_button.configure(state="normal")
        self._set_status("Не вдалося підготувати інструменти.", 0)
        messagebox.showerror("Tracksplit — інструменти", f"{error}\n\nПеревірте підключення до інтернету й повторіть через «Оновити інструменти».")

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
        self.find_button.configure(state="disabled")
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
            messagebox.showwarning("Посилання", "Встав коректне HTTPS-посилання на YouTube.")
            return
        if self.busy:
            return
        self.busy = True
        self.find_button.configure(state="disabled")
        self.download_button.configure(state="disabled")
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
        self.find_button.configure(state="normal")
        for item in self.tree.get_children():
            self.tree.delete(item)
        for chapter in video["chapters"]:
            start = int(chapter["start"])
            self.tree.insert("", "end", iid=str(chapter["index"]), values=(chapter["title"], f"{start // 60:02}:{start % 60:02}"))
        self.tree.selection_set(self.tree.get_children())
        self.video_label.configure(text=video["title"])
        self.chapter_count.configure(text=f"{len(video['chapters'])} РОЗДІЛІВ")
        self.select_button.configure(state="normal", text="Зняти вибір")
        self.download_button.configure(state="normal")
        self._set_status("Вибери треки й натисни «Завантажити вибрані MP3».", 0)

    def _info_failed(self, error: str):
        self.busy = False
        self.find_button.configure(state="normal")
        self._set_status("Не вдалося прочитати відео.", 0)
        messagebox.showerror("YouTube", error)

    def _toggle_selection(self):
        children = self.tree.get_children()
        if len(self.tree.selection()) == len(children):
            self.tree.selection_remove(*children)
            self.select_button.configure(text="Вибрати всі")
        else:
            self.tree.selection_set(children)
            self.select_button.configure(text="Зняти вибір")

    def _download(self):
        if not self.video or self.busy:
            return
        selected = sorted(int(item) for item in self.tree.selection())
        if not selected:
            messagebox.showinfo("Треки", "Вибери хоча б один розділ.")
            return
        self.busy = True
        self.find_button.configure(state="disabled")
        self.download_button.configure(state="disabled")
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
        self.find_button.configure(state="normal")
        self.download_button.configure(state="normal")
        note = " Деякі обкладинки не вдалося вбудувати." if cover_warning else " Обкладинку відео додано до треків."
        self._set_status(f"Збережено {len(tracks)} MP3 у {output_dir}.{note}", 100)
        messagebox.showinfo("Готово", f"Створено {len(tracks)} треків.{note}\n\nТека: {output_dir}")

    def _download_failed(self, error: str):
        self.busy = False
        self.find_button.configure(state="normal")
        self.download_button.configure(state="normal")
        self._set_status("Помилка під час завантаження.", 0)
        messagebox.showerror("Завантаження", error)

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
            messagebox.showerror("Тека", str(exc))

    def _open_downloads(self):
        self._open_path(self.last_output_dir or self.download_parent)


def main():
    root = tk.Tk()
    TrackSplitApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
