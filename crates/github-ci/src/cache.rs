//! Best-effort, credential-scoped cache of successful immutable job log bodies.
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};

const TTL: u64 = 24 * 60 * 60;
const MAX_BODY: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 64;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const HEADER: u64 = 40;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct CachedLog {
    pub bytes: Vec<u8>,
    pub fetched_at: u64,
}

#[derive(Debug)]
pub(crate) struct Cache {
    directory: PathBuf,
    ttl: u64,
}

impl Cache {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            ttl: TTL,
        }
    }

    pub(crate) fn with_ttl(mut self, seconds: u64) -> Self {
        self.ttl = seconds;
        self
    }

    pub(crate) fn get_file(&self, key: &str) -> Option<(File, u64)> {
        self.private_directory()?;
        let path = self.directory.join(format!("{key}.log"));
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file()
            || metadata.len() > MAX_BODY + HEADER
            || metadata.permissions().mode() & 0o077 != 0
        {
            return None;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(path)
            .ok()?;
        let mut timestamp = [0; 8];
        let mut digest = [0; 32];
        file.read_exact(&mut timestamp).ok()?;
        file.read_exact(&mut digest).ok()?;
        let fetched_at = u64::from_be_bytes(timestamp);
        if now()
            .checked_sub(fetched_at)
            .is_none_or(|age| age >= self.ttl)
        {
            return None;
        }
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 64 * 1024];
        let mut length = 0;
        loop {
            let count = file.read(&mut buffer).ok()?;
            if count == 0 {
                break;
            }
            length += count as u64;
            if length > MAX_BODY {
                return None;
            }
            hasher.update(buffer.get(..count)?);
        }
        if length == 0 || hasher.finalize().as_slice() != digest {
            return None;
        }
        file.rewind().ok()?;
        file.seek(std::io::SeekFrom::Start(HEADER)).ok()?;
        Some((file, fetched_at))
    }

    pub(crate) fn put_file(&self, key: &str, source: &mut File, fetched_at: u64) -> bool {
        if !self.directory.exists() {
            let _ = DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory);
        }
        if self.private_directory().is_none() {
            return false;
        }
        let temporary = self.directory.join(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> std::io::Result<()> {
            source.rewind()?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&fetched_at.to_be_bytes())?;
            file.write_all(&[0; 32])?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0; 64 * 1024];
            let mut length = 0;
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                length += count as u64;
                if length > MAX_BODY {
                    return Err(std::io::Error::other("log too large"));
                }
                let chunk = buffer.get(..count).unwrap_or_default();
                hasher.update(chunk);
                file.write_all(chunk)?;
            }
            if length == 0 {
                return Err(std::io::Error::other("empty log"));
            }
            file.seek(std::io::SeekFrom::Start(8))?;
            file.write_all(&hasher.finalize())?;
            fs::rename(&temporary, self.directory.join(format!("{key}.log")))?;
            Ok(())
        })();
        let _ = source.rewind();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        self.prune();
        result.is_ok()
    }

    pub(crate) fn get(&self, key: &str) -> Option<CachedLog> {
        self.read(key).ok()
    }

    pub(crate) fn read(&self, key: &str) -> crate::Result<CachedLog> {
        let invalid =
            |reason: &str| crate::Error::InvalidResponse(format!("Saved observation {reason}"));
        let io_error = |error: std::io::Error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                invalid("is absent for the active credential/cache location")
            } else if error.kind() == std::io::ErrorKind::UnexpectedEof {
                invalid("is truncated or corrupt")
            } else {
                invalid(&format!(
                    "storage is inaccessible: {error}; check cache permissions and sandbox access"
                ))
            }
        };
        let directory = fs::symlink_metadata(&self.directory).map_err(io_error)?;
        if !directory.is_dir() || directory.permissions().mode() & 0o077 != 0 {
            return Err(invalid(
                "directory is not a private directory; check cache permissions",
            ));
        }
        let path = self.directory.join(format!("{key}.log"));
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file()
            || metadata.len() > MAX_BODY + HEADER
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(invalid(
                "file is unsafe or oversized; check cache permissions",
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&path)
            .map_err(io_error)?;
        let mut timestamp = [0; 8];
        let mut digest = [0; 32];
        file.read_exact(&mut timestamp).map_err(io_error)?;
        file.read_exact(&mut digest).map_err(io_error)?;
        let fetched_at = u64::from_be_bytes(timestamp);
        if now()
            .checked_sub(fetched_at)
            .is_none_or(|age| age >= self.ttl)
        {
            return Err(invalid("has expired or has an invalid timestamp"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BODY + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.is_empty()
            || bytes.len() as u64 > MAX_BODY
            || Sha256::digest(&bytes).as_slice() != digest
        {
            return Err(invalid("failed its integrity check"));
        }
        Ok(CachedLog { bytes, fetched_at })
    }

    pub(crate) fn put(&self, key: &str, bytes: &[u8], fetched_at: u64) {
        if bytes.is_empty() || bytes.len() as u64 > MAX_BODY {
            return;
        }
        if !self.directory.exists()
            && DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)
                .is_err()
        {
            return;
        }
        if self.private_directory().is_none() {
            return;
        }
        let temporary = self.directory.join(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&fetched_at.to_be_bytes())?;
            file.write_all(&Sha256::digest(bytes))?;
            file.write_all(bytes)?;
            fs::rename(&temporary, self.directory.join(format!("{key}.log")))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        self.prune();
    }

    #[expect(
        clippy::verbose_bit_mask,
        reason = "octal masks express Unix permission bits directly"
    )]
    fn private_directory(&self) -> Option<()> {
        let metadata = fs::symlink_metadata(&self.directory).ok()?;
        (metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0).then_some(())
    }

    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let mut retained = Vec::new();
        let mut total = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "log") {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let fetched_at = read_timestamp(&path).unwrap_or(0);
            if now()
                .checked_sub(fetched_at)
                .is_none_or(|age| age >= self.ttl)
                || metadata.len() > MAX_BODY + HEADER
            {
                let _ = fs::remove_file(path);
                continue;
            }
            total += metadata.len();
            retained.push((fetched_at, metadata.len(), path));
        }
        retained.sort_by_key(|(fetched_at, _, _)| *fetched_at);
        let mut count = retained.len();
        for (_, size, path) in retained {
            if count <= MAX_FILES && total <= MAX_BYTES {
                break;
            }
            if fs::remove_file(path).is_ok() {
                count -= 1;
                total = total.saturating_sub(size);
            }
        }
    }
}

fn read_timestamp(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut timestamp = [0; 8];
    file.read_exact(&mut timestamp).ok()?;
    Some(u64::from_be_bytes(timestamp))
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn key(credential_digest: &[u8], host: &str, endpoint: &str, attempt: u64) -> String {
    let mut digest = Sha256::new();
    digest.update(b"dots-job-logs-v1\0");
    digest.update(credential_digest);
    digest.update([0]);
    digest.update(host.as_bytes());
    digest.update([0]);
    digest.update(endpoint.as_bytes());
    digest.update(attempt.to_be_bytes());
    format!("{:x}", digest.finalize())
}

/// Anonymous private spool, removed from the filesystem immediately after opening.
pub(crate) fn spool() -> std::io::Result<File> {
    let path = std::env::temp_dir().join(format!(
        "dots-log-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    fs::remove_file(path)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dots-log-cache-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }
        fn cache(&self) -> Cache {
            Cache::new(self.0.clone())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saved_read_distinguishes_missing_expired_integrity_and_permissions() {
        let directory = Fixture::new();
        let cache = directory.cache();
        assert!(
            cache
                .read("missing")
                .unwrap_err()
                .to_string()
                .contains("absent")
        );
        cache.put("old", b"body", now());
        let old = cache.directory.join("old.log");
        let mut bytes = fs::read(&old).unwrap();
        bytes
            .get_mut(..8)
            .unwrap()
            .copy_from_slice(&now().saturating_sub(TTL).to_be_bytes());
        fs::write(old, bytes).unwrap();
        assert!(
            cache
                .read("old")
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        cache.put("bad", b"body", now());
        let path = cache.directory.join("bad.log");
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b'x');
        fs::write(&path, bytes).unwrap();
        assert!(
            cache
                .read("bad")
                .unwrap_err()
                .to_string()
                .contains("integrity")
        );
        cache.put("private", b"body", now());
        fs::set_permissions(
            cache.directory.join("private.log"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(
            cache
                .read("private")
                .unwrap_err()
                .to_string()
                .contains("permissions")
        );
    }

    #[test]
    fn spooled_cache_streams_validates_and_rewinds() {
        let fixture = Fixture::new();
        let cache = fixture.cache();
        let mut source = spool().unwrap();
        source.write_all(b"neutral log\ncafe\xCC\x81\n").unwrap();
        cache.put_file("spool", &mut source, now());
        assert_eq!(source.stream_position().unwrap(), 0);
        let (mut cached, _) = cache.get_file("spool").unwrap();
        let mut body = String::new();
        cached.read_to_string(&mut body).unwrap();
        assert_eq!(body, "neutral log\ncafe\u{301}\n");
        let path = fixture.0.join("spool.log");
        let mut corrupt = OpenOptions::new().append(true).open(path).unwrap();
        corrupt.write_all(b"corrupt").unwrap();
        assert!(cache.get_file("spool").is_none());
    }

    #[test]
    fn entries_are_private_bounded_expiring_and_corruption_is_a_miss() {
        let fixture = Fixture::new();
        let cache = fixture.cache();
        cache.put("test", b"hello", now());
        assert_eq!(cache.get("test").unwrap().bytes, b"hello");
        assert_eq!(
            fs::metadata(&fixture.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = fixture.0.join("test.log");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&path, b"corrupted").unwrap();
        assert!(cache.get("test").is_none());
        cache.put("expired", b"hello", now().saturating_sub(TTL));
        assert!(cache.get("expired").is_none());
        cache.put(
            "huge",
            &vec![0; usize::try_from(MAX_BODY).unwrap() + 1],
            now(),
        );
        assert!(cache.get("huge").is_none());
        for id in 0..100 {
            cache.put(&format!("test-{id}"), b"hello", now());
        }
        assert!(fs::read_dir(&fixture.0).unwrap().count() <= MAX_FILES);
    }

    #[test]
    fn credentials_repository_host_and_attempt_isolate_keys() {
        let original = key(
            b"credential-a",
            "github.com",
            "repos/example/project/actions/jobs/1",
            1,
        );
        assert_ne!(
            original,
            key(
                b"credential-b",
                "github.com",
                "repos/example/project/actions/jobs/1",
                1
            )
        );
        assert_ne!(
            original,
            key(
                b"credential-a",
                "other.example.test",
                "repos/example/project/actions/jobs/1",
                1
            )
        );
        assert_ne!(
            original,
            key(
                b"credential-a",
                "github.com",
                "repos/example/other/actions/jobs/1",
                1
            )
        );
        assert_ne!(
            original,
            key(
                b"credential-a",
                "github.com",
                "repos/example/project/actions/jobs/1",
                2
            )
        );
    }

    #[test]
    fn unsafe_or_unwritable_directories_and_symlinks_disable_cache() {
        let fixture = Fixture::new();
        let cache = fixture.cache();
        fs::create_dir(&fixture.0).unwrap();
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o755)).unwrap();
        cache.put("test", b"private", now());
        assert!(cache.get("test").is_none());
        assert!(!fixture.0.join("test.log").exists());
        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o700)).unwrap();
        let target = fixture.0.join("original");
        fs::write(&target, b"unchanged").unwrap();
        symlink(&target, fixture.0.join("test.log")).unwrap();
        assert!(cache.get("test").is_none());
        cache.put("test", b"private", now());
        assert_eq!(fs::read(&target).unwrap(), b"unchanged");
        let blocked = Cache::new(target);
        blocked.put("test", b"private", now());
        assert!(blocked.get("test").is_none());
    }
}
