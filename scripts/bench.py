"""Local-only M0 harness: python3 scripts/bench.py [--binary target/release/httpied]."""
import argparse
import contextlib
import http.server
import json
import os
import platform
import signal
import socket
import statistics
import subprocess
import threading
import time


class Fixture(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, *_):
        pass


@contextlib.contextmanager
def proxy(binary):
    start = time.perf_counter()
    process = subprocess.Popen([binary, "--headless", "--listen", "127.0.0.1:0"],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               text=True)
    try:
        # A queue bounds the readiness wait even if a regression never emits READY.
        import queue
        lines = queue.Queue(maxsize=1)
        threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True).start()
        line = lines.get(timeout=10)
        if not line.startswith("READY "):
            raise RuntimeError(f"proxy startup failed: {line!r}")
        port = int(line.strip().rsplit(":", 1)[1])
        yield process, port, (time.perf_counter() - start) * 1000
    finally:
        if process.poll() is None:
            if os.name == "posix":
                process.send_signal(signal.SIGINT)
            else:
                process.terminate()  # Windows harness termination; Ctrl+C is manual shutdown test.
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        process.stdout.close()


def exchange(port, request):
    start = time.perf_counter_ns()
    with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
        sock.sendall(request)
        received = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            received += chunk
    if not received.startswith(b"HTTP/1.1 200") or not received.endswith(b"ok"):
        raise RuntimeError(f"unexpected response: {received!r}")
    return (time.perf_counter_ns() - start) / 1e6


def distribution(samples):
    ordered = sorted(samples)
    return {"median_ms": statistics.median(samples),
            "p95_ms": ordered[min(len(ordered) - 1, int(len(ordered) * .95))]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/release/httpied")
    parser.add_argument("--runs", type=int, default=1000)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host = f"127.0.0.1:{server.server_port}"
    direct = f"GET /sample?q=%20 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n".encode()
    proxied = direct.replace(b"GET /", f"GET http://{host}/".encode(), 1)
    try:
        startup = []
        for _ in range(10):
            with proxy(args.binary) as (_, _, elapsed):
                startup.append(elapsed)
        with proxy(args.binary) as (process, port, _):
            time.sleep(.2)
            rss = None
            if os.path.exists(f"/proc/{process.pid}/status"):
                with open(f"/proc/{process.pid}/status") as status:
                    rss = next((line.strip() for line in status if line.startswith("VmRSS:")), None)
            pairs = [(exchange(server.server_port, direct), exchange(port, proxied)) for _ in range(args.runs)]
            print(json.dumps({"os": platform.platform(), "cpu": platform.processor(),
                "logical_cpus": os.cpu_count(), "toolchain": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                "binary": args.binary, "runs": args.runs, "fixture": host,
                "mode": "release expected; headless, bounded capture drained, 2-byte response",
                "startup_condition": "10 fresh process launches; OS cache NOT flushed; warm filesystem",
                "startup_to_ready": distribution(startup), "idle_rss_linux": rss,
                "direct": distribution([p[0] for p in pairs]),
                "proxied": distribution([p[1] for p in pairs]),
                "paired_overhead": distribution([p[1]-p[0] for p in pairs])}, indent=2))
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
