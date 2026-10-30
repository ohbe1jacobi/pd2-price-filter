//! Guarded, atomic writes of separate derivatives and cache records.
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub fn write_derivative(
    source: &Path,
    destination: &Path,
    bytes: &[u8],
    overwrite: bool,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let source = std::fs::canonicalize(source).map_err(|e| format!("source: {e}"))?;
    if destination.exists() {
        let target = std::fs::canonicalize(destination).map_err(|e| format!("destination: {e}"))?;
        if source == target || same_file(&source, &target)? {
            return Err("destination aliases the source filter".into());
        }
        if std::fs::symlink_metadata(destination)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("refusing a symlink destination".into());
        }
        if !overwrite {
            return Err("output exists; pass --overwrite to replace it".into());
        }
    }
    write_atomic(destination, bytes, overwrite, cancelled)
}

fn same_file(a: &Path, b: &Path) -> Result<bool, String> {
    same_file::is_same_file(a, b).map_err(|e| format!("file identity: {e}"))
}

fn write_atomic(
    destination: &Path,
    bytes: &[u8],
    replace: bool,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    use std::{io::Write, sync::atomic::Ordering};
    if cancelled.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("create temporary output: {e}"))?;
    temp.write_all(bytes).map_err(|e| format!("write: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("sync: {e}"))?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    if replace {
        temp.persist(destination)
            .map_err(|e| format!("atomic replace: {e}"))?;
    } else {
        temp.persist_noclobber(destination)
            .map_err(|e| format!("publish new output: {e}"))?;
    }
    Ok(())
}

pub fn atomic_write(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic(destination, bytes, true, &AtomicBool::new(false))
}

pub fn default_cache_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("LOCALAPPDATA is unset; use --cache")?;
    #[cfg(not(windows))]
    let base = match std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unset; use --cache")?)
            .join(".cache"),
    };
    Ok(base.join("pd2-price-filter"))
}

pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("expected a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn dir() -> PathBuf {
        let root = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let p = root.join(format!(
            "pd2-io-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn bounded_reads_reject_oversized_inputs() {
        let d = dir();
        let p = d.join("input");
        fs::write(&p, b"12345").unwrap();
        assert_eq!(read_bounded(&p, 5).unwrap(), b"12345");
        assert!(read_bounded(&p, 4).is_err());
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn separate_unicode_destination_preserves_source() {
        let d = dir();
        let s = d.join("original.filter");
        let o = d.join("SC_é filter.filter");
        fs::write(&s, b"source").unwrap();
        write_derivative(&s, &o, b"priced", false, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(&s).unwrap(), b"source");
        assert_eq!(fs::read(&o).unwrap(), b"priced");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn rejects_same_file_and_unapproved_overwrite() {
        let d = dir();
        let s = d.join("source");
        let o = d.join("output");
        fs::write(&s, b"source").unwrap();
        fs::write(&o, b"old").unwrap();
        assert!(write_derivative(&s, &s, b"bad", true, &AtomicBool::new(false)).is_err());
        assert!(write_derivative(&s, &o, b"bad", false, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&s).unwrap(), b"source");
        assert_eq!(fs::read(&o).unwrap(), b"old");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn cancellation_preserves_previous_output() {
        let d = dir();
        let s = d.join("source");
        let o = d.join("output");
        fs::write(&s, b"source").unwrap();
        fs::write(&o, b"old").unwrap();
        assert!(write_derivative(&s, &o, b"new", true, &AtomicBool::new(true)).is_err());
        assert_eq!(fs::read(&o).unwrap(), b"old");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn approved_overwrite_and_failed_destination_preserve_source() {
        let d = dir();
        let s = d.join("source");
        let o = d.join("output");
        fs::write(&s, b"source").unwrap();
        fs::write(&o, b"old").unwrap();
        write_derivative(&s, &o, b"new", true, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(&o).unwrap(), b"new");
        assert!(
            write_derivative(
                &s,
                &d.join("missing").join("out"),
                b"bad",
                false,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(fs::read(&s).unwrap(), b"source");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn atomic_cache_replace_is_complete() {
        let d = dir();
        let o = d.join("cache");
        atomic_write(&o, b"first").unwrap();
        atomic_write(&o, b"second").unwrap();
        assert_eq!(fs::read(&o).unwrap(), b"second");
        fs::remove_dir_all(d).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn rejects_hardlink_and_symlink_aliases() {
        use std::os::unix::fs::symlink;
        let d = dir();
        let s = d.join("source");
        let h = d.join("hard");
        let l = d.join("sym");
        fs::write(&s, b"source").unwrap();
        fs::hard_link(&s, &h).unwrap();
        symlink(&s, &l).unwrap();
        assert!(write_derivative(&s, &h, b"bad", true, &AtomicBool::new(false)).is_err());
        assert!(write_derivative(&s, &l, b"bad", true, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&s).unwrap(), b"source");
        fs::remove_dir_all(d).unwrap();
    }
}
