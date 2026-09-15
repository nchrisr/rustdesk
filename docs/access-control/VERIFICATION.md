# RustDesk-Velour — verification run (two machines)

Setup (see WINDOWS_TEST_GUIDE.md): Mac `436072673` at `192.168.2.40`,
backend on port 8787, device key `velour-dev-key`, Alice (admin, connects
from the Mac, token `alice-token`), Uma (user, connects from the PC, token
`uma-token`, assigned to the Mac). Terminal 1 runs `serve`; Terminal 2 tails
`~/velour-mac.log`.

## Before you start

1. In Terminal 1 press Ctrl+C to stop `serve`, then run:
   ```
   python3 test_backend.py reset-usage Uma
   python3 test_backend.py clear-quota Uma
   python3 test_backend.py quota Uma --period day --hours 0.05 --device 436072673
   python3 test_backend.py serve
   ```
   This gives Uma a 3-minute allowance so the cut-off happens quickly.
2. Mac: Settings → Access Control → **Enable access control** ticked.
3. PC: Personal token is `uma-token`.

## Test 1 — Countdown and automatic cut-off

1. PC: connect to `436072673`, enter the Mac's password.
2. Expected: Terminal 1 prints `allowed … "remaining_seconds": 180`. The PC
   shows a **red** "Time left 00:02:5x" badge top-right. The Mac's
   connection-manager window shows "Uma · user" and "· Time left".
3. Wait. Expected: near the 1-minute mark a toast "Time left in this
   session: 00:01:xx".
4. Wait until zero. Expected: the session closes; the PC shows "Access
   control: Your time allowance for this device has been reached." Terminal 1
   shows `continue: false` then `session_end`. The PC does not reconnect.
5. Connect again. Expected: "Your time allowance for this device is used up."

## Test 2 — Wrong token

1. PC: Settings → Access Control → Personal token `nonsense` → Apply.
2. Connect. Expected: "Invalid access token." Dashboard
   (`http://192.168.2.40:8787/`) → Denied attempts row, decided by backend.

## Test 3 — No token

1. PC: clear the Personal token → Apply.
2. Connect. Expected: "This device requires an access token." Terminal 1
   shows no authorize line, only an `auth_denied` event. Dashboard row says
   decided by device.

## Test 4 — Restore Uma

1. PC: Personal token `uma-token` → Apply.
2. Terminal 1: Ctrl+C, then
   ```
   python3 test_backend.py reset-usage Uma
   python3 test_backend.py clear-quota Uma
   python3 test_backend.py quota Uma --period day --hours 2 --device 436072673
   python3 test_backend.py serve
   ```
3. Connect. Expected: allowed, "Time left 01:59:xx". Stay connected.

## Test 5 — Admin ends the session from the dashboard

1. Dashboard → Uma's row → **End session**.
2. Expected: within 30 s the PC shows "Access control: Session ended by
   administrator." Terminal 1: `continue: false` then `session_end`.

## Test 6 — Backend down, offline cache

1. Terminal 1: Ctrl+C to stop the backend.
2. PC: connect. Expected: allowed anyway; Terminal 2 shows
   `allowed peer … as user (Uma) [offline cache]`; the badge shows "Elapsed"
   instead of "Time left".
3. Disconnect. Terminal 1: `python3 test_backend.py serve`.
4. Connect once more. Expected: allowed normally, countdown back. Disconnect.

## Test 7 — Master switch off

1. Mac: untick **Enable access control**.
2. PC: connect. Expected: password prompt immediately; Terminal 1 sees
   nothing; Terminal 2 has no access-control line. Disconnect.
3. Mac: tick **Enable access control** again.

## Test 8 — The PC as a protected device, admin connects in

1. PC: Settings → Access Control → Backend URL `http://192.168.2.40:8787`
   → Apply; Device API key `velour-dev-key` → Apply; tick **Enable access
   control**. Settings → Security: a permanent password must be set.
2. Mac home screen: connect to the PC's ID with the PC's password.
3. Expected: Terminal 1 shows `allowed … "role": "admin"` (Alice). The PC's
   screen appears on the Mac and can be controlled.
4. Disconnect.

## Test 9 — Monitor tab

1. PC: connect to the Mac and leave it open.
2. Mac: click the small **grid icon** beside the "Velour" tag on the home
   screen. A **Monitor** tab opens.
3. Expected: the list shows "Mac — Uma · user" with its time.
4. Do not click Watch: with two machines the only session to watch is on the
   Mac itself, which this app already holds. Record "Watch: needs a third
   machine". (With a third machine: connect it to the PC, then Mac → Monitor
   → Watch the PC; Terminal 1 shows the tile's authorize with
   `"monitoring": true`, the tile is view-only, footer says "1 / 6 tiles".)

## Wrap-up

1. PC: untick **Enable access control** if the PC should not stay protected.
2. Record results in TEST_LOG.md (or report them to Claude).
