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
///
/// A `temp_path` left behind by an earlier attempt is resumed rather than
/// discarded -- the 256 MB model file used to restart from byte zero every
/// time a slow connection dropped, or the reader pressed Cancel. Resuming is
/// only ever an optimisation: the digest is still computed over the whole
/// file, and a resumed attempt that fails to verify throws the partial away
/// and refetches from scratch, so bytes of unknown provenance can never be
/// installed.
pub async fn download_verified<F>(
    client: &reqwest::Client,
    spec: Download<'_>,
    mut on_progress: F,
) -> AppResult<()>
where
    F: FnMut(u64, u64) -> AppResult<()>,
{
    let resume_from = resumable_bytes(spec.temp_path, spec.expected_size)?;

    if resume_from > 0 {
        if let Attempt::Verified =
            download_once(client, &spec, &mut on_progress, resume_from).await?
        {
            return Ok(());
        }
        // Removing it, rather than leaning on the truncation below, matters:
        // if the fresh attempt fails before it writes anything, a partial this
        // one has already rejected must not be waiting to be resumed again.
        tokio::fs::remove_file(spec.temp_path).await?;
    }

    download_once(client, &spec, &mut on_progress, 0)
        .await
        .map(|_| ())
}

/// How much of `temp_path` is worth trying to resume from.
///
/// Zero means "start over": no partial, an empty one, or one that is already
/// at or past the finished size and so cannot be a prefix of it.
fn resumable_bytes(temp_path: &Path, expected_size: u64) -> AppResult<u64> {
    let existing = match std::fs::metadata(temp_path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(AppError::Io(error)),
    };

    if expected_size > 0 && existing >= expected_size {
        return Ok(0);
    }

    Ok(existing)
}

/// The result of one request. Verification failures are only reported as an
/// `Attempt` -- rather than an error -- when the bytes already on disk are a
/// plausible cause, which is the single case the caller retries.
enum Attempt {
    Verified,
    PartialRejected,
}

async fn download_once<F>(
    client: &reqwest::Client,
    spec: &Download<'_>,
    on_progress: &mut F,
    resume_from: u64,
) -> AppResult<Attempt>
where
    F: FnMut(u64, u64) -> AppResult<()>,
{
    let &Download {
        url,
        temp_path,
        expected_sha256,
        expected_size,
        read_timeout,
        error,
    } = spec;

    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let response = request.send().await?;

    // A server that thinks the partial runs past the end of the file answers
    // 416. `error_for_status` would turn a bad partial into a permanent
    // failure; it is the partial that has to go instead.
    if resume_from > 0 && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(Attempt::PartialRejected);
    }
    let response = response.error_for_status()?;

    // 206 is the only answer that means "the tail you asked for". A 200 is the
    // whole file from a server that ignored the header, so the partial has to
    // be truncated away rather than appended to.
    let resumed = resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;

    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut file = if resumed {
        // Seeded from what is actually on disk, not from the byte count the
        // last attempt believed it had written: an interrupted write may have
        // landed short, and the digest has to cover the real bytes.
        downloaded = hash_file(temp_path, &mut hasher)?;
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(temp_path)
            .await?
    } else {
        tokio::fs::File::create(temp_path).await?
    };

    let total = if expected_size > 0 {
        expected_size
    } else {
        downloaded + response.content_length().unwrap_or(0)
    };
    let mut stream = response.bytes_stream();

    on_progress(downloaded, total)?;

    loop {
        // Abort a stalled download if no chunk arrives within the read timeout.
        // There is deliberately no overall deadline: a large file on a slow
        // connection is not a failure.
        let next = tokio::time::timeout(read_timeout, stream.next())
            .await
            .map_err(|_| error("download stalled: no data received".to_string()));
        let next = match next {
            Ok(next) => next,
            // Flushed before the error escapes so the bytes that did arrive
            // survive to be resumed on the next attempt.
            Err(stalled) => return flush_then(file, Err(stalled)).await,
        };
        let Some(chunk) = next else {
            break;
        };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(dropped) => return flush_then(file, Err(dropped.into())).await,
        };
        downloaded += chunk.len() as u64;
        if expected_size > 0 && downloaded > expected_size {
            return reject(
                resumed,
                error(format!(
                    "download size mismatch: expected {expected_size} bytes, got at least {downloaded}"
                )),
            );
        }
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        if let Err(cancelled) = on_progress(downloaded, total) {
            return flush_then(file, Err(cancelled)).await;
        }
    }

    file.flush().await?;

    if expected_size > 0 && downloaded != expected_size {
        return reject(
            resumed,
            error(format!(
                "download size mismatch: expected {expected_size} bytes, got {downloaded}"
            )),
        );
    }

    let actual_sha256 = hex::encode(hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return reject(
            resumed,
            error(format!(
                "download SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
            )),
        );
    }

    Ok(Attempt::Verified)
}

/// A verification failure is the caller's problem to retry only when this
/// attempt built on bytes it did not fetch itself.
fn reject(resumed: bool, error: AppError) -> AppResult<Attempt> {
    if resumed {
        Ok(Attempt::PartialRejected)
    } else {
        Err(error)
    }
}

/// Push whatever is buffered to disk before an abandoned download's error
/// escapes, so the next attempt can resume from it.
async fn flush_then(mut file: tokio::fs::File, outcome: AppResult<Attempt>) -> AppResult<Attempt> {
    let _ = file.flush().await;
    outcome
}

/// Feed a whole file into `hasher`, returning how many bytes it held.
fn hash_file(path: &Path, hasher: &mut Sha256) -> AppResult<u64> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut buffer = [0_u8; 64 * 1024];
    let mut hashed = 0_u64;
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        hashed += read as u64;
    }
    Ok(hashed)
}

/// Whether a file already on disk matches an expected digest.
pub fn file_matches_sha256(path: &Path, expected_sha256: &str) -> AppResult<bool> {
    let mut hasher = Sha256::new();
    match hash_file(path, &mut hasher) {
        Ok(_) => {}
        Err(AppError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false)
        }
        Err(error) => return Err(error),
    }

    Ok(hex::encode(hasher.finalize()) == expected_sha256)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use sha2::{Digest, Sha256};
    use wiremock::matchers::{method, path as request_path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::{download_verified, Download};
    use crate::error::{AppError, AppResult};

    /// Long enough that a prefix is unambiguously a prefix.
    const BODY: &[u8] = b"a supertonic model, in miniature: sixty-four bytes of payload!!!";

    /// How much of `BODY` a test pretends an earlier attempt already fetched.
    const PREFIX: usize = 20;

    fn sha256_of(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    /// The first byte offset a request asked for, or `None` for a whole-file GET.
    fn range_start(request: &Request) -> Option<u64> {
        request
            .headers
            .get("range")?
            .to_str()
            .ok()?
            .strip_prefix("bytes=")?
            .split('-')
            .next()?
            .parse()
            .ok()
    }

    /// A well-behaved origin: `206` with the requested tail, `200` otherwise.
    async fn server_honouring_range() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(request_path("/model"))
            .respond_with(|request: &Request| match range_start(request) {
                Some(start) => ResponseTemplate::new(206)
                    .insert_header(
                        "content-range",
                        format!("bytes {start}-{}/{}", BODY.len() - 1, BODY.len()),
                    )
                    .set_body_bytes(&BODY[start as usize..]),
                None => ResponseTemplate::new(200).set_body_bytes(BODY),
            })
            .mount(&server)
            .await;
        server
    }

    /// An origin that answers every request with the whole file, `Range` or not.
    async fn server_ignoring_range() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(request_path("/model"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
            .mount(&server)
            .await;
        server
    }

    async fn download_to(
        server: &MockServer,
        temp_path: &Path,
        expected_sha256: &str,
    ) -> AppResult<Vec<(u64, u64)>> {
        let url = format!("{}/model", server.uri());
        let mut progress = Vec::new();
        download_verified(
            &reqwest::Client::new(),
            Download {
                url: &url,
                temp_path,
                expected_sha256,
                expected_size: BODY.len() as u64,
                read_timeout: Duration::from_secs(5),
                error: AppError::Model,
            },
            |downloaded, total| {
                progress.push((downloaded, total));
                Ok(())
            },
        )
        .await?;
        Ok(progress)
    }

    #[tokio::test]
    async fn a_partial_file_resumes_instead_of_starting_over() {
        let server = server_honouring_range().await;
        let directory = tempfile::tempdir().expect("temporary download directory");
        let temp_path = directory.path().join("model.download");
        std::fs::write(&temp_path, &BODY[..PREFIX]).expect("a partial file on disk");

        let progress = download_to(&server, &temp_path, &sha256_of(BODY))
            .await
            .expect("a resumed download should verify");

        assert_eq!(std::fs::read(&temp_path).expect("the finished file"), BODY);
        assert_eq!(
            progress.first().copied(),
            Some((PREFIX as u64, BODY.len() as u64)),
            "progress should open at the bytes already on disk, not at zero"
        );
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1, "the file should be fetched once");
        assert_eq!(
            range_start(&requests[0]),
            Some(PREFIX as u64),
            "the request should ask only for the missing tail"
        );
    }

    #[tokio::test]
    async fn a_server_that_ignores_range_still_produces_a_correct_file() {
        let server = server_ignoring_range().await;
        let directory = tempfile::tempdir().expect("temporary download directory");
        let temp_path = directory.path().join("model.download");
        std::fs::write(&temp_path, &BODY[..PREFIX]).expect("a partial file on disk");

        download_to(&server, &temp_path, &sha256_of(BODY))
            .await
            .expect("a whole-file answer should still verify");

        assert_eq!(
            std::fs::read(&temp_path).expect("the finished file"),
            BODY,
            "the whole-file body must replace the partial, not be appended to it"
        );
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("recorded requests")
                .len(),
            1,
            "a 200 answer is already the whole file; it must not cost a second fetch"
        );
    }

    #[tokio::test]
    async fn a_corrupt_partial_file_is_discarded_and_refetched() {
        let server = server_honouring_range().await;
        let directory = tempfile::tempdir().expect("temporary download directory");
        let temp_path = directory.path().join("model.download");
        std::fs::write(&temp_path, vec![b'x'; PREFIX]).expect("a corrupt partial file");

        download_to(&server, &temp_path, &sha256_of(BODY))
            .await
            .expect("a bad partial should be thrown away, not installed");

        assert_eq!(std::fs::read(&temp_path).expect("the finished file"), BODY);
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(
            requests.len(),
            2,
            "the resume should be retried from scratch"
        );
        assert_eq!(
            range_start(&requests[1]),
            None,
            "the retry should ask for the whole file"
        );
    }

    #[tokio::test]
    async fn a_partial_file_already_at_the_expected_size_is_not_resumed() {
        let server = server_honouring_range().await;
        let directory = tempfile::tempdir().expect("temporary download directory");
        let temp_path = directory.path().join("model.download");
        std::fs::write(&temp_path, vec![b'x'; BODY.len()]).expect("an oversized partial file");

        download_to(&server, &temp_path, &sha256_of(BODY))
            .await
            .expect("a partial with nothing left to fetch should restart");

        assert_eq!(std::fs::read(&temp_path).expect("the finished file"), BODY);
        let requests = server.received_requests().await.expect("recorded requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            range_start(&requests[0]),
            None,
            "a partial that is not a prefix of anything should not be resumed"
        );
    }

    #[tokio::test]
    async fn a_fresh_download_that_fails_its_digest_is_not_retried() {
        let server = server_honouring_range().await;
        let directory = tempfile::tempdir().expect("temporary download directory");
        let temp_path = directory.path().join("model.download");

        let error = download_to(&server, &temp_path, &sha256_of(b"a different file"))
            .await
            .expect_err("a wrong digest should fail");

        assert!(matches!(error, AppError::Model(_)), "{error}");
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("recorded requests")
                .len(),
            1,
            "only a resumed attempt earns a second try"
        );
    }
}
