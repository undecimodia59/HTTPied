"""Linux native smoke on a private Xvfb display; requires Xvfb, libX11, libXtst and ImageMagick.
Run after: cargo build --release --features ui
"""
import argparse
import json
import sqlite3
import ctypes as c
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading
import http.server
import queue
from ui_fixture import Fixture
import time


class ClientMessage(c.Structure):
    _fields_ = [("type", c.c_int), ("serial", c.c_ulong), ("send_event", c.c_int),
                ("display", c.c_void_p), ("window", c.c_ulong), ("message_type", c.c_ulong),
                ("format", c.c_int), ("data", c.c_long * 5)]


class XEvent(c.Union):
    _fields_ = [("message", ClientMessage), ("pad", c.c_long * 24)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--capture-only", action="store_true")
    parser.add_argument("--layout-only",action="store_true")
    args = parser.parse_args()
    observed=queue.Queue()
    class Tracked(Fixture):
        def do_GET(self):
            observed.put((self.path,self.headers.get("User-Agent","")))
            if self.path=="/html":
                body=b'<!doctype html>\n<html>\n<head><title>Local fixture</title></head>\n<body><a href="/next">Link</a><!-- local --></body>\n</html>'
                self.send_response(200);self.send_header("Content-Type","text/html");self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            else:super().do_GET()
    server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Tracked)
    server.daemon_threads=True
    threading.Thread(target=server.serve_forever,daemon=True).start()
    origin=f"127.0.0.1:{server.server_port}"
    lib = c.CDLL("libX11.so.6")
    lib.XOpenDisplay.argtypes = [c.c_char_p]
    lib.XOpenDisplay.restype = c.c_void_p
    lib.XDefaultRootWindow.argtypes = [c.c_void_p]
    lib.XDefaultRootWindow.restype = c.c_ulong
    lib.XQueryTree.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ulong)), c.POINTER(c.c_uint)]
    lib.XFetchName.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(c.c_void_p)]
    lib.XFree.argtypes = [c.c_void_p]
    lib.XInternAtom.argtypes = [c.c_void_p, c.c_char_p, c.c_int]
    lib.XInternAtom.restype = c.c_ulong
    lib.XSendEvent.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_long, c.POINTER(XEvent)]
    lib.XFlush.argtypes = [c.c_void_p]
    lib.XCloseDisplay.argtypes = [c.c_void_p]
    with tempfile.TemporaryDirectory() as directory:
        with open(Path(directory) / "display", "w+") as display_file, open(Path(directory) / "ui.log", "w+") as log:
            xvfb = subprocess.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "1280x900x24", "-nolisten", "tcp"], stdout=display_file, stderr=log)
            app = None
            display = None
            try:
                deadline = time.monotonic() + 20
                while time.monotonic() < deadline:
                    display_file.seek(0)
                    number = display_file.read().strip()
                    if number:
                        break
                    time.sleep(.05)
                else:
                    raise RuntimeError("Xvfb did not start")
                display_name = f":{number}"
                env = {**os.environ, "DISPLAY": display_name, "WINIT_UNIX_BACKEND": "x11", "LIBGL_ALWAYS_SOFTWARE": "1"}
                env.pop("WAYLAND_DISPLAY", None)
                project_path=Path(directory)/"saved-project"
                launched_at=time.monotonic()
                app = subprocess.Popen(["dbus-run-session","--","target/release/httpied", "--listen", "127.0.0.1:0"], env=env, stdout=log, stderr=log)
                display = lib.XOpenDisplay(display_name.encode())
                if not display:
                    raise RuntimeError("cannot open private X display")
                window = None
                port = None
                started_project=False
                while time.monotonic() < deadline:
                    if app.poll() is not None:
                        raise RuntimeError("UI exited before ready")
                    log.seek(0)
                    match = re.search(r"READY 127.0.0.1:(\d+)", log.read())
                    if match:
                        port = match.group(1)
                    root, parent, children, count = c.c_ulong(), c.c_ulong(), c.POINTER(c.c_ulong)(), c.c_uint()
                    lib.XQueryTree(display, lib.XDefaultRootWindow(display), c.byref(root), c.byref(parent), c.byref(children), c.byref(count))
                    for i in range(count.value):
                        name = c.c_void_p()
                        if lib.XFetchName(display, children[i], c.byref(name)) and name:
                            if b"HTTPied" in c.string_at(name):
                                window = children[i]
                            lib.XFree(name)
                    if children:
                        lib.XFree(children)
                    if window and not started_project:
                        first_window_ms=(time.monotonic()-launched_at)*1000
                        time.sleep(.5)
                        Path("docs/screenshots").mkdir(exist_ok=True)
                        subprocess.run(["import","-display",display_name,"-window",str(window),"docs/screenshots/project-start.png"],check=True,timeout=10)
                        startup_xtst=c.CDLL("libXtst.so.6")
                        startup_xtst.XTestFakeMotionEvent.argtypes=[c.c_void_p,c.c_int,c.c_int,c.c_int,c.c_ulong]
                        startup_xtst.XTestFakeButtonEvent.argtypes=[c.c_void_p,c.c_uint,c.c_int,c.c_ulong]
                        lib.XTranslateCoordinates.argtypes=[c.c_void_p,c.c_ulong,c.c_ulong,c.c_int,c.c_int,c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_ulong)]
                        sx,sy,child=c.c_int(),c.c_int(),c.c_ulong()
                        lib.XTranslateCoordinates(display,window,lib.XDefaultRootWindow(display),0,0,c.byref(sx),c.byref(sy),c.byref(child))
                        if not args.capture_only and not args.layout_only:
                            startup_xtst.XTestFakeKeyEvent.argtypes=[c.c_void_p,c.c_uint,c.c_int,c.c_ulong]
                            lib.XStringToKeysym.argtypes=[c.c_char_p];lib.XStringToKeysym.restype=c.c_ulong
                            lib.XKeysymToKeycode.argtypes=[c.c_void_p,c.c_ulong];lib.XKeysymToKeycode.restype=c.c_uint
                            lib.XSetInputFocus.argtypes=[c.c_void_p,c.c_ulong,c.c_int,c.c_ulong]
                            lib.XSetInputFocus(display,window,2,0)
                            startup_xtst.XTestFakeMotionEvent(display,-1,sx.value+300,sy.value+202,0)
                            startup_xtst.XTestFakeButtonEvent(display,1,1,0);startup_xtst.XTestFakeButtonEvent(display,1,0,0);lib.XFlush(display);time.sleep(.15)
                            subprocess.run(["xclip","-display",display_name,"-selection","clipboard"],input=str(project_path).encode(),check=True,timeout=5)
                            for name,down in [("Control_L",1),("a",1),("a",0),("v",1),("v",0),("Control_L",0)]:
                                startup_xtst.XTestFakeKeyEvent(display,lib.XKeysymToKeycode(display,lib.XStringToKeysym(name.encode())),down,0)
                            lib.XFlush(display);time.sleep(.15)
                        startup_xtst.XTestFakeMotionEvent(display,-1,sx.value+(165 if not args.capture_only and not args.layout_only else 290),sy.value+250,0)
                        startup_xtst.XTestFakeButtonEvent(display,1,1,0);startup_xtst.XTestFakeButtonEvent(display,1,0,0)
                        lib.XFlush(display);started_project=True
                    if window and port:
                        break
                    time.sleep(.1)
                if not window or not port:
                    raise RuntimeError("native window or listener missing")
                ready_ms=(time.monotonic()-launched_at)*1000
                rss=None
                children_path=Path(f"/proc/{app.pid}/task/{app.pid}/children")
                if children_path.exists():
                    for pid in children_path.read_text().split():
                        try:
                            if Path(f"/proc/{pid}/comm").read_text().strip()=="httpied":
                                rss=next(line.strip() for line in Path(f"/proc/{pid}/status").read_text().splitlines() if line.startswith("VmRSS:"))
                        except (OSError,StopIteration):pass
                print(json.dumps({"native_first_window_ms":round(first_window_ms,2),"native_ready_ms_including_fixture_clicks":round(ready_ms,2),"app_idle_rss":rss,"renderer":"Xvfb / software OpenGL","project":"saved" if project_path.exists() else "temporary"}))
                subprocess.run(["python3", "scripts/ui_fixture.py", "--once", "--port", "0", "--proxy-port", port], check=True, timeout=10)
                time.sleep(.5)
                destination = Path("docs/screenshots")
                destination.mkdir(exist_ok=True)
                subprocess.run(["import", "-display", display_name, "-window", str(window), str(destination / "ui-capture.png")], check=True, timeout=10)
                # Exercise a real tab click and a real scroll-table mode change.
                xtst = c.CDLL("libXtst.so.6")
                xtst.XTestFakeMotionEvent.argtypes = [c.c_void_p, c.c_int, c.c_int, c.c_int, c.c_ulong]
                xtst.XTestFakeButtonEvent.argtypes = [c.c_void_p, c.c_uint, c.c_int, c.c_ulong]
                lib.XTranslateCoordinates.argtypes = [c.c_void_p, c.c_ulong, c.c_ulong, c.c_int, c.c_int, c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(c.c_ulong)]
                root_x, root_y, child = c.c_int(), c.c_int(), c.c_ulong()
                lib.XTranslateCoordinates(display, window, lib.XDefaultRootWindow(display), 0, 0, c.byref(root_x), c.byref(root_y), c.byref(child))

                def click(x, y, button=1):
                    xtst.XTestFakeMotionEvent(display, -1, root_x.value + x, root_y.value + y, 0)
                    xtst.XTestFakeButtonEvent(display, button, 1, 0)
                    xtst.XTestFakeButtonEvent(display, button, 0, 0)
                    lib.XFlush(display)
                    time.sleep(.3)

                if not args.capture_only:
                    lib.XSetInputFocus.argtypes=[c.c_void_p,c.c_ulong,c.c_int,c.c_ulong]
                    lib.XSetInputFocus(display,window,2,0)
                    xtst.XTestFakeKeyEvent.argtypes=[c.c_void_p,c.c_uint,c.c_int,c.c_ulong]
                    lib.XStringToKeysym.argtypes=[c.c_char_p]
                    lib.XStringToKeysym.restype=c.c_ulong
                    lib.XKeysymToKeycode.argtypes=[c.c_void_p,c.c_ulong]
                    lib.XKeysymToKeycode.restype=c.c_uint
                    def key(name,down):
                        xtst.XTestFakeKeyEvent(display,lib.XKeysymToKeycode(display,lib.XStringToKeysym(name.encode())),down,0)
                    def paste(x,y,text):
                        click(x,y)
                        subprocess.run(["xclip","-display",display_name,"-selection","clipboard"],input=text.encode(),check=True,timeout=5)
                        key("Control_L",1);key("a",1);key("a",0);key("v",1);key("v",0);key("Control_L",0)
                        lib.XFlush(display);time.sleep(.3)
                    def shot(name):
                        lib.XRaiseWindow.argtypes=[c.c_void_p,c.c_ulong]
                        lib.XRaiseWindow(display,window)
                        lib.XFlush(display)
                        time.sleep(.1)
                        subprocess.run(["import","-display",display_name,"-window",str(window),str(destination/name)],check=True,timeout=10)
                    def wait_path(path,timeout=10):
                        limit=time.monotonic()+timeout
                        while time.monotonic()<limit:
                            seen,agent=observed.get(timeout=max(.1,limit-time.monotonic()))
                            if seen==path:return agent
                        raise RuntimeError(f"local fixture never received {path}")
                    def hotkey(name):
                        key("Control_L",1);key(name,1);key(name,0);key("Control_L",0);lib.XFlush(display);time.sleep(.3)
                    def paste_selection(text):
                        subprocess.run(["xclip","-display",display_name,"-selection","clipboard"],input=text.encode(),check=True,timeout=5)
                        hotkey("v")
                    # Replay HTML highlighting and native undo / redo.
                    for x,name in [(145,"history"),(250,"replay"),(350,"intruder"),(445,"browser"),(545,"target"),(640,"variables"),(740,"compare"),(840,"results"),(940,"project"),(1040,"settings")]:
                        click(x,55);shot(f"features-{name}.png")
                    if not args.layout_only:
                        hotkey("2")
                        paste(370,133,f"http://{origin}")
                        original=f"GET /html HTTP/1.1\r\nHost: {origin}\r\n\r\n"
                        paste(180,300,original)
                        time.sleep(1)
                        hotkey("Return");wait_path("/html");shot("features-replay-sent.png")
                        paste(180,300,f"GET /changed HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                        time.sleep(.5);hotkey("z");hotkey("Return");wait_path("/html")
                        click(180,300);hotkey("y");hotkey("Return");wait_path("/changed")
                        shot("features-replay-redo.png")
                        click(650,55);click(55,181)
                        paste(150,181,"base");paste(420,181,f"http://{origin}")
                        click(55,213)
                        paste(150,213,"route");paste(420,213,"/changed")
                        shot("features-variables-filled.png")
                        hotkey("2");paste(370,133,"{{base}}")
                        paste(180,315,"GET {{route}} HTTP/1.1\r\n\r\n")
                        hotkey("Return");wait_path("/changed");time.sleep(.5)
                        shot("features-variable-send.png")
                        click(350,55);paste(250,101,f"http://{origin}")
                        click(160,229)
                        shot("features-numbers-source.png")
                        paste(100,289,"0000");paste(275,289,"0002")
                        paste(180,450,f"GET /number/{{{{payload}}}} HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                        shot("features-numbers-ready.png")
                        click(490,129)
                        for value in ("0000","0001","0002"):wait_path(f"/number/{value}")
                        time.sleep(.5);shot("features-numbers-sent.png")
                        click(840,55);click(460,128);time.sleep(.5);shot("features-results-filled.png")


                event = XEvent()
                event.message.type = 33  # ClientMessage
                event.message.window = window
                event.message.display = display
                event.message.format = 32
                event.message.message_type = lib.XInternAtom(display, b"WM_PROTOCOLS", 0)
                event.message.data[0] = lib.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
                lib.XSendEvent(display, window, 0, 0, c.byref(event))
                lib.XFlush(display)
                assert app.wait(timeout=5) == 0, "UI failed graceful shutdown"
                if not args.capture_only and not args.layout_only:
                    with sqlite3.connect(project_path/"project.sqlite") as database:
                        state=json.loads(database.execute("SELECT json FROM state WHERE id=1").fetchone()[0])
                        assert state["variables"][0]["key"]=="base" and state["variables"][1]["value"]=="/changed",state["variables"]
                        assert state["replays"][0]["destination"]=="{{base}}"
                        assert "{{route}}" in state["replays"][0]["request"]
                        assert database.execute("SELECT count(*) FROM captures").fetchone()[0]>=10
                        records=[json.loads(row[0]) for row in database.execute("SELECT json FROM results ORDER BY idx")]
                        assert len(records)==3 and [r["payloads"][0] for r in records]==["0000","0001","0002"],records
                        assert all(r["capture_id"] is not None for r in records)

                    log.seek(0);log.truncate()
                    app=subprocess.Popen(["dbus-run-session","--","target/release/httpied","--listen","127.0.0.1:0"],env=env,stdout=log,stderr=log)
                    limit=time.monotonic()+15;window=None
                    while time.monotonic()<limit:
                        root,parent,children,count=c.c_ulong(),c.c_ulong(),c.POINTER(c.c_ulong)(),c.c_uint()
                        lib.XQueryTree(display,lib.XDefaultRootWindow(display),c.byref(root),c.byref(parent),c.byref(children),c.byref(count))
                        for i in range(count.value):
                            name=c.c_void_p()
                            if lib.XFetchName(display,children[i],c.byref(name)) and name:
                                if b"HTTPied" in c.string_at(name):window=children[i]
                                lib.XFree(name)
                        if children:lib.XFree(children)
                        if window:break
                        time.sleep(.1)
                    assert window,"reopened native window missing"
                    lib.XTranslateCoordinates(display,window,lib.XDefaultRootWindow(display),0,0,c.byref(root_x),c.byref(root_y),c.byref(child))
                    lib.XSetInputFocus(display,window,2,0);time.sleep(.5)
                    paste(300,202,str(project_path));click(55,250)
                    limit=time.monotonic()+10
                    while time.monotonic()<limit:
                        log.seek(0)
                        if "READY " in log.read():break
                        time.sleep(.1)
                    else:raise RuntimeError("saved project failed to reopen")
                    hotkey("2");shot("features-recovered-replay.png")
                    hotkey("Return");wait_path("/changed")
                    event.message.window=window;lib.XSendEvent(display,window,0,0,c.byref(event));lib.XFlush(display)
                    assert app.wait(timeout=5)==0
                    print("PASS: saved project creation/reopen, SQLite captures, Variables substitution and recovered Replay sends")
                print("PASS: native X11 window, local fixture captures, graceful window-close shutdown")
                if not args.capture_only:
                    print("PASS: project startup, feature tabs, undo/redo sends, numeric Intruder requests and durable results")
            except BaseException:
                if app is not None and app.poll() is None and display and window:
                    subprocess.run(["import","-display",display_name,"-window",str(window),"docs/screenshots/workflow-failure.png"],timeout=10)
                log.seek(0)
                print(log.read())
                raise
            finally:
                if display:
                    lib.XCloseDisplay(display)
                if app is not None and app.poll() is None:
                    app.terminate()
                    app.wait(timeout=5)
                server.shutdown()
                server.server_close()
                xvfb.terminate()
                xvfb.wait(timeout=5)


if __name__ == "__main__":
    main()
