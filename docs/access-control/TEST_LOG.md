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
| — | — | WP2 | Two-machine matrix (user/manager/user-blocked, stock peer, stock device) | pending |

Single-machine method: run `python3 -u docs/access-control/mock_backend.py --port 8787 --users <file>`
with the Mac's own ID registered; launch the debug app with `RUST_LOG=info` and
stderr captured; `RustDesk --connect <own id>`; restart the app between
attempts (a second connect to the same ID only focuses the open tab).
