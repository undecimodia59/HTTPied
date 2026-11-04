"""Regression checks for Coder recovery and scope context menus in the native window."""
import json
import sqlite3
import subprocess
import time


def clipboard(display_name):
    return subprocess.check_output(["xclip", "-display", display_name, "-selection", "clipboard", "-o"], timeout=5).decode()


def exercise(click, paste, shot, hotkey, wait_path, origin, project_path, display_name):
    # Responses retain their actual target even while the editable destination changes.
    hotkey("2");hotkey("Return");wait_path("/compressed.js");time.sleep(.5)
    click(800, 400);hotkey("a");hotkey("c")
    click(800, 400, 3);click(850, 626);shot("followup-response-scope-menu.png")
    click(1030, 646)
    paste(500, 134, "https://lab.example.test")
    click(400, 400);hotkey("a");hotkey("c")
    click(400, 300, 3);click(450, 638);shot("followup-request-scope-menu.png")
    click(640, 690)
    paste(500, 134, f"http://{origin}")
    # Keep a non-default codec, byte input format and Encode operation across restart.
    hotkey("5");click(70, 134);shot("followup-coder-codec-menu.png");click(80, 200)
    click(65, 174)
    paste(180, 300, "ff00");click(230, 134)
    click(800, 300);hotkey("a");hotkey("c")
    assert clipboard(display_name) == "_wA", clipboard(display_name)
    shot("followup-coder-ready.png")
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        with sqlite3.connect(project_path / "project.sqlite") as database:
            state = json.loads(database.execute("SELECT json FROM state WHERE id=1").fetchone()[0])
        if state.get("coder_workspace", {}).get("input") == "ff00":
            verify_state(state)
            break
        time.sleep(.1)
    else:
        raise AssertionError("Coder preferences were not autosaved")


def verify_state(state):
    coder = state["coder_workspace"]
    assert coder["input"] == "ff00" and coder["input_hex"] is True, coder
    assert coder["codec"] == "Base64Url" and coder["operation"] == "Encode" and coder["converted"] is True, coder
    scope = state["scope"]
    assert any(rule["host"] == "127.0.0.1" and not rule["subdomains"] for rule in scope), scope
    assert any(rule["host"] == "lab.example.test" and rule["subdomains"] for rule in scope), scope


def verify_reopened(click, hotkey, shot, display_name):
    hotkey("5")
    click(180, 300);hotkey("a");hotkey("c");assert clipboard(display_name) == "ff00"
    click(800, 300);hotkey("a");hotkey("c");assert clipboard(display_name) == "_wA"
    shot("followup-coder-recovered.png")
    print("PASS: Coder input, codec, byte format, operation and result recovered; request/response scope actions saved")
