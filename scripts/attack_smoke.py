"""Additional private-X11 workflows; invoked by feature_smoke.py --attack-only."""
import json
import sqlite3
import subprocess
import time
import base64
import hashlib
import hmac


def _workspace(kind, steps, **options):
    return {"kind": kind, "steps": steps, **options}


def _step(origin, path, **options):
    return {"name": path, "destination": f"http://{origin}",
            "request": f"GET {path} HTTP/1.1\r\nHost: {origin}\r\n\r\n", **options}


def seed_plans(project_path, origin):
    access = _workspace("Access", [_step(origin, "/native-access", request=f"GET /native-access HTTP/1.1\r\nHost: {origin}\r\nAuthorization: Bearer high\r\nCookie: original=1\r\n\r\n")],
                        profiles=[{"name": "User B", "headers": "Authorization: Bearer low"}])
    sequence = _workspace("Sequence", [
        _step(origin, "/native-login", extractors=[{"name": "token", "source": "Json", "selector": "/token", "required": True}]),
        _step(origin, "/native-use/{{token}}", request=f"GET /native-use/{{{{token}}}} HTTP/1.1\r\nHost: {origin}\r\nCookie: session=stale\r\nX-CSRF: {{{{token}}}}\r\n\r\n")])
    with sqlite3.connect(project_path / "project.sqlite") as database:
        for run_id, name, workspace in [(100, "Fixture access", access), (200, "Fixture sequence", sequence)]:
            database.execute("INSERT INTO campaign_runs VALUES(?,?)", (run_id, json.dumps({"id": run_id, "name": name, "workspace": workspace, "variables": []})))


def exercise(click, paste, shot, hotkey, key, wait_path, origin, project_path, display_name, jwt_only=False):
    def copied(x, y):
        click(x, y);hotkey("a");hotkey("c")
        return subprocess.check_output(["xclip", "-display", display_name, "-selection", "clipboard", "-o"], timeout=5).decode()

    def wait_results(kind, expected):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            with sqlite3.connect(project_path / "project.sqlite") as database:
                found = database.execute("SELECT id FROM campaign_runs WHERE id>200 AND json_extract(json,'$.workspace.kind')=? ORDER BY id DESC LIMIT 1", (kind,)).fetchone()
                if found:
                    records = [json.loads(row[0]) for row in database.execute("SELECT json FROM campaign_results WHERE run_id=? ORDER BY idx", found)]
                    if len(records) == expected:
                        assert all(record["capture_id"] is not None for record in records), records
                        time.sleep(.6)
                        return records
            time.sleep(.1)
        raise AssertionError(f"{kind}: expected {expected} saved results")

    if jwt_only:
        return exercise_jwt(click,paste,shot,hotkey,copied,display_name)
    seed_plans(project_path, origin)
    hotkey("2")
    paste(500, 134, f"http://{origin}")
    paste(400, 300, f"GET /native-race HTTP/1.1\r\nHost: {origin}\r\n\r\n")
    assert "/native-race" in copied(400, 300)
    paste(80, 155, "CTF lab");click(185, 155)
    shot("attack-replay-group.png")
    # The Replay toolbar hands selected requests to Attack without sending them.
    click(420, 104);shot("attack-race-ready.png")
    click(285, 104)
    for _ in range(10):wait_path("/native-race")
    records = wait_results("Race", 10)
    assert all(record["release_us"] is not None for record in records), records
    shot("attack-race-results.png")
    # Refresh fixture plans through the saved-runs menu, then load Access-control.
    click(400, 104);shot("attack-saved-menu-before-refresh.png");click(450, 140)
    click(400, 104);shot("attack-saved-menu.png")
    click(450, 257);time.sleep(.5);shot("attack-access-ready.png")
    click(285, 104)
    for _ in range(3):wait_path("/native-access")
    records = wait_results("Access", 3)
    assert records[0]["status"] == 200 and records[1]["status"] == records[2]["status"] == 403, records
    assert all("Denied" in record["verdict"] for record in records[1:]), records
    shot("attack-access-results.png")
    # The selected fixture sequence carries its token and cookie into the second step.
    click(400, 104);shot("attack-sequence-menu.png");click(450, 249)
    time.sleep(.5);shot("attack-sequence-ready.png");click(285, 104)
    wait_path("/native-login");wait_path("/native-use/fresh-native")
    records = wait_results("Sequence", 2)
    assert all(record["status"] == 200 and not record["error"] for record in records), records
    shot("attack-sequence-results.png")
    # Inspect the exact saved response rather than the editable template.
    click(200, 770);time.sleep(.5);shot("attack-result-inspection.png")
    assert "fresh-native" in copied(400, 400)
    exercise_jwt(click,paste,shot,hotkey,copied,display_name)
    print("PASS: native Replay group handoff, race bursts, identity comparison, token/cookie sequence and saved result inspection")


def exercise_jwt(click,paste,shot,hotkey,copied,display_name):
    hotkey("5");click(240, 102)
    token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
    paste(300, 190, token);shot("attack-jwt-input.png")
    click(40, 238);time.sleep(.3);shot("attack-jwt-decoded.png")
    assert "John Doe" in copied(800, 400)
    paste(330, 238, "native-test-secret")
    paste(800, 400, '{"sub":"native-edited","exp":0}')
    shot("attack-jwt-edited.png")
    click(60, 505);shot("attack-jwt-signed.png")
    click(350, 505)
    signed = subprocess.check_output(["xclip", "-display", display_name, "-selection", "clipboard", "-o"], timeout=5).decode()
    head, payload, signature = signed.split(".")
    assert json.loads(base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4)))["sub"] == "native-edited", signed
    expected = hmac.new(b"native-test-secret", f"{head}.{payload}".encode(), hashlib.sha256).digest()
    assert base64.urlsafe_b64decode(signature + "=" * (-len(signature) % 4)) == expected, signed
    print("PASS: native JWT signing verified independently with Python HMAC")


def verify(project_path, jwt_only=False):
    with sqlite3.connect(project_path / "project.sqlite") as database:
        state = json.loads(database.execute("SELECT json FROM state WHERE id=1").fetchone()[0])
        assert "native-test-secret" not in json.dumps(state), "JWT signing key was persisted"
        if jwt_only:
            return
        assert "CTF lab" in state["replay_groups"], state["replay_groups"]
        assert state["attack_workspace"]["kind"] == "Sequence", state["attack_workspace"]
        assert len(state["attack_workspace"]["steps"]) == 2
        assert "native-test-secret" not in json.dumps(state), "JWT signing key was persisted"
        for kind, expected in [("Race", 10), ("Access", 3), ("Sequence", 2)]:
            run = database.execute("SELECT id FROM campaign_runs WHERE id>200 AND json_extract(json,'$.workspace.kind')=? ORDER BY id DESC LIMIT 1", (kind,)).fetchone()
            assert run and database.execute("SELECT count(*) FROM campaign_results WHERE run_id=?", run).fetchone()[0] == expected
            saved = json.loads(database.execute("SELECT json FROM campaign_runs WHERE id=?",run).fetchone()[0])
            assert saved["status"] == "Completed",saved
    print("PASS: Replay groups, Attack plans/results and capture links saved; JWT signing key remains session-only")
