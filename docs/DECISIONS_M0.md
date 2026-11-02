# M0 implementation decisions — 2026-10-31

## ADR-013 — Bounded raw HTTP/1.1 experiment

Accepted for M0 only. Use one Cargo workspace package with model, proxy and optional UI
modules. Tokio owns async sockets and task lifecycle; httparse validates HTTP syntax and
http::Uri parses routing. These small existing libraries avoid a custom syntax parser.
Eframe is optional because native platform dependencies should not burden headless mode.
Versions resolve in Cargo.lock; no permanent UI selection is implied.

Use a bounded single buffered exchange: strict Content-Length, no chunking, no informational
responses, upgrades or Expect, no keepalive/pipelining. This is a smaller testable alternative
to a general standards transport, with visibly rejected unsupported cases. Retained header
lines are byte-preserved, and target/connection changes are recorded. Hyper remains the
provisional standards-mode candidate; it is not falsely advertised as wire preserving.

Global task cap plus bounded event channel provide backpressure. Configured connect/read/total
deadlines bound exchanges. Abort and join active tasks on shutdown. No filesystem I/O occurs
inside networking tasks. Evidence: nine integration regressions and local release measurements.

## ADR-014 — Native UI experiment

Benchmark pending. Optional egui/eframe with winit/OpenGL; network runtime on a separate OS
thread; bounded event receiver drained in limited batches. Generate 100k synthetic rows lazily,
limit the editor draft to 64 KiB and discard completed captures after updating status. This is
not persistent history. CPU layout harness measures virtualization/editor layout only.

Slint, presented-frame/input latency, Windows/Linux interactive operation, accessibility,
licensing audit and distribution comparisons remain required before selection. See UI_SPIKE.md.

## ADR-015 — TLS boundary

Active rustls interception remains provisional, unimplemented. The M0 HTTPS feasibility path
is opaque CONNECT plus a local TLS server. Python/OpenSSL generates a temporary self-signed
fixture certificate outside networking workers. Explicitly trusted clients succeed; default
trust rejects the certificate. This tests tunnel carriage and client verification semantics,
not MITM certificate handling. No trust store is modified. Smallest next TLS experiment:
rustls upstream verification and locally generated SAN certificate behind an opt-in CONNECT
interception mode, with tests for trust, hostname mismatch and handshake cancellation.

The environment mounts .codex read-only; the initial attempt to record ADRs there failed.
This reviewable copy preserves decisions in the repository; updating the handoff files
requires elevated filesystem access.

## ADR-016 — Opt-in TLS spike (continuation, 2026-10-31)

Provisional for M0. Add optional `tls-spike` feature using rustls/tokio-rustls (ring),
rcgen and WebPKI roots. An ephemeral in-memory CA signs one fixed-host certificate at startup,
outside network workers. Export only the public CA DER to an explicitly requested path;
never write private keys or change trust stores. The operator selects a single CONNECT
authority for interception; other authorities remain opaque tunnels. Verify upstream TLS
before sending CONNECT success. Offer only HTTP/1.1 ALPN and stream decrypted bytes through
fixed buffers under existing total/shutdown bounds; this is TLS transport feasibility, not
HTTP inspection or pause/edit. Optional extra upstream CA uses an explicit DER file.

## ADR-017 — Testable native capture harness

Accepted for the remaining M0 UI slice. Extend the existing egui experiment with bounded
in-memory capture rows, original/outgoing/response previews, an unsent draft, explicit
listener/error status and a separate synthetic 100k-row mode. Retain at most 128 entries
and 16 MiB of accounted payload bytes, evict oldest entries with a visible count. Limit
text previews/drafts to 64 KiB. No persistence or Replay executor is introduced. A local
fixture supplies reproducible real traffic. Verify model retention and offscreen UI layout,
then run the actual native window on an isolated Xvfb display for smoke validation.


## ADR-018 — Operator-requested workspace usability pass

Accepted for the existing native harness. The operator requested a less dry UI and continued
work. Add a coherent dark theme, working Traffic/Problems/Performance navigation, aligned
method/status/destination/size columns, bounded live filtering, keyboard selection with
scrolling, copy feedback, hex preview and explicit draft/reset/clear states. Keep networking,
wire guarantees and retention bounds unchanged. No extra dependency, permanent UI-stack
selection, persistent history or Replay implementation is implied. Native X11 smoke and
model/layout regressions verify the changed interactions; presented-frame latency, Slint,
accessibility, Windows and hardware/Wayland qualification remain outstanding.

## ADR-019 — Explicitly requested tabbed workflow and external browser

Accepted for the user-requested slice spanning M1/basic M2. The latest instruction supersedes
M0-only scope: implement native top-level Intercept/History/Replay/Intruder/Browser tabs,
request pause/edit/forward/drop, history context actions and single-request Replay. Chromium
is an external operator-launched test browser, never the desktop UI or a webview. Use an
isolated disposable profile, explicit HTTP proxy and loopback bypass override; never disable
certificate verification or Chromium's sandbox. No OS trust-store modifications.

Keep one bounded control object for interception (off by default), a bounded pending queue,
oneshot decisions, hold deadline and cancellation. Turning interception off releases pending
requests unchanged. Edited requests are validated with the same raw HTTP/1.1 parser and
remain restricted to their original authority. Replay has an explicit destination and raw
origin-form editor; destination is visible, per-run cancellation/timeout is mandatory and
sending bypasses interception. Intruder initially receives a template and provides a narrow
sequential bounded payload runner, not a claim of the full M4 engine. Existing framing/TLS
limits remain explicit. The GUI uses typed bounded channels; disk and browser lifecycle work
run on a dedicated worker thread, not the renderer or async reactor.


ADR-019 evidence: 39 debug/release tests passed and the native Linux workflow smoke passed,
including actual Chromium traffic through the proxy. Replay sessions are capped at eight,
run snapshots at eight each, execution at one active job, payload jobs at 100 requests /
16 MiB with 250 ms delay. Browser stderr diagnostics retain at most 4 KiB. Platform-specific
Windows/Wayland behavior and general HTTPS capture remain unverified/unimplemented.

## ADR-020 — Transport fixes and operator UI requirements

Accepted for the explicit eleven-item request. Treat empty speculative connections separately
from malformed partial messages; report connection failures with destination context. Stream
HTTP responses while retaining bounded, explicitly truncated captures. Add standards-based
Content-Length/chunked/close-delimited response handling using httparse syntax validation;
keep ambiguous framing rejection. CONNECT uses an idle timeout rather than the HTTP exchange
deadline. Expose HTTP timeout/capture settings without claiming external failures are fixable.

Add stable column sorting, HTTP/HTML display highlighting that never mutates wire bytes,
per-Replay editor undo/redo instead of recent-run snapshots, and lazy numeric/wordlist payload
sources. File selection/loading runs on a separate worker and remains bounded/cancellable.
Remove runtime synthetic/demo features. Replace decorative badges/cards and repeated prose
with compact native controls, meaningful state and precise errors. No new desktop/webview
framework, certificate bypass or UI-stack commitment is introduced.

## ADR-021 — Project-based application and expanded requested features

Accepted under the latest explicit implementation request. Add serde/JSON for versioned
settings and import/export and rusqlite (bundled SQLite) for recoverable local projects.
Project open/create/save/capture operations run on a bounded worker, never the UI/network
reactor. UI opens with a project chooser; a temporary project uses the same storage path
and is deleted after orderly shutdown. Historical traffic is paged, with bounded body views.
Use {{key}} for explicit Replay variable substitution; preserve {{payload}} for Intruder.
An implementation checklist in docs/IMPLEMENTATION.md records real completion evidence.
TLS/protocol expansion must use mature libraries, explicit trust and fidelity annotations.

## ADR-022 — Project CA and protocol adapters

General interception uses rustls with verified upstream certificates and rcgen-signed
per-host leaves. Enable rcgen's X.509 parser to reopen the same project CA rather than
silently rotating trust. Store the CA key only in the project with owner-only Unix
permissions; expose the public certificate path and never install trust automatically.
TLS leaf generation runs on the blocking pool with the existing connection bound.
Protocol adapters may present normalized HTTP/1 editor text, with that transformation
recorded separately from observed bytes. HTTP/2 uses h2; WebSocket framing uses tungstenite.

## ADR-023 — Text interchange and bounded inspection

Use shlex only to tokenize imported cURL text; never evaluate a shell or execute imports.
HAR bodies use base64 for binary content and disclose normalization. Inspection decoders
dechunk/decompress into a separate bounded view. Original and serialized bytes stay distinct.

## ADR-024 — Project locking and native accessibility

Use fs2 OS file locks to prevent two processes from allocating the same capture IDs in
one project; the OS releases locks after crashes. Enable eframe's native AccessKit bridge.
Use quick-xml for display-only XML formatting, with input/output limits and no external
entity resolution. Keep text comparison bounded by input size and an LCS work limit.

## ADR-025 — Simplified navigation and automatic HTTPS setup

The operator explicitly requests automatic CA installation and always-on HTTPS, superseding
the previous opt-in/manual-trust UI requirement. Native UI always configures its project CA.
Install only the public certificate in current-user Chromium NSS trust (Linux) / CurrentUser
Root (Windows), through a bounded worker with visible outcome; do not alter admin-wide stores
or bypass verification. Tests use temporary trust stores. SHA-256 labels make setup idempotent.
Scope/Variables/Browser/Project move into uniform Settings subsections; Intruder owns results.
Domain scope includes children by default. Legacy display-only scope entries migrate to domain
matching; explicitly enforced legacy execution scopes preserve their narrower host rules.

Linux paths and trust flags follow Chromium's
[certificate management documentation](https://chromium.googlesource.com/chromium/src/+/master/docs/linux/cert_management.md).
Evidence: isolated NSS idempotence regression, native project/workflow smoke and real Chromium
HTTPS fixture with certificate verification and sandbox enabled. Windows runtime validation
remains outstanding. Public roots are retained after project close; removal/rotation UI is deferred.

## ADR-026 — Independent URL columns, saved widths and automatic response decoding

For the requested History changes, expose Host (hostname), Endpoint (path and query) and
Destination (complete target) independently. Persist column widths in project state;
drag header boundaries, double-click to restore automatic sizing. One horizontal scroll
contains headers and virtualized rows. Add indexed host/endpoint metadata via a transactional,
bounded-page schema migration so saved-history sorting covers the entire result set.

Make Auto the initial inspector/Replay response view: decode transfer/content encodings into
a bounded display copy with a visible note. Keep captured headers and wire bytes unchanged;
Raw/Hex remain explicit original-byte views. Reuse existing gzip/deflate/Brotli decoders,
report truncated/unsupported/binary content rather than presenting compressed bytes as text.
No TLS or upstream verification changes. Validate using local compressed-JavaScript fixtures,
saved-history migration/sorting, persisted widths and native header dragging/response inspection.

## ADR-027 — Separate verbose diagnostic executable

The operator requests evidence before transport fixes. Build `httpied-debug` with the same CLI/UI
and networking behavior, plus trace-level tracing-subscriber output and a bounded asynchronous
tracing-appender writer. Add connection/HTTP2 stream IDs, phase/duration/error-kind records around
DNS/TCP/TLS/execution and stage-specific downstream handshake diagnostics. Retain existing dial,
timeout, reset and retry behavior; no speculative transport fixes. Logs rotate with bounded file
sizes, queue overflow counts are reported on exit, and the log path is printed on startup.
The dependencies are optional under debug-logging. The regular executable remains unchanged.
Implementation references: [tracing-subscriber formatting](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/index.html)
and [tracing-appender queue behavior](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/struct.NonBlockingBuilder.html).

## ADR-028 — Coalesced, bounded DNS resolution after operator trace

The supplied session records 11 five-second failures after DNS start and before any TCP
attempt, while other streams resolve the same host in 4–7 ms and complete TCP/TLS. The adapter
currently repeats system lookup for every HTTP/2 stream. Add a shared resolver to Config:
coalesce concurrent identical lookups, cache successful answers for 30 seconds, retain at most
128 hosts/ports and 32 addresses per answer, and cache failures briefly (500 ms). Shared futures
survive cancellation of one waiter; lookup deadlines remain bounded. IP literals bypass DNS.
Use the same resolver for CONNECT/HTTP/Replay so the initial lookup is reusable; invalidate an
answer after all its TCP attempts fail. Keep sequential address ordering and TLS verification.
Distinguish DNS deadline failures from TCP failures. Test with injected delayed lookups and
concurrent local HTTPS/HTTP2 requests. This mitigates duplicate resolver pressure; the trace
does not establish why the OS/DNS lookup stalled or explain the earlier, rotated-out TLS EOF.

Also write a compact application-only diagnostic file independently from dependency TRACE
output, so frame/window trace rotation cannot remove early connection failures from both logs.

2026-11-02 clarification: the operator's AdGuard screenshot shows fast server processing.
The original DNS-start timestamp precedes the resolver worker; it does not isolate queue time,
OS resolver execution or packet delivery. Add actual worker queued/start/finish timing around
the same standard ToSocketAddrs operation. A local test demonstrates queue-only expiry without
attributing that mechanism to the historical incident. AdGuard's configured rate limit is also
requested as a possible burst-related factor; no setting or drops are assumed or modified.

## ADR-029 — Join HTTP/2 Cookie fields during HTTP/1 translation

The supplied 98028 session's saved captures contain 37 lab HTTP 400 responses with the exact
body `{"error":"Duplicate header names are not allowed"}`. Every rejected outgoing request
contains two Cookie lines; its other header names are unique, and no rewrites are configured.
The HTTP/2 adapter currently forwards split Cookie fields as duplicate HTTP/1 fields. Follow
[RFC 9113 section 8.2.3](https://datatracker.ietf.org/doc/html/rfc9113#section-8.2.3): join their
opaque values with `; ` when building HTTP/1. Record the normalization in capture transformations.
Do not parse/deduplicate cookie names, modify raw HTTP/1 requests, collapse other duplicate
fields or combine response Set-Cookie values. Validate before/after using a strict local HTTP/1
fixture, then verified HTTPS/HTTP2 multiplexing. No remote lab requests are required.

## ADR-030 — Bottom activity log, explicit stream IDs and resilient Auto view

Move runtime errors and warnings out of request History into a bottom Logs drawer. Keep a
bounded, searchable severity-filtered log with timestamps and repeated-message counts,
persisted by the existing project worker in an additive table; expose historical Error
captures there while keeping their original records intact. HTTP responses (including 4xx/5xx)
remain traffic. Use HTTP/2 as the protocol label and a separate optional numeric Stream ID.

Auto means display-only decoding, not recompression of wire traffic. Add mature zstd and
encoding_rs libraries for Zstandard and declared text charsets. Decode repeated/stacked
content encodings in reverse order with bounds at every stage; support gzip/x-gzip, zlib/raw
deflate, Brotli, Zstandard and chunk framing. Sniff gzip when its header is absent. Render
readable text; on binary/corrupt/unsupported data, keep captured bytes visible with a note.
Explicit Decoded reports failures. Preserve raw bytes, framing headers and proxy behavior.
Verify bounded logs/legacy migration, traffic isolation, codecs/chains/binary fallback and
native footer/drawer interactions. No new top-level tab or networking work on the renderer.


## ADR-031 — Focused usability and export privacy review (2026-11-02)

Accepted for the user-requested review, with UI/usability prioritized. Keep the current native
architecture and existing protocol behavior. Use one HistoryFilter match for live/saved views,
fetch saved captures asynchronously for context actions, and retain bounded editor/previews.
Clear response bytes and derived views together; apply Find selections after editor click
handling so toolbar navigation retains focus and supports Copy.

Use shared URL/header redaction for export and cURL, and mask configured secret variables
before redacted cURL expansion. Replace complete URL queries rather than guessing sensitive
parameter names. Redaction remains limited to documented fields and cannot identify arbitrary
application secrets. Exported copies discard obsolete body framing when replacing bodies.

Temporary projects must exclusively create their directory before acquiring project state;
accepting a preexisting directory is incompatible with delete-on-exit ownership. Export files
use mode 0600 on Unix and retain create-new/non-overwrite behavior. Windows retains inherited
ACLs. No new dependencies, schema changes or trust-policy changes are introduced.

Evidence and remaining platform/dependency review limits: `docs/REVIEW_2026-11-02.md`.


## ADR-032 — Coder and selected-text transfers (2026-11-02)

The operator requests a Coder tab and right-click transfer of mouse/keyboard text selections.
Add bounded, session-only conversion state and a shared editor context action. Snapshot a
selection before right-click focus changes, use character indexes for Unicode, and transfer
through a single UI-local action slot. Captures and source editors remain independent.

Reuse base64/percent-encoding and enable quick-xml's existing `escape-html` feature for HTML5
named entities; no additional crate is needed. Keep URL percent encoding distinct from form
encoding (`+` means space only in forms), validate malformed input, preserve decoded binary
bytes and expose hex input/output plus explicit output-to-input chaining. Limit input to
128 KiB of bytes and conversion output to 1 MiB. Do not save selected credentials to project
state by adding Coder persistence.

Reported EOF/reset logs lack destination/phase context. Enrich TLS, HTTPS and WebSocket
errors while preserving failure semantics, certificate verification and truncation checks.
The supplied excerpts cannot prove whether cancellation, peer behavior or a defect caused
any individual failure. Do not globally suppress or downgrade these failures.
