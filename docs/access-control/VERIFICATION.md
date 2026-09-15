# RustDesk-Velour — verification run

Setup assumed (from WINDOWS_TEST_GUIDE.md): Mac `436072673` at
`192.168.2.40`, backend on port 8787 with key `velour-dev-key`, Alice (admin,
Mac, token `alice-token`), Uma (user, PC, token `uma-token`, assigned to the
Mac). Terminal 1 runs `serve`; Terminal 2 tails `~/velour-mac.log`
(launched via `open`, see WINDOWS_TEST_GUIDE.md Part 4). Uma currently has a
2 h/day quota on the Mac.

Fill the Result column as you go (pass / fail + what you saw).

## Before starting

```sh
cd ~/My-Projects/velour/RustDesk/rustdesk/docs/access-control
python3 test_backend.py reset-usage Uma
python3 test_backend.py list          # confirm users, devices, assignment, quota
```
Mac: Settings → Access Control → Enable access control **ticked**.
PC: Personal token `uma-token`.

## A. Countdown, warnings, cut-off (WP3/WP4)

| Step | Do | Expect | Result |
|---|---|---|---|
| A1 | Terminal 1: stop `serve` (Ctrl+C). Run `python3 test_backend.py clear-quota Uma` then `python3 test_backend.py quota Uma --period day --hours 0.05 --device 436072673` (= 3 min). Start `serve` again. | Prints `Uma: 0.05 h per day on 436072673`. | |
| A2 | PC: connect to `436072673`, enter password. | Terminal 1: `allowed … "remaining_seconds": 180`. PC: badge **Time left 00:02:5x** top-right, **red** (under 30 min). Mac CM window: "Uma · user" and "· Time left 00:02:xx". | |
| A3 | Wait. | Toast "Time left in this session: 00:01:xx" around the 1-minute mark. Badge counts down every second between heartbeats. | |
| A4 | Wait until zero. | Session closes; PC shows "Access control: Your time allowance for this device has been reached." Terminal 1: heartbeat answered `continue: false`, then `session_end … backend_stop`. PC does **not** reconnect on its own. | |
| A5 | PC: connect again. | Refused: "Your time allowance for this device is used up." Terminal 1: `quota_exhausted`. | |

## B. Denials (WP2)

| Step | Do | Expect | Result |
|---|---|---|---|
| B1 | PC: Settings → Access Control → Personal token `nonsense` → Apply. Connect. | "Invalid access token." Terminal 1: `invalid_token`. Dashboard `http://192.168.2.40:8787/` → Denied attempts row, decided by **backend**. | |
| B2 | PC: clear the token → Apply. Connect. | "This device requires an access token." Terminal 1: **no** authorize line, only an `auth_denied` event. Dashboard row decided by **device**. | |
| B3 | PC: token `uma-token` → Apply. On the Mac, Terminal 1: stop `serve`; run `python3 test_backend.py reset-usage Uma`, `clear-quota Uma`, `quota Uma --period day --hours 2 --device 436072673`; start `serve`. Connect. | Allowed, remaining 7200 (Time left 01:59:xx). Leave connected for B4. | |

## C. Backend end-session and offline cache (WP3/WP2)

| Step | Do | Expect | Result |
|---|---|---|---|
| C1 | Dashboard → Uma's row → **End session**. | Within 30 s the PC shows "Access control: Session ended by administrator." Terminal 1: `continue: false` then `session_end … backend_stop`. | |
| C2 | Terminal 1: stop `serve` (Ctrl+C). PC: connect. | Allowed anyway. Terminal 2: `allowed peer … as user (Uma) [offline cache]`. No Time left badge (unknown while offline); Elapsed shows instead. | |
| C3 | Disconnect. Terminal 1: start `serve`. PC: connect. | Allowed, normal (Terminal 1 shows the authorize; countdown back). Disconnect. | |

## D. Master switch (WP1)

| Step | Do | Expect | Result |
|---|---|---|---|
| D1 | Mac: untick **Enable access control**. PC: connect. | Password prompt straight away; Terminal 1 sees nothing; Terminal 2 has no access-control line. Disconnect. | |
| D2 | Mac: tick it back on. | — | |

## E. Monitor wall (WP5) — needs the PC as a protected device

| Step | Do | Expect | Result |
|---|---|---|---|
| E1 | PC: Settings → Access Control → Backend URL `http://192.168.2.40:8787`, Device API key `velour-dev-key` (Apply each), tick **Enable access control**. Settings → Security: make sure a permanent password is set. Note: on Windows nothing else is needed. | Dashboard reachable from the PC's browser (it already was). | |
| E2 | Mac: home screen → connect to `<PC_ID>` with the PC's password. | Terminal 1: `allowed … role admin` (Alice). The PC's screen appears on the Mac; you can control it. Disconnect (close the window). | |
| E3 | PC: connect to the Mac (Uma → Mac) and leave it open. Mac: click the **grid icon** beside the Velour tag → **Monitor** tab. | List shows "Mac — Uma · user" with elapsed/time left. | |
| E4 | This is the limitation: Watch on that row targets the Mac, which this app *is*. Instead test the wall the other way: on the **PC**, add Alice's token? No — Alice's ID is the Mac. So: give the Mac's Monitor a target it is not already connected to: from the Mac, Watch is only meaningful for a device other than itself. Skip Watch here unless a third machine holds a session to the PC; record "not testable with two machines". | — | |

(If you do have a third machine: register it as a user, connect it to the PC,
then on the Mac Monitor → Watch the PC. Expect Terminal 1 to show the tile's
authorize with `"monitoring": true`, a view-only tile, and "1 / 6 tiles".)

## F. Wrap-up

| Step | Do | Expect | Result |
|---|---|---|---|
| F1 | PC: untick Enable access control (if set in E1). | — | |
| F2 | Copy this table's results into TEST_LOG.md, or tell Claude the results. | — | |
