# M0 fixed-authority TLS interception experiment

Build with `cargo build --locked --release --features tls-spike`. The default binary and
all connections without the explicit option retain opaque CONNECT behavior.

Example using an authorized local TLS fixture on port 8443:

```sh
cargo run --locked --release --features tls-spike -- --headless \
  --tls-intercept localhost:8443 --tls-ca-out ./httpied-test-ca.der \
  --tls-upstream-ca ./fixture-ca.der
openssl x509 -inform DER -in httpied-test-ca.der -out httpied-test-ca.pem
curl --noproxy "" --proxy http://127.0.0.1:8080 \
  --cacert httpied-test-ca.pem https://localhost:8443/
```

`fixture-ca.der` must contain the explicitly trusted public upstream CA certificate. Omit
`--tls-upstream-ca` to use the WebPKI root set alone. The origin must offer TLS for the
configured hostname. The command is an operator recipe, not a claim that this particular
fixture server is started automatically. Automated fixtures run with:

```sh
cargo test --locked --all-features
cargo test --locked --release --features tls-spike
```

The exported DER file is a **public certificate only**. The path must not exist; the program
refuses to overwrite it. Each process creates a new CA and leaf, valid from five minutes
before generation until one day after generation. The private keys stay in memory; they
are never logged, exported, persisted or installed. Remove the old public test certificate
before a later run, or choose a fresh output filename. Point a test client at the exported
certificate explicitly; no OS trust-store installation is necessary.

Interception matches exactly one CONNECT authority, including its port. Other authorities
still use passthrough. The proxy verifies the upstream chain, hostname/IP SAN and validity
period before sending CONNECT success. Upstream handshake failure produces HTTP 502 plus
an error event. After CONNECT success, client rejection or handshake timeout produces an
error event and closes the connection; no second HTTP response is injected into TLS.

Both TLS handshakes use the configured connect timeout (5 seconds by default), and the decrypted tunnel
expires after 300 seconds of inactivity rather than a fixed total lifetime. Shutdown cancels pending handshakes and copies.
ALPN offers HTTP/1.1 only. Decrypted application bytes stream through bounded copy buffers
and an `InterceptedTls` event reports their counts. TLS record bytes are not preserved,
and decrypted HTTP is not parsed, retained, edited or governed by the cleartext body cap.
The lifetime deadline and stream buffers bound this transport experiment; general HTTPS
capture, per-host certificate caching, secure persistent CA lifecycle, traffic scope policy,
HTTP/2 and manual interception remain future work.

Tests cover trusted HTTP roundtrip, generated leaf SAN, untrusted client CA, upstream
unknown issuer/hostname mismatch/expiry, both handshake deadlines, and cancellation during
the downstream handshake. Existing tunnel and cleartext proxy tests run alongside them.
No public server or OS trust store is used by the tests. Windows runtime behavior remains
unverified in this environment.

Implementation references: [rcgen certificate signing](https://docs.rs/rcgen/0.14.10/rcgen/struct.CertificateParams.html)
and [tokio-rustls accept/connect](https://docs.rs/tokio-rustls/0.26.6/tokio_rustls/).
