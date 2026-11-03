"""Linux native smoke on a private Xvfb display; requires Xvfb, libX11, libXtst and ImageMagick.
Run after: cargo build --release --features ui
"""
import argparse
import json
import gzip
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
import http.client
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
    parser.add_argument("--attack-only", action="store_true")
    parser.add_argument("--jwt-only", action="store_true")
    parser.add_argument("--layout-only",action="store_true")
    parser.add_argument("--history-only",action="store_true")
    parser.add_argument("--review-only", action="store_true", help="check History search, saved actions and failed Replay views")
    parser.add_argument("--screenshots-dir", type=Path, default=Path("docs/screenshots"))
    args = parser.parse_args()
    args.attack_only = args.attack_only or args.jwt_only
    executable=os.environ.get("HTTPIED_TEST_BINARY","target/release/httpied")
    observed=queue.Queue()
    class Tracked(Fixture):
        def do_GET(self):
            observed.put((self.path,self.headers.get("User-Agent","")))
            if self.path=="/native-login":
                body=b'{"token":"fresh-native"}'
                self.send_response(200);self.send_header("Content-Type","application/json");self.send_header("Set-Cookie","session=fresh-native; Path=/; HttpOnly");self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            elif self.path=="/native-use/fresh-native":
                valid="session=fresh-native" in self.headers.get("Cookie","") and self.headers.get("X-CSRF")=="fresh-native"
                body=b'sequence-completed' if valid else b'credentials-missing'
                self.send_response(200 if valid else 403);self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            elif self.path=="/native-access":
                valid=self.headers.get("Authorization")=="Bearer high"
                body=b'protected-resource' if valid else b'access-denied'
                self.send_response(200 if valid else 403);self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            elif self.path=="/compressed.js":
                body=gzip.compress(b'window.httpiedCompressedFixture = "decoded JavaScript";\n',mtime=0)
                self.send_response(200);self.send_header("Content-Type","application/javascript; charset=utf-8");self.send_header("Content-Encoding","gzip");self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            elif self.path=="/compressed-binary":
                body=gzip.compress(bytes([0,255,1,2]),mtime=0)
                self.send_response(200);self.send_header("Content-Type","application/octet-stream");self.send_header("Content-Encoding","gzip");self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            elif self.path=="/html":
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
                env["HTTPIED_TEST_TRUST_HOME"]=str(Path(directory)/"trust-home")
                project_path=Path(directory)/"saved-project"
                launched_at=time.monotonic()
                app = subprocess.Popen(["dbus-run-session","--",executable, "--listen", "127.0.0.1:0"], env=env, stdout=log, stderr=log)
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
                        args.screenshots_dir.mkdir(parents=True, exist_ok=True)
                        subprocess.run(["import","-display",display_name,"-window",str(window),str(args.screenshots_dir / "project-start.png")],check=True,timeout=10)
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
                destination = args.screenshots_dir
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
                    if args.attack_only:
                        from attack_smoke import exercise
                        exercise(click,paste,shot,hotkey,key,wait_path,origin,project_path,display_name,jwt_only=args.jwt_only)
                    else:
                        # Current task tabs: layout inspection and actual local CTF workflows.
                        for shortcut,name in [("1","history"),("2","replay"),("3","intruder"),("4","intercept"),("5","coder"),("6","compare"),("7","attack"),("comma","settings")]:
                            hotkey(shortcut);shot(f"glow-{name}.png")
                        if args.layout_only:
                            for y,name in [(174,"scope"),(206,"variables"),(238,"project"),(302,"execution"),(335,"interception"),(400,"network"),(432,"browser"),(465,"certificates"),(531,"display"),(563,"shortcuts")]:
                                click(70,y);shot(f"glow-settings-{name}.png")
                        else:
                            # Selection using Shift+arrows must survive a secondary click.
                            hotkey("2")
                            paste(400,300,"GET /SGVsbG8= HTTP/1.1\r\n\r\n")
                            hotkey("Home")
                            for _ in range(5):key("Right",1);key("Right",0)
                            key("Shift_L",1)
                            for _ in range(8):key("Right",1);key("Right",0)
                            key("Shift_L",0);lib.XFlush(display);time.sleep(.2)
                            click(410,235,3);shot("glow-selection-menu.png")
                            click(510,367);time.sleep(.3);shot("glow-coder-selected.png")
                            click(180,300);hotkey("a");hotkey("c")
                            selected=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert selected=="SGVsbG8=",selected
                            click(160,134);time.sleep(.2)
                            click(800,300);hotkey("a");hotkey("c")
                            decoded=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert decoded=="Hello",decoded
                            shot("glow-coder-decoded.png")
                            # Mouse drag selection uses the same exact transfer path.
                            click(180,300);hotkey("a");paste_selection("prefix SGVsbG8= suffix")
                            xtst.XTestFakeMotionEvent(display,-1,root_x.value+79,root_y.value+220,0)
                            xtst.XTestFakeButtonEvent(display,1,1,0);lib.XFlush(display);time.sleep(.1)
                            for x in range(87,144,8):
                                xtst.XTestFakeMotionEvent(display,-1,root_x.value+x,root_y.value+220,0);lib.XFlush(display);time.sleep(.06)
                            xtst.XTestFakeButtonEvent(display,1,0,0);lib.XFlush(display);time.sleep(.2)
                            click(110,224,3);shot("glow-mouse-selection.png");click(180,244)
                            click(180,300);hotkey("a");hotkey("c")
                            dragged=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert dragged=="SGVsbG8=",dragged
                            # Compare two real message bodies in the paired highlighted view.
                            hotkey("6")
                            paste(180,300,"HTTP/1.1 200 OK\r\nX-Test: one\r\n\r\nallowed=false\n")
                            paste(800,300,"HTTP/1.1 200 OK\r\nX-Test: two\r\n\r\nallowed=true\n")
                            click(185,101);time.sleep(.3);shot("glow-compare-diff.png")
                            click(180,300);hotkey("a");hotkey("c")
                            baseline=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "allowed=false" in baseline and "allowed=true" not in baseline,baseline
                            click(800,300);hotkey("a");hotkey("c")
                            candidate=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "allowed=true" in candidate,candidate
                            # The Replay edit/send loop remains functional after command consolidation.
                            hotkey("2");paste(500,134,f"http://{origin}")
                            paste(400,300,f"GET /compressed.js HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                            hotkey("Return");wait_path("/compressed.js");time.sleep(.5)
                            click(800,400);hotkey("a");hotkey("c")
                            response=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "decoded JavaScript" in response and "Content-Encoding: gzip" in response,response
                            shot("glow-replay-sent.png")
                            paste(400,300,f"GET /glow-replay HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                            time.sleep(.5);hotkey("z");hotkey("Return");wait_path("/compressed.js")
                            click(400,300);hotkey("y");hotkey("Return");wait_path("/glow-replay")
                            # Variables remain directly usable in an actual Replay request.
                            hotkey("comma");click(70,206);click(225,196)
                            paste(250,196,"route");paste(570,196,"/glow-replay");shot("glow-variables-filled.png")
                            hotkey("2");paste(400,300,f"GET {{{{route}}}} HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                            hotkey("Return");wait_path("/glow-replay")
                            # Numeric payload generation and attributed durable results.
                            hotkey("3");paste(300,104,f"http://{origin}")
                            click(180,170);click(180,225)
                            paste(120,203,"0000");paste(300,203,"0002")
                            paste(180,420,f"GET /number/{{{{payload}}}} HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                            shot("glow-intruder-ready.png");click(860,104)
                            for value in ("0000","0001","0002"):wait_path(f"/number/{value}")
                            time.sleep(.5);shot("glow-intruder-results.png")
                            # Select by payload cell, inspect its exact capture, then transfer it.
                            click(200,764);time.sleep(.7);shot("followup-intruder-inspection.png")
                            click(180,400);hotkey("a");hotkey("c")
                            inspected=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "GET /number/0001 HTTP/1.1" in inspected,inspected
                            click(800,400);hotkey("a");hotkey("c")
                            inspected_response=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "/number/0001" in inspected_response,inspected_response
                            click(180,400,3);click(220,420);time.sleep(.4)
                            hotkey("Return");wait_path("/number/0001")
                            # Inspecting a result must never overwrite the Intruder attack template.
                            hotkey("3");click(70,103)
                            click(180,420);hotkey("a");hotkey("c")
                            template=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "{{payload}}" in template,template
                            # Both directions are available from the editors themselves.
                            click(180,420,3);click(220,440);time.sleep(.3)
                            click(400,300);hotkey("a");hotkey("c")
                            replay_template=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert replay_template==template,(template,replay_template)
                            click(400,300,3);click(450,352);time.sleep(.3)
                            click(180,420);hotkey("a");hotkey("c")
                            returned=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert returned==template,(template,returned)
                            # Per-pane wrapping must change only layout; the global default is saved.
                            hotkey("2");click(75,222)
                            paste(400,300,f"GET {{{{route}}}} HTTP/1.1\r\nHost: {origin}\r\nX-Wrap: {'abc' * 300}\r\n\r\n")
                            hotkey("a");hotkey("c")
                            before_wrap=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            click(282,203);shot("followup-replay-wrapped.png")
                            click(400,300);hotkey("a");hotkey("c")
                            after_wrap=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert after_wrap==before_wrap,(before_wrap,after_wrap)
                            hotkey("comma");click(70,531);click(365,178);shot("followup-settings-wrap.png")
                            # History's MIME search must show saved captures and transfer their executed request.
                            hotkey("1");paste(180,101,"javascript");time.sleep(.8);shot("glow-history-mime.png")
                            click(180,177);hotkey("r");time.sleep(.6);shot("glow-history-to-replay.png")
                            click(400,300);hotkey("a");hotkey("c")
                            transferred=subprocess.check_output(["xclip","-display",display_name,"-selection","clipboard","-o"],timeout=5).decode()
                            assert "GET /compressed.js HTTP/1.1" in transferred,transferred
                            # Show a real refusal in the resizable Logs drawer.
                            with socket.socket() as refused:
                                refused.bind(("127.0.0.1",0))
                                conn=http.client.HTTPConnection("127.0.0.1",int(port),timeout=5)
                                endpoint=f"127.0.0.1:{refused.getsockname()[1]}"
                                conn.request("GET",f"http://{endpoint}/refused",headers={"Host":endpoint,"Connection":"close"})
                                conn.getresponse().read();conn.close()
                            time.sleep(.5)
                            height=int(subprocess.check_output(["identify","-format","%h",str(destination/"ui-capture.png")]))
                            click(30,height-14);shot("glow-logs.png");click(30,height-14)
                            # Inspect every task at the minimum supported window size.
                            lib.XResizeWindow.argtypes=[c.c_void_p,c.c_ulong,c.c_uint,c.c_uint]
                            lib.XResizeWindow(display,window,940,700);lib.XFlush(display);time.sleep(.4)
                            for shortcut,name in [("1","history"),("2","replay"),("3","intruder"),("4","intercept"),("5","coder"),("6","compare"),("7","attack"),("comma","settings")]:
                                hotkey(shortcut);shot(f"glow-compact-{name}.png")
                            print("PASS: selection to Coder, Compare, Replay Undo/Redo, variables, per-payload inspection, editor transfers, wrapping, MIME search and compact layouts")
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
                if args.attack_only:
                    from attack_smoke import verify
                    verify(project_path,jwt_only=args.jwt_only)
                if not args.capture_only and not args.layout_only and not args.attack_only:
                    with sqlite3.connect(project_path/"project.sqlite") as database:
                        state=json.loads(database.execute("SELECT json FROM state WHERE id=1").fetchone()[0])
                        assert state["settings"]["wrap_lines"] is True,state["settings"]
                        assert "{{route}}" in state["replays"][0]["request"],state["replays"]
                        assert state["variables"][0]["key"]=="route" and state["variables"][0]["value"]=="/glow-replay",state["variables"]
                        records=[json.loads(row[0]) for row in database.execute("SELECT json FROM results ORDER BY idx")]
                        assert len(records)==3 and [record["payloads"][0] for record in records]==["0000","0001","0002"],records
                        assert all(record["capture_id"] is not None for record in records),records
                        assert database.execute("SELECT count(*) FROM captures WHERE method='ERROR'").fetchone()[0]==0
                        assert database.execute("SELECT count(*) FROM app_logs WHERE json LIKE '%refused%'").fetchone()[0]>=1
                    print("PASS: variable Replay drafts, captures and numeric Intruder attribution saved; connection errors isolated in Logs")
                    log.seek(0);log.truncate()
                    app=subprocess.Popen(["dbus-run-session","--",executable,"--listen","127.0.0.1:0"],env=env,stdout=log,stderr=log)
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
                    assert window,"reopened window missing"
                    lib.XTranslateCoordinates(display,window,lib.XDefaultRootWindow(display),0,0,c.byref(root_x),c.byref(root_y),c.byref(child))
                    lib.XSetInputFocus(display,window,2,0);time.sleep(.5)
                    paste(300,202,str(project_path));click(55,250)
                    limit=time.monotonic()+10
                    while time.monotonic()<limit:
                        log.seek(0)
                        if "READY " in log.read():break
                        time.sleep(.1)
                    else:raise RuntimeError("saved project failed to reopen")
                    hotkey("2");click(75,222);shot("glow-recovered-replay.png")
                    hotkey("Return");wait_path("/glow-replay")
                    event.message.window=window;lib.XSendEvent(display,window,0,0,c.byref(event));lib.XFlush(display)
                    assert app.wait(timeout=5)==0
                    print("PASS: saved project reopened and recovered variable-driven Replay executed")
                print("PASS: native X11 window, local fixture captures, graceful window-close shutdown")
                if args.layout_only:print("PASS: all task tabs and Settings sections captured")
            except BaseException:
                if app is not None and app.poll() is None and display and window:
                    subprocess.run(["import","-display",display_name,"-window",str(window),str(args.screenshots_dir / "workflow-failure.png")],timeout=10)
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
