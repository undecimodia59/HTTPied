# Requested proxy and UI corrections — 2026-10-31

| Request | Implemented behavior |
| --- | --- |
| Repeated incomplete request heads | Empty browser preconnections close quietly; genuinely partial heads still produce an error |
| Connection refused | Error identifies destination host/port and likely listener issue; dummy localhost destinations removed from new Replay/browser inputs |
| Response body limit | Responses stream independently of bounded capture; total size and truncation are recorded |
| Total exchange timeout | Active CONNECT/TLS tunnels use inactivity expiry; HTTP phase timeout is configurable, default 120 s |
| Sort History | All five headers toggle ascending/descending; numeric status/size and stable request-ID selection |
| HTTP highlighting | Display-only colored request/status lines and headers in Intercept, History and Replay |
| HTML response highlighting | Tags, quoted attributes and comments highlighted as text, without rendering HTML or changing bytes |
| Replay undo/redo | Recent runs/snapshots removed; per-session native Ctrl+Z / Ctrl+Y editing |
| Intruder sources | Lazy inclusive numeric ranges with zero padding and steps; UTF-8 wordlists through a native picker or path/load |
| Debug UI | Runtime synthetic rows, controls, demo payloads and dummy endpoints removed |
| UI design review | Decorative branding/pills/rounded panels and repetitive empty-state prose removed; compact neutral native controls |

## Implementation

`src/proxy.rs`, new `src/response.rs` and `src/tunnel.rs` separate validated HTTP response
framing/streaming from retained captures. Buffers remain bounded; no unbounded whole-body
buffer or silent body-size fallback is introduced. Chunked, Content-Length and close-delimited
responses are supported, including chunk extensions/trailers and bounded informational heads.
Ambiguous framing remains an error. Once output begins, failures close the stream rather than
injecting a second HTTP response. `WireExecution` now records final status, total bytes and
capture truncation independently from retained bytes.

`src/payload.rs` and `src/jobs.rs` generate requests lazily. Numeric sources support up to
1,000,000 entries, inclusive bounds, checked arithmetic, positive steps and inferred padding.
Wordlists are regular UTF-8 files capped at 16 MiB / 1,000,000 lines / 4 KiB per line. Native
file selection and parsing run on a worker. Closing the app stops an owned picker and joins
the worker. Sending is blocked while a wordlist loads, and editing the path invalidates old
loaded values. No serialized request array is generated for a large range/wordlist.

`src/ui.rs`, `src/ui/workflow.rs`, `src/ui/widgets.rs` and new `src/ui/syntax.rs` implement
sorting, editor identities and view-only highlighting. Sorting preserves selected IDs and
syntax layout preserves supplied text exactly. Replay sends do not replace editor content;
undo history belongs to its request session. Binary/compressed content is not decoded by the
highlighter. See [UI audit](UI_AUDIT.md) for design-review references and concrete changes.

## Verification

- `cargo fmt --check` and `git diff --check`.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`.
- `cargo check --locked --no-default-features`.
- `cargo test --locked --workspace --all-features`.
- `cargo test --locked --release --features ui,tls-spike`.
- `cargo build --locked --release --features ui`.
- `python3 scripts/revision_smoke.py` on a private Xvfb/D-Bus session.

Tests cover preconnection suppression versus partial errors, contextual refusal, large body
streaming with truncated capture, early bytes before origin completion, chunked/trailer and
close-delimited framing, framing ambiguity, long-lived active CONNECT, sorting, byte-preserving
highlighting, native undo/redo, padded ranges/overflow, wordlist content validation and stale
source dispatch prevention. Existing proxy, TLS verification and interception tests remain.

Native smoke verified Ctrl+Z/Ctrl+Y by observing actual restored request paths on the fixture,
clicked a sortable header, inspected HTTP/HTML colors, sent 0000/0001/0002, selected a real
wordlist via kdialog, sent both file payloads, and closed HTTPied with another chooser open.
Screenshots were inspected. The first zenity path exposed an isolated desktop-portal issue;
the direct kdialog path succeeded, with zenity retained as a fallback and typed-path loading
available. No browser sandbox/certificate bypass was introduced.

Final results: all **54 tests** passed in debug and release (18 unit/UI/payload, 17 proxy,
9 TLS and 10 workflow tests). Formatting, Clippy with warnings denied, the headless check
and the release UI build passed. Native revision smoke passed, including owned-dialog
shutdown cleanup.

## Remaining limits and next bounded task

A genuinely unavailable origin can still refuse a connection. Timeouts and truncation limits
are explicit policies, not guarantees that every remote endpoint responds. History retains
128 events / 16 MiB accounted payload; large Intruder runs do not persist every result. The
request path remains bounded HTTP/1.1 Content-Length; HTTP/2, WebSocket, request chunking and
general decrypted HTTPS editing remain unimplemented. Close-delimited bodies retain HTTP's
inherent ambiguity between some clean closes and truncation. Windows/Wayland native runtime
and file-picker behavior remain unverified.

Next bounded transport task: general HTTPS request capture/edit with an explicit CA trust
workflow, followed by broader request framing; retain the current streaming/cancellation
regressions. Performance claims for the revised UI are not inferred from earlier M0 samples.
