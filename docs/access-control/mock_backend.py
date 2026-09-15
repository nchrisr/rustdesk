#!/usr/bin/env python3
"""Mock RustDesk-Velour backend for manual testing. Standard library only.

Implements EXTERNAL_SYSTEM_SPEC.md well enough to exercise the RustDesk side:
  POST /v1/authorize          -> decision from mock_users.json
  POST /v1/events             -> records sessions; heartbeat answers with
                                 continue + remaining_seconds
  GET  /v1/sessions/active    -> live sessions (admin token required)

Test controls (no auth):
  POST /mock/down             -> toggle "backend down" (every /v1 call -> 503)
  POST /mock/remaining/<peer_id>/<seconds>
                              -> set remaining_seconds for that peer's
                                 sessions (drives the countdown)
  POST /mock/stop/<session_id> -> next heartbeat gets continue:false
  GET  /mock/state            -> dump users, sessions, flags

Run:  python3 -u mock_backend.py [--port 8787] [--users mock_users.json]
Every request is printed. Point a device's Backend URL at
http://<this machine>:<port> and set its Device API key to one of the keys in
mock_users.json. Plain HTTP is fine for local testing only.
"""
import argparse
import json
import sys
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

STATE = {
    "users": {},        # (app, remote_id) -> user dict
    "device_keys": set(),
    "devices": {},      # (app, remote_id) -> name
    "sessions": {},     # session_id -> session dict
    "processed": set(), # event ids
    "down": False,
    "stop": set(),      # session ids to stop on next heartbeat
    "remaining": {},    # peer_id -> remaining seconds override
    "users_require_assignment": True,
}


def load_users(path):
    with open(path) as f:
        data = json.load(f)
    STATE["device_keys"] = set(data.get("device_api_keys", []))
    STATE["users_require_assignment"] = data.get("users_require_assignment", True)
    for d in data.get("devices", []):
        STATE["devices"][(d.get("app", "rustdesk"), d["remote_id"])] = d.get("name", d["remote_id"])
    for u in data["users"]:
        for rid in u.get("remote_ids", []):
            STATE["users"][(rid.get("app", "rustdesk"), rid["remote_id"])] = u


def now_iso():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # quieter default log
        pass

    # ---- helpers -------------------------------------------------------
    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        try:
            return json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            return None

    def send(self, status, obj):
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        print(f"  -> {status} {json.dumps(obj)}")

    def bearer(self):
        auth = self.headers.get("Authorization", "")
        return auth[7:] if auth.startswith("Bearer ") else ""

    def device_authed(self):
        if self.bearer() not in STATE["device_keys"]:
            self.send(401, {"error": "bad_device_key"})
            return False
        return True

    # ---- routing -------------------------------------------------------
    def do_POST(self):
        b = self.body()
        print(f"\n[{time.strftime('%H:%M:%S')}] POST {self.path} {json.dumps(b)}")
        if self.path.startswith("/mock/"):
            return self.mock_control()
        if STATE["down"]:
            return self.send(503, {"error": "mock backend is down"})
        if b is None:
            return self.send(400, {"error": "bad_json"})
        if self.path == "/v1/authorize":
            return self.authorize(b)
        if self.path == "/v1/events":
            return self.event(b)
        self.send(404, {"error": "no_such_route"})

    def do_GET(self):
        print(f"\n[{time.strftime('%H:%M:%S')}] GET {self.path}")
        if self.path == "/mock/state":
            return self.send(200, {
                "down": STATE["down"],
                "sessions": list(STATE["sessions"].values()),
                "remaining": STATE["remaining"],
                "users": sorted(f"{k[0]}:{k[1]} -> {v['name']} ({v['role']})" for k, v in STATE["users"].items()),
            })
        if STATE["down"]:
            return self.send(503, {"error": "mock backend is down"})
        if self.path.startswith("/v1/sessions/active"):
            return self.active_sessions()
        self.send(404, {"error": "no_such_route"})

    # ---- /mock controls -------------------------------------------------
    def mock_control(self):
        parts = self.path.strip("/").split("/")
        if parts[1:] == ["down"]:
            STATE["down"] = not STATE["down"]
            return self.send(200, {"down": STATE["down"]})
        if len(parts) == 4 and parts[1] == "remaining":
            STATE["remaining"][parts[2]] = int(parts[3])
            return self.send(200, {"peer_id": parts[2], "remaining_seconds": int(parts[3])})
        if len(parts) == 3 and parts[1] == "stop":
            STATE["stop"].add(parts[2])
            return self.send(200, {"stop": parts[2]})
        self.send(404, {"error": "unknown mock control"})

    # ---- /v1/authorize ----------------------------------------------------
    def authorize(self, b):
        if not self.device_authed():
            return
        app = b.get("app")
        if not app:
            return self.send(400, {"error": "missing_app"})
        user = STATE["users"].get((app, b.get("peer_id", "")))
        if not user:
            return self.send(404, {"error": "unknown_peer"})
        dev_key = (app, b.get("device_id", ""))
        if STATE["devices"] and dev_key not in STATE["devices"]:
            return self.send(404, {"error": "unknown_device"})
        if not user.get("active", True) or b.get("peer_token") != user.get("token"):
            return self.send(200, {"allowed": False, "reason_code": "invalid_token",
                                   "reason": "Invalid access token."})
        role = user["role"]
        monitoring = bool(b.get("monitoring"))
        if role != "admin":
            need = role == "manager" or STATE["users_require_assignment"]
            if need and b.get("device_id") not in user.get("assigned_devices", []):
                return self.send(200, {"allowed": False, "reason_code": "not_assigned",
                                       "reason": "You are not assigned to this device."})
        if role == "user" and not monitoring:
            if any(c.get("role") == "user" and not c.get("monitoring") for c in b.get("connected", [])):
                return self.send(200, {"allowed": False, "reason_code": "user_already_connected",
                                       "reason": "Another user is currently connected."})
        if user.get("deny_reason") and not monitoring:
            return self.send(200, {"allowed": False, "reason_code": "other", "reason": user["deny_reason"]})
        remaining = STATE["remaining"].get(b.get("peer_id"), user.get("remaining_seconds"))
        resp = {"allowed": True, "user_id": user["id"], "display_name": user["name"], "role": role}
        if remaining is not None:
            resp["remaining_seconds"] = remaining
        self.send(200, resp)

    # ---- /v1/events -------------------------------------------------------
    def event(self, b):
        if not self.device_authed():
            return
        eid = b.get("event_id")
        kind = b.get("event")
        s = b.get("session") or {}
        sid = s.get("session_id")
        if eid in STATE["processed"]:
            print("  (duplicate event_id, ignored)")
        elif sid:
            STATE["processed"].add(eid)
            rec = STATE["sessions"].setdefault(sid, {"session_id": sid, "status": "active",
                                                     "started_at": s.get("started_at")})
            rec.update({k: s.get(k) for k in ("app", "device_id", "device_name", "peer_id", "peer_name",
                                              "user_id", "role", "conn_type", "monitoring",
                                              "offline_authorized", "elapsed_seconds")})
            rec["last_event"] = kind
            rec["last_seen"] = now_iso()
            if kind == "session_end":
                rec["status"] = "ended"
                rec["end_reason"] = (b.get("data") or {}).get("reason")
            elif kind == "auth_denied":
                rec["status"] = "denied"
        if kind == "heartbeat":
            peer = s.get("peer_id")
            remaining = STATE["remaining"].get(peer)
            if remaining is None:
                u = STATE["users"].get((s.get("app", "rustdesk"), peer))
                remaining = u.get("remaining_seconds") if u else None
            if sid in STATE["stop"]:
                STATE["stop"].discard(sid)
                return self.send(200, {"continue": False, "reason": "Session ended by administrator (mock)."})
            if remaining is not None:
                remaining = max(0, remaining - (s.get("elapsed_seconds") or 0))
                if remaining <= 0:
                    return self.send(200, {"continue": False, "reason": "Your time limit has been reached (mock)."})
            resp = {"continue": True}
            if remaining is not None:
                resp["remaining_seconds"] = remaining
            return self.send(200, resp)
        self.send(200, {"ok": True})

    # ---- /v1/sessions/active ----------------------------------------------
    def active_sessions(self):
        tok = self.bearer()
        admin = any(u.get("token") == tok and u.get("role") == "admin" for u in STATE["users"].values())
        if not admin:
            return self.send(401, {"error": "admin_token_required"})
        live = [s for s in STATE["sessions"].values()
                if s.get("status") in ("active", "stale") and not s.get("monitoring")]
        self.send(200, {"sessions": live})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8787)
    ap.add_argument("--users", default="mock_users.json")
    a = ap.parse_args()
    load_users(a.users)
    print(f"mock backend on http://0.0.0.0:{a.port}  users={len(STATE['users'])} device_keys={len(STATE['device_keys'])}")
    print("controls: POST /mock/down | POST /mock/remaining/<peer>/<secs> | POST /mock/stop/<session> | GET /mock/state")
    try:
        ThreadingHTTPServer(("0.0.0.0", a.port), Handler).serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
