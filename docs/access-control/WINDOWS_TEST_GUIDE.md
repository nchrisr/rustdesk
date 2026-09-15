# Testing RustDesk-Velour with a Windows machine

Two machines on the same Wi-Fi: this **Mac** (runs the test backend, and is a
RustDesk-Velour device/peer) and a **Windows PC** (RustDesk-Velour installed
from a flash drive). Everything below was written for that setup.

## Part 1 — Get a Windows build (no Windows toolchain needed)

The fork's GitHub Actions build the Windows app on GitHub's own machines.

1. Push the branch to your fork (once, then again after any change):
   `git push -u origin rustdesk_velour`
2. On github.com open your fork → **Actions** → in the left list pick
   **Velour Windows build** → **Run workflow** → branch `rustdesk_velour` →
   **Run workflow**.
   - First run: about 40–60 minutes (native libraries are compiled and then
     cached). Later runs are faster.
   - If Actions are disabled on the fork, enable them on the Actions tab first.
3. When the run is green, open it and download the artifact
   **rustdesk-velour-windows-x64** (a zip).
4. Copy the zip to the flash drive.

The zip contains a folder with `rustdesk.exe` plus its DLLs. It is not
code-signed (that needs a paid certificate), so Windows will warn once — see
below.

## Part 2 — Install on the Windows PC

1. Copy the zip from the flash drive to e.g. `C:\Velour\` and extract it there.
2. Double-click `rustdesk.exe`.
   - **SmartScreen** ("Windows protected your PC"): click **More info** →
     **Run anyway**. This is only because the build is unsigned.
3. The app opens in *portable* mode. For a realistic test (so the PC can be
   connected **to** while nobody is logged in, and so it survives reboots),
   click **Install** on the banner at the top of the main window (or the
   Install button in Settings) and accept the UAC prompt.
4. Confirm it is the right build: Settings → About should say
   **About RustDesk-Velour** with an **Edition** line.
5. Note the PC's **ID** (home screen) — you will register it in the backend.
6. Set a **permanent password** on the PC: Settings → Security → Password.
   The token is checked first, then this password.

## Part 3 — Run the test backend on the Mac and expose it on the Wi-Fi

```sh
cd ~/My-Projects/velour/RustDesk/rustdesk/docs/access-control
python3 test_backend.py init
python3 test_backend.py add-key                           # prints a device API key — keep it
python3 test_backend.py add-device <MAC_ID> --name Mac
python3 test_backend.py add-device <PC_ID>  --name WindowsPC
python3 test_backend.py add-user Alice --role admin   --remote-id <MAC_ID>     # prints Alice's token
python3 test_backend.py add-user Uma   --role user    --remote-id <PC_ID>      # prints Uma's token
python3 test_backend.py assign Uma <MAC_ID>
python3 test_backend.py quota  Uma --period day --hours 0.25 --device <MAC_ID>
python3 test_backend.py serve --port 8787
```

`serve` prints two URLs. The **"from other machines"** one
(`http://192.168.x.x:8787`) is the Backend URL for any device that is not the
Mac itself. Keep this terminal open; it prints every decision.

- The first time, macOS asks whether Python may accept incoming connections
  — click **Allow**. If the PC still cannot reach the Mac, check System
  Settings → Network → Firewall.
- Quick check from the PC's browser: `http://192.168.x.x:8787/` should show
  the dashboard.
- Each user's remote ID must be the ID of the machine they connect **from**.
  Above: Alice connects from the Mac, Uma from the PC. Swap the IDs if you
  want the PC to be the admin.

## Part 4 — Configure the two apps

**Mac** (device Uma will connect to, and Alice's admin peer):
Settings → Access Control → Backend URL `http://127.0.0.1:8787`, Device API
key = the `add-key` output, Personal token = Alice's token, tick **Enable
access control**.

**Windows PC** (Uma's peer; optionally also a device):
Settings → Access Control → Personal token = Uma's token. If you also want the
PC to be protected: Backend URL `http://192.168.x.x:8787`, the same Device API
key, tick **Enable access control**.

Grant the Mac's RustDesk-Velour app **Screen Recording** in System Settings →
Privacy & Security, or sessions to the Mac show "Connecting…" forever.

## Part 5 — Scenarios

Watch the backend terminal (or the dashboard) after each step.

| # | On the PC (Uma) | Expected |
|---|---|---|
| 1 | Connect to the Mac's ID with the Mac's password | Backend: `allowed … role user … remaining_seconds 900`. Session opens; a **Time left 00:14:xx** badge (red under 30 min) appears top-right; the Mac's connection-manager window shows "Uma · user" and the time left. |
| 2 | Stay connected 15 minutes (or `quota` with `--hours 0.02` ≈ 72 s) | Badge counts down; toasts at 10, 5, 1 min; at zero the session closes with "Access control: Your time allowance for this device has been reached." — and it does **not** reconnect by itself. |
| 3 | Connect again | Refused: "Your time allowance for this device is used up." |
| 4 | Change the token to nonsense, connect | Refused: "Invalid access token." Denial shows on the dashboard. |
| 5 | Clear the token, connect | Refused: "This device requires an access token." Dashboard shows a denial *decided by device*. |
| 6 | Stop the backend (Ctrl+C), restore Uma's token, connect | Allowed from the Mac's offline cache (backend log has nothing; Mac log says `[offline cache]`). Restart the backend afterwards. |
| 7 | With Uma connected, from the Mac open the **Monitor** tab (grid icon beside the Velour tag) | Uma's session is listed. Click **Watch**: a view-only tile of the Mac's own screen opens *only if* the Mac is not the device being watched — for a true wall test make the **PC** a protected device and have a third party (or the Mac itself, as Alice) connect to it, then watch from the other machine. Backend shows the tile's authorize with `monitoring: true`. |
| 8 | Dashboard → **End session** on Uma's row | Within 30 s Uma's session closes with "Access control: Session ended by administrator." |
| 9 | On the Mac untick **Enable access control**; connect from the PC | Password prompt straight away; backend sees nothing; a `session_end` with reason `access_control_disabled` arrives for any session that was open. |

Two-User rule (needs two non-admin peers): give a third machine a `user`
token, connect both to the same device — the second is refused with "Another
user is currently connected to this device." while an admin/manager is not.

Record results in `TEST_LOG.md`.

## Rebuilding after code changes

Push the branch, run **Velour Windows build** again, copy the new zip over
the old folder on the PC (close RustDesk first; if installed, run the new
`rustdesk.exe` and click Install again to update).
