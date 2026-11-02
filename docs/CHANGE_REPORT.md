# History Show WS toggle — 2026-11-02

Added **Show WS** immediately beside **In scope only**. Enabled by default, including older
projects. Turning it off hides WebSocket handshakes/messages from live and saved History;
recording continues. The preference persists with the project's filters and presets.
Changed `src/state.rs`, `src/ui/features.rs`, `src/project.rs` and the native smoke script.
The regression covers fast/combined filters, pagination, retained payloads and project reopen.
Validation: 52 library tests and all-target Clippy pass; native checkbox persistence/placement
checked with `python3 scripts/feature_smoke.py --history-only`. Linux binaries/packages rebuilt.
Windows interaction remains unverified; next operator check is WS visibility on real traffic.

# Logs, protocol labels and Auto decoding — 2026-11-02

Application errors/warnings now live in a resizable **Logs** drawer at the bottom, with unread
counts, search, severity filters, copy, clear and repeated-message counts. Retention is bounded
at 500 entries / 4 KiB per message, persisted on the project worker. Existing Error rows are
copied once to an additive table without deleting their source records; History queries and
traffic exports exclude them. HTTP 4xx/5xx responses remain in History. Execution failures
appear in Logs and retain their per-run Intruder result.

Protocol now shows **HTTP/2**, with stream numbers in an optional **Stream ID** column and
inspector metadata. Stream sorting is numeric across the complete saved History.

Auto/Pretty decode gzip/x-gzip, zlib/raw deflate, Brotli, Zstandard, stacked/repeated encoding
headers and chunk framing. Declared text charsets and UTF-16 BOMs are supported. Display views
can recognize gzip/Zstandard magic without headers. Readable text displays automatically;
binary, corrupt, unsupported or oversized bodies fall back to captured bytes with a note.
Captured headers/wire bytes remain intact. Input is capped at 2 MiB, decoded stages at 256 KiB,
text preview at 64 KiB and Zstandard window at 8 MiB.

Main files: new `src/activity.rs`, `src/decoding.rs`, `src/ui/log_view.rs`; updated
`src/project.rs`, `src/state.rs`, `src/exchange.rs`, `src/ui.rs`, `src/ui/features.rs`,
`src/ui/workflow.rs`, Cargo dependencies/lockfile and native smoke/documentation.

Validation: 93 tests pass in both debug and release, including legacy log migration/clear,
Unicode message bounds, saved-page stream sorting, all codecs, binary fallback, corruption and
expansion limits. All-target Clippy, headless check, formatting and diff checks pass. Native
X11 input tests verify gzip JavaScript, binary fallback, a real refused local upstream persisted
only in Logs, drawer search/copy, project recovery, Replay and Intruder. Screenshots:
`docs/screenshots/revision4-logs-filtered.png`, `revision4-logs.png`, `revision4-binary-fallback.png`.

Commands: `cargo test --locked --features ui,debug-logging`,
`cargo test --locked --release --features ui,debug-logging`,
`cargo clippy --locked --features ui,debug-logging --all-targets -- -D warnings`,
`cargo check --locked --no-default-features`, `cargo fmt --all -- --check`,
`git diff --check`, `python3 scripts/feature_smoke.py`,
`bash scripts/package-linux.sh`, `bash scripts/package-debug-linux.sh`.

Linux executables/archives are rebuilt. Windows/Wayland interaction remains untested here.
Next bounded validation: test this diagnostic build against the operator's browser/lab traffic,
then check Windows log-drawer keyboard/clipboard behavior. Existing detailed trace files remain
separate from the application drawer. Decoding is a display transform, not outgoing recompression.

# HTTP/2 Cookie translation fix — 2026-11-02

The supplied 98028 session and its read-only project captures confirm the lab's exact error:
`{"error":"Duplicate header names are not allowed"}`. All 37 HTTP 400 requests contain two
Cookie fields, with no other duplicate names or configured rewrites. HTTPied's HTTP/2-to-HTTP/1
adapter now joins Cookie values with semicolon-space as required by RFC 9113 section 8.2.3,
and records that normalization in capture transformations. This fixes a demonstrated
translation defect; the [investigation](INVESTIGATION_HEADERS.md) records evidence and bounds.

Changed files: `src/protocols.rs`, new `tests/h2_cookies.rs`, verified TLS/HTTP2 coverage in
`tests/project_https.rs`, and raw HTTP/1 fidelity coverage in `tests/proxy.rs`. A strict local
fixture failed with status 400 before the fix and passes with 200 after it. Tests cover
absent/single/multiple Cookie fields and preserve pair ordering, repeated cookie names,
non-Cookie duplicates, response Set-Cookie fields and intentionally duplicate raw HTTP/1
Cookie lines. The real verified-TLS fixture validates split cookies over nine HTTP/2 streams.

No public lab requests were sent. The new session has no DNS timeouts. Four downstream TLS
handshake EOFs for browser background services remain a separate diagnostic issue.

Validation: all 87 tests pass in debug and release; formatting, diff checks, all-feature
all-target Clippy and the headless check pass. Instrumented verified TLS/HTTP2 tests record
nine Cookie normalization events and successful streams with no log drops. Both Linux build
and package scripts refresh the ordinary/debug executables and checksums. Commands:
`cargo test --locked --all-features`, `cargo test --locked --release --all-features`,
`cargo clippy --locked --all-features --all-targets -- -D warnings`,
`cargo check --locked --no-default-features`, `bash scripts/package-linux.sh`, and
`bash scripts/package-debug-linux.sh`.

## Previous resolver timing follow-up

The AdGuard processing times do not establish where HTTPied's system-resolver wait was spent.
The updated diagnostic executable records worker queue time separately from OS resolver
execution and logs late completion after request expiry. The local queued-worker regression
and verified HTTPS/HTTP2 logging fixture pass; all 86 Rust tests pass in debug/release, as do
Clippy, formatting, diff checks and the headless build check. Native diagnostic capture smoke
also passes with both logs flushed without drops. Files: `src/resolver.rs`, the investigation
and debugging guides, with the rebuilt diagnostic Linux package.

One additional configuration check concerns the operator's AdGuard rate-limit value:
[official documentation](https://github.com/AdguardTeam/KnowledgeBaseDNS/blob/master/docs/adguard-home/configuration.md#dns)
says its default is 20 queries/second and excess queries are silently dropped. This is a
possibility to test against the lookup burst. The operator confirmed the previous value was
20 and changed it to 0 on 2026-11-02. The log records 17 same-host lookups starting in 6.144 ms;
separate A/AAAA queries could exceed that limit. Dropped packets and causation remain
unconfirmed until further evidence/retest. Server processing times alone cannot identify drops.

## Supplied-log investigation and shared DNS resolution — 2026-11-01

Follow-up evidence: the operator's AdGuard screenshot shows fast A/AAAA processing. The trace
only establishes that HTTPied awaited the system resolver at expiry; it does not establish
AdGuard latency. Added blocking-worker queued/start/finish timing in `src/resolver.rs`, using
the same standard ToSocketAddrs operation that Tokio delegates to. A local regression
deliberately occupies the only blocking worker, observes a lookup deadline before resolution
starts, then verifies recovery. This is a possible mechanism, not the established historical
cause. No timeout, nameserver or address-order policy was changed for this instrumentation.

The supplied trace and rotated siblings establish 11 DNS-stage deadline failures before
any TCP attempt. Nearby requests complete DNS/TCP/TLS successfully. HTTPied was asking the
system resolver separately for each HTTP/2 stream. [Investigation](INVESTIGATION_DNS.md)
records timestamps, exact source lines, supported conclusions and limits of the evidence.
The first reported TLS EOF has rotated out and remains unexplained.

Added `src/resolver.rs` and shared Config state: concurrent same-host/port lookups coalesce;
successful answers are cached for 30 seconds, errors for 500 ms. State is bounded to 128
entries/32 addresses per answer. IP literals bypass DNS. A cancelled waiter does not cancel
the shared result; cache entries expire and TCP failures invalidate answers. DNS and TCP
retain their combined configured deadline, sequential address order and TLS verification.
DNS deadline errors now explicitly say that no TCP connection was attempted.

Diagnostic builds also write a compact application-only `.summary.log` with its own 8 MiB
files/three backups, independent of full dependency TRACE rotation. Files changed:
`src/resolver.rs`, `src/proxy.rs`, `src/lib.rs`, `src/diagnostics.rs`,
`tests/project_https.rs`, the workflow/debug guides and ADR-028. The mitigation removes
duplicate system-lookup pressure; it does not prove why the external/system resolver stalled.

Local regressions cover delayed concurrent lookup sharing, cached reuse when another lookup
would hang, first-waiter cancellation, DNS timeout classification, invalidation, expiry,
bounded storage and literals. The local verified TLS/HTTP2 fixture uses localhost DNS and
eight simultaneous additional streams. No requests were sent to the public lab during the
investigation. Next validation is the operator's reproduction using the rebuilt debug binary,
including its summary file to retain any new TLS EOF.

The instrumented local fixture verified exactly one system lookup for localhost across
verified HTTPS and nine HTTP/2 streams, with cached reuse and no dropped log entries.
Its full trace was 13,351,788 bytes; application summary was 68,927 bytes. These are diagnostic
fixture sizes, not performance benchmarks. Validation commands: fmt/diff checks, all-feature
all-target Clippy, debug/release all-feature tests, no-default-feature check, release binary
builds and native capture smoke. Both Linux package scripts refresh binaries/checksums.
All 85 Rust tests pass in both debug and release. The rebuilt diagnostic native window also
captures local HTTP traffic and flushes both log queues with zero dropped entries/write errors.

## Previous verbose diagnostic build

The operator requests a diagnostic executable before investigating the reported TLS EOF and
HTTP/2 connection-timeout flood. No root cause is asserted and no transport fix is applied.
`httpied-debug` runs the same CLI/native UI with trace-level application/dependency logs,
timestamps, source/thread details, connection IDs and HTTP/2 stream IDs. Records distinguish
DNS results, sequential TCP address attempts, upstream/downstream TLS, ALPN/cipher/version,
HTTP response phases, stream resets and job attempts. Current connection/timeouts/retry/reset
policies and TLS verification are preserved. The ordinary executable keeps Tokio's original
connection call; diagnostic dialing exposes the same sequential policy when debug logging is on.

Files: `src/bin/httpied-debug.rs`, shared `src/cli.rs`, `src/diagnostics.rs`, optional Cargo
features/dependencies, and tracing additions in proxy/authority/protocols/jobs/UI trust setup.
Asynchronous logging uses 8,192 queue entries and four 32 MiB files per session; overflow/write
errors are visible. CLI startup prints the log path and orderly shutdown flushes the writer.
See [diagnostic instructions](DEBUGGING.md) and ADR-027.

Validation includes the 81-test suite, bounded-file rotation, all-feature/all-target Clippy,
format/diff checks and a release build of the diagnostic executable. Native X11 smoke ran
the actual diagnostic UI, captured local HTTP fixture traffic and verified trace records,
connection IDs, DNS/TCP/HTTP phases and orderly log flush with zero dropped entries/write errors.
A local verified TLS/HTTP2 fixture produced upstream/downstream handshake and correlated
stream diagnostics without dropped entries. These local checks do not identify the cause of
the operator's lab-server failures; the next task is to inspect their reproduction logs.
A real diagnostic headless process also reproduced a controlled local TLS handshake EOF;
the log classified it as an upstream TLS failure and retained the connection/DNS/TCP context.

Build command: `cargo build --locked --release --features ui,debug-logging --bin httpied-debug`.
The diagnostic Linux archive is separate from the ordinary application archive.

## Previous History columns and compressed response display

The latest three requests are implemented:

1. **Host**, **Endpoint** and **Destination** are independent History columns. Host contains
   the hostname; Endpoint contains the encoded path/query; Destination retains the complete
   target URL. Columns enables any combination. New projects show Host/Endpoint; existing
   projects retain their column selection. Both new columns sort across the saved result set.
2. Drag a header's right edge to resize; double-click for automatic width. Widths persist
   across hide/reorder/reopen, with a Reset all widths action. Wide tables scroll horizontally
   with a shared header/row layout.
3. Gzip response bytes were displayed through the default raw-text preview. This was a view
   issue, not evidence of TLS corruption. **Auto** now decodes gzip/deflate/Brotli and chunked
   bodies in History and Replay/Intruder, with captured headers and a visible decoding note.
   Pretty decodes first too. Raw/Hex keep original encoded bytes. Truncated, unsupported,
   binary or oversized decoded content reports a view error; forwarding/storage are unchanged.

Principal files: `src/state.rs`, `src/project.rs`, `src/exchange.rs`, `src/ui.rs`,
`src/ui/features.rs`, `src/ui/workflow.rs`, `tests/project_https.rs`, and
`scripts/feature_smoke.py`. ADR-026 records the shared layout, bounded decoding and migration.
Project metadata upgrades transactionally to schema 2 with indexed host/endpoint columns;
backfill reads 128 metadata rows at a time on the storage worker and leaves bodies intact.
Executables supporting only schema 1 cannot reopen upgraded projects.

Verification: 80 Rust tests pass in debug/release; fmt, diff checks, all-target/all-feature
Clippy and the no-default-feature check pass. The HTTPS integration fixture verifies gzip
wire bytes and readable decoded JavaScript using verified TLS. Regression tests cover
encoded paths/queries, IPv6, migration, sorting beyond the first saved page, saved widths,
truncated gzip, bounded expansion and unchanged captures.

Native X11 smoke passes real header dragging, saved-width assertions, default Auto response
copying for a local gzip JavaScript fixture, then existing scope/Replay/Intruder/reopen flows.
Screenshots: `revision3-columns-resized.png`, `revision3-compressed-js-auto.png`.
Local warm native sample: 101.16 ms first-window detection, 1,180.01 ms readiness including
create-form interactions, 111,908 KiB idle RSS under Xvfb/software OpenGL. These are not
cold-start or general platform guarantees. Windows/Wayland interaction remains unverified;
zstd decoding and decoder limits remain documented in the workflow guide.

Commands for this slice: `cargo fmt --check`, `git diff --check`,
`cargo clippy --locked --all-features --all-targets -- -D warnings`,
`cargo test --locked --all-features`, `cargo test --locked --release --all-features`,
`cargo check --locked --no-default-features`, `cargo build --locked --release --features ui`,
`python3 scripts/feature_smoke.py`, and `bash scripts/package-linux.sh`.

## Previous workspace UI revision

The nine requested fixes are implemented in the native desktop workspace:

1. Target is now **Settings → Scope**.
2. New domain rules include subdomains, including Web Security Academy lab hosts.
   History's **In scope only** flag applies current rules to saved and live traffic;
   scope edits invalidate the saved-history query automatically. The Apply button is removed.
   Legacy display-only rules migrate; enforced execution scopes retain exact-host boundaries.
3. Variables are in Settings with a readable key/value table.
4. Intruder results appear below its editors with payload/error search, status/error/match
   filters, latest/all-run selection, paging, CSV export and sorting across the saved result set.
5. Settings has consistent sidebar subsections, including Shortcuts, Browser and Project.
6. Desktop HTTPS is always enabled. Project opening installs the public CA into current-user
   Chromium NSS trust on Linux or CurrentUser Root on Windows. Setup runs on the storage
   worker and surfaces failures. Browser launch waits for setup and the listener.
7. Interception conditions have readable input widths and format examples.
8. History Filters and Columns are separate dialogs with aligned controls and text reorder buttons.
9. Navigation is reduced to six task tabs. Intruder's advanced controls and inspector view/compare
   options are grouped, and redundant History action buttons are removed.

Principal revision files: `src/ui/settings.rs`, `src/ui/features.rs`, `src/ui/workflow.rs`,
`src/ui.rs`, `src/state.rs`, `src/project.rs`, `src/jobs.rs`, `src/trust.rs`,
`scripts/feature_smoke.py`, `scripts/trust_smoke.py`, and `examples/setup_test_ca.rs`.
ADR-025 records the operator-authorized change to automatic current-user certificate setup.
Linux CI installs NSS tools. Documentation and the Linux archive are refreshed.
Release validation also exposed a project-lock release race with concurrent subprocess
startup. Project close now explicitly unlocks before closing its descriptor; the reopen
regression retains a cloned descriptor to model fork-time inheritance.

## Earlier project feature delivery

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
python3 scripts/feature_smoke.py --layout-only
python3 scripts/trust_smoke.py
bash scripts/package-linux.sh
```

The Rust suite covers 75 tests in debug/release, including scope defaults/migration/query
invalidation, full-run Intruder sorting/filtering, idempotent isolated NSS setup, a 10 MiB HTTP response retained
at 1 KiB, a 3 MiB HTTP/2 response retained at 1 MiB, verified HTTPS Replay, WebSocket messages,
import rollback, project locking/paging/body loading and cancellation under result backpressure.
Native X11 smoke verifies creation/opening, capture, Undo/Redo followed by actual sends,
variable substitution, padded numeric Intruder requests, durable results and recovered Replay
sends after reopening. Scope screenshots confirm hidden/restored saved captures with the
History flag enabled. Layout screenshots cover all Settings subsections and separate
filter/column dialogs. Current images are `docs/screenshots/revision2-*.png` and populated
`docs/screenshots/features-*.png`.

The real Chromium trust smoke passed on Chromium 156.0.8078.47: it loaded a local HTTPS
fixture signed by the automatically installed project CA. It uses the browser sandbox and
certificate verification, with no bypass flags. Both native/trust harnesses use isolated
test trust databases and leave the operator's ordinary certificate store untouched.

The final revised native build measured 101.69 ms to first-window detection, 1,071.00 ms
to readiness including automated create-form interactions, and 112,184 KiB idle RSS under
Xvfb/software OpenGL. This is a local warm sample, not a cold-start/platform guarantee.

## Measurements and limits of evidence

Reference environment: x86_64 Linux 7.3.0-1-default / glibc 2.44, AMD Ryzen AI 7 350,
16 logical CPUs, Rust 1.99.0. Release builds; warm filesystem/OS cache. These are local
measurements, not general hardware or platform guarantees. The following performance samples
are from the preceding project baseline, before this UI revision. Final samples overlapped native
smoke/build activity. Raw data: benchmark-projects-linux.json and benchmark-http-projects-linux.json.
Those benchmarks used `python3 scripts/bench.py --runs 500` and
`cargo run --release --example project_bench -- 100000`; they were not rerun for the UI revision.

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
WebPKI roots. Linux automatic Chromium trust requires NSS `certutil`; other browsers may use
different stores. Installed public CA roots remain in the current-user store after project
close/deletion; removal/rotation UI is deferred. Temporary projects delete on orderly exit;
a crash can leave their directory for manual cleanup.

Text/hex/decoded/diff views have explicit preview/expansion limits. Project quotas account
for retained content, not every SQLite/index filesystem byte. Complex history/result filters
scan metadata on the bounded storage worker; result sorting covers the saved selected run.
Variables/traffic are local plaintext; redacted exports cannot identify arbitrary secrets
embedded in URLs/custom headers. HAR timestamps unavailable from capture are marked with
a sentinel and annotation.

Windows/Linux CI definitions are supplied, but Windows execution, Windows packaging,
Wayland/IME and screen-reader interaction have not run in this environment. Linux packaging
is an unsigned native archive; signing/installers and a long mixed-protocol soak remain
release work. The previous milestone report is archived as CHANGE_REPORT_M0.md.

Recommended next bounded validation: Windows/Wayland browser-trust and clipboard/IME testing,
then a mixed HTTPS/HTTP2/WebSocket soak with captured disk/RAM/frame-time measurements.
