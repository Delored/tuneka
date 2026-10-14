# План реалізації Tuneka на Rust

Дата: 2026-10-14. Статус: спільний pipeline та CLI/TUI/MCP/GPUI реалізовані; macOS ARM64, Linux ARM64 і Linux amd64 (емуляція) build/runtime перевірені, Docker відновлено. Повні кросплатформні критерії M0–M6 ще не закрито. Актуальні докази й обмеження: `docs/validation.md`.

Мета — реалізувати в `rust/` незалежну Rust-версію Tuneka зі спільним ядром для CLI, TUI, MCP та графічного інтерфейсу GPUI. Отримання даних YouTube і завантаження реалізуємо самостійно; `rusty_ytdl` використовуємо як довідковий код. FFmpeg підключаємо як бібліотеки. Наявна Python-версія залишається робочим орієнтиром поведінки.

Оновлення CI PR #14: [Windows job 115868400941](https://github.com/Delored/tuneka/actions/runs/38568631667/job/115868400941) зупинився на LF/CRLF припущенні нового тесту після успішного збирання каталогу ліцензій. Тест виправлено: порівняння з вихідними байтами та окрема перевірка LF/CRLF. Чотири packaging tests локально успішні; три license tests також перевірено з імітацією Windows text-write CRLF. Повторний native Windows CI ще потрібен. Обидва Linux jobs цього запуску успішні.

## 1. Узгоджені вимоги та межі

- Rust-код, Cargo workspace, тести, документація й пакування розташовані в `rust/`. Кореневі CI та `.gitignore` можна доповнювати для Rust, зберігаючи Python-перевірки.
- Жодної runtime-залежності від Python, виконуваного yt-dlp або crate `rusty_ytdl`. Не підміняти власний extractor обгорткою над зовнішнім завантажувачем.
- Rust керує extraction і завантаженням; за потреби дозволено вбудований JS-рушій для обробки коду плеєра. Це не означає переписування FFmpeg з C на Rust.
- FFmpeg використовується через API бібліотек, без запуску `ffmpeg`/`ffprobe` у готовому продукті. Командні інструменти допустимі як незалежний засіб перевірки тестових артефактів.
- UI використовує GPUI з екосистеми Zed. Не замінювати на вебінтерфейс або інший GUI framework.
- Перший реліз: публічні звичайні відео YouTube, Shorts, плейлисти, кілька URL, розділи, обкладинки, MP3/M4A/FLAC/WAV, MP4/MKV/WebM, вибір якості, прогрес, пауза/продовження/скасування, українська/англійська мови та три теми.
- Авторизовані/приватні відео, DRM, платний контент, прямі трансляції й інші сайти не входять у перший реліз. Непідтримуваний сценарій повертає явну помилку.
- Обсяг — функції Tuneka для YouTube; повний багатосайтовий аналог yt-dlp не є критерієм готовності.
- Працювати локально. Публікація релізів, push, зовнішній fork і зміни інфраструктури не потрібні для виконання цього плану.

## 2. Вихідні докази та довідковий код

Поточні `app.py`, `ui.py`, `i18n.py`, `tests/` та `docs/design.md` задають поведінку. Особливо зберегти правила назв і тек, fallback одного треку без розділів, спільні налаштування форматів, попередження про відсутню обкладинку та очищення тимчасових файлів. Моковані Python-тести не доводять коректність реального медіаконвеєра.

Перевірено `rusty_ytdl` на коміті `b1c6eb7c83f0d6189f256ed5df50019a5803c734` від 2026-01-18:

- На macOS з Rust 1.96 пройшла збірка з `search,rustls`, без `live`/`ffmpeg`.
- Отримано метадані й перші 16 385 байтів аудіо для `FZ8BxMU3BYc` та `M4gD1WSo5mA`; друге відео повернуло 6 розділів. Для `aqz-KE-bpKQ` отримано метадані й перший відеофрагмент.
- `0ThMultL4PY` повернуло `Video is private`; це не доказ дефекту.
- Плейлисти `PLwMEL7UNT4o9iMzrvNBXZqXbNPFfT6rVD` і `PLt5yu3-wZAlSLRHmI1qNm0wjyVNWw1pCU` повернули помилку структури сторінки. Повні медіафайли не завантажувались і не перевірялися.
- Код використовує Boa, має окремі структури форматів і розділів. Його FFmpeg feature запускає зовнішню програму; live-модуль викликає ffprobe. `Playlist::fetch()` приховує помилку наступної сторінки, повертаючи частковий список. Ці рішення не переносити.
- Тимчасове дослідження може залишатися в `/tmp/tuneka-ytdl-review.uXyiG0/`, але не покладатися на його існування. Не витрачати токени на повторення вже доведеного без потреби.

Джерела: [reference commit](https://github.com/Mithronn/rusty_ytdl/tree/b1c6eb7c83f0d6189f256ed5df50019a5803c734), [403 issue](https://github.com/Mithronn/rusty_ytdl/issues/53), [незлитий PR 62](https://github.com/Mithronn/rusty_ytdl/pull/62), [GPUI](https://gpui.rs/), [FFmpeg bindings](https://github.com/zmwangx/rust-ffmpeg), [MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).

Довідковий код читати вибірково. Якщо переноситься конкретний код, перевірити фактичну ліцензію файлу/репозиторію, зберегти потрібні повідомлення та attribution; додати запис у third-party notices. Не приписувати скопійованому коду незалежне авторство.

## 3. Архітектура та контракти

```text
rust/
  Cargo.toml, Cargo.lock, rust-toolchain.toml
  crates/
    tuneka-core/       моделі, API завдань, оркестрація, події, налаштування
    tuneka-youtube/    метадані, формати, плейлисти, підписи/challenges
    tuneka-download/   HTTP, range, повтори, часткові файли, checkpoints
    tuneka-media/      FFmpeg API, mux/remux, кодування, розділи, обкладинки
    tuneka-cli/        CLI та структурований JSON
    tuneka-tui/        Ratatui
    tuneka-mcp/        rmcp server
    tuneka-gui/        GPUI
  tests/fixtures/
  docs/decisions/
  packaging/
```

Контракти та напрямки залежностей зафіксувати до паралельного написання модулів. `core` не залежить від UI й транспорту MCP. Не створювати циклічні crate-залежності: зовнішні модулі реалізують порти ядра, а entry points складають конкретні реалізації. Не виділяти додаткові crates без потреби.

- Моделі: `MediaInfo`, `Chapter`, `Format`, `PlaylistPage`, `DownloadRequest`, `JobId`, `JobSnapshot`, `Artifact`, типізовані помилки та попередження. Формати мережевих відповідей не витікають у UI.
- Порти: отримання метаданих/сторінки плейлиста, оновлення адрес потоків, завантаження, медіаобробка, сховище стану. Метадані плейлиста містять ознаку завершеності й continuation; частковий успіх відрізняється від повного.
- Job states: queued, resolving, downloading, processing, paused, completed, partially_completed, failed, cancelled. Визначити допустимі переходи та семантику паузи для кожної стадії; не оголошувати paused, поки worker ще працює. Batch/playlist зберігає per-item outcomes; суміш успіхів і помилок дає partially_completed, ненульовий CLI exit code та явні aggregate/per-item статуси MCP. Готові файли успішних елементів зберігаються; повтор запускає лише невдалі/незавершені елементи за явною політикою.
- Події прогресу містять стадію, байти/тривалість, total за наявності, швидкість і попередження. Кінцевий snapshot є джерелом істини, тому втрата проміжної події не втрачає результат.
- Async I/O через Tokio; блокувальний FFmpeg/JS код — обмежені worker-потоки. Cancellation кооперативна, з таймаутами й обмеженнями ресурсів. GPUI отримує події без блокування свого event loop.
- Output: `.part`/тимчасові файли, атомарне завершення, явна політика конфлікту назв, жодного випадкового перезапису готових файлів.
- Resume: перевіряти HTTP 206/Content-Range, ідентичність потоку, розмір та оновлення прострочених URL; не дописувати повну відповідь 200 до часткового файлу.
- Перший реліз: кожен CLI/TUI/GUI/MCP процес володіє своїми активними jobs. На виході — контрольована зупинка й checkpoint; після перезапуску можна відновити підтримуване завантаження. Не обіцяти роботу після завершення процесу. Спільний daemon/IPC відкладено; settings/checkpoints повинні уникати конфліктів між процесами.
- Конфігурація Rust зберігається окремо; імпорт сумісних Python settings одноразовий, з перевіркою схеми та збереженням оригіналу. Активний checkpoint має ексклюзивного власника через OS file lock; цільовий файл резервується до початку запису. Відновлення stale ownership перевіряє lock, а не лише PID. Settings записуються атомарно під lock із read-modify-write.

## 4. Бібліотеки та технічні рішення

Кандидати: Tokio, reqwest/rustls, serde, clap, Ratatui/crossterm, rmcp, GPUI, ffmpeg-next із локалізованим FFI за необхідності. Вбудований JS-рушій обрати після прототипу (Boa — кандидат, не затверджена вимога). `ffmpeg-next` має режим підтримки; підтвердити потрібні API та конкретну версію FFmpeg.

Зафіксувати сумісні версії, MSRV, Cargo.lock і набір FFmpeg codecs. GUI — окремий пакет, щоб CLI/TUI/MCP збиралися без графічного стеку. Linux headless не повинен вимагати дисплея. Описати динамічне/статичне лінкування, пошук бібліотек, redistributable залежності та ліцензійні повідомлення до release-пакування.

Дозволяти лише підтримувані URL та перевіряти redirects. Імена з мережі не можуть виходити за output directory. JS без доступу до файлової системи/мережі, з обмеженням часу/пам'яті. MCP приймає операції з медіа, а не shell-команди чи довільні FFmpeg arguments; output roots конфігуровані. Логи не містять cookies, токенів або повних підписаних media URLs.

## 5. Етапи виконання та критерії приймання

### M0 Архітектура і перевірка ризиків

- Архітектурний сабагент робить вузьке рев'ю контрактів; координатор фіксує рішення в `docs/decisions/`.
- Перевірити GPUI window/input/background updates на macOS та можливість збірки на Windows/Linux; платформні runtime-перевірки не підміняти компіляцією.
- Через FFmpeg API створити й прочитати короткий аудіофайл, нарізати сегмент; без subprocess у прототипі продукту.
- Власний YouTube spike: метадані й перші байти аудіо/відео, розділи, плейлист із наступною сторінкою. Локально обмежити трафік і час. Зафіксувати, що реально перевірено.
- Вихід: рішення про JS engine, FFmpeg bindings, GPUI revision, контракти та відомі блокери. За локального блокера однієї гілки продовжувати незалежні частини; не оголошувати M0 завершеним без доказів.

### M1 Workspace та спільне ядро

- Створити Cargo workspace, формати/помилки, scheduler, конфігурацію, jobs/events, керування файлами; каркас CLI.
- Offline тести URL, назв/тек, розділів без end, одного треку без chapters, переходів стану, cleanup і конфліктів файлів.
- CI для fmt/clippy/test; не ламати Python jobs. Координатор володіє Cargo workspace/lockfile і shared contracts.
- Вихід: kernel працює через тестові адаптери, headless збірка не тягне GPUI.

### M2 Власний YouTube і downloader

- Метадані, thumbnails/chapters, публічні плейлисти та явна пагінація, формати/кодеки, підписи/challenges і оновлення URL.
- Окремі audio/video потоки, відповідність контейнеру/якості, потрібні сегментовані формати. Чітко повідомляти unsupported combinations.
- Bounded retries, backoff, контроль паралельності, pause/cancel, checkpoints та валідація range.
- Вихід: offline fixtures і live smoke matrix проходять; отримання лише метаданих не вважається успішним download.

### M3 FFmpeg та завершений CLI

- MP3/M4A/FLAC/WAV, нарізка з коректними timestamps, метадані/обкладинки з явним fallback warning, злиття потоків, MP4/MKV/WebM.
- Визначити політику codec/container: безпечний remux, де сумісно; явна помилка або погоджений transcode, де несумісно. Не приховувати дорогий transcode.
- CLI: `inspect URL --json`, `download URL --audio mp3`, `download URL --video --quality 1080p`, вибір playlist items/chapters, output directory, resume.
- Вихід: реальні файли перевірені за duration, codec, контейнером, tags, cover та A/V sync; exit codes і JSON документовані, diagnostic logs у stderr.

### M4 TUI та MCP

- TUI: список/вибір джерел і розділів, jobs, прогрес, помилки, keyboard navigation, pause/resume/cancel та коректне відновлення термінала.
- MCP stdio: inspect_media, list_formats, list_playlist_items, create_download, get_job, list_jobs, pause_job, resume_job, cancel_job. Створення повертає job_id; polling стану працює без вимоги новітньої підтримки MCP Tasks клієнтом.
- Стандартний MCP lifecycle, schemas та errors; stdout тільки для протоколу. На disconnect — контрольована зупинка/checkpoint відповідно до процесної моделі.
- Вихід: protocol integration tests з реальним MCP client і TUI interaction tests; обидва режими використовують те саме ядро.

### M5 GPUI

- Екрани Music, Playlist, Video, Settings; вибір URL/елементів, таблиці, прогрес і керування jobs.
- Українська/англійська локалізація, light/olive/purple, збереження параметрів і тек, клавіатура, фокус, масштабування та довгі назви.
- Вихід: повний шлях URL → вибір → файл працює; немає зависань під час download/FFmpeg; перевірено розміри вікон і скасування. Скріншот не замінює функціональну перевірку.

### M6 Пакування та незалежне рев'ю

- macOS Apple Silicon/x64, Windows x64, Linux amd64/arm64: явно розділити build coverage та runtime coverage. Windows ARM64 — окремий follow-up до підтвердження toolchain/dependencies.
- Локальні пакети/інсталятори, FFmpeg libraries, notices, platform data paths, міграція налаштувань, README з точними командами. Release-публікацію не запускати.
- Незалежний reviewer перевіряє extraction, FFI, jobs, файлові операції, MCP і відсутність прихованих runtime subprocess dependencies.
- Вихід: немає відкритих блокуючих дефектів; smoke на чистих доступних системах, готові артефакти та чесний перелік неперевірених платформ.

## 6. Стратегія тестування

- За замовчуванням offline: збережені знеособлені відповіді/JS fixtures, локальний HTTP server для 200/206/403/429, truncated response, timeout, expired URL, cancel та retry. Не зберігати credentials і підписані URL.
- Для плейлистів: кілька сторінок, недоступні елементи, дублікати, failure наступної сторінки, змішаний успіх batch зі збереженням готових файлів та коректними CLI/MCP статусами. Для chapters: невідомий duration, fallback одного треку, межі та дробовий час. Окремий multi-process test: два процеси відновлюють той самий checkpoint або резервують однаковий output; другий отримує контрольований conflict без пошкодження даних.
- Реальні короткі медіафікстури: всі чотири аудіоформати, cover/no-cover, chapter boundaries, mux/remux, A/V sync, cleanup при cancel/error. Порівнювати семантику, не байтову ідентичність різних encoder builds.
- Live YouTube — окремий явний smoke набір з датою, URL/id, версією extractor і коротким результатом; не робити мережу обов'язковою для всіх unit tests. 403/географічний блок відокремлювати від parser defects.
- Gate commands після появи workspace: `cargo fmt --all -- --check`, scoped `cargo clippy ... -- -D warnings`, `cargo test ...`; точні feature/platform команди записати після фіксації manifests. Не вимагати сліпого `--all-features` із несумісними backend features.
- Повторювати тести після релевантних змін; не проганяти всю матрицю повторно без причини. Не писати тести лише для дублювання реалізації.

## 7. Сабагенти та економія токенів

Користувач прямо дозволив делегування та вибір моделі для кожного сабагента. Це розподіл за складністю з доступного каталогу, а не обіцянка точного тарифу. Координатор нового чату зберігає модель користувача за замовчуванням.

| Робота | Модель | Reasoning | Коли залучати |
|---|---|---|---|
| Архітектура, async/FFI контракти | `gpt-6.1-sol` | high | Один початковий огляд, далі лише зміни контрактів |
| YouTube, FFmpeg, складний GPUI | `gpt-6.1-sol` | medium; high для складної помилки | По одному обмеженому модулю |
| CLI/TUI/MCP реалізація | `gpt-6.1-sol` | medium | Коли контракти ядра вже зафіксовані |
| Документація, fixtures, прості зміни, запуск відомих тестів | `gpt-6-luna` | low або medium | Вузькі перевірювані задачі |
| Незалежне рев'ю і складні regression tests | `gpt-6.1-sol` | high | Після завершеного вертикального зрізу |
| Складний нерозв'язаний архітектурний/YouTube дефект | `gpt-6-astra` | high | Тільки після конкретного невдалого розбору дешевшою моделлю |

- Зазвичай координатор + 1–2 агенти; максимум 3 сабагенти одночасно, якщо доступні слоти й незалежні задачі. Не запускати всю таблицю агентів одразу.
- При model override використовувати `fork_turns="none"`; передавати мету, контракти, дозволені файли, критерії перевірки та потрібні посилання. Не копіювати всю історію чату.
- Кожен автор володіє окремими файлами. Workspace manifests, lockfile, shared models та інтеграцію змінює координатор; залежності погоджувати коротким повідомленням.
- Архітектор/reviewer за замовчуванням read-only. Автор не є єдиним reviewer своєї роботи. Координатор сам перевіряє інтеграцію та результати тестів.
- Звіт агента: змінені файли, важливі рішення, виконані команди/результати, блокери. Без повторного опису всього проєкту.
- Не делегувати прості команди, якщо це дорожче за пряме виконання. Повторно використовувати агентів із релевантним контекстом.
- Недоступність моделі: обрати найближчу доступну за складністю, коротко зафіксувати заміну; не ескалувати все на найдорожчу.

## 8. Порядок роботи нового чату

1. Прочитати кореневий `AGENTS.md`, `rust/AGENTS.md`, цей план; перевірити branch/status і зберегти чужі/untracked зміни.
2. Розпочати M0: коротке архітектурне рев'ю, незалежний огляд тестових ризиків, власна перевірка toolchains. Паралелити лише незалежні прототипи.
3. Рухатися до завершених вертикальних зрізів: власний extractor → download → FFmpeg → CLI, потім TUI/MCP та GPUI. Scaffold або демовікно не є готовим продуктом.
4. Після кожного етапу оновлювати таблицю нижче з посиланням на перевірки; продовжувати дозволену роботу без повторного запиту дозволу на звичайні зміни.
5. За блокера записати точний симптом, спроби, вплив і наступний крок; виконувати незалежну роботу. Не заявляти завершення неперевірених сценаріїв.

| Етап | Статус | Докази / блокери |
|---|---|---|
| M0 | macOS ARM64/Linux ARM64 та amd64 (емуляція) перевірено; Windows gate відкритий | Контракти й незалежне рев’ю, Rust 1.96 / FFmpeg 7 / bounded QuickJS. Реальні GPUI macOS та Linux X11/Vulkan window/input/background updates. Linux xattr/libc compatibility виправлено з attribution; Windows build/runtime ще не підтверджені. |
| M1 | Локальні критерії виконано; remote CI не запускали | Вісім crates, lockfile, ports/scheduler/settings, міжпроцесні OS locks, recovery/partial/conflict regressions; headless entry points без GPUI. Окремий `.github/workflows/rust.yml` для Linux amd64/arm64 на Debian 13/FFmpeg 7 та GUI smoke додано згідно з дозволеним винятком; Python workflow незмінений. |
| M2 | signatureCipher CDN gate закрито; довгий SABR перевірено з proof; автоматичне отримання proof відкрите | Повні Direct/HLS/DASH/SABR медіафайли, плейлисти, pause/URL-refresh/resume. Genuine MWEB signatureCipher + вкладений n → повний MP4 192 с; попередній 403 спричинив пропущений n у пробі. SABR 192 с повністю пройшов із серверним video-bound proof, також після паузи; proof отримано окремим QA-інструментом, native acquisition/renewal ще немає. Без proof сервер відхиляє довге відтворення. Деталі: `docs/m2-cdn-followup.md`. |
| M3 | Локальні критерії виконано | Чотири аудіоформати, covers/tags/chapters, MP4/MKV/WebM mux/remux. Десять media tests включають реальні B-frames/negative DTS/nonzero origin і A/V sync до 2 ms; live 720p MP4/WebM повністю декодуються. CLI JSON/exit 2, pause/retry/cancel перевірені; M4A timestamps defect закритий регресією й live повтором. |
| M4 | Локальні критерії виконано | Реальний rmcp protocol integration test плюс live create→poll→completed→EOF (26 коректних JSON-RPC повідомлень). TUI PTY URL→inspect→select→completed→quit, термінал повністю відновлюється. Native FFmpeg log overlap усунуто й live повтор пройшов; per-item errors/warnings відображаються, додано rendered-row regression. Окремі state/output директорії для фінальних перевірок. |
| M5 | macOS/Linux X11 основні критерії виконано | URL→файл на обох ОС; native folder/Unicode/themes/settings/selection/pause/cancel. 640×480 та zoom/resize, повна Tab/Shift-Tab навігація, Enter/Space, focus reveal, keyboard artifact reveal. Чотири GUI tests. Formal AX/Wayland/physical GPU та Windows залишаються окремою неперевіреною матрицею. |
| M6 | Частково; обидва native Linux CI jobs успішні, packaging fixes очікують remote перевірки | macOS ARM64 development .app, Linux ARM64/amd64 CLI/TUI/MCP та GUI .deb, чисте Debian install/run/uninstall і X11 GUI. Локальний amd64 runtime виконано під емуляцією на ARM64. У [першому artifact run PR #14](https://github.com/Delored/tuneka/actions/runs/38554135260) native Linux amd64/arm64 jobs успішні; Windows x64/macOS Intel build і Rust tests пройшли, packaging впав. Локально виправлено Windows UTF-8 metadata та порядок macOS signing; три регресійні packaging tests і actionlint пройшли, тести додано до CI. macOS ARM64 job ще виконувався на момент перевірки. Потрібен remote run виправлень; Windows/macOS x64 portable runtime, повні release notices/signing і hardware matrix відкриті. Docker зупинено на прохання користувача. Публікацій не було. |

Груба попередня трудомісткість: 17–30 людино-тижнів до резерву; орієнтир 5–9 місяців для одного досвідченого розробника з резервом. Це не оцінка тривалості роботи агентів і не обіцянка строку. Переоцінити після M0 за фактичними результатами власного extractor та платформних прототипів.
