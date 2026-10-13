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
    "Відкрити теку": "Open folder", "Вставити": "Paste",
    "ПОСИЛАННЯ НА YOUTUBE": "YOUTUBE LINK",
    "Знайти розділи  →": "Find chapters  →",
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
    "Буфер обміну порожній. Скопіюй посилання з YouTube.":
        "The clipboard is empty. Copy a YouTube link first.",
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
    "Буфер обміну порожній. Скопіюй посилання з YouTube.":
        "The clipboard is empty. Copy a YouTube link first.",
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
