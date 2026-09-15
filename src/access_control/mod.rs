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
