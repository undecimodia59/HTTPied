"""Verify automatic NSS setup with real Chromium and a local HTTPS fixture; no trust bypass flags."""
import http.server
import json
import os
from pathlib import Path
import shutil
import ssl
import subprocess
import tempfile
import threading

def main():
    browser=shutil.which("chromium") or shutil.which("google-chrome")
    if not browser:
        raise RuntimeError("Chromium is required")
    with tempfile.TemporaryDirectory(prefix="httpied-trust-smoke-") as directory:
        root=Path(directory);project=root/"project";home=root/"home"
        setup=subprocess.run(["cargo","run","--quiet","--release","--features","tls-spike","--example","setup_test_ca","--",str(project),str(home)],capture_output=True,text=True,check=True,timeout=120)
        status=json.loads(setup.stdout)
        ext=root/"server.ext";ext.write_text("subjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:false\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
        key=root/"server.key";csr=root/"server.csr";cert=root/"server.pem"
        subprocess.run(["openssl","req","-newkey","rsa:2048","-nodes","-subj","/CN=127.0.0.1","-keyout",str(key),"-out",str(csr)],check=True,capture_output=True,timeout=15)
        subprocess.run(["openssl","x509","-req","-in",str(csr),"-CA",str(project/"ca.pem"),"-CAkey",str(project/"ca.key.pem"),"-CAcreateserial","-days","1","-sha256","-extfile",str(ext),"-out",str(cert)],check=True,capture_output=True,timeout=15)
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                body=b"<html><body>HTTPied trusted CA fixture</body></html>"
                self.send_response(200);self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
            def log_message(self,*args):pass
        server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(cert,key)
        server.socket=context.wrap_socket(server.socket,server_side=True)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        env={**os.environ,"HOME":str(home),"XDG_CONFIG_HOME":str(home/".config"),"XDG_DATA_HOME":str(home/".local/share")}
        env.pop("WAYLAND_DISPLAY",None)
        try:
            result=subprocess.run([browser,"--headless=new","--dump-dom","--disable-gpu","--no-first-run","--no-default-browser-check","--disable-background-networking",f"--user-data-dir={root/'browser'}",f"https://127.0.0.1:{server.server_port}/"],env=env,capture_output=True,text=True,timeout=30)
            if result.returncode or "HTTPied trusted CA fixture" not in result.stdout:
                raise RuntimeError(f"Chromium did not trust the installed CA: {result.returncode}\n{result.stderr[-3000:]}\n{result.stdout[-1000:]}")
            print(json.dumps({"browser":subprocess.check_output([browser,"--version"],text=True).strip(),"store":status["store"],"result":"PASS: Chromium verified a local HTTPS fixture using the automatically installed CA; sandbox and certificate verification stayed enabled"}))
        finally:
            server.shutdown();server.server_close();thread.join()

if __name__=="__main__":main()
