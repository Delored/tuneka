use crate::{
    files::{FileLock, atomic_json},
    *,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema: u32,
    pub language: String,
    pub theme: String,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub python_imported: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            language: "uk".into(),
            theme: "olive".into(),
            output_dir: std::env::var_os("TUNEKA_DOWNLOADS")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("downloads")),
            format: OutputFormat::Mp3,
            python_imported: false,
        }
    }
}
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("TUNEKA_RUST_DATA_DIR") {
        return p.into();
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/TunekaRust")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("TunekaRust")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("tuneka-rust")
    }
}
pub fn update(path: &Path, change: impl FnOnce(&mut Settings)) -> Result<Settings> {
    let _lock = FileLock::acquire(&path.with_extension("lock"))?;
    let mut s = if path.exists() {
        serde_json::from_slice::<Settings>(&fs::read(path)?)?
    } else {
        Settings::default()
    };
    if s.schema != 1 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "unknown settings schema",
        ));
    }
    change(&mut s);
    if !["uk", "en"].contains(&s.language.as_str())
        || !["light", "olive", "purple"].contains(&s.theme.as_str())
    {
        return Err(Error::new(
            ErrorCode::InvalidInput,
            "unsupported language or theme",
        ));
    }
    atomic_json(path, &s)?;
    Ok(s)
}
pub fn import_python(path: &Path, source: &Path) -> Result<Settings> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(source)?)?;
    if !value.is_object() {
        return Err(Error::new(
            ErrorCode::InvalidInput,
            "Python settings must be an object",
        ));
    }
    update(path, |s| {
        if s.python_imported {
            return;
        }
        if let Some(v) = value["language"]
            .as_str()
            .filter(|v| ["en", "uk"].contains(v))
        {
            s.language = v.into();
        }
        if let Some(v) = value["theme"]
            .as_str()
            .filter(|v| ["light", "olive", "purple"].contains(v))
        {
            s.theme = v.into();
        }
        if let Some(v) = value["download_parent"].as_str() {
            s.output_dir = v.into();
        }
        if let Some(v) = value["default_playlist_music_format"]
            .as_str()
            .and_then(|v| v.parse().ok())
        {
            s.format = v;
        }
        s.python_imported = true;
    })
}
