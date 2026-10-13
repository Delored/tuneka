"""Small built-in UI translations for Tuneka."""
import re

UK_TO_EN = {
    "uk": "Ukrainian", "en": "English",
    "ВАША КОЛЕКЦІЯ": "YOUR LIBRARY",
    "♫   Музика": "♫   Music", "≡   Плейлисти": "≡   Playlists",
    "▷   Відео": "▷   Video", "⚙   Налаштування": "⚙   Settings",
    "Медіа, які завжди поруч.": "Your media, always close.",
    "На вашому пристрої": "On your device",
    "↻  Оновити інструменти": "↻  Update tools",
    "Змінити…": "Change…", "ЗБЕРЕГТИ В": "SAVE TO",
    "Пауза": "Pause", "Продовжити": "Resume", "Скасувати": "Cancel",
    "Відкрити теку  ↗": "Open folder  ↗",
    "Відкрити теку": "Open folder",
    "ПОСИЛАННЯ НА YOUTUBE": "YOUTUBE LINK",
    "Обробити  →": "Process  →",
    "Обробка…": "Processing…",
    "Знайти відео  →": "Find videos  →",
    "Перевірити посилання  →": "Check link  →",
    "Зняти вибір": "Clear selection", "Завантажити MP3": "Download MP3",
    "Вибрати всі": "Select all",
    "↓  Завантажити вибрані": "↓  Download selected",
    "↓  Завантажити відео": "↓  Download video",
    "Зберегти опції": "Save settings", "Вибрати теку…": "Choose folder…",
    "↓  Завантажити MP3": "↓  Download MP3",
    "Колекція  /  Музика по розділах": "Library  /  Music by chapters",
    "Колекція  /  Плейлист": "Library  /  Playlist",
    "Колекція  /  Відео": "Library  /  Video",
    "Колекція  /  Опції": "Library  /  Settings",
    "Музика по розділах": "Music by chapters",
    "Плейлист": "Playlist", "Відео": "Video", "Опції": "Settings",
    "Тривалість": "Duration",
    "Назва треку": "Track title", "Початок": "Start",
    "Кожен мікс починається з посилання": "Every mix starts with a link",
    "Улюблене — цілим плейлистом": "A playlist full of favorites",
    "Додай посилання вище — тут з’являться треки для вибору.":
        "Add a link above and tracks will appear here.",
    "Встав посилання на YouTube-плейлист, щоб вибрати відео.":
        "Paste a YouTube playlist link to choose videos.",
    "Твій наступний плейлист.": "Your next playlist.",
    "Мікс із YouTube → окремі MP3 з назвами й обкладинкою. Без розділів — один трек.":
        "A YouTube mix becomes separate MP3 files with titles and cover art. No chapters? One track.",
    "Відео, яке лишається з тобою.": "Video that stays with you.",
    "Додати кілька…": "Add multiple…",
    "Додати": "Add",
    "Кілька посилань": "Multiple links",
    "Додай посилання на YouTube": "Add YouTube links",
    "Встав одне посилання на рядок. Повтори не додаватимуться.":
        "Paste one link per line. Duplicates are ignored.",
    "Не схожі на посилання YouTube:": "These do not look like YouTube links:",
    "Додай хоча б одне посилання.": "Add at least one link.",
    "Відеофайли буде завантажено послідовно у вибраній якості.":
        "Videos will be downloaded one by one at the selected quality.",
    "Не вдалося перевірити:": "Could not check:",
    "Обери якість і формат. Дивись улюблене, коли зручно.":
        "Choose the quality and format. Watch your favorites whenever you like.",
    "Встав посилання на відео з розділами — отримай окремі MP3. Якщо розділів немає, відео стане одним треком.":
        "Paste a video link with chapters to get separate MP3 files. Without chapters, the video becomes one track.",
    "Вибери відео з плейлиста й завантаж їх як окремі MP3 або відеофайли.":
        "Choose videos from a playlist and download them as separate MP3 or video files.",
    "Збережи окреме відео у вибраній якості та стандартному контейнері.":
        "Save a video at the selected quality and in a standard format.",
    "Задай теки та формати, які Tuneka використовуватиме за замовчуванням.":
        "Set the folders and formats Tuneka will use by default.",
    "Треки": "Tracks", "MP3 · з обкладинкою": "MP3 · with cover art",
    "ФОРМАТ ЗАВАНТАЖЕННЯ": "DOWNLOAD FORMAT",
    "MP3 · 1 файл на відео": "MP3 · 1 file per video",
    "MP3 · один файл на відео": "MP3 · one file per video",
    "ЯКІСТЬ": "QUALITY", "ФОРМАТ": "FORMAT",
    "ВІДЕО · ЯКІСТЬ І ФОРМАТ": "VIDEO · QUALITY AND FORMAT",
    "ЗА ЗАМОВЧУВАННЯМ · ЯКІСТЬ І ФОРМАТ": "DEFAULT · QUALITY AND FORMAT",
    "РОЗДІЛЬНІСТЬ": "RESOLUTION", "ФОРМАТ ПЛЕЙЛИСТІВ": "PLAYLIST FORMAT",
    "КОЛІРНА ТЕМА": "COLOR THEME", "ТЕКА ДЛЯ ЗАВАНТАЖЕНЬ": "DOWNLOAD FOLDER",
    "МОВА ІНТЕРФЕЙСУ": "INTERFACE LANGUAGE",
    "Автоматично": "Automatic", "Оливкова": "Olive", "Фіолетова": "Purple",
    "Світла": "Light", "Українська": "Ukrainian",
    "Знайди відео з розділами — отримай окремі MP3 з правильними назвами.":
        "Find videos with chapters and save separate MP3 files with the right names.",
    "Мікс, який стає плейлистом.": "A mix that becomes a playlist.",
    "Встав посилання на відео з розділами — вибери треки та збережи їх з обкладинкою.":
        "Paste a video link with chapters, choose tracks, and save them with cover art.",
    "Уся добірка. Одним рухом.": "The whole playlist. In one go.",
    "Вибери потрібне з плейлиста. Збережи як музику або відео.":
        "Choose what you want from a playlist. Save it as music or video.",
    "Тека для завантажень": "Download folder",
    "Перевір посилання": "Check link",
    "Перевірити посилання": "Check link",
    "Встав коректне HTTPS-посилання на YouTube плейлист.":
        "Enter a valid HTTPS link to a YouTube playlist.",
    "Встав коректне HTTPS-посилання на YouTube відео.":
        "Enter a valid HTTPS link to a YouTube video.",
    "Встав коректне HTTPS-посилання на YouTube.": "Enter a valid HTTPS link to YouTube.",
    "Читаю плейлист…": "Reading playlist…",
    "Не вдалося прочитати плейлист.": "Could not read the playlist.",
    "Не вибрано відео": "No videos selected",
    "Вибери хоча б одне відео з плейлиста.": "Select at least one video from the playlist.",
    "Готую завантаження плейлиста…": "Preparing playlist download…",
    "Не вдалося завантажити плейлист.": "Could not download the playlist.",
    "Завантаження плейлиста завершено": "Playlist download complete",
    "Завантаження завершено": "Download complete",
    "Завантаження відео завершено": "Video downloads complete",
    "Частину посилань не вдалося прочитати": "Some links could not be read",
    "Помилка завантаження плейлиста": "Playlist download error",
    "Перевіряю відео…": "Checking video…",
    "Готово до завантаження:": "Ready to download:",
    "Не вдалося прочитати відео.": "Could not read the video.",
    "Не вдалося завантажити відео.": "Could not download the video.",
    "Значення збережено як параметри за замовчуванням.": "Values saved as defaults.",
    "Керування завантаженням": "Download controls",
    "Продовжую завантаження…": "Resuming download…",
    "Скасовую завантаження…": "Cancelling download…",
    "Читаю розділи відео…": "Reading video chapters…",
    "Оновлюю бінарники…": "Updating tools…",
    "Не вибрано треки": "No tracks selected",
    "Вибери хоча б один розділ.": "Select at least one chapter.",
    "Помилка під час завантаження.": "Download failed.",
    "Завантаження плейлиста": "Playlist download",
    "Підпапок: ": "Subfolders: ",
    "У плейлисті ": "Playlist has ",
    " відео. Вибери потрібні й завантаж.": " videos. Select the ones you want and download them.",
    "Вибери треки й натисни «Завантажити вибрані MP3».":
        "Select tracks and click ‘Download selected MP3’.",
    "Теку вибрано, але не вдалося зберегти вибір:": "Folder selected, but the choice could not be saved:",
    "Під час завантаження сталася помилка.": "An error occurred during download.",
    "Завантажити плейлист": "Download playlist",
    "Завантажити вибрані MP3": "Download selected MP3",
    "Завантажити вибрані": "Download selected",
    "Треки з відео з’являться тут": "Tracks will appear here",
    "Відео плейлиста з’являться тут": "Playlist videos will appear here",
    "Відео плейлиста": "Playlist videos",
    "Треки з відео з'являться тут": "Tracks will appear here",
    "Встав посилання на YouTube…": "Paste a YouTube link…",
    "Треки з’являться тут": "Tracks will appear here",
    "ВІДЕО": "VIDEO", "ТРИВАЛІСТЬ": "DURATION", "НАЗВА ТРЕКУ": "TRACK TITLE",
    "ПОЧАТОК": "START", "РОЗДІЛИ": "CHAPTERS",
    "Місце для твого наступного відео": "Your next video goes here",
    "Перевір посилання, щоб побачити назву й тривалість.": "Check a link to see its title and duration.",
    "YouTube · Готове до завантаження": "YouTube · Ready to download",
    "Готове до завантаження": "Ready to download",
    "Куди зберігати файли?": "Where should files be saved?",
    "Обери теку зі списку або введи шлях вручну.": "Choose a folder from the list or enter a path.",
    "Шлях до теки…": "Folder path…", "Подвійне натискання відкриває теку.": "Double-click a folder to open it.",
    "У цій теці немає підпапок. Її можна вибрати.": "This folder has no subfolders. You can select it.",
    "Вгору ↑": "Up ↑", "Вибрати цю теку": "Choose this folder",
    "Гаразд": "OK", "Стан завантаження": "Download status",
    "Опції збережено": "Settings saved",
    "Значення збережено як параметри за замовчуванням.": "Values saved as defaults.",
    "Відео завантажено": "Video downloaded", "Файл збережено:": "File saved:",
    "Помилка завантаження відео": "Video download error",
    "Не вдалося завантажити відео.": "Could not download the video.",
    "Завантаження скасовано. Вже готові файли збережено.": "Download cancelled. Completed files were saved.",
    "Завантаження призупинено.": "Download paused.",
    "Продовжую завантаження…": "Resuming download…",
    "Скасовую завантаження…": "Cancelling download…",
    "Оновлюю бінарники…": "Updating tools…",
    "Готово до завантаження:": "Ready to download:",
    "Вибери треки й натисни «Завантажити вибрані MP3».":
        "Select tracks and click ‘Download selected MP3’.",
    "Завантажую відео…": "Downloading video…",
    "Завантажую аудіо з YouTube…": "Downloading audio from YouTube…",
    "Перевіряю інструменти…": "Checking tools…",
    "Готово до роботи. Додай посилання, щоб почати.": "Ready. Add a link to get started.",
    "Не вдалося підготувати інструменти.": "Could not prepare tools.",
    "Інструменти оновлено.": "Tools updated.",
    "Не вдалося оновити інструменти.": "Could not update tools.",
    "Збережено ": "Saved ", "Відео збережено:": "Video saved:",
    "Вибрано ": "Selected ", "Завантаження скасовано": "Download cancelled",
    "Налаштування": "Settings", "Теку вибрано, але не вдалося зберегти вибір:":
        "Folder selected, but the choice could not be saved:",
    "Не вдалося відкрити теку": "Could not open folder",
    "Не вдалося прочитати відео": "Could not read the video",
    "Не вдалося прочитати плейлист": "Could not read the playlist",
    "Помилка завантаження": "Download error",
    "Не вдалося відкрити теку:": "Could not open folder:",
    "Перевір посилання": "Check link",
    "Не вдалося підготувати інструменти": "Could not prepare tools",
    "Відео завантажено": "Video downloaded",
    "Треків": "Tracks",
    "Створено ": "Created ",
    "MP3-файлів": "MP3 files", "MP3 у ": "MP3 in ",
    "Деякі обкладинки не вдалося вбудувати.": "Some cover art could not be embedded.",
    "Обкладинку відео додано до треків.": "Video cover art was added to the tracks.",
    "Тека:": "Folder:",
    "треки": "tracks", "відео": "videos", "ВІДЕО": "VIDEO",
}

# Internal error messages are translated only when shown in a dialog. Unknown
# lines (such as yt-dlp or ffmpeg diagnostics) are kept verbatim for debugging.
ERROR_TO_EN = {
    "Завантаження перенаправлено на недозволене джерело.":
        "The download was redirected to an untrusted source.",
    "GitHub API перенаправив запит на неочікуваний хост.":
        "The GitHub API request was redirected to an unexpected host.",
    "GitHub повернув неочікувані метадані релізу yt-dlp.":
        "GitHub returned unexpected yt-dlp release metadata.",
    "Назва релізу yt-dlp має неочікуваний формат.":
        "The yt-dlp release name has an unexpected format.",
    "GitHub повернув неочікувану адресу yt-dlp.":
        "GitHub returned an unexpected yt-dlp download URL.",
    "Для yt-dlp немає коректної контрольної суми або розмір файлу неочікуваний.":
        "The yt-dlp checksum is invalid or its file size is unexpected.",
    "Не вдалося перевірити коміт репозиторію бінарників ImageIO.":
        "Could not verify the ImageIO binary repository commit.",
    "Не вдалося повністю перевірити дерево репозиторію ImageIO.":
        "Could not fully verify the ImageIO repository tree.",
    "Розмір або Git blob SHA бінарника FFmpeg неочікуваний.":
        "The FFmpeg binary size or Git blob SHA is unexpected.",
    "Відхилено завантаження з недозволеного джерела.":
        "Download from an untrusted source was blocked.",
    "Фактичне джерело завантаження не пройшло перевірку.":
        "The actual download source failed verification.",
    "Розмір відповіді не збігається з офіційними метаданими.":
        "The response size does not match the official metadata.",
    "Завантажений файл більший за офіційний розмір.":
        "The downloaded file is larger than the official size.",
    "Контрольна сума або розмір завантаженого файлу не збігаються.":
        "The downloaded file checksum or size does not match.",
    "Не вдалося прочитати відео.": "Could not read the video.",
    "Не вдалося прочитати плейлист.": "Could not read the playlist.",
    "У плейлисті не знайдено доступних відео.":
        "No available videos were found in the playlist.",
    "Не знайдено вбудований ffmpeg. Онови інструменти й повтори завантаження.":
        "The bundled ffmpeg was not found. Update the tools and try again.",
    "Підтримуються формати MP4, MKV та WebM.":
        "Supported formats are MP4, MKV, and WebM.",
    "yt-dlp зберіг окремі аудіо- та відеопотоки замість готового відео. Перевір наявність ffmpeg і повтори завантаження.":
        "yt-dlp saved separate audio and video streams instead of a playable video. Check that ffmpeg is available and try again.",
    "Перевірте доступ до відео та інтернет-з'єднання.":
        "Check that the video is accessible and that you are connected to the internet.",
    "Помилка ffmpeg": "ffmpeg failed to process the media.",
    "Не вдалося завантажити жодне відео.": "Could not download any videos.",
    "Жодне відео не вдалося прочитати.": "Could not read any videos.",
    "Не вдалося завантажити відео.": "Could not download the video.",
    "Не вдалося завантажити вибрані треки.": "Could not download the selected tracks.",
    "Пауза процесу наразі підтримується в Linux та macOS.":
        "Pausing downloads is currently supported on Linux and macOS.",
    "Продовження процесу наразі підтримується в Linux та macOS.":
        "Resuming downloads is currently supported on Linux and macOS.",
    "Зображення обкладинки перевищує допустимий розмір.":
        "The cover image exceeds the allowed size.",
}

ERROR_PATTERNS = (
    (re.compile(r"Платформа (.+) поки не підтримується\."),
     lambda match: f"Platform {match.group(1)} is not supported yet."),
    (re.compile(r"Офіційний реліз yt-dlp не містить файл (.+)\."),
     lambda match: f"The official yt-dlp release does not contain {match.group(1)}."),
    (re.compile(r"Репозиторій ImageIO не містить очікуваний FFmpeg: (.+)\."),
     lambda match: f"The ImageIO repository does not contain the expected FFmpeg binary: {match.group(1)}."),
    (re.compile(r"Перевірений файл (.+) не запускається на цій системі\."),
     lambda match: f"The verified {match.group(1)} binary cannot run on this system."),
    (re.compile(r"Розмір перевіреного файлу (.+) змінився перед встановленням\."),
     lambda match: f"The verified {match.group(1)} file size changed before installation."),
    (re.compile(r"yt-dlp не завантажив (відео|аудіо) \(код (\d+)\)\."),
     lambda match: f"yt-dlp failed to download {'video' if match.group(1) == 'відео' else 'audio'} (exit code {match.group(2)})."),
    (re.compile(r"yt-dlp не завантажив аудіо \(код (\d+)\):"),
     lambda match: f"yt-dlp failed to download audio (exit code {match.group(1)}):"),
    (re.compile(r"yt-dlp не завантажив аудіо \(код (\d+)\)\. Перевірте доступ до відео та інтернет-з'єднання\."),
     lambda match: f"yt-dlp failed to download audio (exit code {match.group(1)}). Check that the video is accessible and that you are connected to the internet."),
)


def translate_error(text, language="uk"):
    """Translate known application errors while preserving external diagnostics."""
    text = str(text)
    if language != "en":
        return text

    translated_lines = []
    for line in text.split("\n"):
        stripped = line.strip()
        translated = ERROR_TO_EN.get(stripped)
        if translated is None:
            for pattern, render in ERROR_PATTERNS:
                match = pattern.fullmatch(stripped)
                if match:
                    translated = render(match)
                    break

        translated_lines.append(translated if translated is not None else translate(line, language))
    return "\n".join(translated_lines)


def translate(text, language="uk"):
    """Translate known interface text while leaving media names and paths intact."""
    text = str(text)
    if language != "en":
        return text
    exact = UK_TO_EN.get(text)
    if exact is not None:
        return exact
    match = re.fullmatch(r"(\d+) ВІДЕО", text)
    if match:
        count = int(match.group(1))
        return f"{count} video" if count == 1 else f"{count} videos"
    match = re.fullmatch(r"Посилань: (\d+)", text)
    if match:
        return f"Links: {match.group(1)}"
    match = re.fullmatch(r"(\d+) треків", text)
    if match:
        return f"{match.group(1)} tracks"
    match = re.fullmatch(r"(\d+) відео", text)
    if match:
        count = int(match.group(1))
        return f"{count} video" if count == 1 else f"{count} videos"
    match = re.fullmatch(r"(\d+) відео готові до завантаження", text)
    if match:
        return f"{match.group(1)} videos ready to download"
    match = re.fullmatch(r"Готово до завантаження: (\d+) відео\.", text)
    if match:
        return f"Ready to download: {match.group(1)} videos."
    match = re.fullmatch(r"Готую завантаження (\d+) відео…", text)
    if match:
        return f"Preparing {match.group(1)} videos for download…"
    match = re.fullmatch(r"Збережено відео: (\d+)\.", text)
    if match:
        return f"Saved {match.group(1)} videos."
    match = re.fullmatch(r"(Читаю|Перевіряю) відео (\d+)/(\d+)…", text)
    if match:
        action = "Reading" if match.group(1) == "Читаю" else "Checking"
        return f"{action} video {match.group(2)}/{match.group(3)}…"
    match = re.fullmatch(r"Не вдалося завантажити: (\d+)\.", text)
    if match:
        return f"Could not download: {match.group(1)}."
    match = re.fullmatch(r"Не вдалося обробити: (\d+)\.", text)
    if match:
        return f"Could not process: {match.group(1)}."
    match = re.fullmatch(r"Знайдено (\d+) треків у (\d+) відео\. Вибери потрібні й завантаж\.(?: Не вдалося прочитати: (\d+)\.)?", text)
    if match:
        result = f"Found {match.group(1)} tracks in {match.group(2)} videos. Select tracks to download."
        if match.group(3):
            result += f" Could not read: {match.group(3)}."
        return result
    match = re.fullmatch(r"(\d+)/(\d+) · (.+)", text)
    if match:
        return f"{match.group(1)}/{match.group(2)} · {match.group(3)}"
    match = re.fullmatch(r"(\d+) (РОЗДІЛ|РОЗДІЛИ|РОЗДІЛІВ)", text)
    if match:
        count = int(match.group(1))
        return f"{count} chapter" if count == 1 else f"{count} chapters"
    match = re.fullmatch(r"Підпапок: (\d+)", text)
    if match:
        count = int(match.group(1))
        return f"{count} subfolders" if count != 1 else "1 subfolder"
    match = re.fullmatch(r"У плейлисті (\d+) відео\. Вибери потрібні й завантаж\.", text)
    if match:
        return f"Playlist has {match.group(1)} videos. Select the ones you want and download them."
    match = re.match(r"Збережено (\d+) (MP3-файлів|відео)", text)
    if match:
        unit = "MP3 files" if match.group(2) == "MP3-файлів" else "videos"
        text = f"Saved {match.group(1)} {unit}" + text[match.end():]
    match = re.match(r"Створено (\d+) треків", text)
    if match:
        text = f"Created {match.group(1)} tracks" + text[match.end():]
    match = re.match(r"(\d+) (MP3-файлів|відео) збережено", text)
    if match:
        unit = "MP3 files" if match.group(2) == "MP3-файлів" else "videos"
        text = f"Saved {match.group(1)} {unit}" + text[match.end():]
    for source, target in (
        (". Тека:", ". Folder:"), ("\n\nТека:", "\n\nFolder:"), ("Тека:", "Folder:"),
        (" MP3 у ", " MP3 to "),
        (" Обкладинку відео додано до треків.", " Video cover art was added to the tracks."),
        (" Деякі обкладинки не вдалося вбудувати.", " Some cover art could not be embedded."),
        (" Не вдалося завантажити:", " Could not download:"),
        ("Перевірте підключення до інтернету й повторіть через «Оновити інструменти».",
         "Check your internet connection and retry with ‘Update tools’.") ,
        ("\n\nДля частини MP3 не вдалося додати обкладинку.",
         "\n\nCover art could not be added to some MP3 files."),
    ):
        text = text.replace(source, target)
    for source in sorted(UK_TO_EN, key=len, reverse=True):
        if text.startswith(source):
            return UK_TO_EN[source] + text[len(source):]
    return text
