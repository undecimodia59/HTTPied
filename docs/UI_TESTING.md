# Native UI validation

Build and exercise the current project workflow:

```sh
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py
python3 scripts/trust_smoke.py
```

Requires Linux Xvfb, libX11/libXtst, D-Bus, xclip, NSS `certutil` and ImageMagick. The smoke creates an
isolated display, trust store and local HTTP fixture. It verifies saved project creation, actual
Replay sends after Undo/Redo and cursor movement, Variables substitution, padded numeric
Intruder requests, embedded attributed results, scope edits with the History flag enabled,
orderly shutdown, project reopening and
sending a recovered Replay draft. No public service is contacted.
It also drags a real History header boundary and checks the saved width, then copies the
default Replay response view for a local gzip JavaScript request and asserts readable code
alongside the captured Content-Encoding header. `revision3-columns-resized.png` and
`revision3-compressed-js-auto.png` record these interactions.

`--capture-only` checks temporary project creation/capture/shutdown.
`--layout-only` captures six workspace tabs, Settings subsections and both History dialogs.
Current layout images use `revision2-*.png`; populated workflows use `features-*.png` and
`project-start.png`. Older `revision-*` / `workflow-*` images are historical.
`revision_smoke.py` and `ui_smoke.py` forward to this current harness.

`trust_smoke.py` additionally needs Chromium and OpenSSL. It uses `examples/setup_test_ca.rs`
to create a project CA and install it into an isolated current-user NSS database, then starts
a localhost HTTPS fixture and loads it in real headless Chromium. It checks the response body;
certificate verification and Chromium's sandbox remain enabled. Neither smoke changes the
operator's normal trust database. The tested browser was Chromium 156.0.8078.47 on Linux.

The smoke reports first native-window detection, proxy readiness including automated form
interactions, and idle process RSS. These are Xvfb/software-OpenGL measurements on one
machine, not cold-launch or Windows/Wayland certification.

Network/storage tests separately cover TLS trust/expiry/hostname verification, HTTP/2
streaming beyond the capture cap, bidirectional WebSocket capture, 10 MiB HTTP streaming,
interception/rewrite fidelity, import rollback, paging, scope matching, expansion limits,
attack generation and cancellation while results are backpressured.
`tests/h2_cookies.rs` rejects duplicate Cookie lines at a strict local origin and checks
HTTP/2 split-cookie joining, exact forwarded values, capture annotation, preserved custom
duplicate fields and separate response Set-Cookie fields. The verified TLS/HTTP2 fixture
also sends split cookies on all nine streams; raw HTTP/1 tests retain intentional duplicates.
URL-part migration tests cover multiple saved pages, IPv6, encoded paths/queries and saved
column widths. Verified HTTPS Replay checks both gzip wire bytes and the decoded JavaScript.
The same local fixture also exercises localhost DNS and eight concurrent additional HTTP/2
streams. Resolver tests inject delayed/pending lookups to verify coalescing, cached reuse,
cancellation, expiry and DNS-specific deadlines without querying a public service.

To capture fixture diagnostics into a temporary caller-chosen directory, set
`HTTPIED_TEST_DEBUG_LOGS=1` and `HTTPIED_DEBUG_LOG_DIR=/path/to/test-logs`, then run
`cargo test --locked --all-features --test project_https`. The `.summary.log` should show one
localhost lookup and shared/cached use, with successful TLS and nine completed HTTP/2 streams.

Manual platform checks still needed: Windows file picker/browser cleanup, hardware OpenGL,
Wayland clipboard/IME, screen-reader operation, long-running mixed-protocol workloads.

ADR-030 regression coverage: bounded/coalesced diagnostics, durable log retention/clearing,
legacy Error migration without capture deletion, exclusion from both plain and filtered History
and traffic exports, HTTP status filtering, numeric Stream ID sorting across saved pages,
all supported codecs, stacked/repeated headers, chunk trailers, declared charsets, magic sniffing,
binary fallback, malformed/truncated data and expansion limits. `scripts/feature_smoke.py`
checks native Replay gzip text and binary fallback, induces a refused local upstream, asserts
it is persisted only in app_logs, and captures the bottom drawer/search UI. It uses an isolated
project and trust directory; screenshots are under `docs/screenshots/revision4-*.png`.


## Review regression smoke (2026-11-02)

```sh
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py --review-only --screenshots-dir /tmp/httpied-review-screenshots
python3 scripts/feature_smoke.py --screenshots-dir /tmp/httpied-full-review-screenshots
```

`--review-only` exercises MIME search, Copy URL, context transfer of an unselected saved
capture, an actual Replay send, response Find selection/clipboard output, and a failed send
followed by a Raw view switch. Clipboard sentinels prevent an unchanged clipboard from
passing a selection assertion. The full smoke continues to verify the existing workflow.
`--screenshots-dir` keeps verification output separate from existing repository screenshots.
See [the review report](REVIEW_2026-11-02.md) for findings, checks and remaining limits.
