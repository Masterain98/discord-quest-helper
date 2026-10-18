//! Optional artwork must never prevent the text notification from being submitted.
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_BYTES: usize = 4 * 1024 * 1024;

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
    let directory = directory.join("notification-artwork");
    tokio::fs::create_dir_all(&directory).await.ok()?;
    let path = directory.join(format!("{:x}.png", Sha256::digest(source.as_bytes())));
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
    #[tokio::test]
    async fn caches_local_art_and_rejects_unsupported_sources_without_fetching() {
        let directory = std::env::temp_dir().join(format!("dqh-art-{}", uuid::Uuid::new_v4()));
        let first = prepare(&directory, "/icons/orbs.png").await.unwrap();
        assert_eq!(prepare(&directory, "/icons/orbs.png").await, Some(first));
        assert!(prepare(&directory, "http://127.0.0.1/private.png")
            .await
            .is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
