//! Session events (spec §4.2): `session_start` once, `heartbeat` every N
//! seconds with the backend's answer fed back to the connection, `session_end`
//! once, and `auth_denied` for refusals the device decided itself.
//!
//! `SessionReporter` owns one background task per authorized connection. The
//! connection only starts it, receives `HeartbeatResult`s, and ends it.

use super::backend::AccessBackend;
use super::flow::AcSession;
use super::types::EventResponse;
use super::ACCESS_APP_NAME;
use hbb_common::{
    log,
    tokio::{self, sync::mpsc, task::JoinHandle, time},
};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
// tokio's Instant so the paused test clock drives `elapsed_seconds`.
use hbb_common::tokio::time::Instant;

/// Delivery attempts for `session_start` / `session_end` before giving up.
/// Backoff doubles from 2 s: 2+4+8+16+32+64+128 ≈ 4 min total.
pub const MAX_ATTEMPTS: u32 = 8;
pub const FIRST_BACKOFF: Duration = Duration::from_secs(2);

/// Everything the envelope needs, fixed for the life of the session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub device_id: String,
    pub device_name: String,
    pub peer_id: String,
    pub peer_name: String,
    pub conn_type: &'static str,
    pub session: AcSession,
    pub started_at: SystemTime,
}

/// What a heartbeat answer means for the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatResult {
    /// Keep going; `remaining_seconds` drives the countdown (WP4).
    Continue { remaining_seconds: Option<i64> },
    /// The backend ended the session; `reason` is shown to the peer.
    Stop { reason: String },
    /// The master switch was turned off; reporting has stopped.
    Disabled,
}

fn rfc3339(t: SystemTime) -> String {
    hbb_common::chrono::DateTime::<hbb_common::chrono::Utc>::from(t)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// The common envelope (spec §4.2). `elapsed` is the device's own count.
pub fn envelope(event: &str, info: &SessionInfo, elapsed_seconds: u64, data: Value) -> Value {
    json!({
        "event": event,
        "event_id": uuid::Uuid::new_v4().to_string(),
        "ts": rfc3339(SystemTime::now()),
        "session": {
            "session_id": info.session.session_id,
            "app": ACCESS_APP_NAME,
            "device_id": info.device_id,
            "device_name": info.device_name,
            "peer_id": info.peer_id,
            "peer_name": info.peer_name,
            "user_id": info.session.user_id,
            "role": info.session.role.as_str(),
            "conn_type": info.conn_type,
            "monitoring": info.session.monitoring,
            "offline_authorized": info.session.offline_authorized,
            "started_at": rfc3339(info.started_at),
            "elapsed_seconds": elapsed_seconds,
        },
        "data": data,
    })
}

/// Posts `body` until the backend accepts it or attempts run out. The same
/// `event_id` is resent, so the backend dedups (spec: at-least-once).
pub async fn post_with_retry(backend: &dyn AccessBackend, body: &Value) -> Option<EventResponse> {
    let mut backoff = FIRST_BACKOFF;
    for attempt in 1..=MAX_ATTEMPTS {
        match backend.post_event(body).await {
            Ok(r) => return Some(r),
            Err(e) => {
                log::warn!(
                    "access control: event {} attempt {attempt}/{MAX_ATTEMPTS} failed: {e}",
                    body["event"].as_str().unwrap_or("?")
                );
                if attempt < MAX_ATTEMPTS {
                    time::sleep(backoff).await;
                    backoff *= 2;
                }
            }
        }
    }
    None
}

pub struct SessionReporter {
    backend: Arc<dyn AccessBackend>,
    info: SessionInfo,
    started: Instant,
    task: JoinHandle<()>,
}

impl SessionReporter {
    /// Starts reporting: `session_start` (with retry), then a heartbeat every
    /// `interval`. Each heartbeat's meaning is sent on `tx`. `is_enabled` is
    /// polled every tick so turning the master switch off ends reporting
    /// without a hook in the settings code.
    pub fn start(
        backend: Arc<dyn AccessBackend>,
        info: SessionInfo,
        interval: Duration,
        is_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
        tx: mpsc::UnboundedSender<HeartbeatResult>,
    ) -> Self {
        let started = Instant::now();
        let task = tokio::spawn(Self::run(
            backend.clone(),
            info.clone(),
            started,
            interval,
            is_enabled,
            tx,
        ));
        Self {
            backend,
            info,
            started,
            task,
        }
    }

    async fn run(
        backend: Arc<dyn AccessBackend>,
        info: SessionInfo,
        started: Instant,
        interval: Duration,
        is_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
        tx: mpsc::UnboundedSender<HeartbeatResult>,
    ) {
        post_with_retry(&*backend, &envelope("session_start", &info, 0, json!({}))).await;
        let mut ticker = time::interval_at(Instant::now() + interval, interval);
        loop {
            ticker.tick().await;
            if !is_enabled() {
                let body = envelope(
                    "session_end",
                    &info,
                    started.elapsed().as_secs(),
                    json!({ "reason": "access_control_disabled" }),
                );
                post_with_retry(&*backend, &body).await;
                let _ = tx.send(HeartbeatResult::Disabled);
                return;
            }
            let body = envelope("heartbeat", &info, started.elapsed().as_secs(), json!({}));
            // Not retried: the next tick supersedes it, and a backend outage
            // must never end a live session.
            match backend.post_event(&body).await {
                Ok(r) if r.should_stop() => {
                    let _ = tx.send(HeartbeatResult::Stop {
                        reason: r
                            .reason
                            .unwrap_or_else(|| "Session ended by the access-control backend.".into()),
                    });
                    return;
                }
                Ok(r) => {
                    let _ = tx.send(HeartbeatResult::Continue {
                        remaining_seconds: r.remaining_seconds,
                    });
                }
                Err(e) => log::warn!("access control: heartbeat failed: {e}"),
            }
        }
    }

    pub fn elapsed_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Stops the heartbeat and sends `session_end` from a detached task so
    /// the connection's shutdown is not delayed by retries.
    pub fn end(self, reason: &'static str, detail: &str) {
        self.task.abort();
        let body = envelope(
            "session_end",
            &self.info,
            self.elapsed_seconds(),
            json!({ "reason": reason, "detail": detail }),
        );
        let backend = self.backend;
        tokio::spawn(async move {
            post_with_retry(&*backend, &body).await;
        });
    }
}

/// Maps the connection's free-text close reason onto the spec's end reasons.
pub fn end_reason(close_reason: &str) -> &'static str {
    let r = close_reason.to_ascii_lowercase();
    if r == "end" || r.contains("peer") || r.contains("closed manually") {
        "peer_disconnected"
    } else if r.contains("connection manager") || r.contains("cm ") {
        "device_closed"
    } else if r.contains("timeout") || r.contains("io error") || r.contains("reset") || r.contains("broken") {
        "network_lost"
    } else {
        "other"
    }
}

/// `auth_denied` for a refusal the device decided itself (spec §4.2). Best
/// effort, no retry, detached.
pub fn report_denied(
    backend: Arc<dyn AccessBackend>,
    device_id: String,
    device_name: String,
    peer_id: String,
    peer_name: String,
    conn_type: &'static str,
    reason_code: String,
    reason: String,
) {
    let body = json!({
        "event": "auth_denied",
        "event_id": uuid::Uuid::new_v4().to_string(),
        "ts": rfc3339(SystemTime::now()),
        "session": {
            "session_id": uuid::Uuid::new_v4().to_string(),
            "app": ACCESS_APP_NAME,
            "device_id": device_id,
            "device_name": device_name,
            "peer_id": peer_id,
            "peer_name": peer_name,
            "conn_type": conn_type,
            "monitoring": false,
            "offline_authorized": false,
            "started_at": rfc3339(SystemTime::now()),
            "elapsed_seconds": 0,
        },
        "data": { "reason_code": reason_code, "reason": reason },
    });
    tokio::spawn(async move {
        if let Err(e) = backend.post_event(&body).await {
            log::warn!("access control: auth_denied not delivered: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::backend::mock::MockBackend;
    use super::super::types::Role;
    use super::*;

    fn info() -> SessionInfo {
        SessionInfo {
            device_id: "999".into(),
            device_name: "Lab".into(),
            peer_id: "111".into(),
            peer_name: "Ada's laptop".into(),
            conn_type: "remote",
            session: AcSession {
                session_id: "sess-1".into(),
                role: Role::Manager,
                user_id: "u1".into(),
                display_name: "Ada".into(),
                monitoring: false,
                offline_authorized: true,
                remaining_seconds: Some(600),
            },
            started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        }
    }

    fn always_on() -> Arc<dyn Fn() -> bool + Send + Sync> {
        Arc::new(|| true)
    }

    #[test]
    fn envelope_has_the_spec_shape() {
        let v = envelope("heartbeat", &info(), 42, json!({}));
        assert_eq!(v["event"], "heartbeat");
        assert_eq!(v["event_id"].as_str().unwrap().len(), 36);
        assert!(v["ts"].as_str().unwrap().ends_with('Z'));
        let s = &v["session"];
        assert_eq!(s["session_id"], "sess-1");
        assert_eq!(s["app"], "rustdesk");
        assert_eq!(s["device_id"], "999");
        assert_eq!(s["peer_id"], "111");
        assert_eq!(s["role"], "manager");
        assert_eq!(s["conn_type"], "remote");
        assert_eq!(s["offline_authorized"], true);
        assert_eq!(s["started_at"], "2023-11-14T22:13:20Z");
        assert_eq!(s["elapsed_seconds"], 42);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_resends_same_event_until_accepted() {
        let b = MockBackend::default();
        *b.fail_events.lock().unwrap() = 2;
        let body = envelope("session_start", &info(), 0, json!({}));
        let r = post_with_retry(&b, &body).await;
        assert!(r.is_some());
        let sent = b.events.lock().unwrap();
        assert_eq!(sent.len(), 3);
        assert!(sent.iter().all(|e| e["event_id"] == body["event_id"]));
    }

    #[tokio::test(start_paused = true)]
    async fn retry_gives_up_after_max_attempts() {
        let b = MockBackend::default();
        *b.fail_events.lock().unwrap() = u32::MAX;
        let r = post_with_retry(&b, &envelope("session_end", &info(), 5, json!({}))).await;
        assert!(r.is_none());
        assert_eq!(b.events.lock().unwrap().len() as u32, MAX_ATTEMPTS);
    }

    #[tokio::test(start_paused = true)]
    async fn start_then_heartbeats_then_end_in_order() {
        let b = Arc::new(MockBackend::default());
        b.event_responses.lock().unwrap().extend([
            EventResponse::default(), // session_start ack
            EventResponse { cont: Some(true), remaining_seconds: Some(570), ..Default::default() },
            EventResponse { cont: Some(true), remaining_seconds: Some(540), ..Default::default() },
        ]);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let rep = SessionReporter::start(b.clone(), info(), Duration::from_secs(30), always_on(), tx);
        time::sleep(Duration::from_secs(61)).await;
        assert_eq!(rx.recv().await, Some(HeartbeatResult::Continue { remaining_seconds: Some(570) }));
        assert_eq!(rx.recv().await, Some(HeartbeatResult::Continue { remaining_seconds: Some(540) }));
        rep.end("peer_disconnected", "End");
        time::sleep(Duration::from_millis(10)).await;
        let sent = b.events.lock().unwrap();
        let kinds: Vec<&str> = sent.iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["session_start", "heartbeat", "heartbeat", "session_end"]);
        assert_eq!(sent[1]["session"]["elapsed_seconds"], 30);
        assert_eq!(sent[2]["session"]["elapsed_seconds"], 60);
        assert_eq!(sent[3]["data"]["reason"], "peer_disconnected");
        assert_eq!(sent[3]["data"]["detail"], "End");
    }

    #[tokio::test(start_paused = true)]
    async fn stop_answer_is_forwarded_and_heartbeats_cease() {
        let b = Arc::new(MockBackend::default());
        b.event_responses.lock().unwrap().extend([
            EventResponse::default(),
            EventResponse { cont: Some(false), reason: Some("Quota reached.".into()), ..Default::default() },
        ]);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _rep = SessionReporter::start(b.clone(), info(), Duration::from_secs(30), always_on(), tx);
        time::sleep(Duration::from_secs(95)).await;
        assert_eq!(rx.recv().await, Some(HeartbeatResult::Stop { reason: "Quota reached.".into() }));
        let n = b.events.lock().unwrap().iter().filter(|e| e["event"] == "heartbeat").count();
        assert_eq!(n, 1, "no heartbeats after a stop");
    }

    #[tokio::test(start_paused = true)]
    async fn failed_heartbeats_do_not_end_the_session() {
        let b = Arc::new(MockBackend::default());
        *b.fail_events.lock().unwrap() = 0;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _rep = SessionReporter::start(b.clone(), info(), Duration::from_secs(30), always_on(), tx);
        time::sleep(Duration::from_secs(1)).await; // session_start delivered
        *b.fail_events.lock().unwrap() = u32::MAX;
        time::sleep(Duration::from_secs(120)).await;
        assert!(rx.try_recv().is_err(), "nothing forwarded while heartbeats fail");
        let n = b.events.lock().unwrap().iter().filter(|e| e["event"] == "heartbeat").count();
        assert_eq!(n, 4, "one attempt per tick, no retries");
    }

    #[tokio::test(start_paused = true)]
    async fn switch_off_sends_session_end_disabled() {
        let b = Arc::new(MockBackend::default());
        let enabled = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let e2 = enabled.clone();
        let is_enabled: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || e2.load(std::sync::atomic::Ordering::SeqCst));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _rep = SessionReporter::start(b.clone(), info(), Duration::from_secs(30), is_enabled, tx);
        time::sleep(Duration::from_secs(31)).await;
        enabled.store(false, std::sync::atomic::Ordering::SeqCst);
        time::sleep(Duration::from_secs(30)).await;
        assert_eq!(rx.recv().await, Some(HeartbeatResult::Continue { remaining_seconds: None }));
        assert_eq!(rx.recv().await, Some(HeartbeatResult::Disabled));
        let sent = b.events.lock().unwrap();
        let last = sent.last().unwrap();
        assert_eq!(last["event"], "session_end");
        assert_eq!(last["data"]["reason"], "access_control_disabled");
    }

    #[test]
    fn end_reason_mapping() {
        assert_eq!(end_reason("End"), "peer_disconnected");
        assert_eq!(end_reason("Closed manually by the peer"), "peer_disconnected");
        assert_eq!(end_reason("connection manager window closed"), "device_closed");
        assert_eq!(end_reason("read timeout"), "network_lost");
        assert_eq!(end_reason("auto disconnect"), "other");
    }
}
