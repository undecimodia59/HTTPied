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
