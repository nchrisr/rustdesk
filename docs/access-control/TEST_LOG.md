# RustDesk-Velour manual test log

| Date | Build | WP | Scenario | Result |
|---|---|---|---|---|
| 2026-09-14 | 14baa66e0 | WP0 | Velour tag on home screen; About page title + Edition row | pass (user) |
| 2026-09-14 | 14baa66e0 | WP0 | Main window title on macOS | n/a — custom tab bar, no native title |
| 2026-09-14 | 7b59c2978 | WP1 | Warning banner when enabled without URL/key; disappears once both applied | pass (user) |
| 2026-09-14 | 7b59c2978 | WP1 | Inline red validation on heartbeat < 5; Apply disabled | pass (user) |
| 2026-09-14 | 7b59c2978 | WP1 | Secrets masked; values persist across restart | pass (user) |
| 2026-09-14 | WP2c (pre-commit) | WP2 | Self-connect, wrong token → backend `invalid_token`, peer refused | pass (device log + mock log) |
| 2026-09-14 | WP2c (pre-commit) | WP2 | Self-connect, valid token → allowed as admin; cache file written | pass |
| 2026-09-14 | WP2c (pre-commit) | WP2 | Backend 503, cached approval, same token → allowed `[offline cache]` | pass |
| 2026-09-14 | WP2c (pre-commit) | WP2 | No token → denied `no_token`, backend not called | pass |
| 2026-09-14 | WP2c (pre-commit) | WP2 | Switch off → no access-control activity, backend not called | pass |
| 2026-09-15 | 2bdee5b22+ | WP2 | Self-connect via UI with `tok-admin` after mock restart → allowed, password prompt | pass (user); screen image blank — Screen Recording permission not granted to the debug app, unrelated |
| 2026-09-15 | 2bdee5b22+ | WP2 | Stale mock (users file edited after start) gave `invalid_token` | root cause found; mock now reloads the file on change |
| 2026-09-15 | WP3 (pre-commit) | WP3 | Self-connect, heartbeat 5 s: `session_start`, heartbeats with elapsed 5/10/15…, `continue:true` | pass (mock log) |
| 2026-09-15 | WP3 (pre-commit) | WP3 | `POST /mock/stop/<sid>` → next heartbeat `continue:false` → device closes, `session_end` reason `backend_stop`, peer shown "Access control: …" | pass |
| 2026-09-15 | WP3 (pre-commit) | WP3 | Peer must not auto-reconnect after a backend stop (found reconnect; fixed via reason prefix + `check_if_retry`) | pass after fix |
| 2026-09-15 | WP4 (pre-commit) | WP4 | Mock reports 3 h 59 m left → badge "Time left" on remote view, name·role and "· Time left" in CM | pass (user) |
| 2026-09-15 | WP4 (pre-commit) | WP4 | Remaining set to 9 min → badge red, milestone toast | pass (user) |
| 2026-09-15 | WP5 (pre-commit) | WP5 | Monitor tab opens, lists the backend's active session, Watch adds a tile | pass (user) |
| 2026-09-15 | WP5 (pre-commit) | WP5 | Tile logs in as `monitoring: true` | not testable alone — same app already held a session to that peer, tile joined it (see plan) |
| — | — | WP5 | Wall from a second (admin) machine: monitoring flag, view-only, 6-tile limit | pending |
| — | — | WP2 | Two-machine matrix (user/manager/user-blocked, stock peer, stock device) | pending — user will test from a Windows machine |

Single-machine method: run `python3 -u docs/access-control/mock_backend.py --port 8787 --users <file>`
with the Mac's own ID registered; launch the debug app with `RUST_LOG=info` and
stderr captured; `RustDesk --connect <own id>`; restart the app between
attempts (a second connect to the same ID only focuses the open tab).
