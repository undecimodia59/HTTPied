"""Local HTTP/1.1 fixture and sample capture generator. Ctrl+C stops the fixture."""
import argparse
import http.client
import http.server
import json
import threading


class Fixture(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        self.respond(b"HTTPied local fixture\n" if self.path == "/" else
                     json.dumps({"path": self.path, "method": "GET", "message": "Capture works"}, indent=2).encode())

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        if length > 1024 * 1024:
            self.send_error(413)
            return
        self.respond(b"Received POST: " + self.rfile.read(length))

    def respond(self, body):
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def seed(proxy_host, proxy_port, origin):
    for method, path, body in [("GET", "/", None), ("GET", "/sample?q=%20", None),
                               ("POST", "/echo", b"hello from the native UI test")]:
        conn = http.client.HTTPConnection(proxy_host, proxy_port, timeout=5)
        try:
            conn.request(method, f"http://{origin}{path}", body=body,
                         headers={"Host": origin, "Connection": "close", "X-HTTPied-Fixture": "local-only"})
            response = conn.getresponse()
            result = response.read()
            if response.status != 200:
                raise RuntimeError(f"proxy returned {response.status}: {result!r}")
            print(f"Captured {method} {path}: {response.status}, {len(result)} response bytes", flush=True)
        finally:
            conn.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--proxy-port", type=int, default=8080)
    parser.add_argument("--port", type=int, default=8081)
    parser.add_argument("--once", action="store_true", help="seed three captures and exit")
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Fixture)
    server.daemon_threads = True
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        origin = f"127.0.0.1:{server.server_port}"
        seed("127.0.0.1", args.proxy_port, origin)
        if not args.once:
            print(f"Fixture ready at http://{origin}/ — Ctrl+C stops it.", flush=True)
            print(f'curl --noproxy "" --proxy http://127.0.0.1:{args.proxy_port} http://{origin}/sample', flush=True)
            threading.Event().wait()
    except KeyboardInterrupt:
        pass
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


if __name__ == "__main__":
    main()
