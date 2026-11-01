# M0 implementation report — 2026-10-31

The architecture foundation is implemented as a compiling, locally testable restricted proxy
and optional native UI spike. It demonstrates bounded networking independent of UI execution.
M0's full UI comparison/performance evidence is still open; this is not a general-purpose
intercepting browser proxy or a claim that subsequent milestones are complete.

## Files and behavior

- `Cargo.toml`, `Cargo.lock`: single-package Rust workspace, locked dependencies, optional UI.
- `src/model.rs`: immutable original capture, parsed ordered headers, edit revision intent,
  outgoing execution and response bytes; explicit transformation descriptions.
- `src/proxy.rs`: loopback listener, strict HTTP/1.1 Content-Length forwarding, opaque CONNECT,
  bounded task/event work, connect/read/total deadlines, abort-and-join shutdown.
- `src/main.rs`, `src/ui.rs`: usable CLI, Ctrl+C shutdown, optional native virtualized table
  and request editor with networking on a separate runtime thread. No editor execution action.
- `tests/proxy.rs`: nine local integration regressions, including negative framing cases,
  exact retained header lines, encoded URI, bodies, fragmented input/output, HEAD semantics,
  truncated/unsupported/oversize response errors, deadlines, half-close and cancellation.
- `scripts/bench.py`, `scripts/tls_tunnel.py`, `examples/ui_layout.rs`: reproducible local
  release performance harness, TLS-through-CONNECT fixture, CPU layout experiment.
- `README.md`, `docs/UI_SPIKE.md`, `docs/DECISIONS_M0.md`: Windows/Linux commands, limits,
  candidate evaluation procedure and consequential implementation decisions.
- `.codex/DECISIONS.md`, `.codex/TASKS.md`: updated handoff with verified work and open gates.
  These pre-existing handoff files are gitignored; tracked decisions also live under docs.

## Commands and outcomes

- `cargo check`: passed headless build after dependency download approval.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --workspace --all-targets --features ui -- -D warnings`: passed.
- `cargo test --locked --workspace --features ui`: nine integration tests passed, plus
  binary/library/doc targets. This also compiles and links the optional native UI.
- `cargo test --locked --release --workspace`: all nine integration tests passed.
- `cargo build --locked --release`: passed.
- `python3 scripts/tls_tunnel.py`: passed local trusted TLS ping/pong and default-untrusted
  certificate rejection through opaque CONNECT. The fixture certificate is temporary.
- `python3 scripts/bench.py > docs/benchmark-linux.json`: completed 1,000 local direct/proxied
  pairs and 10 process launches with a two-byte response and capture events drained.
- `cargo run --locked --release --features ui --example ui_layout`: passed the virtualization
  assertion and measured 300 CPU layout samples after 30 warmup frames.

Initial tests inside the sandbox failed because loopback bind is prohibited. Socket tests,
TLS fixture and benchmarks were rerun outside that sandbox with approval. An added timeout
assertion initially expected different OS error wording; it was fixed to accept `timed out`.
An initial optional UI build exposed the current App::ui API; implementation was corrected
against the downloaded eframe source and subsequently compiled/linted successfully.

## Measured environment and limits

Linux 7.3.0-1-default, x86_64, glibc 2.44; AMD Ryzen AI 7 350 w/ Radeon 860M,
16 logical CPUs; 29,194 MiB RAM reported by `free -m`. Rust 1.99.0
(b940084d7, 2026-09-28), Cargo 1.99.0. Headless binary uses release optimization and two
Tokio workers. Background machine load was not controlled; these are feasibility samples,
not product acceptance measurements. Local fixture uses Python ThreadingHTTPServer;
connections close after every request, with no TLS and no persistent history.

Raw headless measurements: [benchmark-linux.json](benchmark-linux.json).

| Metric | Measured |
| --- | --- |
| Fresh process to READY, median / p95, 10 runs | 1.083 / 1.706 ms |
| Idle Linux RSS sample, 200 ms after READY | 3,700 kB |
| Direct request latency median / p95, 1,000 runs | 0.336 / 0.518 ms |
| Proxied request latency median / p95 | 0.391 / 0.580 ms |
| Paired proxied-minus-direct median / p95 | 0.058 / 0.208 ms |

Process launches used warm filesystem caches. They are **not true cold startup** and exclude
UI initialization and first interactive frame. Idle RSS is one sample, not a peak/private
working set. Do not compare it directly with a future desktop acceptance run. Benchmarks
redirect proxy diagnostic logs to the null device and do not retain captures.

Raw CPU-only UI measurements: [ui-layout-linux.json](ui-layout-linux.json). The 100,000-row
virtual table rendered at most 13 rows per layout with a 35,840-byte editor draft. Median
layout time was 0.280 ms and p95 0.307 ms over 300 samples after 30 warmup frames. No window,
GPU presentation or keyboard interaction was involved; these are not end-to-end UI timings.

For reproducible true cold measurements: record machine/OS/toolchain/build, reboot before
each first launch (or record an explicit OS cache-conditioning procedure), capture process
creation to READY and separately first interactive frame, repeat at least 30 times, and
publish raw timings. Follow UI_SPIKE.md for presented-frame and input latency measurements.

## Unimplemented and risks

Chunked/close-delimited transport, streaming, keepalive, HTTP/2, WebSocket inspection,
manual pause/edit/forward and persistent CA lifecycle are absent. An optional active rustls
transport experiment is now implemented; see the continuation below. The proxy visibly
rejects unsupported framing. CONNECT only carries opaque encrypted bytes; its whole-tunnel
lifetime is 30 seconds by default. No scope allowlist, project persistence, scripts, Replay
or Intruder is implemented. The original Python TLS tests validate client trust through a tunnel. The continuation adds
proxy-side TLS termination and verification tests; production CA lifecycle is still open.

Bounded buffers and channels have enforced capacities, but sustained-load peak RSS, 100-client
behavior and long-run leak checks have not been measured. The prototype can hold multiple
copies of each buffered exchange. It closes sockets after a response even when original
response headers advertise keepalive, and records that limitation. Timeout after output has
started closes the connection rather than injecting another HTTP response.

Windows execution, true cold/UI startup, interactive row/editor latency, accessibility,
Slint parity, package/license audit and GPU fallback are unverified. The egui CPU layout
harness is supplementary and cannot validate screen responsiveness or product budgets.
The event consumer discards captures after reporting status; there is no project/history store.

## Exact next bounded tasks

1. Complete the remaining M0 evidence: run the egui spike on Windows/Linux, add equivalent
   Slint table/editor/body-viewer, and publish cold startup, memory, input/present timing,
   accessibility and packaging comparisons before choosing a stack.
2. Run an opt-in rustls CONNECT interception experiment using a generated local CA and SAN
   certs, with trust/hostname/expiry failures, handshake deadlines, cancellation and secure
   key handling; never install trust automatically.
3. After M0 review, take M1's standards HTTP/1.1 transport slice: mature chunked/streaming
   framing, keepalive, hop-by-hop semantics and explicit fidelity annotations, with local
   malformed/framing regressions. Keep raw mode separately bounded and explicit.


## M0 continuation — optional active TLS transport

Added `src/tls.rs`, `tests/tls.rs`, the optional `tls-spike` dependencies, fixed-authority
configuration in `src/proxy.rs` and explicit CLI options in `src/main.rs`. `src/model.rs`
distinguishes intercepted TLS from opaque tunnels. See [TLS_SPIKE.md](TLS_SPIKE.md) for
operator commands, certificate lifetime, trust and wire fidelity limits. This supersedes
the original active-TLS gap and next task 2 above; it does not complete persistent CA handling.

Nine additional local TLS fixtures cover trusted HTTP application bytes through both TLS
sessions, required client trust, generated SAN, upstream trust/name/expiry failures,
upstream and downstream handshake deadlines, and shutdown during a pending handshake.
No private key is written to disk. A new public CA DER output must be explicitly selected.
Existing headless and CPU-layout measurements describe the original non-TLS build; they
are not TLS performance measurements. Interactive UI/Slint/cold-start and Windows evidence
remain the next bounded M0 task.

Continuation validation: `cargo test --locked --workspace --all-features` passed all 18
integration tests; `cargo test --locked --release --features tls-spike` passed all 18.
`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`,
`cargo fmt --check`, and `git diff --check` passed. Socket tests require execution outside
this environment's restricted sandbox, as in the original M0 run.

## Native capture UI continuation

The UI now retains real completed exchanges with bounded in-memory history (128 entries /
16 MiB accounted payload), explicit eviction count, stable selectable row IDs, original /
outgoing / response previews, and a separate unsent request draft. Preview text is capped
at 64 KiB; captures are immutable and preview fidelity is labeled as lossy UTF-8. Listener
readiness, failures and worker disconnect are visible. Synthetic 100k rows remain a separate
performance mode. This supersedes the original statement that the UI discards all captures;
persistence, Replay and manual interception remain unimplemented.

Files: `src/ui.rs`, readiness event in `src/model.rs` and headless handling in `src/main.rs`,
`scripts/ui_fixture.py`, `scripts/ui_smoke.py`, `docs/UI_TESTING.md`, README and handoff updates.
Automated UI model/layout tests run with the existing proxy and TLS regressions. The actual
Linux X11 window was launched on a private Xvfb display with software GL. Three real local
GET/POST exchanges appeared, the response tab and synthetic control were clicked, screenshots
were inspected, and WM_DELETE_WINDOW produced clean shutdown. No existing desktop session
was used. The run did not measure input latency or compare Slint and does not qualify
Wayland, hardware rendering, Windows or accessibility. The UI is ready for operator testing
using the two-terminal instructions in `docs/UI_TESTING.md`.

Final UI validation: all 22 tests passed with `cargo test --locked --workspace --all-features`
(four UI tests plus eighteen proxy/TLS regressions); combined Clippy, formatting and release
UI build passed. `python3 scripts/ui_smoke.py` passed the actual native-window smoke.


## Operator-requested UI usability continuation

`src/ui.rs` and new `src/ui/widgets.rs` now provide a native traffic workspace with a slate /
teal theme, aligned traffic columns, working Traffic/Problems/Performance navigation,
case-insensitive search, arrow-key selection and scrolling, copy actions with feedback,
byte/ASCII hex preview, clear confirmation, draft reset and explicit unsent state. Empty
and disconnected states give actionable guidance. Search never mutates captured bytes;
retention and preview bounds remain enforced. Hex is capped at 8 KiB of source data to bound
its expanded representation. `scripts/ui_smoke.py` and `docs/UI_TESTING.md` cover new controls.

Validation: `cargo test --locked --workspace --all-features` passed 24 tests (six UI plus
18 proxy/TLS); `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`,
`cargo fmt --check`, `cargo build --locked --release --features ui` passed. Actual X11 smoke
on a private Xvfb display exercised capture selection, response inspection, typed `post`
filter, local framing-error/Problems inspection, Performance navigation and 940×700 resize.
Screenshots were visually inspected, then the window closed cleanly. No performance numbers
for this redesigned workspace are claimed. Existing CPU-layout measurements describe the
original synthetic harness; UI-stack/performance selection remains pending.

Next bounded task: complete M0's platform and comparable performance evidence, then review
the streaming/framing transport slice before advertising general browser support. HTTPS
capture/edit, sending drafts, persistent projects and manual pause/forward remain absent.


## Explicitly requested Intercept / History / Replay / Browser slice

The latest operator request superseded the M0-only boundary and asked for working tabs and
an external Chromium launcher. Implemented native top-level Intercept, History, Replay,
Intruder and Browser tabs. Request interception uses bounded pending channels and oneshot
forward/edit/drop decisions before any upstream connection. Turning OFF releases originals;
operator edits retain original bytes, enforce destination ownership, validate framing and
normalize editor header line endings with explicit Content-Length repair. The History table
has left-click inspection and right-click transfers of actual executed requests, including
manual edits. Replay has eight independent sessions, editable request/read-only response,
Send/Cancel, elapsed/status/size and eight restorable run snapshots per session. A bounded
single-marker Intruder runner sends up to 100 payloads sequentially with delay and cancellation.

New modules: `src/control.rs`, `src/replay.rs`, `src/jobs.rs`, `src/browser.rs` and
`src/ui/workflow.rs`. `src/proxy.rs` now separates preparation from connection/execution so
holding requests never contacts the origin and Replay shares the parser without reentering
Interception. The browser uses an isolated temporary profile, explicit proxy and loopback
bypass override. Browser launch/profile/stderr/lifecycle work stays on dedicated workers;
only the last 4 KiB of startup stderr is retained. No private certificate keys/trust stores,
Chromium sandbox or certificate verification settings are weakened.

Validation: `cargo test --locked --workspace --all-features` and
`cargo test --locked --release --features ui,tls-spike` passed all 39 tests (11 unit/UI/browser,
9 original proxy, 9 TLS and 10 workflow integrations). Workflow regressions include genuine
pause/edit/forward/drop, interception-off release, deadline/shutdown handling, blocked
cross-authority edits, Replay bypass and cancellation closing a live upstream connection.
Clippy with all features/all targets and warnings denied, formatting, headless check and
release UI build passed. `scripts/workflow_smoke.py` passed with an actual native Linux
window: edited request reached the fixture; history context action loaded Replay; Replay
received a response; three Intruder payloads reached the fixture; installed Chromium made
a local HTTP request and that request appeared in History. Screenshots were inspected and
normal window shutdown completed. A distro wrapper initially forced Chromium to Wayland
on an X11-only display; selecting the X11 backend in that environment resolved startup.
No claim of Windows/native-Wayland browser lifecycle qualification is made.

`docs/WORKFLOW.md` replaces the older sidebar-only testing instructions. The cleartext
transport is still restricted to Content-Length HTTP/1.1. General HTTPS capture/edit/Replay,
chunked/streaming/framing expansion, persistent projects, the full Intruder scheduler and
scope editor remain open. Browser HTTPS runs through opaque CONNECT. Full M1/M2/M4 exit
criteria are not marked complete. Next bounded task is standards HTTP/1.1 framing/streaming,
then verified general HTTPS capture/edit with explicit trusted-CA lifecycle.


## Requested transport / sorting / editor / payload corrections

The original buffering/framing, fixed CONNECT lifetime, synthetic UI and Replay snapshot
limitations are superseded by [CHANGE_REPORT.md](CHANGE_REPORT.md) and the current workflow
guide. That report maps all eleven operator requests to code, verification and remaining
limits. Historical benchmark numbers in this file still describe their original build.
