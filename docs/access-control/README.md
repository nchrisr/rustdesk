# RustDesk-Velour access control

* `VELOUR_PLAN.md` — complete implementation plan for the RustDesk side.
  Start here when resuming work.
* `EXTERNAL_SYSTEM_SPEC.md` — API contract the external backend must implement.
  Hand this to whoever builds the backend; it is self-contained.
* `BUILD_MACOS.md` — building and running the fork on this Mac.
* `WINDOWS_TEST_GUIDE.md` — getting a Windows build from GitHub Actions,
  installing it, and the two-machine test scenarios.
* `TEST_LOG.md` — manual test results.
* `test_backend.py` — SQLite implementation of the spec for testing on a LAN
  (users, devices, schedules, quotas, sessions, dashboard). Use this.
* `mock_backend.py` + `mock_users.json` — the earlier in-memory mock with
  `/mock/...` controls; handy for scripted single-machine checks.
