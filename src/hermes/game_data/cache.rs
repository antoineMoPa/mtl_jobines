//! Only the automatic disk cache is managed here, never packaged/exported data.
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Best effort: cache maintenance must never prevent a map from loading.
pub fn maintain(current: &Path) {
    let now = SystemTime::now();
    let result = (|| {
        // Explicitly record hits: filesystem access times are not always updated.
        fs::File::open(current)?.set_modified(now)?;
        prune(current, now, MAX_AGE, MAX_BYTES)
    })();
    match result {
        Ok((count, bytes)) if count > 0 => {
            eprintln!("Pruned {count} game caches ({} MiB)", bytes / (1024 * 1024));
        }
        Ok(_) => {}
        Err(error) => eprintln!("Could not prune game caches: {error}"),
    }
}

struct Entry {
    path: PathBuf,
    bytes: u64,
    used: SystemTime,
    modified: SystemTime,
}

fn is_cache_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".game-data.gz"))
        .is_some_and(|key| key.len() == 16 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn last_use(metadata: &fs::Metadata) -> io::Result<SystemTime> {
    let modified = metadata.modified()?;
    // Preserve legacy entries read more recently than they were generated.
    Ok(metadata.accessed().unwrap_or(modified).max(modified))
}

fn prune(
    current: &Path,
    now: SystemTime,
    max_age: Duration,
    max_bytes: u64,
) -> io::Result<(usize, u64)> {
    if !is_cache_name(current) {
        return Ok((0, 0));
    }
    let Some(parent) = current.parent().filter(|path| !path.as_os_str().is_empty()) else {
        return Ok((0, 0));
    };
    let mut entries = Vec::new();
    let mut total = 0u64;
    for item in fs::read_dir(parent)? {
        let item = item?;
        let path = item.path();
        // Do not follow symlinks, descend into directories, or touch temp files.
        if !is_cache_name(&path) || !item.file_type()?.is_file() {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        total = total.saturating_add(metadata.len());
        entries.push(Entry {
            path,
            bytes: metadata.len(),
            used: last_use(&metadata)?,
            modified: metadata.modified()?,
        });
    }
    entries.sort_by(|a, b| a.used.cmp(&b.used).then_with(|| a.path.cmp(&b.path)));
    let mut removed = (0, 0);
    for entry in entries {
        if entry.path == current {
            continue;
        }
        let expired = now.duration_since(entry.used).unwrap_or_default() > max_age;
        if !expired && total <= max_bytes {
            continue;
        }
        let metadata = match fs::symlink_metadata(&entry.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                total = total.saturating_sub(entry.bytes);
                continue;
            }
            Err(error) => return Err(error),
        };
        // Another game instance may have refreshed/replaced this entry meanwhile.
        if !metadata.is_file()
            || metadata.len() != entry.bytes
            || metadata.modified()? != entry.modified
            || last_use(&metadata)? != entry.used
        {
            continue;
        }
        match fs::remove_file(&entry.path) {
            Ok(()) => {
                removed.0 += 1;
                removed.1 += entry.bytes;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        total = total.saturating_sub(entry.bytes);
    }
    // The current entry is retained even if it alone exceeds the budget.
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "mtl-cache-prune-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn entry(&self, name: &str, age: Duration, size: u64) -> PathBuf {
            let path = self.0.join(name);
            let file = fs::File::create(&path).unwrap();
            file.set_len(size).unwrap();
            let time = SystemTime::now() - age;
            file.set_times(fs::FileTimes::new().set_modified(time).set_accessed(time))
                .unwrap();
            path
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn expires_unused_entries_but_preserves_recent_reads_and_unrelated_files() {
        let dir = Directory::new();
        let old = MAX_AGE + Duration::from_secs(60);
        let current = dir.entry("0000000000000001.game-data.gz", old, 10);
        let stale = dir.entry("0000000000000002.game-data.gz", old, 20);
        let read = dir.entry("0000000000000003.game-data.gz", old, 30);
        fs::File::open(&read)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(SystemTime::now()))
            .unwrap();
        let export = dir.entry("export.game-data.gz", old, 40);
        let temporary = dir.entry(".0000000000000004.game-data.gz.123.tmp", old, 50);
        let nested = dir.0.join("0000000000000005.game-data.gz");
        fs::create_dir(&nested).unwrap();
        assert_eq!(
            prune(&current, SystemTime::now(), MAX_AGE, MAX_BYTES).unwrap(),
            (1, 20)
        );
        assert!(!stale.exists());
        for path in [current, read, export, temporary, nested] {
            assert!(path.exists(), "{}", path.display());
        }
    }

    #[test]
    fn trims_oldest_first_and_keeps_current_even_over_budget() {
        let dir = Directory::new();
        let current = dir.entry("0000000000000001.game-data.gz", Duration::from_secs(40), 10);
        let oldest = dir.entry("0000000000000002.game-data.gz", Duration::from_secs(30), 20);
        let newest = dir.entry("0000000000000003.game-data.gz", Duration::from_secs(10), 30);
        assert_eq!(
            prune(&current, SystemTime::now(), MAX_AGE, 40).unwrap(),
            (1, 20)
        );
        assert!(!oldest.exists());
        assert!(newest.exists());
        assert_eq!(
            prune(&current, SystemTime::now(), MAX_AGE, 1).unwrap(),
            (1, 30)
        );
        assert!(current.exists());
    }

    #[test]
    fn successful_use_refreshes_modified_time() {
        let dir = Directory::new();
        let current = dir.entry("0000000000000001.game-data.gz", MAX_AGE * 2, 10);
        let before = SystemTime::now();
        maintain(&current);
        assert!(fs::metadata(current).unwrap().modified().unwrap() >= before);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_or_remove_symlinks() {
        let dir = Directory::new();
        let current = dir.entry("0000000000000001.game-data.gz", Duration::ZERO, 10);
        let export = dir.entry("export.game-data.gz", MAX_AGE * 2, 20);
        let link = dir.0.join("0000000000000002.game-data.gz");
        std::os::unix::fs::symlink(&export, &link).unwrap();
        assert_eq!(
            prune(&current, SystemTime::now(), MAX_AGE, 1).unwrap(),
            (0, 0)
        );
        assert!(link.is_symlink());
        assert!(export.exists());
    }
}
