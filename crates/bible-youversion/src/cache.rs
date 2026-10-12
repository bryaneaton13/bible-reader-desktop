//! On-disk cache of API responses, outside the repository.
//!
//! Every GET response is kept for [`CACHE_TTL`] (30 days) and never served
//! after that, so licensed text does not outlive the window we chose. One
//! JSON file per request, named by the SHA-256 of its key.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a response may be served from disk.
pub const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Serialize, Deserialize)]
struct Entry {
    key: String,
    /// Seconds since the Unix epoch.
    fetched_at: u64,
    body: String,
}

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
    ttl: Duration,
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_ttl(dir, CACHE_TTL)
    }

    pub fn with_ttl(dir: impl Into<PathBuf>, ttl: Duration) -> Self {
        Self {
            dir: dir.into(),
            ttl,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        let digest = Sha256::digest(key.as_bytes());
        let name: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{name}.json"))
    }

    fn fresh(&self, entry: &Entry, now: u64) -> bool {
        // A clock that went backwards counts as expired rather than immortal.
        now.checked_sub(entry.fetched_at)
            .is_some_and(|age| age < self.ttl.as_secs())
    }

    fn read(path: &Path) -> Option<Entry> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// The cached body for `key`, if one was stored less than the TTL ago.
    pub fn get(&self, key: &str) -> Option<String> {
        self.get_at(key, now())
    }

    /// Like [`get`](Self::get) but for data that must be fresher than the TTL
    /// (copyright notices: the license wants the latest from the API).
    pub fn get_max_age(&self, key: &str, max_age: Duration) -> Option<String> {
        let entry = Self::read(&self.path(key))?;
        let age = now().checked_sub(entry.fetched_at)?;
        (entry.key == key && self.fresh(&entry, now()) && age < max_age.as_secs())
            .then_some(entry.body)
    }

    pub fn get_at(&self, key: &str, now: u64) -> Option<String> {
        let entry = Self::read(&self.path(key))?;
        (entry.key == key && self.fresh(&entry, now)).then_some(entry.body)
    }

    pub fn put(&self, key: &str, body: &str) -> std::io::Result<()> {
        self.put_at(key, body, now())
    }

    pub fn put_at(&self, key: &str, body: &str, now: u64) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let entry = Entry {
            key: key.to_string(),
            fetched_at: now,
            body: body.to_string(),
        };
        let path = self.path(key);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(&entry)?)?;
        std::fs::rename(tmp, path)
    }

    fn files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json" || e == "tmp"))
            .collect()
    }

    /// Delete entries past the TTL (and unreadable ones). Returns how many.
    pub fn purge_expired(&self) -> usize {
        self.purge_expired_at(now())
    }

    pub fn purge_expired_at(&self, now: u64) -> usize {
        let mut removed = 0;
        for path in self.files() {
            let keep = Self::read(&path).is_some_and(|e| self.fresh(&e, now));
            if !keep && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Delete everything. Returns how many entries were removed.
    pub fn clear(&self) -> usize {
        self.files()
            .into_iter()
            .filter(|p| std::fs::remove_file(p).is_ok())
            .count()
    }

    /// Entries on disk and their total size in bytes.
    pub fn stats(&self) -> (usize, u64) {
        self.files()
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .fold((0, 0), |(n, bytes), m| (n + 1, bytes + m.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bible-yv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn serves_for_thirty_days_then_expires() {
        let dir = scratch("ttl");
        let cache = Cache::new(&dir);
        cache
            .put_at("passage:3034:JHN.3", "{\"content\":\"x\"}", 1_000)
            .unwrap();
        assert_eq!(
            cache
                .get_at("passage:3034:JHN.3", 1_000 + 29 * DAY)
                .as_deref(),
            Some("{\"content\":\"x\"}")
        );
        assert_eq!(cache.get_at("passage:3034:JHN.3", 1_000 + 30 * DAY), None);
        assert_eq!(cache.get_at("other-key", 1_000), None);
        // Clock moved backwards: treated as expired.
        assert_eq!(cache.get_at("passage:3034:JHN.3", 500), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn purge_removes_only_expired_entries() {
        let dir = scratch("purge");
        let cache = Cache::new(&dir);
        cache.put_at("old", "1", 0).unwrap();
        cache.put_at("new", "2", 40 * DAY).unwrap();
        std::fs::write(dir.join("junk.json"), "not json").unwrap();
        assert_eq!(cache.purge_expired_at(45 * DAY), 2);
        assert_eq!(cache.get_at("new", 45 * DAY).as_deref(), Some("2"));
        let (entries, bytes) = cache.stats();
        assert_eq!(entries, 1);
        assert!(bytes > 0);
        assert_eq!(cache.clear(), 1);
        assert_eq!(cache.stats(), (0, 0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_directory_is_an_empty_cache() {
        let cache = Cache::new(scratch("missing"));
        assert_eq!(cache.get("anything"), None);
        assert_eq!(cache.purge_expired(), 0);
        assert_eq!(cache.stats(), (0, 0));
    }
}
