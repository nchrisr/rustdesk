//! Offline cache: the last approval the backend gave each peer on *this*
//! device (plan §3.2). Consulted only when the backend gives no decision.
//! The presented token must hash to the cached hash, so an outage never
//! weakens identity; entries expire so revoked access is bounded.

use super::types::Role;
use hbb_common::{config::Config, log, ResultType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const CACHE_FILE: &str = "velour_ac_cache.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub peer_id: String,
    pub role: Role,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
    pub token_sha256: String,
    /// Unix seconds of the last backend approval.
    pub approved_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineCache {
    #[serde(default)]
    pub entries: Vec<CacheEntry>,
}

pub fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

impl OfflineCache {
    pub fn path() -> PathBuf {
        Config::path(CACHE_FILE)
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("access control: cache unreadable, starting empty: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> ResultType<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &PathBuf) -> ResultType<()> {
        let s = serde_json::to_string_pretty(self)?;
        std::fs::write(path, s)?;
        Ok(())
    }

    /// Records a fresh approval, replacing any earlier entry for the peer.
    pub fn upsert(&mut self, entry: CacheEntry) {
        self.entries.retain(|e| e.peer_id != entry.peer_id);
        self.entries.push(entry);
    }

    /// An explicit backend "no" (or unknown peer) forgets the peer so a
    /// revocation cannot be replayed during a later outage.
    pub fn remove(&mut self, peer_id: &str) {
        self.entries.retain(|e| e.peer_id != peer_id);
    }

    /// The cached approval for `peer_id`, if the presented token matches and
    /// the approval is younger than `max_age_days`. Expired entries are
    /// treated as absent (and can be dropped with `prune`).
    pub fn lookup(
        &self,
        peer_id: &str,
        token: &str,
        now_unix: i64,
        max_age_days: u64,
    ) -> Option<&CacheEntry> {
        let max_age = max_age_days as i64 * 86_400;
        self.entries.iter().find(|e| {
            e.peer_id == peer_id
                && e.token_sha256 == token_hash(token)
                && now_unix.saturating_sub(e.approved_at) <= max_age
        })
    }

    pub fn prune(&mut self, now_unix: i64, max_age_days: u64) {
        let max_age = max_age_days as i64 * 86_400;
        self.entries
            .retain(|e| now_unix.saturating_sub(e.approved_at) <= max_age);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(peer: &str, token: &str, at: i64) -> CacheEntry {
        CacheEntry {
            peer_id: peer.into(),
            role: Role::Manager,
            user_id: "u1".into(),
            display_name: "Ada".into(),
            token_sha256: token_hash(token),
            approved_at: at,
        }
    }

    const DAY: i64 = 86_400;

    #[test]
    fn lookup_requires_matching_token() {
        let mut c = OfflineCache::default();
        c.upsert(entry("111", "secret", 1_000));
        assert!(c.lookup("111", "secret", 1_000, 7).is_some());
        assert!(c.lookup("111", "wrong", 1_000, 7).is_none());
        assert!(c.lookup("222", "secret", 1_000, 7).is_none());
    }

    #[test]
    fn lookup_respects_expiry_boundary() {
        let mut c = OfflineCache::default();
        c.upsert(entry("111", "t", 0));
        assert!(c.lookup("111", "t", 7 * DAY, 7).is_some(), "exactly 7 days is still valid");
        assert!(c.lookup("111", "t", 7 * DAY + 1, 7).is_none());
    }

    #[test]
    fn upsert_replaces_and_remove_forgets() {
        let mut c = OfflineCache::default();
        c.upsert(entry("111", "old", 10));
        c.upsert(entry("111", "new", 20));
        assert_eq!(c.entries.len(), 1);
        assert!(c.lookup("111", "old", 20, 7).is_none());
        assert!(c.lookup("111", "new", 20, 7).is_some());
        c.remove("111");
        assert!(c.entries.is_empty());
    }

    #[test]
    fn prune_drops_only_expired() {
        let mut c = OfflineCache::default();
        c.upsert(entry("a", "t", 0));
        c.upsert(entry("b", "t", 5 * DAY));
        c.prune(8 * DAY, 7);
        assert_eq!(c.entries.iter().map(|e| e.peer_id.as_str()).collect::<Vec<_>>(), vec!["b"]);
    }

    #[test]
    fn round_trips_through_disk_and_tolerates_garbage() {
        let dir = std::env::temp_dir().join(format!("velour_cache_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CACHE_FILE);
        let mut c = OfflineCache::default();
        c.upsert(entry("111", "t", 42));
        c.save_to(&path).unwrap();
        assert_eq!(OfflineCache::load_from(&path), c);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(OfflineCache::load_from(&path), OfflineCache::default());
        assert_eq!(OfflineCache::load_from(&dir.join("missing.json")), OfflineCache::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_hash_is_stable_sha256_hex() {
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
