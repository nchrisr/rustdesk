# RustDesk-Velour — Implementation Plan

**Purpose of this document.** This is the complete, self-contained plan for a
personal fork of RustDesk called **RustDesk-Velour**. It is written so that an
engineer or AI assistant with a fresh checkout of this repository and *only*
this document can implement every feature from scratch, in order, with tests.
It records what to build, why each decision was made, where in the codebase the
changes go, and how each piece is verified.

Companion document: [`EXTERNAL_SYSTEM_SPEC.md`](EXTERNAL_SYSTEM_SPEC.md)
defines the HTTP API of the external backend that this fork talks to. That
backend is a separate project owned by the user; this repository only
implements the RustDesk side. Anything about request/response shapes lives in
the spec and is *not* repeated here; this plan references it.

Status legend used below: ☐ not started · ◐ in progress · ☑ done.

---

## 1. Product summary

Stock RustDesk lets anyone who knows a device's password connect to it.
RustDesk-Velour adds an optional **access-control layer** where the device asks
an external backend (the user's existing system, which already has users and
roles) whether a connection may proceed, then reports the session to that
backend while it lasts.

What the user gets, in their own words, mapped to work packages (WP):

| # | Feature as requested | Where it lands |
|---|---|---|
| 1 | Admin can see all active sessions and watch them | Backend dashboard (events) + WP5 monitor wall |
| 2 | Webhooks: session start / ongoing / end, JSON, unique session id | WP3 |
| 3 | Tiers Admin / Manager / User with connection rules | WP2 |
| 4 | See time spent in a session; shared with webhook | WP3 + WP4 |
| 5 | Schedules and hour quotas per user | Backend decides; WP2/WP3 enforce |
| 6 | Password + user whitelisting (admins whitelisted by default) | WP2 (token replaces whitelist; password kept) |
| 7 | Use own rendezvous/relay server | WP6, lowest priority, deferred |
| — | Name the build "RustDesk-Velour" and show it in the UI | WP0 |
| — | A single switch to turn all of this on/off per install | WP1 |

## 2. Glossary

* **Peer** — the machine a person connects *from*.
* **Device** — the machine being connected *to*. All enforcement happens here.
* **RustDesk ID** — stable 9-digit id of an installation
  (`Config::get_id()` in `libs/hbb_common/src/config.rs`). Changes only if the
  user changes it manually or wipes config.
* **Backend** — the user's external system implementing
  `EXTERNAL_SYSTEM_SPEC.md`.
* **Personal token** — per-person secret issued by the backend; the peer sends
  it with every login. Proves identity, because the peer's RustDesk ID is
  self-reported and unverifiable.
* **Device API key** — secret a device uses to call the backend.
* **Access control (AC)** — the whole feature set, behind one master switch.
* **CM** — Connection Manager, the small window on the device listing incoming
  sessions (`flutter/lib/desktop/pages/server_page.dart`).

## 3. Architecture

```
  PEER (person's machine)                     DEVICE (machine being controlled)
  ┌───────────────────────┐   LoginRequest    ┌──────────────────────────────┐
  │ RustDesk-Velour       │ ─ id, password, ─▶│ RustDesk-Velour              │
  │ settings: token       │   token           │ settings: AC on, URL, key    │
  └───────────────────────┘                   │                              │
            ▲   countdown / stop reason       │  1. existing checks          │
            └─────────────────────────────────│  2. POST /v1/authorize ──────┼──▶ BACKEND
                                              │  3. local rules (one User)   │    (user's system)
                                              │  4. password check           │    users, roles,
                                              │  5. session_start ───────────┼──▶ assignments,
                                              │     heartbeat ◀──────────────┼──▶ schedules,
                                              │     session_end ─────────────┼──▶ quotas, sessions
                                              └──────────────────────────────┘
```

Principles:

0. **IDs are application-neutral on the backend.** Every request RustDesk
   sends carries `"app": "rustdesk"` next to `peer_id`/`device_id`, because the
   backend also stores IDs from other remote-desktop tools (historically
   AnyDesk) and keys everything on the `(app, remote_id)` pair. RustDesk-side
   this is a constant `ACCESS_APP_NAME = "rustdesk"` in the request builders
   (`src/access_control/backend.rs`); nothing else depends on it.
1. **The device is the enforcement point; the backend is the decision point.**
   The device never stores roles, schedules or quotas. It obeys answers.
2. **Password is kept, token is added.** Password protects the device; token
   identifies the person. Both are required when AC is on.
3. **Fail closed.** No clear "yes" from the backend → deny, with a specific
   message. Exception: the offline cache (§3.2).
4. **Off by default.** With the master switch off the fork behaves exactly like
   stock RustDesk, and interoperates with stock RustDesk in both directions.
5. **No surprises for the person connected.** Any cut-off is preceded by a
   visible countdown fed by the backend's `remaining_seconds`.

### 3.1 Login sequence on the device (AC on)

Inserted into `Connection::on_message` handling of `LoginRequest` in
`src/server/connection.rs` (search for `if let Some(message::Union::LoginRequest(lr)) = msg.union`).

```
existing: check_login_scope, handle_login_request_without_validation,
          check_id_whitelist, conn-type permission checks
NEW ─────────────────────────────────────────────────────────────────────
  a. AC enabled but URL or API key empty       → deny "Access control is enabled but not configured on this device."
  b. lr.access_token empty                     → deny "This device requires an access token."
  c. call backend /v1/authorize (10 s timeout)
       200 allowed:false                        → deny with backend `reason`; drop offline-cache entry for peer
       404 unknown_peer                         → deny "Your RustDesk ID <id> is not registered. Ask your admin to verify the ID on file."
       network error / timeout / 5xx            → consult offline cache (§3.2); if no usable entry → deny "Authorization service unavailable. Try again later."
       200 allowed:true                         → record role, user_id, display_name, remaining_seconds; upsert offline cache
  d. local rule: role == user && any AUTHED_CONNS remote conn (non-monitoring) has role == user
                                               → deny "Another user is currently connected to this device."
existing: password validation (validate_password), 2FA, click-approve, CM start
NEW: on authorized → create session record (uuid), emit session_start, start heartbeat task
```

All denials go through the existing `send_login_error` and additionally emit an
`auth_denied` event (best-effort) for cases the device decided itself
(`no_token`, `backend_unreachable`, `user_already_connected`, `cache_expired`,
`not_configured`).

### 3.2 Offline cache (device-local)

Purpose: a person who was recently approved for *this device* can still
connect while the backend is down, with the same role. Roles are per device
(a Manager on one device may be a User on another), so the cache is naturally
per device.

* File: `<config dir>/velour_ac_cache.json` (via `Config::path`).
* Entry: `{ peer_id, role, user_id, display_name, token_sha256, approved_at }`.
* Written/refreshed on every `allowed:true`. Removed on any explicit
  `allowed:false` or `404` for that peer.
* Used **only** on transport failure. Requirements: entry exists, not older
  than `access-cache-days` (default 7), and `sha256(presented token) ==
  token_sha256`. Password still checked afterwards as normal.
* A session admitted this way carries `offline_authorized: true` in all events,
  and has no `remaining_seconds` until the first successful heartbeat.

### 3.3 Session events

See spec §4.2 for payloads. Device side:

* One `SessionRecord { session_id: Uuid, started_at: Instant+SystemTime, role,
  user_id, display_name, monitoring, offline_authorized, remaining: Option<i64>,
  remaining_at: Instant }` per authorized connection, owned by `Connection`.
* `session_start` and `session_end` are sent with retry (exponential backoff,
  max ~5 min, persisted in memory only; give up when the process exits).
* `heartbeat` every `access-heartbeat-secs` (default 30) from a tokio task
  owned by the connection; not retried; the response updates `remaining` and
  may carry `continue:false` → the device calls the existing close path with
  the backend's reason (peer sees it via `CloseReason`).
* `max_session_seconds` is *not* used; `remaining_seconds` covers it.
* Failed heartbeats never end a session.
* Turning the master switch off while sessions are active emits `session_end`
  with `data.reason = "access_control_disabled"` for each and stops reporting.

### 3.4 Countdown and elapsed time

* The device forwards `remaining_seconds` (and elapsed) to the peer in a new
  `Misc` message (§5.3). The peer shows a countdown in the remote view once
  `remaining ≤ 4 h`; always visible from then on; turns red under 30 min
  (thresholds configurable). Between heartbeats the peer counts down locally.
* Milestone toasts at 4:00, 3:30, 3:00 … 0:30, then 10, 5, 1 min (list in a
  constant; later a setting).
* The device's CM shows the same countdown next to the existing elapsed timer
  for that connection, and shows the person's `display_name` and role.
* Elapsed time is already tracked in the CM (Flutter-side timer). Velour
  additionally tracks it in Rust (`SessionRecord.started_at`) so events and the
  peer's toolbar can show it.

### 3.5 Roles on the device

* `admin` and `manager`: never blocked locally. Whether a manager may reach
  this device is decided by the backend.
* `user`: blocked locally if another `user` is connected (non-monitoring).
* Arrival of an admin/manager never kicks a connected user; both stay.
* `monitoring` connections (admin wall tiles) are view-only, do not count for
  the "one user" rule, and are flagged in events.

### 3.6 Admin monitor wall

A new desktop window that shows up to **6** live view-only sessions in a grid
(limit is a setting, default 6). The list of watchable sessions comes from the
backend (`GET /v1/sessions/active`, authenticated with the admin's personal
token). Each tile is a standard remote session opened with `monitoring: true`
in its login and view-only forced on. "Watch" is disabled at the limit with the
hint "Close a tile to add another."

## 4. Decision log

Every decision agreed with the user, with the reason, so they are not
re-litigated:

| Decision | Reason |
|---|---|
| Roles come from the user's external system, keyed by RustDesk ID | The system already has users/roles; avoids building user management into RustDesk. |
| Backend IDs are `(app, remote_id)` pairs; RustDesk sends `app: "rustdesk"` in every request | The external system already holds AnyDesk IDs and must stay tool-agnostic. |
| Add a per-person token; keep the device password | Peer ID is self-reported. Password alone cannot tell *who* connected; token can. Password still protects the device. |
| Token stored hashed in backend, plain in peer's local config, never on the device | Device only relays; revocation is backend-only. |
| Fail closed on backend failure, with distinct messages per case | An outage must not silently disable rules; the person must know what to fix. |
| Offline cache of last approval, per device, token-hash-checked, 7-day expiry | Lets known people keep working during outages without weakening identity; expiry bounds revoked access. |
| No "break-glass" password-only fallback, no password-only mode on protected devices | User chose airtight over convenience. |
| Heartbeat every 30 s, two-way; backend answers `continue` + `remaining_seconds` | Backend enforces quotas without RustDesk knowing about them; changes propagate within one heartbeat. |
| Failed heartbeats never cut sessions | A backend outage must not kick everyone. |
| Cut-offs only when `remaining_seconds` hits 0; countdown shown from 4 h | User explicitly did not want abrupt disconnects. |
| Countdown shown on both peer and device (CM) | Requested. |
| Admin/Manager joining does not kick a User | Less disruptive; admin can end sessions via backend. |
| Schedules and quotas are per user **per device**, with a default set and per-device overrides | Requested. |
| Users also require device assignment by default (`users_require_assignment`) | Safer default; backend setting can flip it. |
| Monitor wall limited to 6 tiles (setting) | Performance and screen space. |
| One master switch "Enable access control"; URL, key, token entered in Settings; nothing baked into the build | Requested. Misconfigured-but-enabled device refuses connections rather than falling back. |
| Build named **RustDesk-Velour**, visible in UI | Requested, so the user can tell their build apart. |
| Own rendezvous/relay server is last priority | Public server is sufficient for now. |

## 5. Codebase map (where things go)

Verified against the tree at the time of writing; use the symbol names to
re-locate if line numbers drift.

| Concern | Location |
|---|---|
| Option keys (single import path for all options) | `libs/base/src/config/keys.rs` |
| Config read/write | `hbb_common::config::Config::{get_option,set_option}` and `LocalConfig` |
| Wire protocol (NOT the submodule — safe to edit) | `libs/base/protos/message.proto`; `LoginRequest` (field numbers used: 1–17), `Misc` (union numbers used up to 38), `LoginResponse` |
| Device-side login handling | `src/server/connection.rs`: `Connection::on_message` → `LoginRequest` branch; `validate_password`, `send_login_error`, `try_start_cm`, `self.authorized = true` in the auth section; `AUTHED_CONNS` / `AuthedConn` near the bottom (`mod raii`) |
| Existing sequential HTTP poster on a connection | `Connection.tx_post_seq`, `post_seq_loop`, `post_audit_async`, `post_conn_audit` (Server-Pro audit; leave as is, model the event poster on it) |
| HTTP helpers | `src/common.rs`: `post_request`, `post_request_with_status` |
| Close a connection with a reason shown to the peer | `Connection::on_close`, `send_close_reason_no_retry`, `Misc::CloseReason` |
| Peer-side login builder | `src/client.rs`: `LoginRequest { … }` construction in the login path (`send_login`) |
| Peer-side handling of `Misc` from device | `src/client/io_loop.rs`: `Some(misc::Union::…)` match |
| IPC device ↔ CM | `src/ipc.rs`: `Data::Login { … }` and friends; Flutter `Client` model in `flutter/lib/models/server_model.dart` |
| CM window (device) | `flutter/lib/desktop/pages/server_page.dart` (`_time`, per-connection timer) |
| Remote view toolbar (peer) | `flutter/lib/desktop/widgets/remote_toolbar.dart`, `flutter/lib/desktop/pages/remote_page.dart` |
| Multi-session tabs (basis of the wall) | `flutter/lib/desktop/pages/remote_tab_page.dart` |
| Settings page | `flutter/lib/desktop/pages/desktop_setting_page.dart` (`SettingsTabKey`, `_Network`, `_About`) |
| Flutter ↔ Rust bridge | `src/flutter_ffi.rs` (+ `flutter/lib/models/platform_model.dart` bindings) |
| App name | `hbb_common::config::APP_NAME`, `crate::get_app_name()` in `src/common.rs`; Flutter `bind.mainGetAppNameSync()` |
| Translations | `src/lang/en.rs` (add English strings; other languages fall back) |
| Existing Rust tests in these files | `#[cfg(test)]` modules at the end of `src/server/connection.rs` and in `src/client.rs` |

Constraints to respect:

* `libs/hbb_common` is a git submodule shared with the server. **Do not change
  it.** Everything here fits in `libs/base` and `src/`.
* Follow `AGENTS.md`: no `unwrap()`/`expect()` in production code, options
  imported via `base::config::keys`.
* Keep everything behind the master switch; stock behaviour must be unchanged
  when it is off (this is a test requirement, not just a goal).

## 6. Configuration keys

All in `libs/base/src/config/keys.rs`, stored with `Config::set_option` (device
scope) unless noted. Names use the existing kebab-case convention.

| Key | Scope | Default | Meaning |
|---|---|---|---|
| `access-control` | device | `""` (off); `"Y"` = on | Master switch |
| `access-api-url` | device | `""` | Backend base URL, e.g. `https://access.example.com` |
| `access-api-key` | device | `""` | Device API key (sent as Bearer) |
| `access-token` | local (peer) | `""` | Personal token sent in every outgoing login |
| `access-heartbeat-secs` | device | `30` | Heartbeat interval |
| `access-cache-days` | device | `7` | Offline cache expiry |
| `access-countdown-show-secs` | local | `14400` | Show countdown when remaining ≤ this |
| `access-countdown-red-secs` | local | `1800` | Countdown turns red under this |
| `access-wall-max-tiles` | local | `6` | Monitor wall tile limit |

"Enabled" for the device path means `access-control == "Y"`. The peer path
sends `access-token` whenever it is non-empty, regardless of the switch, so
that a person can connect to protected devices even if their own machine is
not itself protected. (The switch gates *incoming* enforcement.)

## 7. Work packages

Do them in this order; each is independently shippable and testable. Each WP
ends with: automated tests passing, manual test steps executed by the user,
and a short plain-language summary of the change.

### WP0 — Branding: RustDesk-Velour ☑ (2026-09-14)

**Goal:** the user can tell this build from stock RustDesk at a glance, without
breaking anything that keys off the app name.

**Do not** change `APP_NAME` ("RustDesk"): it is used for config paths, IPC
socket names, service names and update checks. Changing it would make the fork
lose existing config and break platform integration.

Changes:
1. `src/common.rs`: add `pub const VELOUR_EDITION: &str = "Velour";` and
   `pub fn get_edition_name() -> String` returning `"RustDesk-Velour"`.
2. `src/flutter_ffi.rs`: expose `main_get_edition_name()`; regenerate bridge
   (`flutter_rust_bridge_codegen`, see `flutter/README.md`) or follow the
   pattern of neighbouring simple getters.
3. Flutter: show "RustDesk-Velour" in (a) the About card title
   (`_About` in `desktop_setting_page.dart`, replacing `About RustDesk` when
   the edition is Velour), (b) the main window title in `flutter/lib/main.dart`
   where `mainGetAppNameSync()` is used, (c) a small "Velour" tag in the home
   page header next to the ID card.
4. `src/lang/en.rs`: add `"About RustDesk-Velour"`.

Tests: Rust unit test for `get_edition_name()`. Manual: Settings → About and
the home-screen tag (verified by the user 2026-09-14). Note: on macOS the main
window has a custom tab bar rather than a native title, so the edition name
shows only on session windows there; the name beside the Apple menu is the
bundle name and intentionally stays "RustDesk".

### WP1 — Master switch, settings UI, config keys ☑ (2026-09-14; mobile settings page deferred)

**Goal:** all keys from §6 exist; a new "Access Control" section in Settings
edits them; nothing else changes behaviour yet.

Changes:
1. Add keys to `libs/base/src/config/keys.rs` (and to the relevant `OPTIONS_*`
   allow-lists in that file if the settings UI requires it — follow how
   `OPTION_APPROVE_MODE` is registered).
2. Rust helper module `src/access_control/mod.rs` with `config.rs`:
   `AcConfig::load()` returning a struct with all keys parsed, plus
   `is_enabled()`, `is_configured()` (url && key non-empty).
3. Flutter: new `SettingsTabKey.accessControl` and `_AccessControl` widget in
   `desktop_setting_page.dart` modelled on `_Network`: switch "Enable access
   control", text fields Backend URL, Device API key (obscured), Personal token
   (obscured), advanced: heartbeat seconds, cache days. Show an inline warning
   when the switch is on and URL/key are empty: "Access control is enabled but
   not configured; incoming connections will be refused."
4. Mobile: same fields on the mobile settings page (`flutter/lib/mobile/`),
   minimal layout.
5. `src/lang/en.rs`: strings.

Tests: Rust tests for `AcConfig` parsing (defaults, bad numbers fall back to
defaults, trimming of URL trailing slash, whitespace key). Manual (user,
2026-09-14): warning banner appears/disappears, inline red validation on the
number fields, masked secrets, values persist across restart.
Deferred: the mobile settings page (item 4) — not testable on the dev Mac.

### WP2 — Authorization on login, roles, offline cache ☑ (2026-09-14; two-machine tests pending)

**Goal:** protected devices consult the backend and enforce roles; unprotected
devices are untouched.

Changes:
1. `libs/base/protos/message.proto`: `LoginRequest` gets
   `string access_token = 18;` and `bool monitoring = 19;`.
   Regenerate protobuf code (`libs/base/build.rs` handles it on build).
2. `src/client.rs`: in the `LoginRequest { … }` builder set
   `access_token: LocalConfig::get_option(keys::OPTION_ACCESS_TOKEN)` and
   `monitoring` from a new session option (set by WP5; false otherwise).
3. `src/access_control/backend.rs`: trait `AccessBackend` with
   `async fn authorize(&self, req: AuthorizeRequest) -> AuthorizeOutcome`
   (`Allowed{role,user_id,display_name,remaining_seconds}`,
   `Denied{reason_code,reason}`, `UnknownPeer`, `Unreachable`) and
   `async fn post_event(&self, ev: Event) -> Result<EventResponse>`.
   `HttpBackend` implements it with `reqwest` (already a dependency; reuse the
   client construction pattern in `src/hbbs_http/http_client.rs`), a
   `ACCESS_HTTP_TIMEOUT_SECS = 10` named constant used as the timeout by both
   `authorize` and `post_event` so it can be tuned in one place, Bearer header,
   and `"app": ACCESS_APP_NAME` in every body. A `MockBackend` (behind `#[cfg(test)]`) records calls and
   returns scripted outcomes.
4. `src/access_control/cache.rs`: the offline cache (§3.2), pure functions
   over a `Vec<CacheEntry>` + load/save; sha256 via the `sha2` crate (already
   in the tree via dependencies; otherwise add).
5. `src/access_control/policy.rs`: pure function
   `check_local_rules(role, connected: &[ConnectedPeer]) -> Result<(), DenyReason>`
   implementing the one-User rule.
6. `src/server/connection.rs`:
   * `Connection` gets `ac: Option<AcSession>` (role, user_id, display_name,
     remaining, session_id, monitoring, offline_authorized).
   * `AuthedConn` gets `role: Option<Role>` and `monitoring: bool` so
     `check_local_rules` can read live connections.
   * Insert steps a–d from §3.1 in the `LoginRequest` branch, before password
     validation. Extract into `async fn velour_authorize(&mut self, lr) -> bool`
     to keep the branch readable.
   * `try_start_cm` / `ipc::Data::Login` gain `display_name` and `role` so the
     CM can show them. (Moved to WP4, which reworks the same CM plumbing.)
7. Enforce view-only for `monitoring` connections (reuse the existing
   view-only permission plumbing; the device must ignore input from such a
   connection even if the peer asks otherwise).
8. Denial messages via `send_login_error`; every denial also emits
   `auth_denied` (best-effort, no retry) when WP3 exists — in WP2, log only.

Tests (Rust, `MockBackend`, no network):
* AC off → backend never called; login proceeds as stock (regression guard).
* AC on, not configured → denied with the "not configured" message.
* No token → denied.
* Allowed admin/manager/user → role stored; user + another user connected →
  denied; user + admin connected → allowed; monitoring conn does not block.
* Denied by backend → message is the backend's `reason`; cache entry removed.
* Unknown peer → registered-ID message.
* Unreachable + fresh cache + matching token → allowed with cached role,
  `offline_authorized = true`; wrong token → denied; expired → denied.
* Cache serialization round-trip.

Manual (with the mock backend script from §8): the full matrix above from two
machines, plus "stock RustDesk peer connects to protected device → refused
with the token message", and "Velour peer with token connects to stock
device → works".

Implementation notes (as built): the decision lives in
`access_control::flow::decide()` (pure; 10 tests with `MockBackend`);
`Connection::velour_authorize()` is a thin wrapper called after the
conn-type permission checks and before password validation; login fields are
captured in `VelourLoginFields` before `lr.union` is consumed. Monitoring
sessions drop mouse/pointer/key/clipboard/file messages and ignore CM
permission toggles. Single-machine smoke test (connect to own ID) passed for
wrong token, valid token, backend down + cache, no token, switch off — see
`TEST_LOG.md`.

### WP3 — Session events and heartbeat ☑ (2026-09-15)

**Goal:** the backend learns about every session, live, and can end one.

Changes:
1. `src/access_control/events.rs`: `Event` enum + envelope builder
   (spec §4.2), `EventSender` owning a `tokio::mpsc` queue per connection with
   retry/backoff for start/end and fire-and-forget for heartbeat/denied.
   `event_id` = `uuid::Uuid::new_v4()`.
2. `Connection`: on `self.authorized = true` with `ac.is_some()`, create the
   session record, send `session_start`, spawn the heartbeat task (interval
   from config). Heartbeat response: update `remaining` (+ timestamp);
   `continue:false` → `on_close(reason)` and `send_close_reason_no_retry`.
3. On close (all paths through `on_close`): send `session_end` with the mapped
   reason (`peer_disconnected`, `device_closed`, `network_lost`,
   `backend_stop`, `access_control_disabled`, `other`) and final elapsed.
4. Master switch turned off at runtime: iterate `AUTHED_CONNS`, send
   `session_end(access_control_disabled)`, drop heartbeat tasks. Hook where
   options are set (`set_option` path in `src/ui_interface.rs` /
   `flutter_ffi.rs`).
5. `auth_denied` from WP2 now actually posts.

Tests (Rust, `MockBackend` + `tokio::time::pause`):
* start → N heartbeats at the configured interval → end; envelope fields
  (including `app == "rustdesk"`) and ordering asserted; `elapsed_seconds`
  increases.
* `continue:false` closes the connection with the reason; `session_end` reason
  is `backend_stop`.
* Heartbeat failures do not close; start/end are retried with backoff and
  deduplicated by `event_id`.
* Switch off → `session_end(access_control_disabled)`.

Manual: mock backend prints events; watch a session start, heartbeat, and end;
flip "stop" in the mock and confirm the peer sees the reason.

Implementation notes (as built): `access_control::events::SessionReporter`
owns one tokio task per connection (start with retry, heartbeat loop, switch
polling); results reach the connection through a dedicated channel
(`tx_ac`/`rx_ac`) and a new `select!` arm, so `src/ipc.rs` is untouched.
Hooks in `connection.rs`: `velour_start_reporting()` after
`AuthedConnID::new`, `velour_on_heartbeat()` in the loop,
`velour_end_reporting()` first thing in `on_close()`. Backend stop reasons
are sent to the peer as "Access control: <reason>" and `check_if_retry` in
`src/client.rs` excludes that prefix, otherwise the peer auto-reconnects.
`session_start`/`session_end` retry 8 times with doubling backoff from 2 s
(≈4 min). 8 unit tests with paused tokio time; live self-connect test passed.

### WP4 — Countdown and elapsed time UI ☑ (2026-09-15; mobile overlay deferred)

**Goal:** the person connected and the person at the device can both see
time remaining, and are warned ahead of a cut-off.

Changes:
1. `libs/base/protos/message.proto`: `Misc` union gets
   `SessionTime session_time = 39;` with
   `message SessionTime { int64 elapsed_seconds = 1; int64 remaining_seconds = 2; bool has_remaining = 3; }`.
   Device sends it after every heartbeat response and once at session start.
2. `src/client/io_loop.rs`: handle `SessionTime` → push to Flutter via the
   existing session event channel (look at how `PermissionInfo` reaches
   Flutter; mirror it).
3. Flutter peer (`remote_page.dart` + a new `session_countdown.dart` widget):
   overlay in the top-right of the remote view; hidden until
   `remaining ≤ access-countdown-show-secs`; local 1 s countdown between
   updates; red under `access-countdown-red-secs`; milestone toasts.
   Show elapsed time in the toolbar's info area regardless.
4. Device CM: `ipc::Data` gets a `SessionTime { id, elapsed, remaining }`
   message; `Client` model gets `remaining`, `displayName`, `role`;
   `server_page.dart` shows them beside the existing timer.
5. Mobile: same countdown overlay on the mobile remote page.

Tests: Rust test that `SessionTime` is emitted after a heartbeat with the right
values; Flutter widget tests for the countdown widget (hidden above threshold,
shown below, red under threshold, milestone toast fires once per threshold).
Manual: mock backend returns decreasing `remaining_seconds`; verify overlay on
peer and CM on device; verify a clean stop at 0 with the reason.

Implementation notes (as built): proto `Misc.session_time = 39`
(`SessionTime{elapsed, has_remaining, remaining}`), built by
`access_control::session_time_misc()`; the device sends it after the login
response and after every heartbeat, plus `ipc::Data::VelourSession` to the
CM. Peer: `InvokeUiSession::session_time` (default no-op) → Flutter event
`session_time` → `SessionTimeModel` (pure Dart, 6 tests: thresholds, local
ticking, resync, one warning per report, formatting) → `SessionCountdown`
badge top-right of the remote view. CM: `InvokeUiCM::velour_session`
(default no-op) → `velour_session` event → `Client.velour*` → name·role line
and "· Time left" beside the elapsed timer. Milestone toasts use
`access-time-left-tip`. Deferred: the mobile remote-page overlay (item 5).

### WP5 — Admin monitor wall ☑ code complete (2026-09-15; live test needs a second machine)

**Goal:** an admin sees up to 6 live sessions at once and can expand one.

Changes:
1. `src/access_control/backend.rs`: `list_active_sessions(token)` →
   `GET /v1/sessions/active`.
2. `src/flutter_ffi.rs`: expose it; expose a way to open a remote session with
   `monitoring = true` and view-only forced (a session option consumed by the
   `LoginRequest` builder from WP2).
3. Flutter: new window/page `flutter/lib/desktop/pages/monitor_wall_page.dart`:
   left list of active sessions (device name, person, role, elapsed,
   remaining) with "Watch"; grid of tiles (2×3 max) each hosting a `RemotePage`
   in view-only; click to expand; close returns to grid. Entry point: a button
   on the home page visible only when a personal token is saved.
4. Enforce the tile limit (`access-wall-max-tiles`) with the hint text.

Tests: Flutter tests for the limit logic and list rendering with fixture data;
Rust test for the list call parsing. Manual: three peers connected to three
devices; admin opens the wall, watches all three, tries a seventh with six
open.

Note: the wall does not exist on mobile.

Implementation notes (as built): `MonitorWallPage` is a tab in the main
window (`DesktopTabPage.onAddMonitorWall`, opened from a grid icon beside the
Velour tag that appears only when a personal token is saved). Left: the
backend's `/v1/sessions/active` via `main_velour_active_sessions()` (blocking
reqwest on the FFI pool, admin token as Bearer). Right: up to
`access-wall-max-tiles` `RemotePage` widgets with `monitoring: true`, which
`FFI.start` applies through `session_set_monitoring_sync` right after
`session_add_sync`; the device forces view-only. `WallTiles` (limit, dedup,
grid columns) and `ActiveSession.parse` are pure Dart with 3 tests.
Single-machine limitation: RustDesk keeps one connection per peer per
process (`sessions::insert_session` → `or_insert`), so a tile for a device
this same app already has open joins that session instead of logging in as
monitoring. The live test therefore needs the admin on a second machine.

### WP6 — Own rendezvous/relay server ☐ (deferred)

Change `RENDEZVOUS_SERVERS` and `RS_PUB_KEY` in
`libs/hbb_common/src/config.rs`, **or** (preferred, avoids the submodule) set
them via the existing custom-server mechanisms (`custom-rendezvous-server`
option / `RENDEZVOUS_SERVER` env). Not scheduled.

## 8. Test infrastructure

* **Rust:** `cargo test -p rustdesk access_control` for the module;
  `cargo test` for regression. Tests use `MockBackend` and paused tokio time;
  no network, no files outside a temp dir.
* **Flutter:** `flutter test` in `flutter/` for widget tests.
* **Mock backend for manual tests:** add `docs/access-control/mock_backend.py`
  (stdlib-only `http.server`, ~150 lines) implementing the spec with an
  in-memory table loaded from `mock_users.json`, printing every event, and
  exposing `POST /mock/stop/<session_id>` and `POST /mock/remaining/<peer_id>/<secs>`
  to script cut-off and countdown scenarios. Written as part of WP2.
* **Manual test matrix:** each WP lists its steps; keep results in
  `docs/access-control/TEST_LOG.md` (date, build, scenario, pass/fail).

## 9. Non-goals and known limitations

* Identity is per installation, not per human: two people sharing one peer
  machine share a token and a role.
* No cross-device enforcement inside RustDesk; that is the backend's job.
* Termination and countdown changes reach the peer within one heartbeat
  (30 s), never instantly.
* A connection attempt may take up to 10 seconds to be refused when the
  backend is slow or down, before the offline cache is consulted.
* Events are at-least-once and may be reordered; the backend dedups.
* The web client (`flutter/web`) is out of scope.
* Sciter (legacy) UI is out of scope.

## 10. How to resume this work later

1. Read this file, then `EXTERNAL_SYSTEM_SPEC.md`.
2. Check the status boxes in §7 and `git log` on the `velour` branch.
3. Continue with the first WP not marked ☑, following its Changes/Tests
   lists. Update the status boxes and `TEST_LOG.md` as you go.
