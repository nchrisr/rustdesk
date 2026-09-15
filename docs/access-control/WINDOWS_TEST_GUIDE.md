# Testing RustDesk-Velour with a Windows machine

Two machines on the same Wi-Fi:

* **Mac** — runs the test backend and the Mac build of RustDesk-Velour. It is
  the *device* being connected to, and Alice's (admin) *peer*.
  RustDesk ID `436072673`, LAN address `192.168.2.40` (check with
  `ipconfig getifaddr en0` if the Wi-Fi changes).
* **Windows PC** — runs RustDesk-Velour in portable mode (no install). It is
  Uma's (user) *peer*. Its ID is `<PC_ID>` below.

Fixed test credentials (chosen so they can be retyped later):
device API key `velour-dev-key`, Alice's token `alice-token`, Uma's token
`uma-token`. Backend on port 8787.

## Part 1 — Get a Windows build (GitHub builds it)

1. Push the branch: `git push origin rustdesk_velour`.
   The workflow file must also exist on the fork's default branch (`master`)
   or GitHub will not list it — that was done once on 2026-09-15.
2. github.com → your fork → **Actions** → **Velour Windows build** →
   **Run workflow** → branch **rustdesk_velour** → **Run workflow**.
   First run ~45–60 min; later runs are faster (cached).
3. When green: open the run → **Artifacts** → download
   **rustdesk-velour-windows-x64** (a zip). Download it on the PC directly,
   or via a flash drive.

## Part 2 — Windows PC: set up the app (portable, no install)

1. Extract the zip to `C:\Velour\`.
2. Run `rustdesk.exe`. SmartScreen ("Windows protected your PC"):
   **More info → Run anyway** (the build is unsigned).
3. Do **not** click Install. Portable mode is enough: the PC only connects
   out, and can still accept connections while the window is open.
   (Install only if the PC must be reachable with the app closed or after a
   reboot.)
4. Settings → About must read **About RustDesk-Velour**. If it reads only
   "About RustDesk", the build is from the wrong branch — rebuild from
   `rustdesk_velour`.
5. Note the PC's **ID** from the home screen → `<PC_ID>`.
6. Settings → Security → set a permanent **password** and note it (needed
   only when someone connects *to* the PC).
7. Settings → Access Control → Personal token `uma-token` → **Apply**.
   Leave Backend URL, key and the switch alone for now.

## Part 3 — Mac, Terminal window 1: the backend

```sh
cd ~/My-Projects/velour/RustDesk/rustdesk/docs/access-control
python3 test_backend.py init                                   # first time only
python3 test_backend.py add-key --key velour-dev-key
python3 test_backend.py add-device 436072673 --name Mac
python3 test_backend.py add-user Alice --role admin --remote-id 436072673 --token alice-token
python3 test_backend.py add-device <PC_ID> --name WindowsPC
python3 test_backend.py add-user Uma --role user --remote-id <PC_ID> --token uma-token
python3 test_backend.py assign Uma 436072673
python3 test_backend.py quota Uma --period day --hours 0.25 --device 436072673
python3 test_backend.py serve
```

Re-running `add-key`/`add-user`/`add-device` is safe (they replace). `serve`
stays in the foreground and prints every request; leave the window open.
If macOS asks whether Python may accept incoming connections, **Allow**.

Check from the PC's browser: `http://192.168.2.40:8787/` shows the dashboard
with Alice and Uma. If not: System Settings → Network → Firewall → Options →
allow Python. `python3 test_backend.py list` shows the database at any time.

## Part 4 — Mac, Terminal window 2: RustDesk-Velour with its log visible

Build first if the code changed (see BUILD_MACOS.md), then:

```sh
cd ~/My-Projects/velour/RustDesk/rustdesk
RUST_LOG=info flutter/build/macos/Build/Products/Debug/RustDesk.app/Contents/MacOS/RustDesk 2>&1 \
  | grep --line-buffered -i "access control\|Connection closed\|velour"
```

The app window opens; this terminal shows only the access-control lines
(drop the `| grep …` for everything). Quit with Cmd+Q.

First time: grant **Screen Recording** (System Settings → Privacy & Security
→ Screen Recording → RustDesk), then quit and rerun the command. Without it,
sessions to the Mac show "Connecting…" forever.

## Part 5 — Mac: RustDesk-Velour settings

Settings → Access Control:
* Backend URL `http://127.0.0.1:8787` → Apply
* Device API key `velour-dev-key` → Apply
* Personal token `alice-token` → Apply
* tick **Enable access control**

## Part 6 — Scenarios

Watch Terminal 1 (backend), Terminal 2 (Mac app), the PC's remote window, and
the Mac's small connection-manager window.

| # | On the PC (Uma) | Expected |
|---|---|---|
| 1 | Connect to `436072673` with the Mac's password | Terminal 1: `allowed … role user … remaining_seconds 900`, then `session_start`, a `heartbeat` every 30 s. Terminal 2: `allowed peer <PC_ID> as user (Uma)`. PC: the Mac's screen with **Time left 00:14:xx** top-right (red under 30 min). Mac CM window: **Uma · user** and "· Time left". |
| 2 | Stay connected until the time runs out (to shorten: stop `serve`, run `quota Uma --period day --hours 0.02 --device 436072673`, start `serve` again) | Badge counts down; toasts at 10, 5, 1 min; at zero the session closes with "Access control: Your time allowance for this device has been reached." and does **not** reconnect. Terminal 1: `continue: false`, then `session_end … backend_stop`. |
| 3 | Connect again | Refused: "Your time allowance for this device is used up." (`quota_exhausted` in Terminal 1). |
| 4 | Personal token → `nonsense`, Apply, connect | Refused: "Invalid access token." Dashboard "Denied attempts" shows it, decided by backend. |
| 5 | Clear the token, Apply, connect | Refused: "This device requires an access token." Terminal 1 shows only an `auth_denied` event (no authorize) — decided by device. |
| 6 | Token back to `uma-token`. Reset her quota (stop `serve`; `sqlite3 velour_test.db "DELETE FROM quotas"` or use `--hours 1`; start `serve`). Connect once (allowed), disconnect. Stop `serve` (Ctrl+C). Connect again | Allowed from the Mac's offline cache: Terminal 2 says `[offline cache]`; Terminal 1 is not running. Start `serve` again afterwards. |
| 7 | With Uma connected: on the Mac click the **grid icon** beside the Velour tag → Monitor tab | Uma's session is listed (device, name, role, time). A real **Watch** test needs the *device* to be a machine the Mac is not already connected to — see the note below. |
| 8 | Dashboard → **End session** on Uma's row | Within 30 s her session closes with "Access control: Session ended by administrator." |
| 9 | Mac: untick **Enable access control**; PC connects | Password prompt straight away; Terminal 1 sees nothing (a `session_end … access_control_disabled` arrives for any session that was open). Tick it back on afterwards. |

**Monitor wall (Watch) test.** Make the PC a protected device: on the PC set
Backend URL `http://192.168.2.40:8787`, Device API key `velour-dev-key`, tick
Enable access control. Assign Alice nothing (admins reach everything). Connect
from the Mac to the PC normally first to confirm it works, disconnect, then
on the Mac open Monitor → Watch on a session someone else holds to the PC.
Terminal 1 shows the tile's authorize with `"monitoring": true`; the tile is
view-only (mouse/keyboard do nothing on the PC).

**Two-User rule** needs two non-admin peers on the same device; covered by
unit tests until a third machine is available.

Record results in `TEST_LOG.md`.

## Rebuilding after code changes

* Mac: `cargo build --features flutter --lib` then `flutter build macos
  --debug` (BUILD_MACOS.md), rerun the Part 4 command.
* PC: push, run **Velour Windows build**, download the new zip, close
  RustDesk on the PC, extract over `C:\Velour\`, run `rustdesk.exe` again.
  Settings and ID survive (they live in the Windows user profile).
