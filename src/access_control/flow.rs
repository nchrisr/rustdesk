//! The authorization decision for one incoming login (plan §3.1 steps a–d),
//! kept free of connection state so it can be tested with `MockBackend`.

use super::backend::AccessBackend;
use super::cache::{token_hash, CacheEntry, OfflineCache};
use super::config::AcConfig;
use super::policy::check_local_rules;
use super::types::*;
use hbb_common::log;

/// Live state of an authorized connection under access control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcSession {
    /// Device-generated UUID that keys every event for this session.
    pub session_id: String,
    pub role: Role,
    pub user_id: String,
    pub display_name: String,
    pub monitoring: bool,
    /// Admitted from the offline cache while the backend was unreachable.
    pub offline_authorized: bool,
    pub remaining_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(AcSession),
    Deny {
        reason_code: String,
        /// Shown to the peer verbatim.
        message: String,
    },
}

pub const MSG_NOT_CONFIGURED: &str =
    "Access control is enabled on this device but not configured. Ask the device owner to set the backend URL and API key.";
pub const MSG_NO_TOKEN: &str =
    "This device requires an access token. Add your personal token in Settings > Access Control.";
pub const MSG_UNAVAILABLE: &str = "Authorization service unavailable. Try again later.";
pub const MSG_UNKNOWN_DEVICE: &str =
    "This device is not registered with the access-control backend.";

pub fn msg_unknown_peer(peer_id: &str) -> String {
    format!(
        "Your RustDesk ID {peer_id} is not registered. Ask your admin to verify the ID on file."
    )
}

fn deny(code: &str, message: impl Into<String>) -> Decision {
    Decision::Deny {
        reason_code: code.to_owned(),
        message: message.into(),
    }
}

/// Decides whether `req` may proceed. `cache` is updated in place (the caller
/// persists it). `connected` are the device's live authorized connections.
pub async fn decide(
    backend: &dyn AccessBackend,
    cfg: &AcConfig,
    cache: &mut OfflineCache,
    req: &AuthorizeRequest,
    now_unix: i64,
) -> Decision {
    if !cfg.is_configured() {
        return deny("not_configured", MSG_NOT_CONFIGURED);
    }
    if req.peer_token.trim().is_empty() {
        return deny("no_token", MSG_NO_TOKEN);
    }

    let (role, user_id, display_name, remaining, offline) = match backend.authorize(req).await {
        AuthorizeOutcome::Allowed(g) => {
            cache.upsert(CacheEntry {
                peer_id: req.peer_id.clone(),
                role: g.role,
                user_id: g.user_id.clone(),
                display_name: g.display_name.clone(),
                token_sha256: token_hash(&req.peer_token),
                approved_at: now_unix,
            });
            (g.role, g.user_id, g.display_name, g.remaining_seconds, false)
        }
        AuthorizeOutcome::Denied {
            reason_code,
            reason,
        } => {
            cache.remove(&req.peer_id);
            return deny(&reason_code, reason);
        }
        AuthorizeOutcome::UnknownPeer => {
            cache.remove(&req.peer_id);
            return deny("unknown_peer", msg_unknown_peer(&req.peer_id));
        }
        AuthorizeOutcome::UnknownDevice => {
            return deny("unknown_device", MSG_UNKNOWN_DEVICE);
        }
        AuthorizeOutcome::Unreachable { detail } => {
            log::warn!("access control: backend unreachable ({detail}); consulting offline cache");
            match cache.lookup(&req.peer_id, &req.peer_token, now_unix, cfg.cache_days) {
                Some(e) => (e.role, e.user_id.clone(), e.display_name.clone(), None, true),
                None => return deny("backend_unreachable", MSG_UNAVAILABLE),
            }
        }
    };

    if let Err(e) = check_local_rules(role, req.monitoring, &req.connected) {
        return deny(e.reason_code(), e.message());
    }

    Decision::Allow(AcSession {
        session_id: uuid::Uuid::new_v4().to_string(),
        role,
        user_id,
        display_name,
        monitoring: req.monitoring,
        offline_authorized: offline,
        remaining_seconds: remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::super::backend::mock::MockBackend;
    use super::*;
    use hbb_common::tokio;

    fn cfg() -> AcConfig {
        AcConfig {
            enabled: true,
            api_url: "https://backend".into(),
            api_key: "k".into(),
            heartbeat_secs: 30,
            cache_days: 7,
        }
    }

    fn req(peer: &str, token: &str) -> AuthorizeRequest {
        AuthorizeRequest {
            app: "rustdesk",
            peer_id: peer.into(),
            peer_token: token.into(),
            device_id: "999".into(),
            conn_type: "remote",
            monitoring: false,
            connected: vec![],
        }
    }

    fn grant(role: Role) -> AuthorizeOutcome {
        AuthorizeOutcome::Allowed(Grant {
            role,
            user_id: "u1".into(),
            display_name: "Ada".into(),
            remaining_seconds: Some(600),
        })
    }

    fn unreachable() -> AuthorizeOutcome {
        AuthorizeOutcome::Unreachable {
            detail: "down".into(),
        }
    }

    const NOW: i64 = 1_000_000;

    #[tokio::test]
    async fn not_configured_denies_before_calling_backend() {
        let b = MockBackend::with(vec![grant(Role::Admin)]);
        let mut c = OfflineCache::default();
        let mut cfg = cfg();
        cfg.api_key = "".into();
        let d = decide(&b, &cfg, &mut c, &req("1", "t"), NOW).await;
        assert!(matches!(d, Decision::Deny { ref reason_code, .. } if reason_code == "not_configured"));
        assert!(b.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_token_denies_before_calling_backend() {
        let b = MockBackend::with(vec![grant(Role::Admin)]);
        let mut c = OfflineCache::default();
        let d = decide(&b, &cfg(), &mut c, &req("1", "  "), NOW).await;
        assert!(matches!(d, Decision::Deny { ref reason_code, .. } if reason_code == "no_token"));
        assert!(b.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn allowed_records_session_and_caches_approval() {
        let b = MockBackend::with(vec![grant(Role::Manager)]);
        let mut c = OfflineCache::default();
        let d = decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await;
        match d {
            Decision::Allow(s) => {
                assert_eq!(s.role, Role::Manager);
                assert_eq!(s.display_name, "Ada");
                assert_eq!(s.remaining_seconds, Some(600));
                assert!(!s.offline_authorized);
                assert!(!s.session_id.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert!(c.lookup("1", "t", NOW, 7).is_some());
        let sent = &b.requests.lock().unwrap()[0];
        assert_eq!(sent.app, "rustdesk");
        assert_eq!(sent.device_id, "999");
    }

    #[tokio::test]
    async fn backend_denial_uses_its_reason_and_forgets_cache() {
        let b = MockBackend::with(vec![
            grant(Role::User),
            AuthorizeOutcome::Denied {
                reason_code: "outside_schedule".into(),
                reason: "Not now.".into(),
            },
        ]);
        let mut c = OfflineCache::default();
        assert!(matches!(decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await, Decision::Allow(_)));
        let d = decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await;
        assert_eq!(
            d,
            Decision::Deny {
                reason_code: "outside_schedule".into(),
                message: "Not now.".into()
            }
        );
        assert!(c.lookup("1", "t", NOW, 7).is_none(), "revocation must not be replayable");
    }

    #[tokio::test]
    async fn unknown_peer_message_names_the_id() {
        let b = MockBackend::with(vec![AuthorizeOutcome::UnknownPeer]);
        let mut c = OfflineCache::default();
        match decide(&b, &cfg(), &mut c, &req("123456789", "t"), NOW).await {
            Decision::Deny { reason_code, message } => {
                assert_eq!(reason_code, "unknown_peer");
                assert!(message.contains("123456789"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn unreachable_with_fresh_cache_and_same_token_allows_offline() {
        let b = MockBackend::with(vec![grant(Role::Manager), unreachable()]);
        let mut c = OfflineCache::default();
        assert!(matches!(decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await, Decision::Allow(_)));
        match decide(&b, &cfg(), &mut c, &req("1", "t"), NOW + 3 * 86_400).await {
            Decision::Allow(s) => {
                assert_eq!(s.role, Role::Manager);
                assert!(s.offline_authorized);
                assert_eq!(s.remaining_seconds, None, "no countdown until the backend answers");
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn unreachable_with_wrong_token_or_expired_cache_denies() {
        let b = MockBackend::with(vec![grant(Role::Manager), unreachable(), unreachable()]);
        let mut c = OfflineCache::default();
        assert!(matches!(decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await, Decision::Allow(_)));
        let d = decide(&b, &cfg(), &mut c, &req("1", "wrong"), NOW).await;
        assert!(matches!(d, Decision::Deny { ref reason_code, .. } if reason_code == "backend_unreachable"));
        let d = decide(&b, &cfg(), &mut c, &req("1", "t"), NOW + 8 * 86_400).await;
        assert!(matches!(d, Decision::Deny { ref reason_code, .. } if reason_code == "backend_unreachable"));
    }

    #[tokio::test]
    async fn unreachable_with_no_history_denies() {
        let b = MockBackend::with(vec![unreachable()]);
        let mut c = OfflineCache::default();
        let d = decide(&b, &cfg(), &mut c, &req("1", "t"), NOW).await;
        assert!(matches!(d, Decision::Deny { ref message, .. } if message == MSG_UNAVAILABLE));
    }

    #[tokio::test]
    async fn local_rule_blocks_second_user_even_when_backend_allows() {
        let b = MockBackend::with(vec![grant(Role::User)]);
        let mut c = OfflineCache::default();
        let mut r = req("1", "t");
        r.connected = vec![ConnectedPeer {
            peer_id: "2".into(),
            role: Role::User,
            session_id: "s".into(),
            monitoring: false,
        }];
        let d = decide(&b, &cfg(), &mut c, &r, NOW).await;
        assert!(matches!(d, Decision::Deny { ref reason_code, .. } if reason_code == "user_already_connected"));
    }

    #[tokio::test]
    async fn admin_joins_over_a_connected_user() {
        let b = MockBackend::with(vec![grant(Role::Admin)]);
        let mut c = OfflineCache::default();
        let mut r = req("1", "t");
        r.connected = vec![ConnectedPeer {
            peer_id: "2".into(),
            role: Role::User,
            session_id: "s".into(),
            monitoring: false,
        }];
        assert!(matches!(decide(&b, &cfg(), &mut c, &r, NOW).await, Decision::Allow(_)));
    }
}
