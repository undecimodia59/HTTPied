"""Linux native smoke on a private Xvfb display; requires Xvfb, libX11, libXtst and ImageMagick.
Run after: cargo build --release --features ui
"""
import argparse
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
    args = parser.parse_args()
    observed=queue.Queue()
    class Tracked(Fixture):
        def do_GET(self):
            observed.put((self.path,self.headers.get("User-Agent","")))
            super().do_GET()
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
                app = subprocess.Popen(["target/release/httpied", "--listen", "127.0.0.1:0"], env=env, stdout=log, stderr=log)
                display = lib.XOpenDisplay(display_name.encode())
                if not display:
                    raise RuntimeError("cannot open private X display")
                window = None
                port = None
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
                    if window and port:
                        break
                    time.sleep(.1)
                if not window or not port:
                    raise RuntimeError("native window or listener missing")
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
                    # Intercept a real local request, edit its path, and forward it.
                    click(120,124)
                    response=queue.Queue()
                    def request():
                        try:
                            with socket.create_connection(("127.0.0.1",int(port)),timeout=10) as sock:
                                sock.sendall(f"GET http://{origin}/before HTTP/1.1\r\nHost: {origin}\r\n\r\n".encode())
                                data=b""
                                while True:
                                    chunk=sock.recv(4096)
                                    if not chunk:break
                                    data+=chunk
                                response.put(data)
                        except BaseException as error:response.put(error)
                    threading.Thread(target=request,daemon=True).start()
                    time.sleep(.5)
                    shot("workflow-held.png")
                    # Coordinates for held editor and Forward are adjusted from native screenshots.
                    paste(200,300,f"GET /edited HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                    shot("workflow-edited.png")
                    click(326,209)
                    # The script fails rather than claiming success if the request is still held.
                    result=response.get(timeout=5)
                    assert isinstance(result,bytes),result
                    assert b"200 OK" in result and b"/edited" in result,result
                    click(120,124)  # Intercept OFF.
                    click(215,65)  # History tab.
                    shot("workflow-history.png")
                    click(400,425,3)  # RMB on edited capture.
                    shot("workflow-context.png")
                    click(470,441)  # Send to Replay.
                    shot("workflow-context-replay.png")
                    # Replay is sent to the local fixture directly, not back through the proxy.
                    click(355,65)
                    shot("workflow-replay-empty.png")
                    paste(230,163,f"http://{origin}")
                    paste(200,350,f"GET /replayed HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                    click(485,163)
                    wait_path("/replayed")
                    time.sleep(.3)
                    shot("workflow-replay.png")
                    click(500,65)  # Intruder tab.
                    paste(230,123,f"http://{origin}")
                    paste(250,335,f"GET /payload/{{{{payload}}}} HTTP/1.1\r\nHost: {origin}\r\n\r\n")
                    shot("workflow-intruder-ready.png")
                    click(495,123)
                    for payload in ("one","two","three"):wait_path(f"/payload/{payload}")
                    time.sleep(.3)
                    shot("workflow-intruder.png")
                    click(640,65)
                    shot("workflow-browser-empty.png")
                    paste(250,219,f"http://{origin}/browser")
                    click(80,257)
                    time.sleep(2)
                    shot("workflow-browser-launch.png")
                    agent=wait_path("/browser",15)
                    assert "Chrome" in agent,agent
                    time.sleep(.5)
                    shot("workflow-browser.png")
                    lib.XRaiseWindow.argtypes=[c.c_void_p,c.c_ulong]
                    lib.XRaiseWindow(display,window)
                    lib.XSetInputFocus(display,window,2,0)
                    lib.XFlush(display)
                    click(215,65)
                    paste(450,199,"browser")
                    shot("workflow-browser-history.png")
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
                print("PASS: native X11 window, local fixture captures, graceful window-close shutdown")
                if not args.capture_only:
                    print("Exercised native workflow tabs, edited interception, Replay and browser launcher; inspect saved screenshots")
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
