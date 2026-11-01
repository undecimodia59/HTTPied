"""Local TLS through CONNECT, including untrusted-cert failure. Requires openssl CLI."""
import os
import socket
import ssl
import subprocess
import tempfile
import threading
from bench import proxy


def main():
    with tempfile.TemporaryDirectory() as directory:
        cert, key = [os.path.join(directory, name) for name in ("cert.pem", "key.pem")]
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                        "-keyout", key, "-out", cert, "-days", "1", "-subj", "/CN=localhost",
                        "-addext", "subjectAltName=DNS:localhost"], check=True, capture_output=True)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(cert, key)
        errors = []
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(5)
            host = f"127.0.0.1:{listener.getsockname()[1]}"

            def serve():
                try:
                    for _ in range(2):
                        raw, _ = listener.accept()
                        raw.settimeout(5)
                        try:
                            with context.wrap_socket(raw, server_side=True) as tls:
                                assert tls.recv(10) == b"ping"
                                tls.sendall(b"pong")
                        except ssl.SSLError:
                            raw.close()  # expected client rejection in second case
                except BaseException as error:
                    errors.append(error)

            worker = threading.Thread(target=serve)
            worker.start()
            with proxy("target/release/httpied") as (_, port, _):
                for trusted in (True, False):
                    client_context = ssl.create_default_context(cafile=cert if trusted else None)
                    with socket.create_connection(("127.0.0.1", port), timeout=5) as raw:
                        raw.sendall(f"CONNECT {host} HTTP/1.1\r\nHost: {host}\r\n\r\n".encode())
                        head = b""
                        while not head.endswith(b"\r\n\r\n"):
                            chunk = raw.recv(1)
                            if not chunk:
                                raise RuntimeError("CONNECT closed prematurely")
                            head += chunk
                        assert head == b"HTTP/1.1 200 Connection Established\r\n\r\n"
                        try:
                            with client_context.wrap_socket(raw, server_hostname="localhost") as tls:
                                assert trusted, "untrusted certificate accepted"
                                tls.sendall(b"ping")
                                assert tls.recv(10) == b"pong"
                        except ssl.SSLCertVerificationError:
                            assert not trusted
            worker.join(timeout=10)
            assert not worker.is_alive(), "TLS fixture failed to shut down"
            assert not errors, errors
    print("PASS: trusted TLS roundtrip and untrusted certificate rejection through opaque CONNECT")


if __name__ == "__main__":
    main()
