# RustDesk_Velour — release retest

Goal: prove the shipped files work the same as the development builds. Same
backend and credentials as before (Mac `436072673` at `192.168.2.40`, key
`velour-dev-key`, Alice `alice-token` on the Mac, Uma `uma-token` on the PC).

## 1. Install the deliverables

### Mac
1. Quit any running RustDesk (Cmd+Q) — including the debug one.
2. Unzip `dist/RustDesk_Velour-macos-arm64.zip` and drag
   `RustDesk_Velour.app` into **Applications**.
3. Open it: right-click → **Open** → **Open** (unsigned build; once only).
4. Settings → About shows **About RustDesk-Velour**.
5. System Settings → Privacy & Security: enable **RustDesk_Velour** under
   **Screen Recording**, **Accessibility** and **Input Monitoring** (it is a
   new app to macOS, so grant all three again). Quit and reopen the app.
6. Settings → Security → Permissions → untick **Enable audio**.
7. Settings → Access Control: Backend URL `http://127.0.0.1:8787`, Device API
   key `velour-dev-key`, Personal token `alice-token`, tick **Enable access
   control**. (Settings carry over from the debug app — same config folder —
   so most of this may already be filled in; check each value.)

### Windows PC
1. Close the old RustDesk-Velour if it is running.
2. Unzip `RustDesk_Velour-windows-x64.zip`; you get a folder
   `RustDesk_Velour`. Put it at `C:\RustDesk_Velour\`. The old `C:\Velour\`
   can be deleted.
3. Run `rustdesk.exe` → More info → Run anyway.
4. Settings → About shows **About RustDesk-Velour**. The ID is the same as
   before (settings live in your Windows profile, not in the folder).
5. Settings → Access Control → Personal token `uma-token` (check it is
   still there).

## 2. Backend

Terminal 1:
```
cd ~/My-Projects/velour/RustDesk/rustdesk/docs/access-control
python3 test_backend.py reset-usage Uma
python3 test_backend.py clear-quota Uma
python3 test_backend.py quota Uma --period day --hours 0.05 --device 436072673
python3 test_backend.py serve
```
(No Terminal 2 this time: the release app logs to
`~/Library/Logs/RustDesk/RustDesk_rCURRENT.log`; `tail -f` it if needed.)

## 3. Tests

### Test 1 — Windows → Mac with countdown and cut-off
1. PC: connect to `436072673`, enter the Mac's password.
2. Expected: screen visible **and controllable**; no audio; red "Time left
   00:02:5x" badge; Mac CM shows "Uma · user". Terminal 1 shows the authorize
   with `remaining_seconds: 180`.
3. Wait for zero. Expected: "Access control: Your time allowance for this
   device has been reached."; no auto-reconnect.
4. Connect again. Expected: "Your time allowance for this device is used up."

### Test 2 — Denials
1. PC token `nonsense` → connect. Expected: "Invalid access token."
2. PC token empty → connect. Expected: "This device requires an access token."
3. PC token `uma-token`. Terminal 1: Ctrl+C, `reset-usage Uma`,
   `clear-quota Uma`, `quota Uma --period day --hours 2 --device 436072673`,
   `serve`.

### Test 3 — Admin end + offline cache
1. PC: connect (allowed, Time left 01:59:xx). Dashboard → End session.
   Expected: "Access control: Session ended by administrator." within 30 s.
2. Terminal 1: Ctrl+C. PC: connect. Expected: allowed; badge shows Elapsed
   (offline cache). Disconnect. Terminal 1: `serve`. Connect: Time left back.

### Test 4 — Mac → Windows (PC as protected device)
1. PC: Settings → Access Control → Backend URL `http://192.168.2.40:8787`,
   key `velour-dev-key`, tick Enable access control; permanent password set.
2. Mac: connect to the PC's ID. Expected: allowed as admin (Alice),
   controllable. Disconnect.
3. PC: untick Enable access control afterwards if it should not stay protected.

### Test 5 — Monitor tab
1. PC connected to the Mac; Mac: grid icon → Monitor. Expected: the session
   is listed. (Watch needs a third machine.)

### Test 6 — Master switch
1. Mac: untick Enable access control. PC: connect. Expected: password prompt
   only; Terminal 1 silent. Tick it back on.

Record results in TEST_LOG.md.
