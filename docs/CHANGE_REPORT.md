# Project workspace feature delivery — 2026-11-01

The selected feature-plan items now have a project-based native implementation.
[Implementation map](IMPLEMENTATION.md) and [workflow](WORKFLOW.md) describe each feature.

## Changes

- Added versioned serde settings/scope/filter/preset/variable/session state and a bounded
  SQLite worker. Startup supports create/open/temporary projects; bodies are separate BLOBs.
  WAL recovery, atomic import, OS project locks, quota reporting and explicit purge are present.
- History has combined filters, target scope, saved presets, configurable ordered columns,
  real timings, numeric sorting, readable units, CONNECT visibility and selected export.
- Replay gains names/groups/order/duplicate/recovery, timeout overrides, resizable panes,
  explicit variable previews, cURL export and bounded text-only Undo/Redo. Cursor movement and
  sending no longer invalidate redo. Closed-editor undo buffers and read-only histories are
  released rather than growing with every inspected capture.
- Intruder has lazy multi-set attack plans, encodings, character sets, reproducible random
  values, bounded concurrency/pacing/retries/backoff, pause/stop/match controls and durable
  paged result metadata. Cancellation closes sockets even with a full result queue.
- HTTPS uses a persistent project CA with per-host leaves and verified upstream rustls TLS.
  Conditional request/response interception and ordered replacements retain original bytes.
- HTTP/2 streams are decoded and forwarded through HTTP/1.1 with bounded flow control;
  large responses stream past capture limits. WebSocket handshakes and messages retain
  direction/close state with explicit frame/message fidelity limits.
- Inspection includes searchable raw/pretty/hex/decoded views and parameter/cookie views.
  Comparison uses bounded line LCS and ignored-header/hex options. HAR exports decode framing
  and compression; imports never execute a shell or expand local file references.
- Added AccessKit, editable shortcuts, package scripts/checksums and Windows/Linux CI.
  Runtime synthetic/debug controls and Replay run-snapshot sections remain absent.

Principal files: `src/state.rs`, `src/project.rs`, `src/authority.rs`,
`src/protocols.rs`, `src/policy.rs`, `src/jobs.rs`, `src/attack.rs`,
`src/interchange.rs`, `src/exchange.rs`, and the `src/ui*` modules.
Protocol/storage/workflow regression tests and the native smoke accompany these changes.

## Verification

Commands used for this revision:

```sh
cargo fmt --check
git diff --check
cargo check --locked --no-default-features
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
cargo test --locked --release --all-features
cargo build --locked --release --features ui
python3 scripts/feature_smoke.py
python3 scripts/bench.py --runs 500
cargo run --release --example project_bench -- 100000
bash scripts/package-linux.sh
```

The Rust suite covers 71 tests in debug/release, including a 10 MiB HTTP response retained
at 1 KiB, a 3 MiB HTTP/2 response retained at 1 MiB, verified HTTPS Replay, WebSocket messages,
import rollback, project locking/paging/body loading and cancellation under result backpressure.
Native X11 smoke verifies creation/opening, capture, Undo/Redo followed by actual sends,
variable substitution, padded numeric Intruder requests, durable results and recovered Replay
sends after reopening. Current screenshots are `docs/screenshots/features-*.png`.

## Measurements and limits of evidence

Reference environment: x86_64 Linux 7.3.0-1-default / glibc 2.44, AMD Ryzen AI 7 350,
16 logical CPUs, Rust 1.99.0. Release builds; warm filesystem/OS cache. These are local
measurements, not general hardware or platform guarantees. Final samples overlapped native
smoke/build activity. Raw data: benchmark-projects-linux.json and benchmark-http-projects-linux.json.

| Workload | Measured result |
|---|---|
| 100,000 SQLite captures | 1.605 s insertion/flush |
| Reopen that project, first 128 metadata rows | 36.46 ms |
| Last metadata page, 20 samples | median 12.99 ms; p95 19.06 ms |
| Storage benchmark process RSS, no renderer | 8,608 KiB |
| Native X11 first-window detection | 102.56 ms |
| Native proxy ready, including automated create-form interactions | 1,072.38 ms |
| Native idle RSS under Xvfb/software OpenGL | 111,584 KiB (109.0 MiB) |
| 500 local headless HTTP exchanges, paired proxy overhead | median 0.032 ms; p95 0.097 ms |
| Headless startup, 10 warm launches | median 0.756 ms; p95 1.304 ms |

The UI samples meet the stated idle-memory/startup targets on this reference setup.
The native readiness number includes deliberate test sleeps/clicks, and first-window detection
does not establish the first fully painted frame. The storage benchmark excludes a renderer
and does not establish complex-filter latency or long-run leak behavior.

## Remaining constraints

HTTP/1 request chunking/Expect/pipelining/keepalive, h2c, extended CONNECT, request trailers,
raw HTTP/2 HPACK/frame capture, WebSocket compression/editing, CA rotation UI and custom
upstream trust configuration remain future work. HTTPS upstream verification uses public
WebPKI roots. Browser trust installation is manual. Temporary projects delete on orderly
exit; a crash can leave their directory for manual cleanup.

Text/hex/decoded/diff views have explicit preview/expansion limits. Project quotas account
for retained content, not every SQLite/index filesystem byte. Results sorting is page-local.
Variables/traffic are local plaintext; redacted exports cannot identify arbitrary secrets
embedded in URLs/custom headers. HAR timestamps unavailable from capture are marked with
a sentinel and annotation.

Windows/Linux CI definitions are supplied, but Windows execution, Windows packaging,
Wayland/IME and screen-reader interaction have not run in this environment. Linux packaging
is an unsigned native archive; signing/installers and a long mixed-protocol soak remain
release work. The previous milestone report is archived as CHANGE_REPORT_M0.md.

Recommended next bounded validation: Windows/Wayland browser-trust and clipboard/IME testing,
then a mixed HTTPS/HTTP2/WebSocket soak with captured disk/RAM/frame-time measurements.
