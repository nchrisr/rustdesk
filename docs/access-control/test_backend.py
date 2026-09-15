#!/usr/bin/env python3
"""RustDesk-Velour test backend: a small but complete implementation of
EXTERNAL_SYSTEM_SPEC.md on SQLite, for testing until the real system exists.
Standard library only. Runs on one machine and serves the whole LAN.

  python3 test_backend.py init                       create the database
  python3 test_backend.py add-key  [--key KEY]       device API key (printed once)
  python3 test_backend.py add-device ID --name NAME  a machine that can be connected TO
  python3 test_backend.py add-user NAME --role admin|manager|user --remote-id ID
                          [--token TOKEN] [--tz Area/City]   (token printed once)
  python3 test_backend.py assign USER DEVICE_ID      manager/user may reach that device
  python3 test_backend.py schedule USER --days mon-fri --from 09:00 --to 17:00 [--device ID]
  python3 test_backend.py quota USER --period day|week|month --hours 2 [--device ID]
  python3 test_backend.py list                       everything, at a glance
  python3 test_backend.py serve [--port 8787] [--db velour_test.db]

Dashboard: http://<this machine>:<port>/  (sessions, denials, "End session").
Users are matched by NAME (or id) in the commands above; IDs are the
remote-desktop IDs shown in RustDesk. App defaults to "rustdesk".
"""
import argparse
import datetime as dt
import hashlib
import hmac
import json
import os
import secrets
import socket
import sqlite3
import sys
import threading
import time
import zoneinfo
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

DB_PATH = os.environ.get("VELOUR_DB", "velour_test.db")
STALE_AFTER = 90     # seconds without a heartbeat -> stale
LOST_AFTER = 300     # seconds without a heartbeat -> ended (lost)
USERS_REQUIRE_ASSIGNMENT = True
DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]

SCHEMA = """
CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT UNIQUE, role TEXT,
  token_hash TEXT, timezone TEXT, active INTEGER DEFAULT 1);
CREATE TABLE IF NOT EXISTS user_remote_ids (user_id INTEGER, app TEXT, remote_id TEXT,
  UNIQUE(app, remote_id));
CREATE TABLE IF NOT EXISTS devices (id INTEGER PRIMARY KEY, app TEXT, remote_id TEXT, name TEXT,
  UNIQUE(app, remote_id));
CREATE TABLE IF NOT EXISTS device_api_keys (id INTEGER PRIMARY KEY, key_hash TEXT, device_id INTEGER,
  active INTEGER DEFAULT 1);
CREATE TABLE IF NOT EXISTS device_assignments (user_id INTEGER, device_id INTEGER, UNIQUE(user_id, device_id));
CREATE TABLE IF NOT EXISTS schedules (user_id INTEGER, device_id INTEGER, weekday INTEGER,
  start_time TEXT, end_time TEXT);
CREATE TABLE IF NOT EXISTS quotas (user_id INTEGER, device_id INTEGER, period TEXT, max_seconds INTEGER);
CREATE TABLE IF NOT EXISTS sessions (session_id TEXT PRIMARY KEY, app TEXT, user_id INTEGER,
  peer_remote_id TEXT, device_remote_id TEXT, device_name TEXT, role TEXT, conn_type TEXT,
  monitoring INTEGER, offline_authorized INTEGER, status TEXT, started_at TEXT,
  last_heartbeat_at TEXT, ended_at TEXT, end_reason TEXT, elapsed_seconds INTEGER DEFAULT 0,
  terminate_requested INTEGER DEFAULT 0);
CREATE TABLE IF NOT EXISTS processed_events (event_id TEXT PRIMARY KEY, received_at TEXT);
CREATE TABLE IF NOT EXISTS denials (id INTEGER PRIMARY KEY, ts TEXT, app TEXT, peer_remote_id TEXT,
  device_remote_id TEXT, reason_code TEXT, reason TEXT, source TEXT);
"""


# ----------------------------------------------------------------------------
# storage helpers
# ----------------------------------------------------------------------------
_lock = threading.Lock()


def db():
    conn = sqlite3.connect(DB_PATH, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def h(secret):
    return hashlib.sha256(secret.encode()).hexdigest()


def now():
    return dt.datetime.now(dt.timezone.utc)


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def parse_iso(s):
    return dt.datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)


def user_by_name(conn, name):
    r = conn.execute("SELECT * FROM users WHERE name=? OR id=?", (name, name)).fetchone()
    if not r:
        sys.exit(f"no such user: {name}")
    return r


def device_by_remote(conn, remote_id, app="rustdesk"):
    return conn.execute("SELECT * FROM devices WHERE app=? AND remote_id=?", (app, remote_id)).fetchone()


# ----------------------------------------------------------------------------
# business rules (spec §5)
# ----------------------------------------------------------------------------
def local_now(tz):
    try:
        return dt.datetime.now(zoneinfo.ZoneInfo(tz or "UTC"))
    except Exception:
        return dt.datetime.now(dt.timezone.utc)


def rows_for(conn, table, user_id, device_id):
    """Device-specific rows if any exist, else the user's default rows."""
    specific = conn.execute(f"SELECT * FROM {table} WHERE user_id=? AND device_id=?", (user_id, device_id)).fetchall()
    if specific:
        return specific
    return conn.execute(f"SELECT * FROM {table} WHERE user_id=? AND device_id IS NULL", (user_id,)).fetchall()


def schedule_window_end(conn, user, device_id):
    """None = no schedule; -1 = outside every window; else seconds until the
    current window ends. A window is checked against today's date and
    yesterday's, so one that crosses midnight (22:00-02:00) still matches at
    01:00 the next day."""
    rows = rows_for(conn, "schedules", user["id"], device_id)
    if not rows:
        return None
    ln = local_now(user["timezone"])
    best = -1
    for r in rows:
        for days_back in (0, 1):
            base = (ln - dt.timedelta(days=days_back)).replace(hour=0, minute=0, second=0, microsecond=0)
            if r["weekday"] != base.weekday():
                continue
            sh, sm = map(int, r["start_time"].split(":"))
            eh, em = map(int, r["end_time"].split(":"))
            start = base + dt.timedelta(hours=sh, minutes=sm)
            end = base + dt.timedelta(hours=eh, minutes=em)
            if end <= start:
                end += dt.timedelta(days=1)
            if start <= ln < end:
                best = max(best, int((end - ln).total_seconds()))
    return best


def period_start(ln, period):
    d = ln.replace(hour=0, minute=0, second=0, microsecond=0)
    if period == "day":
        return d
    if period == "week":
        return d - dt.timedelta(days=d.weekday())
    if period == "month":
        return d.replace(day=1)
    raise ValueError(period)


def usage_seconds(conn, user_id, device_remote_id, since_utc):
    r = conn.execute(
        """SELECT COALESCE(SUM(elapsed_seconds),0) AS s FROM sessions
           WHERE user_id=? AND device_remote_id=? AND monitoring=0 AND started_at>=?""",
        (user_id, device_remote_id, iso(since_utc))).fetchone()
    return int(r["s"])


def quota_remaining(conn, user, device):
    """None = unlimited; else min remaining seconds over all quota rows."""
    rows = rows_for(conn, "quotas", user["id"], device["id"])
    if not rows:
        return None
    ln = local_now(user["timezone"])
    best = None
    for r in rows:
        since = period_start(ln, r["period"]).astimezone(dt.timezone.utc)
        left = r["max_seconds"] - usage_seconds(conn, user["id"], device["remote_id"], since)
        best = left if best is None else min(best, left)
    return max(0, best)


def remaining_seconds(conn, user, device):
    if user["role"] == "admin":
        return None
    q = quota_remaining(conn, user, device)
    s = schedule_window_end(conn, user, device["id"])
    vals = [v for v in (q, s) if v is not None]
    if not vals:
        return None
    return max(0, min(vals))


# ----------------------------------------------------------------------------
# HTTP
# ----------------------------------------------------------------------------
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        try:
            return json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            return None

    def send(self, status, obj, ctype="application/json"):
        data = obj.encode() if isinstance(obj, str) else json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        if ctype == "application/json":
            print(f"  -> {status} {data.decode()[:300]}")

    def bearer(self):
        a = self.headers.get("Authorization", "")
        return a[7:] if a.startswith("Bearer ") else ""

    def device_key_ok(self, conn, app, device_remote_id):
        k = conn.execute("SELECT * FROM device_api_keys WHERE key_hash=? AND active=1", (h(self.bearer()),)).fetchone()
        if not k:
            self.send(401, {"error": "bad_device_key"})
            return False
        if k["device_id"] is not None:
            d = device_by_remote(conn, device_remote_id, app)
            if not d or d["id"] != k["device_id"]:
                self.send(403, {"error": "key_not_for_this_device"})
                return False
        return True

    # ---- routing
    def do_POST(self):
        b = self.body()
        print(f"\n[{time.strftime('%H:%M:%S')}] POST {self.path} {json.dumps(b)[:400]}")
        if b is None:
            return self.send(400, {"error": "bad_json"})
        with _lock, db() as conn:
            if self.path == "/v1/authorize":
                return self.authorize(conn, b)
            if self.path == "/v1/events":
                return self.event(conn, b)
            if self.path.startswith("/admin/end/"):
                conn.execute("UPDATE sessions SET terminate_requested=1 WHERE session_id=?", (self.path.split("/")[-1],))
                return self.redirect("/")
        self.send(404, {"error": "no_such_route"})

    def do_GET(self):
        u = urlparse(self.path)
        with _lock, db() as conn:
            if u.path == "/":
                return self.dashboard(conn)
            if u.path == "/v1/sessions/active":
                print(f"\n[{time.strftime('%H:%M:%S')}] GET {self.path}")
                return self.active_sessions(conn, parse_qs(u.query))
        self.send(404, {"error": "no_such_route"})

    def redirect(self, to):
        self.send_response(303)
        self.send_header("Location", to)
        self.end_headers()

    # ---- /v1/authorize (spec §4.1)
    def authorize(self, conn, b):
        app = b.get("app")
        if not app:
            return self.send(400, {"error": "missing_app"})
        peer_id, device_rid = b.get("peer_id", ""), b.get("device_id", "")
        if not self.device_key_ok(conn, app, device_rid):
            return
        u = conn.execute(
            "SELECT u.* FROM users u JOIN user_remote_ids r ON r.user_id=u.id WHERE r.app=? AND r.remote_id=?",
            (app, peer_id)).fetchone()
        if not u:
            self.denial(conn, b, "unknown_peer", "unknown peer")
            return self.send(404, {"error": "unknown_peer"})
        device = device_by_remote(conn, device_rid, app)
        if not device:
            return self.send(404, {"error": "unknown_device"})

        def deny(code, reason):
            self.denial(conn, b, code, reason)
            return self.send(200, {"allowed": False, "reason_code": code, "reason": reason})

        if not u["active"] or not u["token_hash"] or not hmac.compare_digest(u["token_hash"], h(b.get("peer_token", ""))):
            return deny("invalid_token", "Invalid access token.")
        role = u["role"]
        monitoring = bool(b.get("monitoring"))
        if role != "admin":
            needs = role == "manager" or USERS_REQUIRE_ASSIGNMENT
            assigned = conn.execute("SELECT 1 FROM device_assignments WHERE user_id=? AND device_id=?",
                                    (u["id"], device["id"])).fetchone()
            if needs and not assigned:
                return deny("not_assigned", "You are not assigned to this device.")
        if role == "user" and not monitoring:
            if any(c.get("role") == "user" and not c.get("monitoring") for c in b.get("connected", [])):
                return deny("user_already_connected", "Another user is currently connected.")
        remaining = None
        if not monitoring and role != "admin":
            s = schedule_window_end(conn, u, device["id"])
            if s == -1:
                return deny("outside_schedule", "You are outside your allowed hours for this device.")
            q = quota_remaining(conn, u, device)
            if q == 0:
                return deny("quota_exhausted", "Your time allowance for this device is used up.")
            remaining = remaining_seconds(conn, u, device)
        resp = {"allowed": True, "user_id": str(u["id"]), "display_name": u["name"], "role": role}
        if remaining is not None:
            resp["remaining_seconds"] = remaining
        self.send(200, resp)

    def denial(self, conn, b, code, reason, source="backend"):
        conn.execute("INSERT INTO denials (ts, app, peer_remote_id, device_remote_id, reason_code, reason, source) VALUES (?,?,?,?,?,?,?)",
                     (iso(now()), b.get("app"), b.get("peer_id"), b.get("device_id"), code, reason, source))

    # ---- /v1/events (spec §4.2)
    def event(self, conn, b):
        s = b.get("session") or {}
        app = s.get("app", "rustdesk")
        if not self.device_key_ok(conn, app, s.get("device_id", "")):
            return
        eid, kind, sid = b.get("event_id"), b.get("event"), s.get("session_id")
        dup = conn.execute("SELECT 1 FROM processed_events WHERE event_id=?", (eid,)).fetchone()
        if dup:
            print("  (duplicate event_id)")
        elif kind == "auth_denied":
            d = b.get("data") or {}
            self.denial(conn, {"app": app, "peer_id": s.get("peer_id"), "device_id": s.get("device_id")},
                        d.get("reason_code", ""), d.get("reason", ""), source="device")
            conn.execute("INSERT INTO processed_events VALUES (?,?)", (eid, iso(now())))
        elif sid:
            conn.execute("INSERT INTO processed_events VALUES (?,?)", (eid, iso(now())))
            row = conn.execute("SELECT * FROM sessions WHERE session_id=?", (sid,)).fetchone()
            user_id = int(s["user_id"]) if str(s.get("user_id") or "").isdigit() else None
            if not row:
                conn.execute(
                    """INSERT INTO sessions (session_id, app, user_id, peer_remote_id, device_remote_id, device_name, role,
                       conn_type, monitoring, offline_authorized, status, started_at, last_heartbeat_at, elapsed_seconds)
                       VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
                    (sid, app, user_id, s.get("peer_id"), s.get("device_id"), s.get("device_name"), s.get("role"),
                     s.get("conn_type"), int(bool(s.get("monitoring"))), int(bool(s.get("offline_authorized"))),
                     "active", s.get("started_at") or iso(now()), iso(now()), s.get("elapsed_seconds") or 0))
                row = conn.execute("SELECT * FROM sessions WHERE session_id=?", (sid,)).fetchone()
            if row["status"] != "ended":
                conn.execute("UPDATE sessions SET last_heartbeat_at=?, elapsed_seconds=MAX(elapsed_seconds,?), status='active' WHERE session_id=?",
                             (iso(now()), s.get("elapsed_seconds") or 0, sid))
            if kind == "session_end":
                d = b.get("data") or {}
                conn.execute("UPDATE sessions SET status='ended', ended_at=?, end_reason=?, elapsed_seconds=MAX(elapsed_seconds,?) WHERE session_id=?",
                             (iso(now()), d.get("reason"), s.get("elapsed_seconds") or 0, sid))
        if kind == "heartbeat" and sid:
            row = conn.execute("SELECT * FROM sessions WHERE session_id=?", (sid,)).fetchone()
            if row and row["terminate_requested"]:
                conn.execute("UPDATE sessions SET terminate_requested=0 WHERE session_id=?", (sid,))
                return self.send(200, {"continue": False, "reason": "Session ended by administrator."})
            if row and not row["monitoring"] and row["user_id"]:
                u = conn.execute("SELECT * FROM users WHERE id=?", (row["user_id"],)).fetchone()
                d = device_by_remote(conn, row["device_remote_id"], app)
                if u and not u["active"]:
                    return self.send(200, {"continue": False, "reason": "Your account has been deactivated."})
                if u and d:
                    r = remaining_seconds(conn, u, d)
                    if r is not None and r <= 0:
                        return self.send(200, {"continue": False, "reason": "Your time allowance for this device has been reached."})
                    resp = {"continue": True}
                    if r is not None:
                        resp["remaining_seconds"] = r
                    return self.send(200, resp)
            return self.send(200, {"continue": True})
        self.send(200, {"ok": True})

    # ---- /v1/sessions/active (spec §4.3)
    def active_sessions(self, conn, q):
        tok = h(self.bearer())
        admin = conn.execute("SELECT 1 FROM users WHERE token_hash=? AND role='admin' AND active=1", (tok,)).fetchone()
        if not admin:
            return self.send(401, {"error": "admin_token_required"})
        rows = conn.execute("SELECT * FROM sessions WHERE status IN ('active','stale') AND monitoring=0 ORDER BY started_at").fetchall()
        out = []
        for r in rows:
            u = conn.execute("SELECT * FROM users WHERE id=?", (r["user_id"],)).fetchone() if r["user_id"] else None
            d = device_by_remote(conn, r["device_remote_id"], r["app"])
            out.append({
                "session_id": r["session_id"], "app": r["app"], "device_id": r["device_remote_id"],
                "device_name": (d["name"] if d else None) or r["device_name"], "peer_id": r["peer_remote_id"],
                "user_id": str(r["user_id"]), "display_name": u["name"] if u else "", "role": r["role"],
                "conn_type": r["conn_type"], "status": r["status"], "started_at": r["started_at"],
                "elapsed_seconds": r["elapsed_seconds"],
                "remaining_seconds": remaining_seconds(conn, u, d) if (u and d) else None,
            })
        self.send(200, {"sessions": out})

    # ---- dashboard
    def dashboard(self, conn):
        def esc(v):
            return str(v if v is not None else "").replace("&", "&amp;").replace("<", "&lt;")
        sessions = conn.execute("SELECT s.*, u.name AS uname FROM sessions s LEFT JOIN users u ON u.id=s.user_id ORDER BY started_at DESC LIMIT 50").fetchall()
        denials = conn.execute("SELECT * FROM denials ORDER BY id DESC LIMIT 30").fetchall()
        users = conn.execute("SELECT u.*, GROUP_CONCAT(r.app||':'||r.remote_id, ', ') AS ids FROM users u LEFT JOIN user_remote_ids r ON r.user_id=u.id GROUP BY u.id").fetchall()
        rows = "".join(
            f"<tr><td>{esc(s['status'])}</td><td>{esc(s['uname'] or s['peer_remote_id'])}</td><td>{esc(s['role'])}</td>"
            f"<td>{esc(s['device_name'] or s['device_remote_id'])}</td><td>{esc(s['conn_type'])}{' (monitoring)' if s['monitoring'] else ''}</td>"
            f"<td>{esc(s['elapsed_seconds'])}s</td><td>{esc(s['started_at'])}</td><td>{esc(s['end_reason'])}</td>"
            f"<td>{'<form method=post action=/admin/end/' + esc(s['session_id']) + '><button>End session</button></form>' if s['status'] in ('active','stale') else ''}</td></tr>"
            for s in sessions)
        drows = "".join(f"<tr><td>{esc(d['ts'])}</td><td>{esc(d['peer_remote_id'])}</td><td>{esc(d['device_remote_id'])}</td><td>{esc(d['reason_code'])}</td><td>{esc(d['reason'])}</td><td>{esc(d['source'])}</td></tr>" for d in denials)
        urows = "".join(f"<tr><td>{esc(u['name'])}</td><td>{esc(u['role'])}</td><td>{esc(u['ids'])}</td><td>{'yes' if u['active'] else 'no'}</td></tr>" for u in users)
        html = f"""<!doctype html><meta charset=utf-8><meta http-equiv=refresh content=10><title>Velour test backend</title>
<style>body{{font:14px system-ui;margin:24px}}table{{border-collapse:collapse;margin-bottom:28px}}td,th{{border:1px solid #ccc;padding:4px 8px;text-align:left}}th{{background:#eee}}h2{{margin:0 0 8px}}</style>
<h2>Sessions</h2><table><tr><th>Status</th><th>Who</th><th>Role</th><th>Device</th><th>Type</th><th>Elapsed</th><th>Started (UTC)</th><th>End reason</th><th></th></tr>{rows}</table>
<h2>Denied attempts</h2><table><tr><th>When (UTC)</th><th>Peer</th><th>Device</th><th>Code</th><th>Reason</th><th>Decided by</th></tr>{drows}</table>
<h2>Users</h2><table><tr><th>Name</th><th>Role</th><th>Remote IDs</th><th>Active</th></tr>{urows}</table>
<p style=color:#777>Refreshes every 10 s. Manage users with <code>test_backend.py</code> commands.</p>"""
        self.send(200, html, "text/html; charset=utf-8")


def stale_job():
    while True:
        time.sleep(15)
        with _lock, db() as conn:
            t = now()
            for r in conn.execute("SELECT session_id, last_heartbeat_at FROM sessions WHERE status IN ('active','stale')").fetchall():
                age = (t - parse_iso(r["last_heartbeat_at"])).total_seconds()
                if age > LOST_AFTER:
                    conn.execute("UPDATE sessions SET status='ended', ended_at=?, end_reason='lost' WHERE session_id=?",
                                 (r["last_heartbeat_at"], r["session_id"]))
                elif age > STALE_AFTER:
                    conn.execute("UPDATE sessions SET status='stale' WHERE session_id=?", (r["session_id"],))


def lan_ip():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        return s.getsockname()[0]
    except OSError:
        return "127.0.0.1"


# ----------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------
def cmd_init(a):
    with db() as conn:
        conn.executescript(SCHEMA)
    print(f"database ready: {DB_PATH}")


def cmd_add_key(a):
    key = a.key or "dk-" + secrets.token_urlsafe(18)
    with db() as conn:
        conn.execute("INSERT INTO device_api_keys (key_hash) VALUES (?)", (h(key),))
    print(f"device API key (shown once): {key}")


def cmd_add_device(a):
    with db() as conn:
        conn.execute("INSERT OR REPLACE INTO devices (app, remote_id, name) VALUES (?,?,?)", (a.app, a.remote_id, a.name or a.remote_id))
    print(f"device {a.app}:{a.remote_id} = {a.name or a.remote_id}")


def cmd_add_user(a):
    token = a.token or "tk-" + secrets.token_urlsafe(24)
    with db() as conn:
        conn.execute("INSERT OR REPLACE INTO users (name, role, token_hash, timezone, active) VALUES (?,?,?,?,1)",
                     (a.name, a.role, h(token), a.tz))
        uid = conn.execute("SELECT id FROM users WHERE name=?", (a.name,)).fetchone()["id"]
        if a.remote_id:
            conn.execute("INSERT OR REPLACE INTO user_remote_ids (user_id, app, remote_id) VALUES (?,?,?)", (uid, a.app, a.remote_id))
    print(f"user {a.name} ({a.role}) remote id {a.app}:{a.remote_id}\npersonal token (shown once): {token}")


def cmd_assign(a):
    with db() as conn:
        u = user_by_name(conn, a.user)
        d = device_by_remote(conn, a.device, a.app) or sys.exit("add the device first")
        conn.execute("INSERT OR IGNORE INTO device_assignments VALUES (?,?)", (u["id"], d["id"]))
    print(f"{a.user} may reach {a.device}")


def parse_days(spec):
    out = set()
    for part in spec.lower().split(","):
        if "-" in part:
            x, y = part.split("-")
            i, j = DAYS.index(x), DAYS.index(y)
            out.update(range(i, j + 1) if i <= j else list(range(i, 7)) + list(range(0, j + 1)))
        else:
            out.add(DAYS.index(part))
    return sorted(out)


def cmd_schedule(a):
    with db() as conn:
        u = user_by_name(conn, a.user)
        dev = device_by_remote(conn, a.device, a.app)["id"] if a.device else None
        for wd in parse_days(a.days):
            conn.execute("INSERT INTO schedules VALUES (?,?,?,?,?)", (u["id"], dev, wd, a.frm, a.to))
    print(f"{a.user}: {a.days} {a.frm}-{a.to} on {a.device or 'all devices (default)'}")


def cmd_quota(a):
    with db() as conn:
        u = user_by_name(conn, a.user)
        dev = device_by_remote(conn, a.device, a.app)["id"] if a.device else None
        conn.execute("INSERT INTO quotas VALUES (?,?,?,?)", (u["id"], dev, a.period, int(a.hours * 3600)))
    print(f"{a.user}: {a.hours} h per {a.period} on {a.device or 'all devices (default)'}")


def cmd_list(a):
    with db() as conn:
        print("USERS"); [print(" ", dict(r)) for r in conn.execute("SELECT u.id,u.name,u.role,u.timezone,u.active, (SELECT GROUP_CONCAT(app||':'||remote_id) FROM user_remote_ids WHERE user_id=u.id) AS ids FROM users u")]
        print("DEVICES"); [print(" ", dict(r)) for r in conn.execute("SELECT * FROM devices")]
        print("ASSIGNMENTS"); [print(" ", dict(r)) for r in conn.execute("SELECT u.name, d.remote_id FROM device_assignments a JOIN users u ON u.id=a.user_id JOIN devices d ON d.id=a.device_id")]
        print("SCHEDULES"); [print(" ", dict(r)) for r in conn.execute("SELECT u.name, s.device_id, s.weekday, s.start_time, s.end_time FROM schedules s JOIN users u ON u.id=s.user_id")]
        print("QUOTAS"); [print(" ", dict(r)) for r in conn.execute("SELECT u.name, q.device_id, q.period, q.max_seconds FROM quotas q JOIN users u ON u.id=q.user_id")]
        print("KEYS:", conn.execute("SELECT COUNT(*) FROM device_api_keys WHERE active=1").fetchone()[0])


def cmd_serve(a):
    with db() as conn:
        conn.executescript(SCHEMA)
    threading.Thread(target=stale_job, daemon=True).start()
    ip = lan_ip()
    print(f"Velour test backend on port {a.port}, db {DB_PATH}")
    print(f"  from this machine:   http://127.0.0.1:{a.port}")
    print(f"  from other machines: http://{ip}:{a.port}   <- Backend URL for devices on your Wi-Fi")
    print(f"  dashboard:           http://{ip}:{a.port}/")
    try:
        ThreadingHTTPServer(("0.0.0.0", a.port), Handler).serve_forever()
    except KeyboardInterrupt:
        pass


def main():
    global DB_PATH
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--db", default=DB_PATH)
    sp = p.add_subparsers(dest="cmd", required=True)
    sp.add_parser("init").set_defaults(f=cmd_init)
    x = sp.add_parser("add-key"); x.add_argument("--key"); x.set_defaults(f=cmd_add_key)
    x = sp.add_parser("add-device"); x.add_argument("remote_id"); x.add_argument("--name"); x.add_argument("--app", default="rustdesk"); x.set_defaults(f=cmd_add_device)
    x = sp.add_parser("add-user"); x.add_argument("name"); x.add_argument("--role", required=True, choices=["admin", "manager", "user"])
    x.add_argument("--remote-id"); x.add_argument("--token"); x.add_argument("--tz", default="UTC"); x.add_argument("--app", default="rustdesk"); x.set_defaults(f=cmd_add_user)
    x = sp.add_parser("assign"); x.add_argument("user"); x.add_argument("device"); x.add_argument("--app", default="rustdesk"); x.set_defaults(f=cmd_assign)
    x = sp.add_parser("schedule"); x.add_argument("user"); x.add_argument("--days", required=True, help="e.g. mon-fri or sat,sun")
    x.add_argument("--from", dest="frm", required=True); x.add_argument("--to", required=True); x.add_argument("--device"); x.add_argument("--app", default="rustdesk"); x.set_defaults(f=cmd_schedule)
    x = sp.add_parser("quota"); x.add_argument("user"); x.add_argument("--period", required=True, choices=["day", "week", "month"])
    x.add_argument("--hours", type=float, required=True); x.add_argument("--device"); x.add_argument("--app", default="rustdesk"); x.set_defaults(f=cmd_quota)
    sp.add_parser("list").set_defaults(f=cmd_list)
    x = sp.add_parser("serve"); x.add_argument("--port", type=int, default=8787); x.set_defaults(f=cmd_serve)
    a = p.parse_args()
    DB_PATH = a.db
    a.f(a)


if __name__ == "__main__":
    main()
