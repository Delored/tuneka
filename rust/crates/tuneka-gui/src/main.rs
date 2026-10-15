mod input;

use gpui::{App as GpuiApp, prelude::*, *};
use input::TextInput;
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};
use tuneka_core::Result;
use tuneka_core::{
    settings::{self, Settings},
    *,
};

actions!(tuneka, [Quit, NextField, PreviousField, Inspect, Download]);
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Music,
    Playlist,
    Video,
    Settings,
}
enum Reply {
    Media(Result<MediaInfo>),
    Page(Result<PlaylistPage>),
}

struct ButtonFocus {
    handle: FocusHandle,
    bounds: Option<Bounds<Pixels>>,
}

fn reveal_offset(
    viewport: Bounds<Pixels>,
    control: Bounds<Pixels>,
    mut offset: Point<Pixels>,
) -> Point<Pixels> {
    // Sidebar controls live outside the scrolling pane.
    if control.left() < viewport.left() {
        return offset;
    }
    if control.top() < viewport.top() {
        offset.y += viewport.top() - control.top();
    } else if control.bottom() > viewport.bottom() {
        offset.y -= control.bottom() - viewport.bottom();
    }
    offset
}

struct Tuneka {
    engine: Arc<Engine>,
    runtime: tokio::runtime::Handle,
    url: Entity<TextInput>,
    output: Entity<TextInput>,
    settings: Settings,
    saved_settings: Settings,
    inspected_url: String,
    saved_jobs: Vec<JobSnapshot>,
    screen: Screen,
    info: Option<MediaInfo>,
    playlist: Vec<PlaylistItem>,
    continuation: Option<String>,
    playlist_complete: bool,
    selected_chapters: BTreeSet<usize>,
    selected_items: BTreeSet<usize>,
    quality: Option<u32>,
    status: String,
    busy: bool,
    sender: mpsc::Sender<Reply>,
    receiver: mpsc::Receiver<Reply>,
    scroll: ScrollHandle,
    button_focus: RefCell<HashMap<ElementId, Rc<RefCell<ButtonFocus>>>>,
}

impl Tuneka {
    fn new(
        engine: Arc<Engine>,
        runtime: tokio::runtime::Handle,
        settings: Settings,
        cx: &mut Context<Self>,
    ) -> Self {
        let url = cx.new(|cx| TextInput::new("", "https://www.youtube.com/watch?v=…", cx));
        let output =
            cx.new(|cx| TextInput::new(&settings.output_dir.to_string_lossy(), "Downloads", cx));
        let (sender, receiver) = mpsc::channel();
        let saved_jobs = std::fs::read_dir(settings::data_dir().join("jobs"))
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()?.to_str()? != "json" {
                    return None;
                }
                serde_json::from_slice::<JobSnapshot>(&std::fs::read(path).ok()?).ok()
            })
            .collect();
        let screen = if settings.format.is_audio() {
            Screen::Music
        } else {
            Screen::Video
        };
        cx.spawn(async move |view, cx| {
            loop {
                Timer::after(Duration::from_millis(150)).await;
                if view
                    .update(cx, |view, cx| {
                        while let Ok(reply) = view.receiver.try_recv() {
                            view.busy = false;
                            match reply {
                                Reply::Media(Ok(info)) => {
                                    view.status = info.title.clone();
                                    view.selected_chapters = (1..=info.chapters.len()).collect();
                                    view.info = Some(info);
                                }
                                Reply::Page(Ok(page)) => {
                                    let base = view.playlist.len();
                                    for (index, item) in page.items.iter().enumerate() {
                                        if item.available {
                                            view.selected_items.insert(base + index + 1);
                                        }
                                    }
                                    view.status = page.title;
                                    view.playlist.extend(page.items);
                                    view.continuation = page.continuation;
                                    view.playlist_complete = page.complete;
                                }
                                Reply::Media(Err(error)) | Reply::Page(Err(error)) => {
                                    view.status = error.to_string()
                                }
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            engine,
            runtime,
            url,
            output,
            saved_settings: settings.clone(),
            settings,
            inspected_url: String::new(),
            saved_jobs,
            screen,
            info: None,
            playlist: vec![],
            continuation: None,
            playlist_complete: false,
            selected_chapters: BTreeSet::new(),
            selected_items: BTreeSet::new(),
            quality: Some(1080),
            status: String::new(),
            busy: false,
            sender,
            receiver,
            scroll: ScrollHandle::new(),
            button_focus: RefCell::new(HashMap::new()),
        }
    }
    fn t(&self, uk: &'static str, en: &'static str) -> &'static str {
        if self.settings.language == "uk" {
            uk
        } else {
            en
        }
    }
    fn persist(&mut self) -> bool {
        let current = self.settings.clone();
        let previous = self.saved_settings.clone();
        match settings::update(&settings::data_dir().join("settings.json"), |saved| {
            if current.language != previous.language {
                saved.language = current.language;
            }
            if current.theme != previous.theme {
                saved.theme = current.theme;
            }
            if current.output_dir != previous.output_dir {
                saved.output_dir = current.output_dir;
            }
            if current.format != previous.format {
                saved.format = current.format;
            }
        }) {
            Ok(saved) => {
                self.saved_settings = saved.clone();
                self.settings = saved;
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }
    fn inspect(&mut self, _: &Inspect, _: &mut Window, cx: &mut Context<Self>) {
        self.load(false, cx);
    }
    fn load(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let url = self
            .url
            .read(cx)
            .content
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        let kind = match parse_url(&url) {
            Ok(kind) => kind,
            Err(error) => {
                self.status = error.to_string();
                cx.notify();
                return;
            }
        };
        let engine = self.engine.clone();
        let sender = self.sender.clone();
        let more = more && url == self.inspected_url;
        if !more {
            self.info = None;
            self.playlist.clear();
            self.selected_items.clear();
            self.selected_chapters.clear();
            self.continuation = None;
        }
        self.inspected_url = url.clone();
        self.busy = true;
        self.status = self.t("Отримання метаданих…", "Loading metadata…").into();
        match kind {
            YoutubeUrl::Video(_) => {
                self.runtime.spawn(async move {
                    let _ = sender.send(Reply::Media(engine.inspect(&url).await));
                });
            }
            YoutubeUrl::Playlist(_) => {
                self.screen = Screen::Playlist;
                if !more {
                    self.scroll.set_offset(point(px(0.), px(0.)));
                }
                let cursor = if more {
                    self.continuation.clone()
                } else {
                    self.playlist.clear();
                    self.selected_items.clear();
                    self.playlist_complete = false;
                    None
                };
                self.runtime.spawn(async move {
                    let _ = sender.send(Reply::Page(
                        engine.playlist_page(&url, cursor.as_deref()).await,
                    ));
                });
            }
        }
        cx.notify();
    }
    fn download(&mut self, _: &Download, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let urls: Vec<String> = self
            .url
            .read(cx)
            .content
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let inspected =
            urls.len() == 1 && urls.first().is_some_and(|url| url == &self.inspected_url);
        if inspected
            && self.screen == Screen::Playlist
            && !self.playlist.is_empty()
            && self.selected_items.is_empty()
        {
            self.status = self
                .t("Виберіть елементи плейлиста", "Select playlist items")
                .into();
            cx.notify();
            return;
        }
        if inspected
            && self.info.as_ref().is_some_and(|i| i.has_chapters)
            && self.settings.format.is_audio()
            && self.selected_chapters.is_empty()
        {
            self.status = self.t("Виберіть розділи", "Select chapters").into();
            cx.notify();
            return;
        }
        self.settings.output_dir = PathBuf::from(self.output.read(cx).content.as_str());
        if !self.persist() {
            cx.notify();
            return;
        }
        let request = DownloadRequest {
            urls,
            output_dir: self.settings.output_dir.clone(),
            format: self.settings.format,
            max_height: self.quality,
            chapters: if inspected {
                self.selected_chapters.iter().copied().collect()
            } else {
                vec![]
            },
            playlist_items: if inspected {
                self.selected_items.iter().copied().collect()
            } else {
                vec![]
            },
        };
        let _runtime = self.runtime.enter();
        self.status = match self.engine.create(request) {
            Ok(id) => format!("{} {id}", self.t("Завдання створено", "Job created")),
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next();
        self.reveal_focus(window, cx);
    }
    fn previous_field(&mut self, _: &PreviousField, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev();
        self.reveal_focus(window, cx);
    }
    fn reveal_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = if self.url.focus_handle(cx).is_focused(window) {
            self.url.read(cx).bounds()
        } else if self.output.focus_handle(cx).is_focused(window) {
            self.output.read(cx).bounds()
        } else {
            self.button_focus.borrow().values().find_map(|focus| {
                let focus = focus.borrow();
                focus
                    .handle
                    .is_focused(window)
                    .then_some(focus.bounds)
                    .flatten()
            })
        };
        if let Some(bounds) = bounds {
            self.scroll.set_offset(reveal_offset(
                self.scroll.bounds(),
                bounds,
                self.scroll.offset(),
            ));
            cx.notify();
        }
    }
    fn job_action(&mut self, id: &str, action: &str, cx: &mut Context<Self>) {
        let _runtime = self.runtime.enter();
        let result = match action {
            "pause" => self.engine.pause(id),
            "cancel" => self.engine.cancel(id),
            "resume" => self.engine.resume(id, false),
            _ => self.engine.resume(id, true),
        };
        if let Err(error) = result {
            self.status = error.to_string();
        }
        cx.notify();
    }
    fn restore(&mut self, id: &str, retry: bool, cx: &mut Context<Self>) {
        let _runtime = self.runtime.enter();
        match self.engine.restore(id, retry) {
            Ok(()) => {
                self.saved_jobs.retain(|job| job.id != id);
                self.status = self.t("Завдання відновлено", "Job restored").into();
            }
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }
    fn choose_output(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                self.t("Виберіть теку завантажень", "Choose download directory")
                    .into(),
            ),
        });
        cx.spawn(async move |view, cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |view, cx| {
                        view.output.update(cx, |input, cx| {
                            input.set_text(path.to_string_lossy().into_owned(), cx)
                        });
                        view.settings.output_dir = path;
                        view.persist();
                        cx.notify();
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = view.update(cx, |view, cx| {
                    view.status = error.to_string();
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }
    fn output_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_2()
            .child(div().flex_1().min_w_0().child(self.output.clone()))
            .child(
                self.button(
                    cx,
                    "choose-output",
                    self.t("Вибрати теку…", "Choose folder…"),
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.choose_output(cx))),
            )
    }
    fn button(
        &self,
        cx: &mut Context<Self>,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        selected: bool,
    ) -> Stateful<Div> {
        let id = id.into();
        let focus = self
            .button_focus
            .borrow_mut()
            .entry(id.clone())
            .or_insert_with(|| {
                Rc::new(RefCell::new(ButtonFocus {
                    handle: cx.focus_handle().tab_index(0).tab_stop(true),
                    bounds: None,
                }))
            })
            .clone();
        let handle = focus.borrow().handle.clone();
        div()
            .id(id)
            .relative()
            .track_focus(&handle)
            .tab_index(0)
            .border_2()
            .border_color(rgba(0x00000000))
            .focus(|style| style.border_color(rgb(0x1574d1)))
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .bg(rgb(if selected { 0x42674b } else { 0xe3e8df }))
            .text_color(rgb(if selected { 0xffffff } else { 0x18251b }))
            .hover(|s| s.bg(rgb(0x94b59a)))
            .child(label.into())
            .child(
                canvas(
                    move |bounds, _, _| {
                        // Absolute canvas bounds exclude the button's 2px border.
                        focus.borrow_mut().bounds = Some(bounds.dilate(px(2.)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
    fn state(&self, state: JobState) -> &'static str {
        match state {
            JobState::Queued => self.t("У черзі", "Queued"),
            JobState::Resolving => self.t("Метадані", "Resolving"),
            JobState::Downloading => self.t("Завантаження", "Downloading"),
            JobState::Processing => self.t("Обробка", "Processing"),
            JobState::Paused => self.t("Призупинено", "Paused"),
            JobState::Completed => self.t("Завершено", "Completed"),
            JobState::PartiallyCompleted => self.t("Частково завершено", "Partially completed"),
            JobState::Failed => self.t("Помилка", "Failed"),
            JobState::Cancelled => self.t("Скасовано", "Cancelled"),
        }
    }
}

impl Render for Tuneka {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (background, panel, foreground) = match self.settings.theme.as_str() {
            "purple" => (0x272135, 0x362d48, 0xf3eafa),
            "olive" => (0xe9eddf, 0xf8faef, 0x263120),
            _ => (0xf1f3f5, 0xffffff, 0x1b222b),
        };
        let mut navigation = div()
            .w(px(170.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(div().text_2xl().child("Tuneka"));
        for (id, screen, label) in [
            ("music", Screen::Music, self.t("Музика", "Music")),
            ("playlist", Screen::Playlist, self.t("Плейлист", "Playlist")),
            ("video", Screen::Video, self.t("Відео", "Video")),
            (
                "settings",
                Screen::Settings,
                self.t("Налаштування", "Settings"),
            ),
        ] {
            navigation = navigation.child(
                self.button(cx, id, label, self.screen == screen)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.screen = screen;
                        view.scroll.set_offset(point(px(0.), px(0.)));
                        if screen == Screen::Video && view.settings.format.is_audio() {
                            view.settings.format = OutputFormat::Mp4;
                        } else if matches!(screen, Screen::Music | Screen::Playlist)
                            && !view.settings.format.is_audio()
                        {
                            view.settings.format = OutputFormat::Mp3;
                        }
                        cx.notify();
                    })),
            );
        }
        let mut body = div().min_w_0().flex().flex_col().gap_3().p_5();
        if self.screen == Screen::Settings {
            body = body
                .child(div().text_xl().child(self.t("Налаштування", "Settings")))
                .child(self.t("Мова", "Language"));
            let mut languages = div().flex().gap_2();
            for (language, label) in [("uk", "Українська"), ("en", "English")] {
                languages = languages.child(
                    self.button(cx, language, label, self.settings.language == language)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.settings.language = language.into();
                            view.persist();
                            cx.notify();
                        })),
                );
            }
            body = body.child(languages).child(self.t("Тема", "Theme"));
            let mut themes = div().flex().gap_2();
            for (theme, label) in [
                ("light", self.t("Світла", "Light")),
                ("olive", self.t("Оливкова", "Olive")),
                ("purple", self.t("Фіолетова", "Purple")),
            ] {
                themes = themes.child(
                    self.button(cx, theme, label, self.settings.theme == theme)
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.settings.theme = theme.into();
                            view.persist();
                            cx.notify();
                        })),
                );
            }
            body = body
                .child(themes)
                .child(self.t("Тека завантажень", "Download directory"))
                .child(self.output_editor(cx))
                .child(
                    self.button(cx, "save", self.t("Зберегти", "Save"), true).on_click(cx.listener(
                        |view, _, _, cx| {
                            view.settings.output_dir =
                                PathBuf::from(view.output.read(cx).content.as_str());
                            if view.persist() {
                                view.status = view.t("Збережено", "Saved").into();
                            }
                            cx.notify();
                        },
                    )),
                )
                .child(self.t(
                    "Cmd/Ctrl+V — вставити · Tab — наступний елемент · Cmd/Ctrl+Enter — завантажити",
                    "Cmd/Ctrl+V to paste · Tab for next control · Cmd/Ctrl+Enter to download",
                ));
        } else {
            body = body
                .child(div().text_xl().child(match self.screen {
                    Screen::Video => self.t("Завантажити відео", "Download video"),
                    Screen::Playlist => self.t("Завантажити плейлист", "Download playlist"),
                    _ => self.t("Завантажити музику", "Download music"),
                }))
                .child(self.t(
                    "YouTube URL (кілька адрес через пробіл)",
                    "YouTube URL (separate multiple URLs with spaces)",
                ))
                .child(self.url.clone());
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        self.button(
                            cx,
                            "inspect",
                            if self.busy {
                                self.t("Зачекайте…", "Please wait…")
                            } else {
                                self.t("Перевірити", "Inspect")
                            },
                            false,
                        )
                        .on_click(cx.listener(|view, _, _, cx| view.load(false, cx))),
                    )
                    .child(
                        self.button(cx, "download", self.t("Завантажити", "Download"), true)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.download(&Download, window, cx)
                            })),
                    ),
            );
            let formats: &[OutputFormat] = if self.screen == Screen::Video {
                &[OutputFormat::Mp4, OutputFormat::Mkv, OutputFormat::Webm]
            } else {
                &[
                    OutputFormat::Mp3,
                    OutputFormat::M4a,
                    OutputFormat::Flac,
                    OutputFormat::Wav,
                ]
            };
            let mut row = div().flex().flex_wrap().gap_2();
            for format in formats.iter().copied() {
                row = row.child(
                    self.button(
                        cx,
                        format.extension(),
                        format.extension().to_uppercase(),
                        self.settings.format == format,
                    )
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.settings.format = format;
                        view.persist();
                        cx.notify();
                    })),
                );
            }
            body = body.child(row);
            if self.screen == Screen::Video {
                let mut row = div().flex().flex_wrap().gap_2();
                for (index, height) in [
                    Some(360),
                    Some(720),
                    Some(1080),
                    Some(1440),
                    Some(2160),
                    None,
                ]
                .into_iter()
                .enumerate()
                {
                    row = row.child(
                        self.button(
                            cx,
                            ("quality", index),
                            height
                                .map(|h| format!("{h}p"))
                                .unwrap_or_else(|| self.t("Найкраща", "Best").into()),
                            self.quality == height,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.quality = height;
                            cx.notify();
                        })),
                    );
                }
                body = body.child(row);
            }
            body = body
                .child(self.t("Тека завантажень", "Download directory"))
                .child(self.output_editor(cx));
            if let Some(info) = &self.info {
                body = body.child(div().text_lg().child(info.title.clone()));
                for warning in &info.warnings {
                    body = body.child(warning.clone());
                }
                if info.has_chapters && self.screen != Screen::Video {
                    body = body.child(self.t(
                        "Розділи (виберіть потрібні)",
                        "Chapters (select to include)",
                    ));
                    for (index, chapter) in info.chapters.iter().enumerate() {
                        let number = index + 1;
                        let selected = self.selected_chapters.contains(&number);
                        body = body.child(
                            self.button(
                                cx,
                                ("chapter", index),
                                format!(
                                    "{} {number}. {} · {:.0}s",
                                    if selected { "☑" } else { "☐" },
                                    chapter.title,
                                    chapter.start
                                ),
                                selected,
                            )
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    if !view.selected_chapters.remove(&number) {
                                        view.selected_chapters.insert(number);
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                }
                let heights: BTreeSet<u32> = info
                    .formats
                    .iter()
                    .filter_map(|format| format.height)
                    .collect();
                let available = heights
                    .into_iter()
                    .map(|height| format!("{height}p"))
                    .collect::<Vec<_>>()
                    .join(" · ");
                if !available.is_empty() {
                    body = body.child(div().text_sm().child(format!(
                        "{}: {available}",
                        self.t("Доступна якість", "Available quality")
                    )));
                }
            }
            if self.screen == Screen::Playlist {
                for (index, item) in self.playlist.iter().enumerate() {
                    let number = index + 1;
                    let selected = self.selected_items.contains(&number);
                    let available = item.available;
                    body = body.child(
                        self.button(
                            cx,
                            ("item", index),
                            format!(
                                "{} {number}. {}{}",
                                if selected { "☑" } else { "☐" },
                                item.title,
                                if available { "" } else { " — unavailable" }
                            ),
                            selected,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            if available && !view.selected_items.remove(&number) {
                                view.selected_items.insert(number);
                            }
                            cx.notify();
                        })),
                    );
                }
                if self.continuation.is_some() {
                    body = body.child(
                        self.button(cx, "more", self.t("Наступна сторінка", "Next page"), false)
                            .on_click(cx.listener(|view, _, _, cx| view.load(true, cx))),
                    );
                }
                if !self.playlist.is_empty() {
                    body = body.child(if self.playlist_complete {
                        self.t("Усі сторінки отримано", "All pages loaded")
                    } else {
                        self.t(
                            "Показано частину плейлиста; завантаження охопить вибрані елементи",
                            "Partial playlist shown; download includes selected items",
                        )
                    });
                }
            }
        }
        body = body
            .child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(panel))
                    .child(self.status.clone()),
            )
            .child(div().text_xl().child(self.t("Завдання", "Jobs")));
        for job in self.engine.list() {
            let id = job.id.clone();
            let mut card = div()
                .p_3()
                .rounded_md()
                .bg(rgb(panel))
                .flex()
                .flex_col()
                .gap_2()
                .child(format!(
                    "{} · {}",
                    &id[..8.min(id.len())],
                    self.state(job.state)
                ));
            let total = job
                .progress
                .total
                .map(|n| format!(" / {:.2} MB", n as f64 / 1_000_000.))
                .unwrap_or_default();
            card = card.child(format!(
                "{:.2} MB{total}{}",
                job.progress.bytes as f64 / 1_000_000.,
                job.progress
                    .bytes_per_second
                    .map(|s| format!(" · {:.2} MB/s", s / 1_000_000.))
                    .unwrap_or_default()
            ));
            if let Some(error) = &job.error {
                card = card.child(error.to_string());
            }
            let mut controls = div().flex().flex_wrap().gap_2();
            let options: &[(&str, &str)] = if job.state == JobState::Paused {
                &[
                    ("resume", self.t("Продовжити", "Resume")),
                    ("cancel", self.t("Скасувати", "Cancel")),
                ]
            } else if matches!(
                job.state,
                JobState::Failed | JobState::PartiallyCompleted | JobState::Cancelled
            ) {
                &[("retry", self.t("Повторити незавершені", "Retry unfinished"))]
            } else if job.state == JobState::Completed {
                &[]
            } else {
                &[
                    ("pause", self.t("Пауза", "Pause")),
                    ("cancel", self.t("Скасувати", "Cancel")),
                ]
            };
            for (index, (action, label)) in options.iter().copied().enumerate() {
                let job_id = id.clone();
                controls = controls.child(
                    self.button(
                        cx,
                        (SharedString::from(format!("{id}-action")), index),
                        label,
                        false,
                    )
                    .on_click(
                        cx.listener(move |view, _, _, cx| view.job_action(&job_id, action, cx)),
                    ),
                );
            }
            card = card.child(controls);
            for item in &job.items {
                card = card.child(format!("{} · {}", item.title, self.state(item.state)));
                if let Some(error) = &item.error {
                    card = card.child(error.to_string());
                }
                for warning in &item.warnings {
                    card = card.child(warning.clone());
                }
                for (artifact_index, artifact) in item.artifacts.iter().enumerate() {
                    let path = artifact.path.clone();
                    card = card.child(
                        self.button(
                            cx,
                            SharedString::from(format!(
                                "{}-item-{}-artifact-{artifact_index}",
                                job.id, item.index
                            )),
                            path.to_string_lossy().to_string(),
                            false,
                        )
                        .text_sm()
                        .on_click(move |_, _, cx| cx.reveal_path(&path)),
                    );
                }
            }
            body = body.child(card);
        }
        for job in &self.saved_jobs {
            if job.state == JobState::Completed {
                continue;
            }
            let id = job.id.clone();
            let retry = job.state.terminal();
            let mut card = div()
                .p_3()
                .rounded_md()
                .bg(rgb(panel))
                .flex()
                .flex_col()
                .gap_2()
                .child(format!(
                    "{} · {} · {}",
                    self.t("Збережене завдання", "Saved job"),
                    id,
                    self.state(job.state)
                ));
            if let Some(item) = job.items.first() {
                card = card.child(item.title.clone());
            }
            if let Some(error) = &job.error {
                card = card.child(error.to_string());
            }
            card = card.child(
                self.button(
                    cx,
                    SharedString::from(format!("restore-{id}")),
                    if retry {
                        self.t("Повторити незавершені", "Retry unfinished")
                    } else {
                        self.t("Продовжити", "Resume")
                    },
                    false,
                )
                .on_click(cx.listener(move |view, _, _, cx| view.restore(&id, retry, cx))),
            );
            body = body.child(card);
        }
        div()
            .size_full()
            .tab_group()
            .flex()
            .bg(rgb(background))
            .text_color(rgb(foreground))
            .text_size(px(15.))
            .on_action(cx.listener(Self::inspect))
            .on_action(cx.listener(Self::download))
            .on_action(cx.listener(Self::next_field))
            .on_action(cx.listener(Self::previous_field))
            .child(navigation)
            .child(
                div()
                    .id("main-scroll")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

fn main() {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Cannot start runtime: {error}");
            return;
        }
    };
    let engine = match tuneka_cli::engine() {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("Cannot start Tuneka: {error}");
            return;
        }
    };
    let settings = match settings::update(&settings::data_dir().join("settings.json"), |_| {}) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Cannot load settings: {error}");
            return;
        }
    };
    let shutdown = engine.clone();
    let handle = runtime.handle().clone();
    Application::new().run(move |cx: &mut GpuiApp| {
        input::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("tab", NextField, None),
            KeyBinding::new("shift-tab", PreviousField, None),
            KeyBinding::new("cmd-i", Inspect, None),
            KeyBinding::new("ctrl-i", Inspect, None),
            KeyBinding::new("cmd-enter", Download, None),
            KeyBinding::new("ctrl-enter", Download, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let initial_size = size(px(1040.), px(780.));
        // Native QA may start at a constrained size without scripting the OS.
        // Release builds always use the normal initial size.
        #[cfg(debug_assertions)]
        let initial_size = std::env::var("TUNEKA_GUI_QA_SIZE")
            .ok()
            .and_then(|value| {
                let (width, height) = value.split_once('x')?;
                let (width, height) = (width.parse::<u32>().ok()?, height.parse::<u32>().ok()?);
                ((640..=4000).contains(&width) && (480..=3000).contains(&height))
                    .then(|| size(px(width as f32), px(height as f32)))
            })
            .unwrap_or(initial_size);
        let bounds = Bounds::centered(None, initial_size, cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tuneka".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(640.), px(480.))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Tuneka::new(engine, handle, settings, cx));
                window.focus(&view.read(cx).url.focus_handle(cx));
                view
            },
        );
        if let Err(error) = result {
            eprintln!("Cannot open Tuneka window: {error}");
            cx.quit();
        }
        cx.activate(true);
    });
    // Runtime survives the native event loop. Cooperatively checkpoint all workers
    // before dropping the runtime, including when the last window is closed.
    runtime.block_on(shutdown.shutdown());
}

#[cfg(test)]
mod tests {
    use super::reveal_offset;
    use gpui::{Bounds, point, px, size};
    #[test]
    fn keyboard_focus_reveals_controls_above_and_below_viewport() {
        let viewport = Bounds::new(point(px(170.), px(30.)), size(px(470.), px(480.)));
        let offset = point(px(0.), px(-200.));
        assert_eq!(
            reveal_offset(
                viewport,
                Bounds::new(point(px(200.), px(0.)), size(px(100.), px(40.))),
                offset
            )
            .y,
            px(-170.)
        );
        assert_eq!(
            reveal_offset(
                viewport,
                Bounds::new(point(px(200.), px(500.)), size(px(100.), px(40.))),
                offset
            )
            .y,
            px(-230.)
        );
        assert_eq!(
            reveal_offset(
                viewport,
                Bounds::new(point(px(10.), px(0.)), size(px(100.), px(40.))),
                offset
            ),
            offset
        );
    }
}
