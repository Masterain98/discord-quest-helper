//! Optional artwork must never prevent the text notification from being submitted.
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHE_FILES: usize = 128;
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
// Native notification daemons can read an image after submission. Protect
// recently used files even if a burst temporarily exceeds the cache budget.
const CACHE_GRACE: Duration = Duration::from_secs(5 * 60);
static CACHE_ACCESS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn prune(directory: &Path, current: &Path, now: SystemTime) -> std::io::Result<()> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        let age = now.duration_since(modified).unwrap_or_default();
        if name
            .strip_suffix(".tmp")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        {
            if age > CACHE_GRACE {
                let _ = std::fs::remove_file(path);
            }
        } else if name
            .strip_suffix(".png")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            files.push((path, metadata.len(), modified, age));
        }
    }
    files.sort_by_key(|(_, _, modified, _)| *modified);
    let mut count = files.len();
    let mut bytes: u64 = files.iter().map(|(_, size, _, _)| size).sum();
    for (path, size, _, age) in files {
        if path == current || age < CACHE_GRACE {
            continue;
        }
        if (age > MAX_CACHE_AGE || count > MAX_CACHE_FILES || bytes > MAX_CACHE_BYTES)
            && std::fs::remove_file(path).is_ok()
        {
            count -= 1;
            bytes -= size;
        }
    }
    Ok(())
}

async fn maintain(directory: &Path, current: &Path) {
    let (directory, current) = (directory.to_owned(), current.to_owned());
    let _ = tokio::task::spawn_blocking(move || {
        // Refresh last-use time before eviction; cleanup failure only loses artwork.
        if let Ok(file) = std::fs::File::options().write(true).open(&current) {
            let _ = file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()));
        }
        let _ = prune(&directory, &current, SystemTime::now());
    })
    .await;
}

fn png(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(512, 512);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png).ok()?;
    Some(output.into_inner())
}

async fn prepare_inner(directory: &Path, source: &str) -> Option<PathBuf> {
    let source = super::message::reward_source(source)?;
    let _access = CACHE_ACCESS.lock().await;
    let directory = directory.join("notification-artwork");
    tokio::fs::create_dir_all(&directory).await.ok()?;
    let path = directory.join(format!("{:x}.png", Sha256::digest(source.as_bytes())));
    maintain(&directory, &path).await;
    if path.is_file() {
        return Some(path);
    }
    let bytes = if source == "/icons/orbs.png" {
        include_bytes!("../../../public/icons/orbs.png").to_vec()
    } else {
        let client = super::http::client().ok()?;
        let mut response = client
            .get(source)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES as u64)
        {
            return None;
        }
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if data.len() + chunk.len() > MAX_BYTES {
                return None;
            }
            data.extend_from_slice(&chunk);
        }
        data
    };
    let data = tauri::async_runtime::spawn_blocking(move || png(bytes))
        .await
        .ok()??;
    let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, &data).await.ok()?;
    if tokio::fs::rename(&temporary, &path).await.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
        if !path.is_file() {
            return None;
        }
    }
    maintain(&directory, &path).await;
    Some(path)
}
pub(super) async fn prepare(directory: &Path, source: &str) -> Option<PathBuf> {
    tokio::time::timeout(Duration::from_secs(3), prepare_inner(directory, source))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_bundled_art_and_rejects_non_images() {
        let data = png(include_bytes!("../../../public/icons/orbs.png").to_vec()).unwrap();
        assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png(b"<html>not an image</html>".to_vec()).is_none());
    }
    fn cache_file(directory: &Path, id: usize, size: u64, age: Duration) -> PathBuf {
        let path = directory.join(format!("{id:064x}.png"));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(size).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::now() - age))
            .unwrap();
        path
    }
    #[test]
    fn prunes_oldest_art_to_count_and_size_budgets_and_expires_stale_files() {
        let directory =
            std::env::temp_dir().join(format!("dqh-art-prune-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let old = cache_file(&directory, 0, 1, MAX_CACHE_AGE + Duration::from_secs(1));
        for id in 1..=MAX_CACHE_FILES + 1 {
            cache_file(&directory, id, 1, Duration::from_secs(3600 + id as u64));
        }
        let current = cache_file(&directory, MAX_CACHE_FILES + 2, 1, Duration::ZERO);
        let unrelated = directory.join("keep.txt");
        std::fs::write(&unrelated, "keep").unwrap();
        let temp = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::rename(&old, &temp).unwrap();
        let stale = cache_file(&directory, 999, 1, MAX_CACHE_AGE + Duration::from_secs(1));
        prune(&directory, &current, SystemTime::now()).unwrap();
        assert!(!stale.exists());
        assert!(!temp.exists());
        assert!(current.exists());
        assert!(unrelated.exists());
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            MAX_CACHE_FILES + 1
        );
        let large_old = cache_file(&directory, 2000, MAX_CACHE_BYTES, Duration::from_secs(7200));
        let large_new = cache_file(
            &directory,
            2001,
            MAX_CACHE_BYTES / 2,
            Duration::from_secs(3600),
        );
        prune(&directory, &current, SystemTime::now()).unwrap();
        assert!(!large_old.exists());
        assert!(large_new.exists());
        // Native consumers may still need a recently submitted image.
        let recent = cache_file(&directory, 2002, MAX_CACHE_BYTES, Duration::ZERO);
        prune(&directory, &current, SystemTime::now()).unwrap();
        assert!(recent.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn caches_local_art_and_rejects_unsupported_sources_without_fetching() {
        let directory = std::env::temp_dir().join(format!("dqh-art-{}", uuid::Uuid::new_v4()));
        let first = prepare(&directory, "/icons/orbs.png").await.unwrap();
        std::fs::File::options()
            .write(true)
            .open(&first)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(SystemTime::now() - MAX_CACHE_AGE))
            .unwrap();
        let reused_at = SystemTime::now();
        assert_eq!(
            prepare(&directory, "/icons/orbs.png").await,
            Some(first.clone())
        );
        assert!(std::fs::metadata(first).unwrap().modified().unwrap() >= reused_at);
        assert!(prepare(&directory, "http://127.0.0.1/private.png")
            .await
            .is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
