//! Downloading a file whose size and SHA-256 are known in advance.
//!
//! Both model downloaders grew their own copy of this. They had drifted: one
//! had mirror fallback and hashed as it streamed, the other had neither and
//! re-read the finished file to hash it. This is the version with both.

use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

/// How the caller wants failures reported. The two downloaders raise different
/// `AppError` variants for the same conditions, and their callers branch on it.
pub type ErrorKind = fn(String) -> AppError;

/// One file to fetch, and what it should turn out to be.
pub struct Download<'a> {
    pub url: &'a str,
    pub temp_path: &'a Path,
    pub expected_sha256: &'a str,
    /// Zero when the manifest does not state one.
    pub expected_size: u64,
    pub read_timeout: Duration,
    pub error: ErrorKind,
}

/// Stream a file to disk, verifying size and digest as the bytes arrive.
///
/// Hashing happens inline rather than in a second pass over the finished file:
/// the bytes are already in hand, and a 300 MB model is not worth reading
/// twice. The size check fires mid-stream so an oversized body is abandoned
/// rather than written out in full and rejected afterwards.
pub async fn download_verified<F>(
    client: &reqwest::Client,
    spec: Download<'_>,
    mut on_progress: F,
) -> AppResult<()>
where
    F: FnMut(u64, u64) -> AppResult<()>,
{
    let Download {
        url,
        temp_path,
        expected_sha256,
        expected_size,
        read_timeout,
        error,
    } = spec;

    let response = client.get(url).send().await?.error_for_status()?;
    let total = response.content_length().unwrap_or(expected_size);
    let mut downloaded = 0_u64;
    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::create(temp_path).await?;
    let mut stream = response.bytes_stream();

    on_progress(0, total)?;

    loop {
        // Abort a stalled download if no chunk arrives within the read timeout.
        // There is deliberately no overall deadline: a large file on a slow
        // connection is not a failure.
        let next = tokio::time::timeout(read_timeout, stream.next())
            .await
            .map_err(|_| error("download stalled: no data received".to_string()))?;
        let Some(chunk) = next else {
            break;
        };
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        if expected_size > 0 && downloaded > expected_size {
            return Err(error(format!(
                "download size mismatch: expected {expected_size} bytes, got at least {downloaded}"
            )));
        }
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        on_progress(downloaded, total)?;
    }

    file.flush().await?;

    if expected_size > 0 && downloaded != expected_size {
        return Err(error(format!(
            "download size mismatch: expected {expected_size} bytes, got {downloaded}"
        )));
    }

    let actual_sha256 = hex::encode(hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(error(format!(
            "download SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        )));
    }

    Ok(())
}

/// Whether a file already on disk matches an expected digest.
pub fn file_matches_sha256(path: &Path, expected_sha256: &str) -> AppResult<bool> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::Io(error)),
    };

    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()) == expected_sha256)
}
