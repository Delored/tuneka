use crate::*;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
/// Persistent lock inode: never unlink while another process may hold it.
pub struct FileLock {
    _file: File,
}
impl FileLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock_exclusive()
            .map_err(|_| Error::new(ErrorCode::Conflict, "resource is owned by another process"))?;
        Ok(Self { _file: file })
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing alone can retain flock until a concurrently spawned child
        // finishes exec and closes its inherited open-file description.
        // The guard owns the lock lifetime, so release it explicitly first.
        let _ = FileExt::unlock(&self._file);
    }
}

#[cfg(all(test, unix))]
mod lock_lifetime_tests {
    use super::*;

    #[test]
    fn guard_release_does_not_wait_for_an_inherited_file_description() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resource.lock");
        let guard = FileLock::acquire(&path).unwrap();
        // A child between fork and exec can share this open-file description.
        // A duplicate deterministically reproduces that lifetime without timing.
        let inherited = guard._file.try_clone().unwrap();
        assert!(FileLock::acquire(&path).is_err());
        drop(guard);
        let next = FileLock::acquire(&path).unwrap();
        drop(inherited);
        assert!(FileLock::acquire(&path).is_err());
        drop(next);
        FileLock::acquire(&path).unwrap();
    }
}

pub fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(&serde_json::to_vec_pretty(value)?)?;
        f.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn safe_name(value: &str, folder: bool) -> String {
    let s: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                if folder { ' ' } else { '\0' }
            } else {
                c
            }
        })
        .filter(|c| *c != '\0')
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = s
        .trim_end_matches(['.', ' '])
        .chars()
        .take(if folder { 120 } else { 160 })
        .collect();
    s = s.trim_end_matches(['.', ' ']).into();
    if s.is_empty() {
        s = if folder { "Відео" } else { "track" }.into();
    }
    let stem = s.split('.').next().unwrap_or("").to_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        s.insert(0, '_');
    }
    s
}
pub struct Reservation {
    pub path: PathBuf,
    _lock: FileLock,
}
impl Reservation {
    pub fn acquire(path: PathBuf) -> Result<Self> {
        let lock = FileLock::acquire(&path.with_extension(format!(
            "{}.lock",
            path.extension().unwrap_or_default().to_string_lossy()
        )))?;
        if path.exists() {
            return Err(Error::new(ErrorCode::Conflict, "output already exists"));
        }
        Ok(Self { path, _lock: lock })
    }
}

#[derive(serde::Deserialize)]
struct PublicationReceipt {
    path: PathBuf,
    size: u64,
    sha256: String,
}
pub fn recover_publication(receipt: &Path, output: &Path) -> Result<Option<Artifact>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    if !output.exists() || !receipt.exists() {
        return Ok(None);
    }
    let r: PublicationReceipt = serde_json::from_slice(&fs::read(receipt)?)?;
    if r.path != output || fs::metadata(output)?.len() != r.size {
        return Err(Error::new(
            ErrorCode::Conflict,
            "output differs from publication receipt",
        ));
    }
    let mut file = File::open(output)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != r.sha256 {
        return Err(Error::new(
            ErrorCode::Conflict,
            "output digest differs from publication receipt",
        ));
    }
    Ok(Some(Artifact {
        path: r.path,
        size: r.size,
    }))
}
