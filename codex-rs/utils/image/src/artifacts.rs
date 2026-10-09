//! Content-addressed local image storage. No image bytes are serialized in references.

use std::collections::HashMap;
use std::io;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

pub const MAX_DESCRIPTION_CHARS: usize = 2048;
pub const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DESCRIPTION_CACHE_BYTES: usize = 16 * 1024;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
type DescriptionLocks = HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>;
static DESCRIPTION_LOCKS: LazyLock<Mutex<DescriptionLocks>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub sha256: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub size_bytes: usize,
}

impl ImageArtifact {
    /// Prevent simultaneous callers from paying for the same uncached description twice.
    pub async fn lock_description(&self, model: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = DESCRIPTION_LOCKS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if locks.len() >= 128 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }
            let key = self.description_path(model);
            if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(key, Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }

    pub async fn store(root: &Path, bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(io::Error::other(
                "image exceeds the 64 MiB local artifact limit",
            ));
        }
        let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let format = reader
            .format()
            .ok_or_else(|| io::Error::other("unsupported image format"))?;
        let (width, height) = reader.into_dimensions().map_err(io::Error::other)?;
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        tokio::fs::create_dir_all(root).await?;
        let path = root.join(format!("{sha256}.{}", format.extensions_str()[0]));
        match tokio::fs::read(&path).await {
            Ok(existing) => {
                if existing != bytes {
                    return Err(io::Error::other(
                        "local image artifact failed integrity verification",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_atomic(&path, bytes).await?
            }
            Err(error) => return Err(error),
        }
        Ok(Self {
            sha256,
            path,
            width,
            height,
            size_bytes: bytes.len(),
        })
    }

    fn description_path(&self, model: &str) -> PathBuf {
        let key = format!(
            "{:x}",
            Sha256::digest(format!("caption-v1:{model}").as_bytes())
        );
        self.path.with_extension(format!("{key}.description.json"))
    }

    pub async fn cached_description(&self, model: &str) -> io::Result<Option<String>> {
        let path = self.description_path(model);
        let file = match tokio::fs::File::open(path).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take((MAX_DESCRIPTION_CACHE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > MAX_DESCRIPTION_CACHE_BYTES {
            return Err(io::Error::other(
                "image description cache exceeds its size limit",
            ));
        }
        let description: String = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        Ok(Some(
            description.chars().take(MAX_DESCRIPTION_CHARS).collect(),
        ))
    }

    pub async fn cache_description(&self, model: &str, description: &str) -> io::Result<String> {
        let description: String = description.chars().take(MAX_DESCRIPTION_CHARS).collect();
        let bytes = serde_json::to_vec(&description).map_err(io::Error::other)?;
        write_atomic(&self.description_path(model), &bytes).await?;
        Ok(description)
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = async {
        let mut file = options.open(&temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;
