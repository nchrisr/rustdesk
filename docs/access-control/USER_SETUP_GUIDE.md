# RustDesk-Velour — Setup guide for a new user

This is for someone receiving RustDesk-Velour for the first time. It covers
installing the app, giving it the permissions it needs, turning audio off,
and entering the access-control details you were given. No command line.

You will be given, by your administrator:

* the app (a zip for Windows, or an app for macOS),
* your **personal token** (a long string; keep it private),
* and, only if *this* machine will be connected **to** by others:
  the **backend URL** and a **device API key**.

## 1. Install

### Windows
1. Extract the zip to a folder you will keep, e.g. `C:\Velour\`.
2. Double-click `rustdesk.exe`.
3. Windows shows "Windows protected your PC" because the app is not signed by
   a store. Click **More info**, then **Run anyway**.
4. If this machine will be connected **to** by others, click **Install** in
   the banner at the top of the window and accept the prompt. This lets the
   app accept connections after a reboot or before you log in. If you only
   connect *out* to other machines, you can skip this and just run the app.

### macOS
1. Move `RustDesk.app` to your Applications folder and open it. If macOS says
   it "cannot be opened because the developer cannot be verified", right-click
   the app → **Open** → **Open** (once).
2. macOS will ask for several permissions over the first minutes. Grant the
   ones in step 2 below; decline Microphone.

### Check you have the right build
Settings (gear icon) → **About** must say **About RustDesk-Velour**, with an
**Edition: RustDesk-Velour** line. Your machine's **ID** is shown on the home
screen — give it to your administrator; they register it under your name.

## 2. macOS permissions (only on a Mac that will be connected TO)

Without these, someone connecting to the Mac sees "Connecting…" forever or
can see the screen but cannot click or type. Windows needs none of this.

System Settings → **Privacy & Security**:

| Permission | Why | Action |
|---|---|---|
| **Screen Recording** | lets the remote person see the screen | enable **RustDesk** |
| **Accessibility** | lets them move the mouse and click | enable **RustDesk** |
| **Input Monitoring** | lets them type | enable **RustDesk** |
| Microphone | voice calls only | leave **off** |
| Remote Desktop | Apple's own feature, unrelated | ignore |

If RustDesk is not in a list, click **+**, pick the app, and switch it on.
After changing any of these, **quit RustDesk (Cmd+Q) and open it again** —
grants only take effect on a fresh start.

If control still does not work after granting Accessibility: select the
RustDesk entry, click **−** to remove it, add it again with **+**, and
restart the app. macOS sometimes keeps a stale entry after an update.

## 3. Turn audio off (any machine that will be connected TO)

By default the remote person hears this machine's sound.
Settings → **Security** → **Permissions** → untick **Enable audio**.
This applies to all future sessions. (Clipboard, file transfer and the other
toggles on the same page work the same way.)

## 4. Access Control settings

Settings → **Access Control**.

**Everyone — the machine you connect FROM:**
* **Personal token**: paste the token you were given → **Apply**.
  The eye icon shows what you typed. Nothing else is needed to connect out.

**Only machines that will be connected TO** (your administrator will tell you):
* **Backend URL**: as given, e.g. `https://access.example.com` → Apply
* **Device API key**: as given → Apply
* tick **Enable access control**
* Leave Heartbeat interval and Offline cache expiry at their defaults.
* An orange warning under the checkbox means the URL or key is missing;
  incoming connections are refused until both are set.
* Also set a **permanent password** under Settings → Security; people
  connecting to this machine need your token approval *and* this password.

## 5. What to expect when connecting

* Enter the other machine's ID and its password as usual.
* If you are refused, the message says why — for example "This device
  requires an access token" (add your token in Settings), "Invalid access
  token" (ask your administrator for a new one), "Your RustDesk ID … is not
  registered" (your administrator must add this machine's ID to your user),
  "Another user is currently connected", or "Outside your allowed hours".
* If your time on a machine is limited, a **Time left** badge appears in the
  top-right of the remote window once under four hours, turns red near the
  end, and warns you at 4:00, 3:30 … 0:30, 10, 5 and 1 minute. When it reaches
  zero the session ends with a message; reconnecting is refused until your
  allowance renews.
* When someone connects to your machine, the small connection window shows
  their name, role and time left. You can end their session there.

## 6. Quick checklist

- [ ] App installed; About says RustDesk-Velour
- [ ] (Mac, connected-to) Screen Recording, Accessibility, Input Monitoring on; app restarted
- [ ] (connected-to) Settings → Security → Permissions → Enable audio **off**
- [ ] (connected-to) Backend URL, Device API key, Enable access control, permanent password
- [ ] Personal token entered
- [ ] Your ID sent to the administrator
