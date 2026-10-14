use crate::{
    files::{FileLock, Reservation, atomic_json, safe_name},
    *,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Semaphore, broadcast};
struct Job {
    snapshot: Mutex<JobSnapshot>,
    control: Mutex<Control>,
    running: AtomicBool,
    _lock: FileLock,
}
pub struct Engine {
    metadata: Arc<dyn MetadataProvider>,
    downloader: Arc<dyn Downloader>,
    media: Arc<dyn MediaProcessor>,
    root: PathBuf,
    jobs: Mutex<BTreeMap<String, Arc<Job>>>,
    events: broadcast::Sender<JobSnapshot>,
    slots: Semaphore,
}
impl Engine {
    pub fn new(
        metadata: Arc<dyn MetadataProvider>,
        downloader: Arc<dyn Downloader>,
        media: Arc<dyn MediaProcessor>,
        root: PathBuf,
    ) -> Result<Arc<Self>> {
        fs::create_dir_all(root.join("jobs"))?;
        let (events, _) = broadcast::channel(128);
        Ok(Arc::new(Self {
            metadata,
            downloader,
            media,
            root,
            jobs: Mutex::new(BTreeMap::new()),
            events,
            slots: Semaphore::new(2),
        }))
    }
    pub fn subscribe(&self) -> broadcast::Receiver<JobSnapshot> {
        self.events.subscribe()
    }
    pub async fn inspect(&self, url: &str) -> Result<MediaInfo> {
        parse_url(url)?;
        self.metadata.inspect(url).await
    }
    pub async fn playlist_page(
        &self,
        url: &str,
        continuation: Option<&str>,
    ) -> Result<PlaylistPage> {
        parse_url(url)?;
        self.metadata.playlist_page(url, continuation).await
    }
    fn path(&self, id: &str) -> PathBuf {
        self.root.join("jobs").join(format!("{id}.json"))
    }
    fn job(&self, id: &str) -> Result<Arc<Job>> {
        self.jobs
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::InvalidInput, "unknown job"))
    }
    pub fn get(&self, id: &str) -> Result<JobSnapshot> {
        Ok(self.job(id)?.snapshot.lock().unwrap().clone())
    }
    pub fn list(&self) -> Vec<JobSnapshot> {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .map(|j| j.snapshot.lock().unwrap().clone())
            .collect()
    }
    fn update(&self, job: &Job, f: impl FnOnce(&mut JobSnapshot)) -> Result<()> {
        let mut s = job.snapshot.lock().unwrap();
        f(&mut s);
        s.revision += 1;
        atomic_json(&self.path(&s.id), &*s)?;
        let _ = self.events.send(s.clone());
        Ok(())
    }
    pub fn create(self: &Arc<Self>, mut request: DownloadRequest) -> Result<String> {
        if request.urls.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "at least one URL is required",
            ));
        }
        if request.chapters.contains(&0) || request.playlist_items.contains(&0) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "selection indices are one-based",
            ));
        }
        if !request.format.is_audio() && !request.chapters.is_empty() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "video chapter slicing is not supported",
            ));
        }
        for u in &request.urls {
            parse_url(u)?;
        }
        fs::create_dir_all(&request.output_dir)?;
        request.output_dir = fs::canonicalize(&request.output_dir)?;
        let id = uuid::Uuid::new_v4().to_string();
        let lock = FileLock::acquire(&self.path(&id).with_extension("lock"))?;
        let snapshot = JobSnapshot {
            id: id.clone(),
            revision: 0,
            state: JobState::Queued,
            request,
            enumeration_complete: false,
            items: vec![],
            progress: Progress::default(),
            warnings: vec![],
            error: None,
        };
        atomic_json(&self.path(&id), &snapshot)?;
        let job = Arc::new(Job {
            snapshot: Mutex::new(snapshot),
            control: Mutex::new(Control::default()),
            running: AtomicBool::new(false),
            _lock: lock,
        });
        self.jobs.lock().unwrap().insert(id.clone(), job.clone());
        self.spawn(job);
        Ok(id)
    }
    pub fn restore(self: &Arc<Self>, id: &str, retry_failed: bool) -> Result<()> {
        self.restore_inner(id, retry_failed, None)
    }
    pub fn restore_in_root(
        self: &Arc<Self>,
        id: &str,
        retry_failed: bool,
        allowed_root: &std::path::Path,
    ) -> Result<()> {
        self.restore_inner(id, retry_failed, Some(allowed_root))
    }
    fn restore_inner(
        self: &Arc<Self>,
        id: &str,
        retry_failed: bool,
        allowed_root: Option<&std::path::Path>,
    ) -> Result<()> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(Error::new(ErrorCode::InvalidInput, "invalid job ID"));
        }
        if self.jobs.lock().unwrap().contains_key(id) {
            if let Some(root) = allowed_root
                && !fs::canonicalize(&self.get(id)?.request.output_dir)?.starts_with(root)
            {
                return Err(Error::new(
                    ErrorCode::InvalidInput,
                    "saved job output is outside server root",
                ));
            }
            return self.resume(id, retry_failed);
        }
        let lock = FileLock::acquire(&self.path(id).with_extension("lock"))?;
        let mut snapshot: JobSnapshot = serde_json::from_slice(&fs::read(self.path(id))?)?;
        if let Some(root) = allowed_root
            && !fs::canonicalize(&snapshot.request.output_dir)?.starts_with(root)
        {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "saved job output is outside server root",
            ));
        }
        if snapshot.id != id {
            return Err(Error::new(
                ErrorCode::Protocol,
                "checkpoint identity mismatch",
            ));
        }
        if snapshot.state == JobState::Completed {
            return Err(Error::new(ErrorCode::Conflict, "job is already complete"));
        }
        if snapshot.state.terminal() && !retry_failed {
            return Err(Error::new(
                ErrorCode::Conflict,
                "terminal job requires explicit retry",
            ));
        }
        snapshot.state = JobState::Paused;
        let job = Arc::new(Job {
            snapshot: Mutex::new(snapshot),
            control: Mutex::new(Control::default()),
            running: AtomicBool::new(false),
            _lock: lock,
        });
        self.jobs.lock().unwrap().insert(id.into(), job);
        self.resume(id, retry_failed)
    }
    pub fn pause(&self, id: &str) -> Result<()> {
        let j = self.job(id)?;
        if j.snapshot.lock().unwrap().state.terminal() {
            return Err(Error::new(ErrorCode::Conflict, "job is terminal"));
        }
        j.control.lock().unwrap().pause();
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let j = self.job(id)?;
        if j.snapshot.lock().unwrap().state.terminal() {
            return Err(Error::new(ErrorCode::Conflict, "job is terminal"));
        }
        j.control.lock().unwrap().cancel();
        if !j.running.load(Ordering::SeqCst) {
            self.cleanup_work(id)?;
            self.update(&j, |s| {
                s.state = JobState::Cancelled;
                for item in &mut s.items {
                    if !item.state.terminal() {
                        item.state = JobState::Cancelled;
                    }
                }
            })?;
        }
        Ok(())
    }
    pub fn cancel_saved(&self, id: &str) -> Result<JobSnapshot> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(Error::new(ErrorCode::InvalidInput, "invalid job ID"));
        }
        if self.jobs.lock().unwrap().contains_key(id) {
            self.cancel(id)?;
            return self.get(id);
        }
        let _lock = FileLock::acquire(&self.path(id).with_extension("lock"))?;
        let mut snapshot: JobSnapshot = serde_json::from_slice(&fs::read(self.path(id))?)?;
        if snapshot.id != id || snapshot.state.terminal() {
            return Err(Error::new(ErrorCode::Conflict, "job cannot be cancelled"));
        }
        self.cleanup_work(id)?;
        snapshot.state = JobState::Cancelled;
        snapshot.revision += 1;
        for item in &mut snapshot.items {
            if !item.state.terminal() {
                item.state = JobState::Cancelled;
            }
        }
        atomic_json(&self.path(id), &snapshot)?;
        Ok(snapshot)
    }
    pub fn resume(self: &Arc<Self>, id: &str, retry_failed: bool) -> Result<()> {
        let j = self.job(id)?;
        if j.running.load(Ordering::SeqCst) {
            return Err(Error::new(ErrorCode::Conflict, "worker has not stopped"));
        }
        let state = j.snapshot.lock().unwrap().state;
        if state != JobState::Paused
            && !(retry_failed
                && matches!(
                    state,
                    JobState::Failed | JobState::PartiallyCompleted | JobState::Cancelled
                ))
        {
            return Err(Error::new(ErrorCode::Conflict, "job cannot resume"));
        }
        *j.control.lock().unwrap() = Control::default();
        self.update(&j, |s| {
            s.state = JobState::Queued;
            s.error = None;
            for i in &mut s.items {
                if i.state != JobState::Completed {
                    i.state = JobState::Queued;
                    i.error = None;
                }
            }
        })?;
        self.spawn(j);
        Ok(())
    }
    fn spawn(self: &Arc<Self>, job: Arc<Job>) {
        if job.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let control = job.control.lock().unwrap().clone();
            let result = this.run(&job, &control).await;
            if let Err(e) = result {
                if e.code == ErrorCode::Cancelled {
                    let id = job.snapshot.lock().unwrap().id.clone();
                    let _ = this.cleanup_work(&id);
                }
                let state = match e.code {
                    ErrorCode::Paused => JobState::Paused,
                    ErrorCode::Cancelled => JobState::Cancelled,
                    _ => JobState::Failed,
                };
                let _ = this.update(&job, |s| {
                    s.state = state;
                    if state == JobState::Cancelled {
                        for item in &mut s.items {
                            if !item.state.terminal() {
                                item.state = JobState::Cancelled;
                            }
                        }
                    }
                    s.error = Some(e);
                });
            }
            job.running.store(false, Ordering::SeqCst);
        });
    }
    pub async fn wait(&self, id: &str) -> Result<JobSnapshot> {
        loop {
            let j = self.job(id)?;
            if !j.running.load(Ordering::SeqCst) {
                return self.get(id);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    fn cleanup_work(&self, id: &str) -> Result<()> {
        let work = self.root.join("work").join(id);
        if work.exists() {
            fs::remove_dir_all(work)?;
        }
        Ok(())
    }
    pub async fn shutdown(&self) {
        let jobs: Vec<_> = self.jobs.lock().unwrap().values().cloned().collect();
        for j in &jobs {
            j.control.lock().unwrap().pause();
        }
        while jobs.iter().any(|j| j.running.load(Ordering::SeqCst)) {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    async fn run(self: &Arc<Self>, job: &Arc<Job>, control: &Control) -> Result<()> {
        let _permit = self
            .slots
            .acquire()
            .await
            .map_err(|_| Error::new(ErrorCode::Internal, "scheduler closed"))?;
        control.check()?;
        self.update(job, |s| s.state = JobState::Resolving)?;
        let request = job.snapshot.lock().unwrap().request.clone();
        let enumerate = !job.snapshot.lock().unwrap().enumeration_complete;
        if enumerate {
            // Re-enumeration preserves per-occurrence outcomes and successful artifacts.
            let mut discovered = vec![];
            let mut enumeration_error = None;
            for url in &request.urls {
                control.check()?;
                match parse_url(url)? {
                    YoutubeUrl::Video(_) => discovered.push((url.clone(), String::new(), true)),
                    YoutubeUrl::Playlist(_) => {
                        let mut cursor = None;
                        let mut seen = std::collections::HashSet::new();
                        let mut position = 0;
                        loop {
                            control.check()?;
                            match self.metadata.playlist_page(url, cursor.as_deref()).await {
                                Ok(page) => {
                                    for item in page.items {
                                        position += 1;
                                        if request.playlist_items.is_empty()
                                            || request.playlist_items.contains(&position)
                                        {
                                            discovered.push((
                                                format!(
                                                    "https://www.youtube.com/watch?v={}",
                                                    item.id
                                                ),
                                                item.title,
                                                item.available,
                                            ));
                                        }
                                    }
                                    if page.complete {
                                        if request.playlist_items.iter().any(|i| *i > position) {
                                            enumeration_error = Some(Error::new(
                                                ErrorCode::InvalidInput,
                                                "selected playlist item is out of range",
                                            ));
                                        }
                                        if page.continuation.is_some() {
                                            enumeration_error = Some(Error::new(
                                                ErrorCode::Protocol,
                                                "complete playlist has continuation",
                                            ));
                                        }
                                        break;
                                    }
                                    match page.continuation {
                                        Some(c) if seen.insert(c.clone()) => cursor = Some(c),
                                        _ => {
                                            enumeration_error = Some(Error::new(
                                                ErrorCode::Protocol,
                                                "missing/repeated playlist continuation",
                                            ));
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    enumeration_error = Some(e);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            self.update(job, |s| {
                s.enumeration_complete = enumeration_error.is_none();
                s.error = enumeration_error.clone();
                let old = std::mem::take(&mut s.items);
                s.items = discovered
                    .into_iter()
                    .enumerate()
                    .map(|(index, (url, title, available))| {
                        if let Some(item) = old
                            .get(index)
                            .filter(|i| i.url == url && i.state == JobState::Completed)
                        {
                            return item.clone();
                        }
                        ItemOutcome {
                            index,
                            url,
                            title,
                            state: if available {
                                JobState::Queued
                            } else {
                                JobState::Failed
                            },
                            artifacts: vec![],
                            error: if available {
                                None
                            } else {
                                Some(Error::new(
                                    ErrorCode::Unavailable,
                                    "playlist item unavailable",
                                ))
                            },
                            warnings: vec![],
                        }
                    })
                    .collect();
            })?;
        }
        let count = job.snapshot.lock().unwrap().items.len();
        for index in 0..count {
            control.check()?;
            let item = job.snapshot.lock().unwrap().items[index].clone();
            if matches!(item.state, JobState::Completed | JobState::Failed) {
                continue;
            }
            match self.run_item(job, index, &request, control).await {
                Ok(()) => {
                    self.update(job, |s| s.items[index].state = JobState::Completed)?;
                    let id = job.snapshot.lock().unwrap().id.clone();
                    let work = self.root.join("work").join(id).join(index.to_string());
                    if let Err(error) = fs::remove_dir_all(work) {
                        self.update(job, |s| {
                            s.items[index]
                                .warnings
                                .push(format!("Temporary cleanup: {error}"))
                        })?;
                    }
                }
                Err(e) if matches!(e.code, ErrorCode::Paused | ErrorCode::Cancelled) => {
                    self.update(job, |s| {
                        s.items[index].state = if e.code == ErrorCode::Paused {
                            JobState::Paused
                        } else {
                            JobState::Cancelled
                        }
                    })?;
                    return Err(e);
                }
                Err(e) => self.update(job, |s| {
                    s.items[index].state = JobState::Failed;
                    s.items[index].error = Some(e);
                })?,
            }
        }
        control.check()?;
        self.update(job, |s| {
            let successes = s
                .items
                .iter()
                .filter(|i| i.state == JobState::Completed)
                .count();
            s.state = if successes == s.items.len() && successes > 0 && s.enumeration_complete {
                JobState::Completed
            } else if successes > 0 {
                JobState::PartiallyCompleted
            } else {
                JobState::Failed
            };
        })?;
        Ok(())
    }
    async fn run_item(
        self: &Arc<Self>,
        job: &Arc<Job>,
        index: usize,
        request: &DownloadRequest,
        control: &Control,
    ) -> Result<()> {
        self.update(job, |s| {
            s.state = JobState::Resolving;
            s.items[index].state = JobState::Resolving;
        })?;
        let url = job.snapshot.lock().unwrap().items[index].url.clone();
        let info = self.metadata.inspect(&url).await?;
        control.check()?;
        self.update(job, |s| {
            s.items[index].title = info.title.clone();
            s.items[index].warnings.extend(info.warnings.clone());
        })?;
        if request.chapters.iter().any(|i| *i > info.chapters.len()) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "selected chapter is out of range",
            ));
        }
        let (primary, audio) = select_formats(&info, request)?;
        let id = job.snapshot.lock().unwrap().id.clone();
        let work = self.root.join("work").join(&id).join(index.to_string());
        fs::create_dir_all(&work)?;
        self.update(job, |s| {
            s.state = JobState::Downloading;
            s.items[index].state = JobState::Downloading;
        })?;
        let weak = Arc::downgrade(self);
        let job_progress = job.clone();
        let progress: ProgressSink = Arc::new(move |p| {
            if let Some(engine) = weak.upgrade() {
                let mut s = job_progress.snapshot.lock().unwrap();
                s.progress = p;
                s.revision += 1;
                let _ = engine.events.send(s.clone());
            }
        });
        let input = self
            .fetch(
                &info.id,
                &primary.id,
                work.join("primary"),
                control,
                progress.clone(),
            )
            .await?;
        let additional_audio = if let Some(a) = audio {
            Some(
                self.fetch(&info.id, &a.id, work.join("audio"), control, progress)
                    .await?,
            )
        } else {
            None
        };
        control.check()?;
        self.update(job, |s| {
            s.state = JobState::Processing;
            s.items[index].state = JobState::Processing;
        })?;
        let chapters = if request.format.is_audio()
            && (!request.chapters.is_empty() || info.has_chapters)
        {
            info.chapters
                .iter()
                .enumerate()
                .filter(|(i, _)| request.chapters.is_empty() || request.chapters.contains(&(i + 1)))
                .map(|(i, c)| {
                    (
                        Some(c.clone()),
                        format!("{:02} {}", i + 1, safe_name(&c.title, false)),
                    )
                })
                .collect::<Vec<_>>()
        } else {
            vec![(None, safe_name(&info.title, false))]
        };
        if chapters.is_empty() {
            return Err(Error::new(ErrorCode::InvalidInput, "no selected chapters"));
        }
        let output_dir = if info.has_chapters && request.format.is_audio() {
            request.output_dir.join(safe_name(&info.title, true))
        } else {
            request.output_dir.clone()
        };
        fs::create_dir_all(&output_dir)?;
        let output_dir = fs::canonicalize(output_dir)?;
        if !output_dir.starts_with(&request.output_dir) {
            return Err(Error::new(
                ErrorCode::InvalidInput,
                "output folder escapes configured root through a symlink",
            ));
        }
        let cover = if let Some(url) = info.thumbnails.last() {
            control.check()?;
            let cover_path = work.join("cover");
            if cover_path.is_file() {
                Some(cover_path)
            } else {
                match self.metadata.fetch_thumbnail(url, cover_path).await {
                    Ok(path) => Some(path),
                    Err(_) => {
                        self.update(job, |s| {
                            s.items[index]
                                .warnings
                                .push("Cover download unavailable".into())
                        })?;
                        None
                    }
                }
            }
        } else {
            None
        };
        for (chapter_index, (chapter, name)) in chapters.into_iter().enumerate() {
            control.check()?;
            let output = output_dir.join(format!("{name}.{}", request.format.extension()));
            if job.snapshot.lock().unwrap().items[index]
                .artifacts
                .iter()
                .any(|a| a.path == output && fs::metadata(&output).is_ok_and(|m| m.len() == a.size))
            {
                continue;
            }
            let receipt = work.join(format!("publication-{chapter_index}.json"));
            if let Some(artifact) = files::recover_publication(&receipt, &output)? {
                self.update(job, |s| s.items[index].artifacts.push(artifact))?;
                continue;
            }
            let _reservation = Reservation::acquire(output.clone())?;
            let (artifact, warnings) = self
                .media
                .process(
                    ProcessRequest {
                        publication_receipt: receipt,
                        input: input.clone(),
                        additional_audio: additional_audio.clone(),
                        output,
                        format: request.format,
                        chapter,
                        title: info.title.clone(),
                        cover: cover.clone(),
                    },
                    control.clone(),
                )
                .await?;
            self.update(job, |s| {
                s.items[index].artifacts.push(artifact);
                s.items[index].warnings.extend(warnings);
            })?;
        }
        Ok(())
    }
    async fn fetch(
        &self,
        id: &str,
        format: &str,
        path: PathBuf,
        control: &Control,
        progress: ProgressSink,
    ) -> Result<PathBuf> {
        for attempt in 0..2 {
            control.check()?;
            let stream = self.metadata.resolve_stream(id, format).await?;
            match self
                .downloader
                .download(stream, path.clone(), control.clone(), progress.clone())
                .await
            {
                Err(e) if e.code == ErrorCode::Unavailable && attempt == 0 => continue,
                result => return result,
            }
        }
        unreachable!()
    }
}
pub fn select_formats(
    info: &MediaInfo,
    request: &DownloadRequest,
) -> Result<(Format, Option<Format>)> {
    let mut audio: Vec<_> = info
        .formats
        .iter()
        .filter(|f| f.has_audio && !f.has_video)
        .collect();
    audio.sort_by_key(|f| f.bitrate);
    if request.format.is_audio() {
        return audio
            .last()
            .copied()
            .or_else(|| info.formats.iter().find(|f| f.has_audio))
            .cloned()
            .map(|f| (f, None))
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "no audio format"));
    }
    let compatible = |f: &&Format| match request.format {
        OutputFormat::Mp4 => f.codecs.iter().all(|c| {
            c.starts_with("avc")
                || c.starts_with("mp4a")
                || c.starts_with("hvc")
                || c.starts_with("hev")
        }),
        OutputFormat::Webm => f.codecs.iter().all(|c| {
            c.starts_with("vp")
                || c.starts_with("av01")
                || c.starts_with("opus")
                || c.starts_with("vorbis")
        }),
        _ => true,
    };
    let video = info
        .formats
        .iter()
        .filter(|f| {
            f.has_video
                && request
                    .max_height
                    .is_none_or(|h| f.height.is_none_or(|v| v <= h))
        })
        .filter(compatible)
        .max_by_key(|f| (f.height.unwrap_or(0), f.bitrate))
        .cloned()
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                "no compatible video format; implicit transcoding disabled",
            )
        })?;
    let audio = if video.has_audio {
        None
    } else {
        Some(
            audio
                .into_iter()
                .filter(compatible)
                .max_by_key(|f| f.bitrate)
                .cloned()
                .ok_or_else(|| Error::new(ErrorCode::Unsupported, "no compatible audio stream"))?,
        )
    };
    Ok((video, audio))
}
