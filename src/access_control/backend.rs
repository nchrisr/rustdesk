//! The backend client. `AccessBackend` is the seam: the connection code
//! depends on the trait, `HttpBackend` talks to the real API, and tests use
//! `MockBackend`.

use super::types::*;
use super::{ACCESS_HTTP_TIMEOUT_SECS};
use async_trait::async_trait;
use hbb_common::{log, ResultType};
use serde_json::Value;
use std::time::Duration;

#[async_trait]
pub trait AccessBackend: Send + Sync {
    async fn authorize(&self, req: &AuthorizeRequest) -> AuthorizeOutcome;
    /// Posts one event envelope (spec §4.2). Errors mean "not delivered".
    async fn post_event(&self, body: &Value) -> ResultType<EventResponse>;
}

pub struct HttpBackend {
    base_url: String,
    api_key: String,
}

impl HttpBackend {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn post_json(&self, path: &str, body: &Value) -> ResultType<(u16, String)> {
        let url = self.url(path);
        let client = crate::hbbs_http::create_http_client_async_with_url(&url).await;
        let resp = client
            .post(&url)
            .timeout(Duration::from_secs(ACCESS_HTTP_TIMEOUT_SECS))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }
}

#[async_trait]
impl AccessBackend for HttpBackend {
    async fn authorize(&self, req: &AuthorizeRequest) -> AuthorizeOutcome {
        let body = match serde_json::to_value(req) {
            Ok(v) => v,
            Err(e) => {
                return AuthorizeOutcome::Unreachable {
                    detail: format!("serialize: {e}"),
                }
            }
        };
        match self.post_json("/v1/authorize", &body).await {
            Ok((status, text)) => parse_authorize_response(status, &text),
            Err(e) => {
                log::warn!("access control: authorize request failed: {e}");
                AuthorizeOutcome::Unreachable {
                    detail: e.to_string(),
                }
            }
        }
    }

    async fn post_event(&self, body: &Value) -> ResultType<EventResponse> {
        let (status, text) = self.post_json("/v1/events", body).await?;
        if !(200..300).contains(&status) {
            hbb_common::bail!("events endpoint returned {status}: {text}");
        }
        if text.trim().is_empty() {
            return Ok(EventResponse::default());
        }
        Ok(serde_json::from_str(&text)?)
    }
}

/// `GET /v1/sessions/active` as an admin (spec §4.3), for the monitor wall.
/// Blocking: called from the Flutter FFI thread pool, never from tokio.
pub fn list_active_sessions_blocking(base_url: &str, personal_token: &str) -> ResultType<String> {
    let url = format!("{}/v1/sessions/active", base_url.trim_end_matches('/'));
    let client = crate::hbbs_http::create_http_client_with_url(&url);
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(ACCESS_HTTP_TIMEOUT_SECS))
        .bearer_auth(personal_token)
        .send()?;
    let status = resp.status().as_u16();
    let text = resp.text()?;
    if status == 401 {
        hbb_common::bail!("The backend rejected your personal token (admin required).");
    }
    if !(200..300).contains(&status) {
        hbb_common::bail!("Backend returned {status}.");
    }
    Ok(text)
}

/// Maps an HTTP status + body to an outcome (spec §4.1). Pure, so the mapping
/// is unit-tested without a server.
pub fn parse_authorize_response(status: u16, body: &str) -> AuthorizeOutcome {
    let json: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return AuthorizeOutcome::Unreachable {
                detail: format!("status {status}, invalid JSON: {e}"),
            }
        }
    };
    match status {
        200 => {
            if json.get("allowed").and_then(Value::as_bool) == Some(true) {
                match serde_json::from_value::<Grant>(json.clone()) {
                    Ok(grant) => AuthorizeOutcome::Allowed(grant),
                    Err(e) => AuthorizeOutcome::Unreachable {
                        detail: format!("allowed but grant unparsable: {e}"),
                    },
                }
            } else {
                let str_of = |k: &str| {
                    json.get(k)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                let reason = str_of("reason");
                AuthorizeOutcome::Denied {
                    reason_code: str_of("reason_code"),
                    reason: if reason.is_empty() {
                        "Connection not allowed.".to_owned()
                    } else {
                        reason
                    },
                }
            }
        }
        404 => match json.get("error").and_then(Value::as_str) {
            Some("unknown_peer") => AuthorizeOutcome::UnknownPeer,
            Some("unknown_device") => AuthorizeOutcome::UnknownDevice,
            other => AuthorizeOutcome::Unreachable {
                detail: format!("404 with error {other:?}"),
            },
        },
        _ => AuthorizeOutcome::Unreachable {
            detail: format!("status {status}: {}", body.chars().take(200).collect::<String>()),
        },
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Scripted backend: pops one outcome per `authorize`, records requests.
    /// `post_event` fails while `fail_events` > 0 (decremented per call),
    /// then pops `event_responses` (default response when empty).
    #[derive(Default)]
    pub struct MockBackend {
        pub outcomes: Mutex<VecDeque<AuthorizeOutcome>>,
        pub requests: Mutex<Vec<AuthorizeRequest>>,
        pub events: Mutex<Vec<Value>>,
        pub event_responses: Mutex<VecDeque<EventResponse>>,
        pub fail_events: Mutex<u32>,
    }

    impl MockBackend {
        pub fn with(outcomes: Vec<AuthorizeOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into()),
                ..Default::default()
            }
        }
    }

    #[async_trait]
    impl AccessBackend for MockBackend {
        async fn authorize(&self, req: &AuthorizeRequest) -> AuthorizeOutcome {
            self.requests.lock().unwrap().push(req.clone());
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(AuthorizeOutcome::Unreachable {
                    detail: "mock: no scripted outcome".into(),
                })
        }

        async fn post_event(&self, body: &Value) -> ResultType<EventResponse> {
            self.events.lock().unwrap().push(body.clone());
            {
                let mut fails = self.fail_events.lock().unwrap();
                if *fails > 0 {
                    if *fails != u32::MAX {
                        *fails -= 1;
                    }
                    hbb_common::bail!("mock: injected failure");
                }
            }
            Ok(self
                .event_responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_parses_grant() {
        let out = parse_authorize_response(
            200,
            r#"{"allowed":true,"role":"manager","user_id":"u_42","display_name":"Ada","remaining_seconds":5400}"#,
        );
        assert_eq!(
            out,
            AuthorizeOutcome::Allowed(Grant {
                role: Role::Manager,
                user_id: "u_42".into(),
                display_name: "Ada".into(),
                remaining_seconds: Some(5400),
            })
        );
    }

    #[test]
    fn allowed_without_optional_fields_is_unlimited() {
        let out = parse_authorize_response(200, r#"{"allowed":true,"role":"admin"}"#);
        match out {
            AuthorizeOutcome::Allowed(g) => {
                assert_eq!(g.role, Role::Admin);
                assert_eq!(g.remaining_seconds, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn allowed_with_unknown_role_is_not_a_grant() {
        // Fail closed: a role we cannot enforce must not be admitted.
        let out = parse_authorize_response(200, r#"{"allowed":true,"role":"superuser"}"#);
        assert!(matches!(out, AuthorizeOutcome::Unreachable { .. }));
    }

    #[test]
    fn denied_carries_reason_verbatim() {
        let out = parse_authorize_response(
            200,
            r#"{"allowed":false,"reason_code":"outside_schedule","reason":"Mon-Fri 09:00-17:00 only."}"#,
        );
        assert_eq!(
            out,
            AuthorizeOutcome::Denied {
                reason_code: "outside_schedule".into(),
                reason: "Mon-Fri 09:00-17:00 only.".into()
            }
        );
    }

    #[test]
    fn denied_without_reason_gets_a_generic_one() {
        let out = parse_authorize_response(200, r#"{"allowed":false}"#);
        match out {
            AuthorizeOutcome::Denied { reason, .. } => assert!(!reason.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn not_found_variants() {
        assert_eq!(
            parse_authorize_response(404, r#"{"error":"unknown_peer"}"#),
            AuthorizeOutcome::UnknownPeer
        );
        assert_eq!(
            parse_authorize_response(404, r#"{"error":"unknown_device"}"#),
            AuthorizeOutcome::UnknownDevice
        );
        assert!(matches!(
            parse_authorize_response(404, r#"{"error":"something_else"}"#),
            AuthorizeOutcome::Unreachable { .. }
        ));
    }

    #[test]
    fn server_errors_and_auth_failures_are_unreachable() {
        for status in [401u16, 403, 500, 502, 503] {
            assert!(
                matches!(
                    parse_authorize_response(status, "{}"),
                    AuthorizeOutcome::Unreachable { .. }
                ),
                "status {status}"
            );
        }
        assert!(matches!(
            parse_authorize_response(200, "<html>oops"),
            AuthorizeOutcome::Unreachable { .. }
        ));
    }

    #[test]
    fn event_response_stop_only_on_explicit_false() {
        let stop: EventResponse = serde_json::from_str(r#"{"continue":false,"reason":"x"}"#).unwrap();
        assert!(stop.should_stop());
        let go: EventResponse = serde_json::from_str(r#"{"continue":true,"remaining_seconds":10}"#).unwrap();
        assert!(!go.should_stop());
        assert_eq!(go.remaining_seconds, Some(10));
        let ack: EventResponse = serde_json::from_str(r#"{"ok":true}"#).unwrap();
        assert!(!ack.should_stop());
    }
}
