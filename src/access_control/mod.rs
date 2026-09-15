//! RustDesk-Velour access control.
//!
//! An optional layer where the device being connected to asks an external
//! backend whether a connection may proceed and reports the session while it
//! lasts. Design and work packages: `docs/access-control/VELOUR_PLAN.md`;
//! backend API contract: `docs/access-control/EXTERNAL_SYSTEM_SPEC.md`.
//!
//! Everything here is inert unless the `access-control` option is "Y".

// WP1 only adds settings; the consumers arrive in WP2/WP3. Remove then.
#![allow(dead_code)]

pub mod backend;
pub mod cache;
pub mod config;
pub mod events;
pub mod flow;
pub mod policy;
pub mod types;

pub use config::AcConfig;
pub use flow::{AcSession, Decision};

/// Value of the `app` field in every request to the backend. The backend keys
/// IDs on `(app, remote_id)` because it also stores IDs from other
/// remote-desktop tools.
pub const ACCESS_APP_NAME: &str = "rustdesk";

/// Timeout for every HTTP call to the backend (authorize and events). The
/// backend's own latency budget is 5 s; this leaves room for a cold start.
pub const ACCESS_HTTP_TIMEOUT_SECS: u64 = 10;

/// The device -> peer time message (proto `SessionTime`).
pub fn session_time_misc(elapsed_seconds: i64, remaining: Option<i64>) -> base::message_proto::Misc {
    use base::message_proto::{Misc, SessionTime};
    let mut misc = Misc::new();
    misc.set_session_time(SessionTime {
        elapsed_seconds,
        has_remaining: remaining.is_some(),
        remaining_seconds: remaining.unwrap_or(0),
        ..Default::default()
    });
    misc
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::message_proto::misc;

    #[test]
    fn session_time_misc_encodes_absent_remaining() {
        let m = session_time_misc(42, None);
        let Some(misc::Union::SessionTime(t)) = m.union else { panic!() };
        assert_eq!((t.elapsed_seconds, t.has_remaining, t.remaining_seconds), (42, false, 0));
        let m = session_time_misc(7, Some(0));
        let Some(misc::Union::SessionTime(t)) = m.union else { panic!() };
        assert_eq!((t.elapsed_seconds, t.has_remaining, t.remaining_seconds), (7, true, 0));
    }
}
