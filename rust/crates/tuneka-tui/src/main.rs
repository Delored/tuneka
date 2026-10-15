use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{io, sync::Arc, time::Duration};
use tuneka_core::*;
struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
enum Inspection {
    Video(MediaInfo),
    Playlist(PlaylistPage),
}
struct Ui {
    url: String,
    status: String,
    editing: bool,
    inspection: Option<Inspection>,
    selected: std::collections::BTreeSet<usize>,
    cursor: usize,
    job_cursor: usize,
    jobs_focus: bool,
    format: OutputFormat,
    generation: u64,
    busy: bool,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            url: String::new(),
            status: "Enter a YouTube URL".into(),
            editing: true,
            inspection: None,
            selected: Default::default(),
            cursor: 0,
            job_cursor: 0,
            jobs_focus: false,
            format: OutputFormat::Mp3,
            generation: 0,
            busy: false,
        }
    }
}
impl Ui {
    fn key(&mut self, key: KeyCode) -> bool {
        if self.editing {
            match key {
                KeyCode::Char(c) => self.url.push(c),
                KeyCode::Backspace => {
                    self.url.pop();
                }
                KeyCode::Esc => self.editing = false,
                _ => {}
            };
            return true;
        }
        false
    }
    fn count(&self) -> usize {
        match &self.inspection {
            Some(Inspection::Video(v)) => v.chapters.len(),
            Some(Inspection::Playlist(p)) => p.items.len(),
            None => 0,
        }
    }
    fn rows(&self) -> Vec<String> {
        match &self.inspection {
            Some(Inspection::Video(v)) => v
                .chapters
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "{} {} {:02} {:.1}s {}",
                        if i == self.cursor { ">" } else { " " },
                        if self.selected.contains(&(i + 1)) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        i + 1,
                        c.start,
                        c.title
                    )
                })
                .collect(),
            Some(Inspection::Playlist(p)) => p
                .items
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "{} {} {:02} {}{}",
                        if i == self.cursor { ">" } else { " " },
                        if self.selected.contains(&(i + 1)) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        i + 1,
                        c.title,
                        if c.available { "" } else { " [unavailable]" }
                    )
                })
                .collect(),
            None => vec![],
        }
    }
}
fn job_row(job: &JobSnapshot, focused: bool) -> ListItem<'static> {
    let diagnostic = job
        .error
        .as_ref()
        .or_else(|| job.items.iter().find_map(|item| item.error.as_ref()))
        .map(|error| error.message.as_str())
        .or_else(|| job.warnings.first().map(String::as_str))
        .or_else(|| {
            job.items
                .iter()
                .find_map(|item| item.warnings.first().map(String::as_str))
        })
        .unwrap_or("");
    ListItem::new(format!(
        "{} {} {:?} {} bytes {}",
        if focused { ">" } else { " " },
        &job.id[..8],
        job.state,
        job.progress.bytes,
        diagnostic
    ))
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tuneka_cli::quiet_native_media_logs();
    let engine = tuneka_cli::engine()?;
    let settings = settings::update(&settings::data_dir().join("settings.json"), |_| {})?;
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let screen = Screen;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let result = run(&mut terminal, engine.clone(), settings).await;
    engine.shutdown().await;
    drop(screen);
    result
}
async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: Arc<Engine>,
    settings: settings::Settings,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut ui = Ui::default();
    let (sender, mut receiver) =
        tokio::sync::mpsc::unbounded_channel::<(u64, Result<Inspection>)>();
    loop {
        while let Ok((generation, result)) = receiver.try_recv() {
            if generation != ui.generation {
                continue;
            }
            ui.busy = false;
            match result {
                Ok(value) => {
                    ui.status = "Space select · d download · n next playlist page".into();
                    match (&mut ui.inspection, value) {
                        (Some(Inspection::Playlist(existing)), Inspection::Playlist(page)) => {
                            existing.items.extend(page.items);
                            existing.continuation = page.continuation;
                            existing.complete = page.complete;
                        }
                        (_, value) => ui.inspection = Some(value),
                    }
                }
                Err(e) => ui.status = e.to_string(),
            }
        }
        let jobs = engine.list();
        ui.job_cursor = ui.job_cursor.min(jobs.len().saturating_sub(1));
        terminal.draw(|f|{
   let areas=Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3),Constraint::Min(4),Constraint::Length(8),Constraint::Length(4)]).split(f.area());
   f.render_widget(Paragraph::new(ui.url.as_str()).block(Block::default().borders(Borders::ALL).title(if ui.editing{"URL (Enter inspect)"}else{"URL (e edit)"})),areas[0]);
   let mut source_state=ListState::default().with_selected(Some(ui.cursor));
   let mut job_state=ListState::default().with_selected(Some(ui.job_cursor));
   f.render_stateful_widget(List::new(ui.rows().into_iter().map(ListItem::new)).block(Block::default().borders(Borders::ALL).title(format!("Sources · {} · {} selected (empty = all)",ui.format.extension(),ui.selected.len()))),areas[1], &mut source_state);
   f.render_stateful_widget(List::new(jobs.iter().enumerate().map(|(i,j)|job_row(j, ui.jobs_focus && i == ui.job_cursor))).block(Block::default().borders(Borders::ALL).title("Jobs · Tab focus · p pause · r resume/retry · c cancel")),areas[2], &mut job_state);
   f.render_widget(Paragraph::new(format!("{}\nf format · arrows navigate · Space select · d download · q quit\nOutput: {}",ui.status,settings.output_dir.display())),areas[3]);
  })?;
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Paste(text) if ui.editing => ui.url.push_str(&text),
                Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }
                    if ui.editing && key.code == KeyCode::Enter {
                        ui.editing = false;
                        ui.inspection = None;
                        ui.selected.clear();
                        ui.cursor = 0;
                        ui.status = "Resolving…".into();
                        ui.generation += 1;
                        let generation = ui.generation;
                        ui.busy = true;
                        let url = ui.url.trim().to_owned();
                        let engine = engine.clone();
                        let sender = sender.clone();
                        tokio::spawn(async move {
                            let result = match parse_url(&url) {
                                Ok(YoutubeUrl::Video(_)) => {
                                    engine.inspect(&url).await.map(Inspection::Video)
                                }
                                Ok(YoutubeUrl::Playlist(_)) => engine
                                    .playlist_page(&url, None)
                                    .await
                                    .map(Inspection::Playlist),
                                Err(e) => Err(e),
                            };
                            let _ = sender.send((generation, result));
                        });
                        continue;
                    }
                    if ui.key(key.code) {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('e') => {
                            ui.editing = true;
                            ui.generation += 1;
                            ui.busy = false;
                            ui.inspection = None;
                            ui.selected.clear();
                        }
                        KeyCode::Tab => ui.jobs_focus = !ui.jobs_focus,
                        KeyCode::Down => {
                            if ui.jobs_focus {
                                ui.job_cursor =
                                    (ui.job_cursor + 1).min(jobs.len().saturating_sub(1));
                            } else {
                                ui.cursor = (ui.cursor + 1).min(ui.count().saturating_sub(1));
                            }
                        }
                        KeyCode::Up => {
                            if ui.jobs_focus {
                                ui.job_cursor = ui.job_cursor.saturating_sub(1);
                            } else {
                                ui.cursor = ui.cursor.saturating_sub(1);
                            }
                        }
                        KeyCode::Char(' ') => {
                            let index = ui.cursor + 1;
                            if !ui.selected.remove(&index) {
                                ui.selected.insert(index);
                            }
                        }
                        KeyCode::Char('f') => {
                            ui.format = match ui.format {
                                OutputFormat::Mp3 => OutputFormat::M4a,
                                OutputFormat::M4a => OutputFormat::Flac,
                                OutputFormat::Flac => OutputFormat::Wav,
                                OutputFormat::Wav => OutputFormat::Mp4,
                                OutputFormat::Mp4 => OutputFormat::Mkv,
                                OutputFormat::Mkv => OutputFormat::Webm,
                                OutputFormat::Webm => OutputFormat::Mp3,
                            }
                        }
                        KeyCode::Char('n') => {
                            if !ui.busy
                                && let Some(Inspection::Playlist(page)) = &ui.inspection
                                && let Some(continuation) = page.continuation.clone()
                            {
                                let engine = engine.clone();
                                let sender = sender.clone();
                                let url = ui.url.clone();
                                let generation = ui.generation;
                                ui.busy = true;
                                tokio::spawn(async move {
                                    let _ = sender.send((
                                        generation,
                                        engine
                                            .playlist_page(&url, Some(&continuation))
                                            .await
                                            .map(Inspection::Playlist),
                                    ));
                                });
                            }
                        }
                        KeyCode::Char('d') => {
                            let selected: Vec<_> = ui.selected.iter().copied().collect();
                            let playlist = matches!(ui.inspection, Some(Inspection::Playlist(_)));
                            let r = engine.create(DownloadRequest {
                                urls: vec![ui.url.trim().into()],
                                output_dir: settings.output_dir.clone(),
                                format: ui.format,
                                max_height: None,
                                chapters: if playlist { vec![] } else { selected.clone() },
                                playlist_items: if playlist { selected } else { vec![] },
                            });
                            ui.status = match r {
                                Ok(id) => format!("Created {id}"),
                                Err(e) => e.to_string(),
                            };
                        }
                        KeyCode::Char(c @ ('p' | 'r' | 'c')) => {
                            if let Some(job) = jobs.get(ui.job_cursor) {
                                let result = match c {
                                    'p' => engine.pause(&job.id),
                                    'r' => engine.resume(&job.id, true),
                                    _ => engine.cancel(&job.id),
                                };
                                ui.status = match result {
                                    Ok(()) => "Control requested".into(),
                                    Err(e) => e.to_string(),
                                };
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_input_and_navigation() {
        let mut ui = Ui::default();
        ui.key(KeyCode::Char('ї'));
        ui.key(KeyCode::Char('a'));
        ui.key(KeyCode::Backspace);
        assert_eq!(ui.url, "ї");
        ui.key(KeyCode::Esc);
        assert!(!ui.editing);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn item_failure_and_warning_are_visible_without_aggregate_error() {
        let mut job = JobSnapshot {
            id: "00000000-0000-4000-8000-000000000001".into(),
            revision: 1,
            state: JobState::Failed,
            request: DownloadRequest {
                urls: vec![],
                output_dir: Default::default(),
                format: OutputFormat::Mp3,
                max_height: None,
                chapters: vec![],
                playlist_items: vec![],
            },
            enumeration_complete: true,
            items: vec![ItemOutcome {
                index: 0,
                url: String::new(),
                title: String::new(),
                state: JobState::Failed,
                artifacts: vec![],
                error: Some(Error::new(ErrorCode::Media, "decoder could not open input")),
                warnings: vec!["Cover unavailable".into()],
            }],
            progress: Progress::default(),
            warnings: vec![],
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 1)).unwrap();
        for expected in ["decoder could not open input", "Cover unavailable"] {
            terminal
                .draw(|frame| frame.render_widget(List::new([job_row(&job, false)]), frame.area()))
                .unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(
                rendered.contains(expected),
                "missing diagnostic in {rendered:?}"
            );
            job.items[0].error = None;
            job.items[0].state = JobState::Completed;
            job.state = JobState::Completed;
        }
    }
}
