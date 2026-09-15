//! Data shared between the backend client, the cache, the policy and the
//! connection wiring. Field names mirror `EXTERNAL_SYSTEM_SPEC.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Manager,
    User,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Manager => "manager",
            Role::User => "user",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s.trim().to_ascii_lowercase().as_str() {
            "admin" => Some(Role::Admin),
            "manager" => Some(Role::Manager),
            "user" => Some(Role::User),
            _ => None,
        }
    }
}

/// A connection already authorized on this device, as reported to the backend
/// and as consulted by the local rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectedPeer {
    pub peer_id: String,
    pub role: Role,
    pub session_id: String,
    pub monitoring: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizeRequest {
    pub app: &'static str,
    pub peer_id: String,
    pub peer_token: String,
    pub device_id: String,
    /// `remote`, `file_transfer`, `terminal`, `port_forward`, `view_camera`.
    pub conn_type: &'static str,
    pub monitoring: bool,
    pub connected: Vec<ConnectedPeer>,
}

/// What the backend granted on `allowed: true`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Grant {
    pub role: Role,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub remaining_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeOutcome {
    Allowed(Grant),
    /// The backend made a decision: no. `reason` is shown to the peer verbatim.
    Denied { reason_code: String, reason: String },
    /// 404 unknown_peer: the peer's ID is not on file.
    UnknownPeer,
    /// 404 unknown_device: this device is not on file.
    UnknownDevice,
    /// No decision: network error, timeout, 5xx, 401/403, unparsable body.
    /// The offline cache may be consulted. `detail` is for the log only.
    Unreachable { detail: String },
}

/// Response to an event post. Only heartbeats carry `continue`/`remaining`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct EventResponse {
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(rename = "continue", default)]
    pub cont: Option<bool>,
    #[serde(default)]
    pub remaining_seconds: Option<i64>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl EventResponse {
    /// A heartbeat answer tells the device to end the session only with an
    /// explicit `continue: false`; anything else keeps it running.
    pub fn should_stop(&self) -> bool {
        self.cont == Some(false)
    }
}
